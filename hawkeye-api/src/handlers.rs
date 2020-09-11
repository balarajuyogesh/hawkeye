use hawkeye_core::models::{Status, Watcher};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Service};
use kube::api::{DeleteParams, ListParams, PatchParams, PostParams};
use kube::{Api, Client};
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use uuid::Uuid;
use warp::http::StatusCode;
use warp::reply;
use lazy_static::lazy_static;

const NAMESPACE_ENV: &str = "HAWKEYE_NAMESPACE";
const DOCKER_IMAGE_ENV: &str = "HAWKEYE_DOCKER_IMAGE";

lazy_static! {
    static ref NAMESPACE: String = std::env::var(NAMESPACE_ENV).unwrap_or_else(|_| "default".into());
    static ref DOCKER_IMAGE: String = std::env::var(DOCKER_IMAGE_ENV).unwrap_or_else(|_| "hawkeye-dev:latest".into());
}

pub async fn list_watchers(client: Client) -> Result<impl warp::Reply, Infallible> {
    let lp = ListParams::default()
        .labels("app=hawkeye,watcher_id")
        .timeout(10);

    // Get all deployments we know, we want to return the status of each watcher
    let deployments_client: Api<Deployment> = Api::namespaced(client.clone(), &NAMESPACE);
    let deployments = deployments_client.list(&lp).await.unwrap();
    let mut deployments_index = HashMap::new();
    for deploy in deployments.items {
        if let Some(watcher_id) = deploy.metadata.labels.as_ref().unwrap().get("watcher_id") {
            deployments_index.insert(watcher_id.clone(), deploy.get_watcher_status());
        }
    }

    // We use the ConfigMap as source of truth for what are the watchers we have
    let config_maps_client: Api<ConfigMap> = Api::namespaced(client.clone(), &NAMESPACE);
    let config_maps = config_maps_client.list(&lp).await.unwrap();

    let mut watchers: Vec<Watcher> = Vec::new();
    for config in config_maps.items {
        let data = config.data.unwrap();
        let mut w: Watcher = serde_json::from_str(data.get("watcher.json").unwrap()).unwrap();
        let calculated_status = if let Some(status) =
            deployments_index.get(w.id.as_ref().unwrap_or(&"undefined".to_string()))
        {
            *status
        } else {
            Status::Error
        };
        w.status = Some(calculated_status);
        // TODO: Comes from the service
        w.source.ingest_ip = None;
        watchers.push(w);
    }

    Ok(warp::reply::json(&watchers))
}

pub async fn create_watcher(
    mut watcher: Watcher,
    client: Client,
) -> Result<impl warp::Reply, Infallible> {
    log::debug!("v1.create_watcher: {:?}", watcher);

    let new_id = Uuid::new_v4().to_string();
    watcher.id = Some(new_id.clone());

    // 1. Create ConfigMap
    log::info!("Creating ConfigMap instance");
    let config_file_contents = serde_json::to_string(&watcher).unwrap();
    let config_name = format!("hawkeye-config-{}", new_id);

    let config: ConfigMap = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {
            "name": config_name,
            "labels": {
                "app": "hawkeye",
                "watcher_id": new_id,
            }
        },
        "data": {
            "log_level": "INFO",
            "watcher.json": config_file_contents,
        }
    }))
    .unwrap();

    let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), &NAMESPACE);
    let pp = PostParams::default();
    // TODO: Handle errors
    let _ = config_maps.create(&pp, &config).await.unwrap();

    // 2. Create Deployment with replicas=0
    log::info!("Creating Deployment instance");
    let deploy: Deployment = serde_json::from_value(json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": format!("hawkeye-deploy-{}", new_id),
            "labels": {
                "app": "hawkeye",
                "watcher_id": new_id,
                "target_status": Status::Ready,
            }
        },
        "spec": {
            "replicas": 0,
            "selector": {
                "matchLabels": {
                    "app": "hawkeye",
                    "watcher_id": new_id,
                }
            },
            "template": {
                "metadata": {
                    "labels": {
                        "app": "hawkeye",
                        "watcher_id": new_id,
                    }
                },
                "spec": {
                    "dnsPolicy": "Default",
                    "restartPolicy": "Always",
                    "terminationGracePeriodSeconds": 5,
                    "containers": [
                        {
                            "name": "hawkeye-app",
                            "imagePullPolicy": "IfNotPresent",
                            "image": DOCKER_IMAGE.as_str(),
                            "args": [
                                "/config/watcher.json"
                            ],
                            "env": [
                                {
                                    "name": "RUST_LOG",
                                    "valueFrom": {
                                        "configMapKeyRef": {
                                            "name": config_name,
                                            "key": "log_level"
                                        }
                                    }
                                }
                            ],
                            "resources": {
                                "requests": {
                                    "cpu": "1150m",
                                    "memory": "50Mi"
                                }
                            },
                            "ports": [
                                {
                                    "containerPort": watcher.source.ingest_port,
                                    "protocol": "UDP"
                                },
                                {
                                    "containerPort": 3030,
                                    "protocol": "TCP"
                                }
                            ],
                            "volumeMounts": [
                                {
                                    "mountPath": "/config",
                                    "name": "config",
                                    "readOnly": true
                                }
                            ]
                        }
                    ],
                    "volumes": [
                        {
                            "name": "config",
                            "configMap": {
                                "name": config_name,
                                "items": [
                                    {
                                        "key": "watcher.json",
                                        "path": "watcher.json"
                                    }
                                ]
                            }
                        }
                    ]
                }
            }
        }
    }))
    .unwrap();
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), &NAMESPACE);
    let pp = PostParams::default();
    // TODO: Handle errors
    let _ = deployments.create(&pp, &deploy).await.unwrap();

    // 3. Create Service/LoadBalancer
    log::info!("Creating Service instance");
    let svc: Service = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": format!("hawkeye-vid-svc-{}", new_id),
            "labels": {
                "app": "hawkeye",
                "watcher_id": new_id,
            },
            "annotations": {
                "service.beta.kubernetes.io/aws-load-balancer-type": "nlb"
            }
        },
        "spec": {
            "type": "LoadBalancer",
            "selector": {
                "app": "hawkeye",
                "watcher_id": new_id,
            },
            "ports": [
                {
                    "name": "video-feed",
                    "protocol": "UDP",
                    "port": watcher.source.ingest_port,
                    "targetPort": watcher.source.ingest_port
                }
            ]
        }
    }))
    .unwrap();

    let services: Api<Service> = Api::namespaced(client.clone(), &NAMESPACE);
    let pp = PostParams::default();
    // TODO: Handle errors
    let _ = services.create(&pp, &svc).await.unwrap();

    watcher.status = Some(Status::Updating);
    watcher.source.ingest_ip = None;

    Ok(reply::with_status(
        reply::json(&watcher),
        StatusCode::CREATED,
    ))
}

