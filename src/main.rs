// This example demonstrates how to get a raw video frame at a given position
// and then rescale and store it with the image crate:

// {uridecodebin} - {videoconvert} - {appsink}

// The appsink enforces RGBA so that the image crate can use it. The image crate also requires
// tightly packed pixels, which is the case for RGBA by default in GStreamer.
//
// Based on https://gitlab.freedesktop.org/gstreamer/gstreamer-rs/-/blob/master/examples/src/bin/thumbnail.rs

extern crate gstreamer as gst;
use gst::{gst_element_error, FlowSuccess, FlowError};
use gst::prelude::*;
extern crate gstreamer_app as gst_app;
extern crate gstreamer_video as gst_video;

extern crate image;

use anyhow::Error;
use derive_more::{Display, Error};
use std::time::Instant;
use dssim::*;
use imgref::*;
use std::path::Path;
use load_image::{ImageData, Image};
use image::{ImageEncoder, ColorType};

mod examples_common;

#[derive(Debug, Display, Error)]
#[display(fmt = "Missing element {}", _0)]
struct MissingElement(#[error(not(source))] &'static str);

#[derive(Debug, Display, Error)]
#[display(fmt = "Received error from {}: {} (debug: {:?})", src, error, debug)]
struct ErrorMessage {
    src: String,
    error: String,
    debug: Option<String>,
    source: glib::Error,
}

fn load_data(data: &[u8]) -> Result<ImgVec<RGBAPLU>, anyhow::Error> {
    let img = load_image::load_image_data(data, false)?;
    Ok(match_img_bitmap(img))
}

fn load_path<P: AsRef<Path>>(path: P) -> Result<ImgVec<RGBAPLU>, anyhow::Error> {
    let img = load_image::load_image(path.as_ref(), false)?;
    Ok(match_img_bitmap(img))
}

fn match_img_bitmap(img: Image) -> ImgVec<RGBAPLU> {
    match img.bitmap {
        ImageData::RGB8(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::RGB16(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::RGBA8(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::RGBA16(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::GRAY8(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::GRAY16(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::GRAYA8(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
        ImageData::GRAYA16(ref bitmap) => Img::new(bitmap.to_rgbaplu(), img.width, img.height),
    }
}

fn create_pipeline<P: AsRef<Path>>(ingest_port: u32, slate_img: P) -> Result<gst::Pipeline, Error> {
    gst::init()?;

    let algo = dssim::Dssim::new();
    let slate_img = load_path(slate_img)?;
    let slate = algo.create_image(&slate_img).unwrap();

    // Create our pipeline from a pipeline description string.
    println!("w: {}", slate_img.width());
    println!("h: {}", slate_img.height());
    let pipeline = gst::parse_launch(&format!(
        "udpsrc port={} address=0.0.0.0 caps = \"application/x-rtp, media=(string)video, clock-rate=(int)90000, encoding-name=(string)H264, payload=(int)96\" ! rtph264depay ! decodebin ! videoconvert ! videoscale ! capsfilter caps=\"video/x-raw, width={}, height={}\" ! pngenc snapshot=false ! appsink name=sink",
        ingest_port,
        slate_img.width(),
        slate_img.height()
    ))?
        .downcast::<gst::Pipeline>()
        .expect("Expected a gst::Pipeline");

    // Get access to the appsink element.
    let appsink = pipeline
        .get_by_name("sink")
        .expect("Sink element not found")
        .downcast::<gst_app::AppSink>()
        .expect("Sink element is expected to be an appsink!");

    // Don't synchronize on the clock, we only want a snapshot asap.
    appsink.set_property("sync", &false)?;

    let mut frame_num = 0u32;
    let mut started = Instant::now();

    // Getting data out of the appsink is done by setting callbacks on it.
    // The appsink will then call those handlers, as soon as data is available.
    appsink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            // Add a handler to the "new-sample" signal.
            .new_sample(move |appsink| {
                // Pull the sample in question out of the appsink's buffer.
                let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                let buffer = sample.get_buffer().ok_or_else(|| {
                    gst_element_error!(
                        appsink,
                        gst::ResourceError::Failed,
                        ("Failed to get buffer from appsink")
                    );

                    gst::FlowError::Error
                })?;

                // At this point, buffer is only a reference to an existing memory region somewhere.
                // When we want to access its content, we have to map it while requesting the required
                // mode of access (read, read/write).
                // This type of abstraction is necessary, because the buffer in question might not be
                // on the machine's main memory itself, but rather in the GPU's memory.
                // So mapping the buffer makes the underlying memory region accessible to us.
                // See: https://gstreamer.freedesktop.org/documentation/plugin-development/advanced/allocation.html
                let map = buffer.map_readable().map_err(|_| {
                    gst_element_error!(
                        appsink,
                        gst::ResourceError::Failed,
                        ("Failed to map buffer readable")
                    );

                    gst::FlowError::Error
                })?;
                let frame_img = load_data(map.as_slice()).unwrap();
                let frame = algo.create_image(&frame_img).unwrap();

                let (res, _) = algo.compare(&slate, frame);
                let val: f64 = res.into();
                let val = (val * 1000f64) as u32;

                if val <= 900u32 {
                    println!("Found slate!");
                    return Err(FlowError::Eos);
                }

                frame_num += 1;
                // println!("Have video frame, frame number: {}, comparison: {} , time elapsed since last frame capture: {}ms", frame_num, val, started.elapsed().as_millis());
                started = Instant::now();
                Ok(FlowSuccess::Ok)
            })
            .build(),
    );

    Ok(pipeline)
}

fn main_loop(pipeline: gst::Pipeline) -> Result<(), Error> {
    pipeline.set_state(gst::State::Paused)?;

    let bus = pipeline
        .get_bus()
        .expect("Pipeline without bus. Shouldn't happen!");

    pipeline.set_state(gst::State::Playing)?;
    println!("Pipeline started...");

    for msg in bus.iter_timed(gst::CLOCK_TIME_NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::AsyncDone(..) => {}
            MessageView::Eos(..) => {
                // The End-of-stream message is posted when the stream is done, which in our case
                // happens immediately after creating the thumbnail because we return
                // gst::FlowError::Eos then.
                println!("Got Eos message, done");
                break;
            }
            MessageView::Error(err) => {
                pipeline.set_state(gst::State::Null)?;
                return Err(ErrorMessage {
                    src: msg
                        .get_src()
                        .map(|s| String::from(s.get_path_string()))
                        .unwrap_or_else(|| String::from("None")),
                    error: err.get_error().to_string(),
                    debug: err.get_debug(),
                    source: err.get_error(),
                }
                    .into());
            }
            _ => (),
        }
    }

    pipeline.set_state(gst::State::Null)?;

    Ok(())
}

fn example_main() {
    use std::env;

    let mut args = env::args();

    // Parse commandline arguments: input URI, position in seconds, output path
    let _arg0 = args.next().unwrap();
    let slate_path = args.next().expect("No slate path provided on the commandline");
    println!("Slate path: {}", slate_path);

    let ingest_port = args
        .next()
        .expect("No ingest port provided on the commandline");
    println!("Ingest port: {}", ingest_port);
    let ingest_port: u32 = ingest_port.parse().expect("Ingest port is not a number");

    match create_pipeline(ingest_port, slate_path).and_then(|pipeline| main_loop(pipeline)) {
        Ok(r) => r,
        Err(e) => eprintln!("Error! {}", e),
    }
}

fn main() {
    // tutorials_common::run is only required to set up the application environment on macOS
    // (but not necessary in normal Cocoa applications where this is set up automatically)
    examples_common::run(example_main);
}

#[cfg(test)]
mod test {
    use dssim::*;
    use imgref::*;
    use std::path::Path;
    use load_image::ImageData;
    use super::*;

    #[test]
    fn compare_equal_images() {
        let slate_img = load_path("../slate.jpg").unwrap();

        let algo = dssim::Dssim::new();
        let slate = algo.create_image(&slate_img).unwrap();

        let (res, _) = algo.compare(&slate, slate.clone());
        let val: f64 = res.into();

        assert_eq!((val * 1000f64) as u32, 0u32);
    }

    #[test]
    fn compare_diff_images() {
        let slate_img = load_path("../slate.jpg").unwrap();
        let frame_img = load_path("../non-slate.jpg").unwrap();

        let algo = dssim::Dssim::new();
        let slate = algo.create_image(&slate_img).unwrap();
        let frame = algo.create_image(&frame_img).unwrap();

        let (res, _) = algo.compare(&slate, frame);
        let val: f64 = res.into();

        assert_eq!((val * 1000f64) as u32, 7417u32);
    }
}
