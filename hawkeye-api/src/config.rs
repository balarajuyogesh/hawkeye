use lazy_static::lazy_static;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::iter;

const NAMESPACE_ENV: &str = "HAWKEYE_NAMESPACE";
const DOCKER_IMAGE_ENV: &str = "HAWKEYE_DOCKER_IMAGE";
const FIXED_TOKEN_ENV: &str = "HAWKEYE_FIXED_TOKEN";

lazy_static! {
    pub static ref NAMESPACE: String =
        std::env::var(NAMESPACE_ENV).unwrap_or_else(|_| "default".into());
    pub static ref DOCKER_IMAGE: String =
        std::env::var(DOCKER_IMAGE_ENV).unwrap_or_else(|_| "hawkeye-dev:latest".into());
    pub static ref FIXED_TOKEN: String =
        std::env::var(FIXED_TOKEN_ENV).unwrap_or_else(|_| gen_token());
}

fn gen_token() -> String {
    let mut rng = thread_rng();
    let n: usize = rng.gen_range(20, 30);
    let random_token: String = iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .take(n)
        .collect();

    std::env::set_var(FIXED_TOKEN_ENV, &random_token);
    log::info!(
        "Missing security configuration, default token is: {}",
        random_token
    );
    random_token
}
