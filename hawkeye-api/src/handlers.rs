use hawkeye_core::models::{Status, Watcher};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{ConfigMap, Service};
use kube::api::{ListParams, PostParams, PatchParams};
use kube::{Api, Client};
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use uuid::Uuid;
use warp::http::StatusCode;
use warp::reply;

const NAMESPACE_ENV: &'static str = "HAWKEYE_NAMESPACE";
const DOCKER_IMAGE_ENV: &'static str = "HAWKEYE_DOCKER_IMAGE";

pub async fn list_watchers(client: Client) -> Result<impl warp::Reply, Infallible> {
    let namespace = std::env::var(NAMESPACE_ENV).unwrap_or("default".into());

    let lp = ListParams::default()
        .labels("app=hawkeye,watcher_id")
        .timeout(10);

    // Get all deployments we know, we want to return the status of each watcher
    let deployments_client: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
    let deployments = deployments_client.list(&lp).await.unwrap();
    let mut deployments_index = HashMap::new();
    for deploy in deployments.items {
        if let Some(watcher_id) = deploy.metadata.labels.as_ref().unwrap().get("watcher_id") {
            let status = if deploy
                .status
                .as_ref()
                .unwrap()
                .available_replicas
                .unwrap_or(0)
                > 0
            {
                // TODO: if running we need to get the endpoint from the service
                Status::Running
            } else {
                // TODO: We should check also if the Service is created
                // TODO: We need to get the endpoint from the service
                Status::Ready
            };
            deployments_index.insert(watcher_id.clone(), status);
        }
    }

    // We use the ConfigMap as source of truth for what are the watchers we have
    let config_maps_client: Api<ConfigMap> = Api::namespaced(client.clone(), &namespace);
    let config_maps = config_maps_client.list(&lp).await.unwrap();

    let mut watchers: Vec<Watcher> = Vec::new();
    for config in config_maps.items {
        let data = config.data.unwrap();
        let mut w: Watcher = serde_json::from_str(data.get("watcher.json").unwrap()).unwrap();
        let calculated_status = if let Some(status) =
            deployments_index.get(w.id.as_ref().unwrap_or(&"undefined".to_string()))
        {
            status.clone()
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

    let namespace = std::env::var(NAMESPACE_ENV).unwrap_or("default".into());
    let docker_image = std::env::var(DOCKER_IMAGE_ENV).unwrap_or("hawkeye-dev:0.0.5".into());

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

    let config_maps: Api<ConfigMap> = Api::namespaced(client.clone(), &namespace);
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
                            "image": docker_image,
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
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
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

    let services: Api<Service> = Api::namespaced(client.clone(), &namespace);
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
    let namespace = std::env::var(NAMESPACE_ENV).unwrap_or("default".into());

    let deployments_client: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
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
    let config_maps_client: Api<ConfigMap> = Api::namespaced(client.clone(), &namespace);
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
    let calculated_status = if let Some(status) = deployment.status {
        w.source.ingest_ip = None;
        if status.available_replicas.unwrap_or(0) > 0 {
            // TODO: if running we need to get the endpoint from the service
            Status::Running
        } else {
            Status::Ready
        }
    } else {
        Status::Error
    };
    w.status = Some(calculated_status);

    Ok(reply::with_status(reply::json(&w), StatusCode::OK))
}


pub async fn start_watcher(id: String, client: Client) -> Result<impl warp::Reply, Infallible> {
    let namespace = std::env::var(NAMESPACE_ENV).unwrap_or("default".into());

    let deployments_client: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
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
    if let Some(status) = deployment.status {
        if status.available_replicas.unwrap_or(0) > 0 {
            Ok(reply::with_status(reply::json(&json!({
                "message": "Watcher is already running"
            })), StatusCode::CONFLICT))
        } else {
            // Start watcher / replicas to 1
            let patch_params = PatchParams::default();
            let fs = json!({
                "spec": { "replicas": 1 }
            });
            let o = deployments_client
                .patch_scale(&deployment.metadata.name.unwrap(), &patch_params, serde_json::to_vec(&fs).unwrap())
                .await.unwrap();
            log::debug!("Scale status: {:?}", o);

            Ok(reply::with_status(reply::json(&json!({
                "message": "Watcher is starting"
            })), StatusCode::OK))
        }
    } else {
        Ok(reply::with_status(reply::json(&json!({
            "message": "Watcher is in failed state"
        })), StatusCode::BAD_REQUEST))
    }
}

pub async fn stop_watcher(id: String, client: Client) -> Result<impl warp::Reply, Infallible> {
    let namespace = std::env::var(NAMESPACE_ENV).unwrap_or("default".into());

    let deployments_client: Api<Deployment> = Api::namespaced(client.clone(), &namespace);
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
    if let Some(status) = deployment.status {
        if status.available_replicas.unwrap_or(0) == 0 {
            Ok(reply::with_status(reply::json(&json!({
                "message": "Watcher is already stopped"
            })), StatusCode::OK))
        } else {
            // Stop watcher / replicas to 0
            let patch_params = PatchParams::default();
            let fs = json!({
                "spec": { "replicas": 0 }
            });
            let o = deployments_client
                .patch_scale(&deployment.metadata.name.unwrap(), &patch_params, serde_json::to_vec(&fs).unwrap())
                .await.unwrap();
            log::debug!("Scale status: {:?}", o);

            Ok(reply::with_status(reply::json(&json!({
                "message": "Watcher is stopping"
            })), StatusCode::OK))
        }
    } else {
        Ok(reply::with_status(reply::json(&json!({
            "message": "Watcher is in failed state"
        })), StatusCode::BAD_REQUEST))
    }
}
