mod config;
mod img_detector;
mod video_stream;

use color_eyre::Result;
use config::AppConfig;
use gstreamer as gst;
use img_detector::SlateDetector;
use log::{debug, info};
use reqwest::blocking::Client;
use reqwest::Method;
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use structopt::StructOpt;
use video_stream::{create_pipeline, main_loop};
use reqwest::header::CONTENT_TYPE;

struct HttpCallManager {
    client: Client,
    url: String,
    method: Method,
    username: String,
    password: String,
    payload: String,
    last_call: Option<Instant>,
    receiver: Receiver<bool>,
}

impl HttpCallManager {
    fn new(
        url: String,
        method: Method,
        username: String,
        password: String,
        payload: String,
        receiver: Receiver<bool>,
    ) -> Self {
        let client = Client::new();
        Self {
            client,
            url,
            method,
            username,
            password,
            payload,
            last_call: None,
            receiver,
        }
    }

    fn run_blocking(&mut self) -> Result<()> {
        loop {
            if self.receiver.recv()? {
                break;
            }

            if self.last_call.is_some()
                && self.last_call.unwrap().elapsed() < Duration::from_secs(145)
            {
                continue;
            }

            let _ = self
                .client
                .request(self.method.clone(), &self.url)
                .basic_auth(&self.username, Some(&self.password))
                .header(CONTENT_TYPE, "application/json")
                .body(self.payload.clone())
                .timeout(Duration::from_secs(5))
                .send()?
                .error_for_status()?;

            self.last_call = Some(Instant::now());
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    pretty_env_logger::init();

    let config: AppConfig = AppConfig::from_args();

    info!("Initializing GStreamer..");
    gst::init().expect("Could not initialize GStreamer!");

    let detector = SlateDetector::new(config.slate_path)?;
    let (sender, receiver) = channel();

    let auth = std::env::var("API_AUTH").expect("Variable API_AUTH must be provided");
    let user_pass: Vec<&str> = auth.split(":").collect();
    if user_pass.len() != 2 {
        panic!("Username and password in the env variable API_AUTH must be provided in format 'username:password'");
    }

    let mut action = HttpCallManager::new(
        config.url,
        config.method,
        user_pass[0].to_string(),
        user_pass[1].to_string(),
        config.payload,
        receiver,
    );

    thread::spawn(move || {
        debug!("Running actions manager..");
        action.run_blocking().expect("Did not finish successfully!");
    });

    create_pipeline(detector, config.ingest_port, sender.clone())
        .and_then(|pipeline| main_loop(pipeline, sender))?;

    Ok(())
}
