mod actions;
mod config;
mod img_detector;
mod models;
mod video_stream;

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

    thread::spawn(move || {
        let mut runtime = actions::Runtime::new(receiver, Vec::new());

        debug!("Starting actions runtime..");
        runtime
            .run_blocking()
            .expect("Actions runtime ended unexpectedly!");
    });

    create_pipeline(detector, config.ingest_port, sender.clone())
        .and_then(|pipeline| main_loop(pipeline, sender))?;

    Ok(())
}
