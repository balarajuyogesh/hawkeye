// Based on https://gitlab.freedesktop.org/gstreamer/gstreamer-rs/-/blob/master/examples/src/bin/thumbnail.rs

use crate::img_detector::SlateDetector;
use color_eyre::Result;
use derive_more::{Display, Error};
use gst::prelude::*;
use gst::{gst_element_error, FlowError, FlowSuccess};
use gstreamer as gst;
use gstreamer_app as gst_app;

#[derive(Debug, Display, Error)]
#[display(fmt = "Received error from {}: {} (debug: {:?})", src, error, debug)]
struct ErrorMessage {
    src: String,
    error: String,
    debug: Option<String>,
    source: glib::Error,
}

pub fn create_pipeline(detector: SlateDetector, ingest_port: u32) -> Result<gst::Pipeline> {
    let (width, height) = detector.required_image_size();

    // Create our pipeline from a pipeline description string.
    let pipeline = gst::parse_launch(&format!(
        "udpsrc port={} caps = \"application/x-rtp, media=(string)video, clock-rate=(int)90000, encoding-name=(string)MP2T\" ! .recv_rtp_sink_0 rtpbin ! rtpmp2tdepay ! tsdemux ! avdec_h264 ! videoconvert ! videoscale ! capsfilter caps=\"video/x-raw, width={}, height={}\" ! pngenc snapshot=false ! appsink name=sink",
        ingest_port,
        width,
        height
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

    // Getting data out of the appsink is done by setting callbacks on it.
    // The appsink will then call those handlers, as soon as data is available.
    appsink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            // Add a handler to the "new-sample" signal.
            .new_sample(move |appsink| {
                // Pull the sample in question out of the appsink's buffer.
                let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                let buffer_ref = sample.get_buffer().ok_or_else(|| {
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
                let buffer = buffer_ref.map_readable().map_err(|_| {
                    gst_element_error!(
                        appsink,
                        gst::ResourceError::Failed,
                        ("Failed to map buffer readable")
                    );

                    gst::FlowError::Error
                })?;

                println!("Got an image..");

                if detector.is_match(buffer.as_slice()) {
                    println!("Found slate!");
                    return Err(FlowError::Eos);
                }

                Ok(FlowSuccess::Ok)
            })
            .build(),
    );

    Ok(pipeline)
}

pub fn main_loop(pipeline: gst::Pipeline) -> Result<()> {
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
                // happens immediately after matching the slate image because we return
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
