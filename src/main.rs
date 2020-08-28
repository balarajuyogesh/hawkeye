mod config;
mod img_detector;
mod video_stream;

use color_eyre::Result;
use config::AppConfig;
use gstreamer as gst;
use img_detector::SlateDetector;
use structopt::StructOpt;
use video_stream::{create_pipeline, main_loop};

fn main() -> Result<()> {
    let config: AppConfig = AppConfig::from_args();

    gst::init().expect("Could not initialize GStreamer!");

    let detector = SlateDetector::new(config.slate_path)?;

    create_pipeline(detector, config.ingest_port).and_then(main_loop)?;

    Ok(())
}