pub async fn get_watcher(id: String, client: Client) -> Result<impl warp::Reply, Infallible> {
    let deployments_client: Api<Deployment> = Api::namespaced(client.clone(), &NAMESPACE);
    let deployment = match deployments_client
        .get(&format!("hawkeye-deploy-{}", id))
        .await
    {
        Ok(d) => d,
        Err(_) => {
            return Ok(reply::with_status(
                reply::json(&json!({})),
                StatusCode::NOT_FOUND,
            ))
        }
    };

    // We use the ConfigMap as source of truth for what are the watchers we have
    let config_maps_client: Api<ConfigMap> = Api::namespaced(client.clone(), &NAMESPACE);
    let config_map = match config_maps_client
        .get(&format!("hawkeye-config-{}", id))
        .await
    {
        Ok(c) => c,
        Err(_) => {
            return Ok(reply::with_status(
                reply::json(&json!({})),
                StatusCode::NOT_FOUND,
            ))
        }
    };

    let mut w: Watcher =
        serde_json::from_str(config_map.data.unwrap().get("watcher.json").unwrap()).unwrap();
    w.status = Some(deployment.get_watcher_status());
    // TODO: Comes from the service
    w.source.ingest_ip = None;

    Ok(reply::with_status(reply::json(&w), StatusCode::OK))
}

pub async fn start_watcher(id: String, client: Client) -> Result<impl warp::Reply, Infallible> {
    let deployments_client: Api<Deployment> = Api::namespaced(client.clone(), &NAMESPACE);
    // TODO: probably better to just get the scale
    let deployment = match deployments_client
        .get(&format!("hawkeye-deploy-{}", id))
        .await
    {
        Ok(d) => d,
        Err(_) => {
            return Ok(reply::with_status(
                reply::json(&json!({})),
                StatusCode::NOT_FOUND,
            ))
        }
    };
    match deployment.get_watcher_status() {
        Status::Running => Ok(reply::with_status(
            reply::json(&json!({
                "message": "Watcher is already running"
            })),
            StatusCode::CONFLICT,
        )),
        Status::Updating => Ok(reply::with_status(
            reply::json(&json!({
                "message": "Watcher is updating"
            })),
            StatusCode::CONFLICT,
        )),
        Status::Ready => {
            // Start watcher / replicas to 1
            let patch_params = PatchParams::default();

            let fs = json!({
                "spec": { "replicas": 1 }
            });
            let o = deployments_client
                .patch_scale(
                    deployment.metadata.name.as_ref().unwrap(),
                    &patch_params,
                    serde_json::to_vec(&fs).unwrap(),
                )
                .await
                .unwrap();
            log::debug!("Scale status: {:?}", o);

            let status_label = json!({
                "metadata": {
                    "labels": {
                        "target_status": Status::Running
                    }
                }
            });
            let _ = deployments_client
                .patch(
                    deployment.metadata.name.as_ref().unwrap(),
                    &patch_params,
                    serde_json::to_vec(&status_label).unwrap(),
                )
                .await;

            Ok(reply::with_status(
                reply::json(&json!({
                    "message": "Watcher is starting"
                })),
                StatusCode::OK,
            ))
        }
        Status::Error => Ok(reply::with_status(
            reply::json(&json!({
                "message": "Watcher in error state cannot be set to running"
            })),
            StatusCode::NOT_ACCEPTABLE,
        )),
    }
}

