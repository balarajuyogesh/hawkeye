use lazy_static::lazy_static;

const NAMESPACE_ENV: &str = "HAWKEYE_NAMESPACE";
const DOCKER_IMAGE_ENV: &str = "HAWKEYE_DOCKER_IMAGE";

lazy_static! {
    pub static ref NAMESPACE: String =
        std::env::var(NAMESPACE_ENV).unwrap_or_else(|_| "default".into());
    pub static ref DOCKER_IMAGE: String =
        std::env::var(DOCKER_IMAGE_ENV).unwrap_or_else(|_| "hawkeye-dev:latest".into());
}
