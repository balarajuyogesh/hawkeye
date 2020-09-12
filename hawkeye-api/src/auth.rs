use crate::config;
use warp::Filter;

pub fn verify() -> impl Filter<Extract = (), Error = warp::Rejection> + Clone {
    warp::header::<String>("authorization")
        .and_then(|auth_header: String| async move {
            match verify_token(auth_header) {
                Ok(_) => Ok(()),
                Err(_) => Err(warp::reject::custom(NoAuth)),
            }
        })
        .untuple_one()
}

fn verify_token(auth_header: String) -> Result<(), ()> {
    if auth_header.replace("Bearer ", "").as_str() == config::FIXED_TOKEN.as_str() {
        Ok(())
    } else {
        Err(())
    }
}

#[derive(Debug)]
pub struct NoAuth;

impl warp::reject::Reject for NoAuth {}