pub async fn stop_watcher(id: String, client: Client) -> Result<impl warp::Reply, Infallible> {
    let deployments_client: Api<Deployment> = Api::namespaced(client.clone(), &NAMESPACE);
    // TODO: probably better to just get the scale
    let deployment = match deployments_client
        .get(&format!("hawkeye-deploy-{}", id))
        .await
    {
        Ok(d) => d,
        Err(_) => {
            return Ok(reply::with_status(
                reply::json(&json!({})),
                StatusCode::NOT_FOUND,
            ))
        }
    };
    // TODO: Set target_status to Ready
    match deployment.get_watcher_status() {
        Status::Ready => Ok(reply::with_status(
            reply::json(&json!({
                "message": "Watcher is already stopped"
            })),
            StatusCode::CONFLICT,
        )),
        Status::Updating => Ok(reply::with_status(
            reply::json(&json!({
                "message": "Watcher is updating"
            })),
            StatusCode::CONFLICT,
        )),
        Status::Running => {
            // Stop watcher / replicas to 0
            let patch_params = PatchParams::default();

            let fs = json!({
                "spec": { "replicas": 0 }
            });
            let o = deployments_client
                .patch_scale(
                    deployment.metadata.name.as_ref().unwrap(),
                    &patch_params,
                    serde_json::to_vec(&fs).unwrap(),
                )
                .await
                .unwrap();
            log::debug!("Scale status: {:?}", o);

            let status_label = json!({
                "metadata": {
                    "labels": {
                        "target_status": Status::Ready
                    }
                }
            });
            let _ = deployments_client
                .patch(
                    deployment.metadata.name.as_ref().unwrap(),
                    &patch_params,
                    serde_json::to_vec(&status_label).unwrap(),
                )
                .await;

            Ok(reply::with_status(
                reply::json(&json!({
                    "message": "Watcher is stopping"
                })),
                StatusCode::OK,
            ))
        }
        Status::Error => Ok(reply::with_status(
            reply::json(&json!({
                "message": "Watcher in error state cannot be set to stopped"
            })),
            StatusCode::NOT_ACCEPTABLE,
        )),
    }
}

pub async fn delete_watcher(id: String, client: Client) -> Result<impl warp::Reply, Infallible> {
    let dp = DeleteParams::default();

    let deployments_client: Api<Deployment> = Api::namespaced(client.clone(), &NAMESPACE);
    let _ = deployments_client
        .delete(&format!("hawkeye-deploy-{}", id), &dp)
        .await;

    let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), &NAMESPACE);
    let _ = config_maps
        .delete(&format!("hawkeye-config-{}", id), &dp)
        .await;

    let services: Api<Service> = Api::namespaced(client.clone(), &NAMESPACE);
    match services
        .delete(&format!("hawkeye-vid-svc-{}", id), &dp)
        .await
    {
        Ok(_) => Ok(reply::with_status(
            reply::json(&json!({
                "message": "Watcher has been deleted"
            })),
            StatusCode::OK,
        )),
        Err(_) => Ok(reply::with_status(
            reply::json(&json!({
                "message": "Watcher does not exist"
            })),
            StatusCode::NOT_FOUND,
        )),
    }
}

trait WatcherStatus {
    fn get_watcher_status(&self) -> Status;
}

impl WatcherStatus for Deployment {
    fn get_watcher_status(&self) -> Status {
        let target_status = self
            .metadata
            .labels
            .as_ref()
            .map(|labels| {
                labels
                    .get("target_status")
                    .map(|status| serde_json::from_str(&format!("\"{}\"", status)).ok())
            })
            .flatten()
            .flatten()
            .unwrap_or(Status::Error);
        log::debug!("TARGET_STATUS={:?}", target_status);
        if let Some(status) = self.status.as_ref() {
            let deploy_status = if status.available_replicas.unwrap_or(0) > 0 {
                Status::Running
            } else {
                Status::Ready
            };
            match (deploy_status, target_status) {
                (Status::Running, Status::Running) => Status::Running,
                (Status::Ready, Status::Ready) => Status::Ready,
                (Status::Ready, Status::Running) => Status::Updating,
                (Status::Running, Status::Ready) => Status::Updating,
                (_, _) => Status::Error,
            }
        } else {
            Status::Error
        }
    }
}
