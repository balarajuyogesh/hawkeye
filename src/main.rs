mod actions;
mod config;
mod img_detector;
mod video_stream;

use crate::actions::HttpCallManager;
use crate::config::AppConfig;
use crate::img_detector::SlateDetector;
use crate::video_stream::{create_pipeline, main_loop};
use color_eyre::Result;
use gstreamer as gst;
use log::{debug, info};
use std::sync::mpsc::channel;
use std::thread;
use structopt::StructOpt;

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
