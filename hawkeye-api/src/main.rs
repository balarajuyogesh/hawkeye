mod filters;
mod handlers;

use kube::Client;
use std::env;
use warp::Filter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if env::var_os("RUST_LOG").is_none() {
        // Set `RUST_LOG=watchers=debug` to see debug logs,
        // this only shows access logs.
        env::set_var("RUST_LOG", "watchers=info");
    }
    pretty_env_logger::init();

    let client = Client::try_default().await?;

    let v1 = filters::v1(client);
    let routes = v1.with(warp::log("watchers"));
    warp::serve(routes).run(([127, 0, 0, 1], 8080)).await;

    Ok(())
}
