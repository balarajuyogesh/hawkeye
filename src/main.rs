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

fn create_pipeline(uri: String) -> Result<gst::Pipeline, Error> {
    gst::init()?;

    // Create our pipeline from a pipeline description string.
    let pipeline = gst::parse_launch(&format!(
        "uridecodebin uri={} ! videoconvert ! appsink name=sink",
        uri
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
    appsink.set_property("sync", &false).unwrap();

    // Tell the appsink what format we want.
    // This can be set after linking the two objects, because format negotiation between
    // both elements will happen during pre-rolling of the pipeline.
    appsink.set_caps(Some(
        &gst::Caps::builder("video/x-raw")
            .field("format", &gst_video::VideoFormat::Rgba.to_str())
            .build(),
    ));

    let mut frame_num = 0u32;
    let mut started = Instant::now();

    let algo = dssim::Dssim::new();
    let slate_img = load_path("../slate_240px.jpg").unwrap();
    let slate = algo.create_image(&slate_img).unwrap();

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

                let caps = sample.get_caps().expect("Sample without caps");
                let info = gst_video::VideoInfo::from_caps(&caps).expect("Failed to parse caps");

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

                // Create an ImageBuffer around the borrowed video frame data from GStreamer.
                let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(
                    info.width(),
                    info.height(),
                    map,
                ).expect("Failed to create ImageBuffer, probably a stride mismatch");

                // Calculate a target width/height that keeps the display aspect ratio while having
                // a height of 240 pixels
                let display_aspect_ratio = (info.width() as f64 * *info.par().numer() as f64)
                    / (info.height() as f64 * *info.par().denom() as f64);
                let target_height = 240;
                let target_width = target_height as f64 * display_aspect_ratio;

                // Scale image to our target dimensions
                let scaled_img =
                    image::imageops::thumbnail(&img, target_width as u32, target_height as u32).into_raw();

                let mut buffer = Vec::new();
                image::png::PNGEncoder::new(&mut buffer).write_image(&scaled_img, target_width as u32, target_height as u32, ColorType::Rgba8).unwrap();

                let frame_img = load_data(&buffer).unwrap();
                let frame = algo.create_image(&frame_img).unwrap();

                let (res, _) = algo.compare(&slate, frame);
                let val: f64 = res.into();
                let val = (val * 1000f64) as u32;

                if val <= 900u32 {
                    println!("Found slate!");
                    return Err(FlowError::Eos);
                }

                frame_num += 1;
                println!("Have video frame, frame number: {}, comparison: {} , time elapsed since last frame capture: {}ms", frame_num, val, started.elapsed().as_millis());
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

    let mut seeked = false;

    for msg in bus.iter_timed(gst::CLOCK_TIME_NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::AsyncDone(..) => {
                if !seeked {
                    pipeline.set_state(gst::State::Playing)?;
                    seeked = true;
                } else {
                    println!("Got second AsyncDone message, seek finished");
                }
            }
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
    let uri = args
        .next()
        .expect("No input URI provided on the commandline");

    match create_pipeline(uri).and_then(|pipeline| main_loop(pipeline)) {
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
