use crate::handlers;
use hawkeye_core::models::Watcher;
use kube::Client;
use warp::Filter;

/// API root for v1
pub fn v1(
    client: Client,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    watchers_list(client.clone())
        .or(watcher_create(client.clone()))
        .or(watcher_get(client.clone()))
        .or(watcher_delete(client.clone()))
        .or(watcher_start(client.clone()))
        .or(watcher_stop(client.clone()))
    // .or(watcher_update())
    // .or(watcher_delete())
}

/// GET /v1/watchers
pub fn watchers_list(
    client: Client,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("v1" / "watchers")
        .and(warp::get())
        .and(with_client(client))
        .and_then(handlers::list_watchers)
}

/// POST /v1/watchers
pub fn watcher_create(
    client: Client,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("v1" / "watchers")
        .and(warp::post())
        .and(json_body())
        .and(with_client(client))
        .and_then(handlers::create_watcher)
}

/// GET /v1/watchers/{id}
pub fn watcher_get(
    client: Client,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("v1" / "watchers" / String)
        .and(warp::get())
        .and(with_client(client))
        .and_then(handlers::get_watcher)
}

/// DELETE /v1/watchers/{id}
pub fn watcher_delete(
    client: Client,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("v1" / "watchers" / String)
        .and(warp::delete())
        .and(with_client(client))
        .and_then(handlers::delete_watcher)
}

/// POST /v1/watchers/{id}/start
pub fn watcher_start(
    client: Client,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("v1" / "watchers" / String / "start")
        .and(warp::post())
        .and(with_client(client))
        .and_then(handlers::start_watcher)
}

/// POST /v1/watchers/{id}/stop
pub fn watcher_stop(
    client: Client,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path!("v1" / "watchers" / String / "stop")
        .and(warp::post())
        .and(with_client(client))
        .and_then(handlers::stop_watcher)
}

fn with_client(
    client: Client,
) -> impl Filter<Extract = (Client,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || client.clone())
}

fn json_body() -> impl Filter<Extract = (Watcher,), Error = warp::Rejection> + Clone {
    // When accepting a body, we want a JSON body
    // (and to reject huge payloads)...
    warp::body::content_length_limit(1024 * 16).and(warp::body::json())
}
