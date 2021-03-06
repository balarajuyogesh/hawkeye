// Based on https://gitlab.freedesktop.org/gstreamer/gstreamer-rs/-/blob/master/examples/src/bin/thumbnail.rs

use crate::img_detector::SlateDetector;
use crate::metrics::{FOUND_CONTENT_COUNTER, FOUND_SLATE_COUNTER, SIMILARITY_EXECUTION_COUNTER};
use color_eyre::Result;
use concread::CowCell;
use derive_more::{Display, Error};
use gst::gst_element_error;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use hawkeye_core::models::{Codec, Container, VideoMode};
use lazy_static::lazy_static;
use log::{debug, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

lazy_static! {
    pub(crate) static ref LATEST_FRAME: CowCell<Option<Vec<u8>>> = CowCell::new(None);
}

#[derive(Debug, Display, Error)]
#[display(fmt = "Received error from {}: {} (debug: {:?})", src, error, debug)]
struct ErrorMessage {
    src: String,
    error: String,
    debug: Option<String>,
    source: glib::Error,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Event {
    Terminate,
    Mode(VideoMode),
}

pub fn create_pipeline(
    detector: SlateDetector,
    ingest_port: u32,
    container: Container,
    codec: Codec,
    action_sink: Sender<Event>,
) -> Result<gst::Pipeline> {
    let (width, height) = detector.required_image_size();

    let pipeline_description = match (container, codec) {
        (Container::MpegTs, Codec::H264) => format!(
            "udpsrc port={} caps=\"application/x-rtp, media=(string)video, clock-rate=(int)90000, encoding-name=(string)MP2T, payload=(int)33\" ! .recv_rtp_sink_0 rtpbin ! rtpmp2tdepay ! tsdemux ! h264parse ! avdec_h264 ! videoconvert ! videoscale ! capsfilter caps=\"video/x-raw, width={}, height={}\" ! pngenc snapshot=false ! appsink name=sink",
            ingest_port,
            width,
            height
        ),
        (Container::RawVideo, Codec::H264) => format!(
            "udpsrc port={} caps = \"application/x-rtp, media=(string)video, clock-rate=(int)90000, encoding-name=(string)H264, payload=(int)96\" ! rtph264depay ! decodebin ! videoconvert ! videoscale ! capsfilter caps=\"video/x-raw, width={}, height={}\" ! pngenc snapshot=false ! appsink name=sink",
            ingest_port,
            width,
            height
        ),
        (_, _) => {
            return Err(color_eyre::eyre::eyre!("Container ({:?}) and Codec ({:?}) not available", container, codec));
        }
    };

    // Create our pipeline from a pipeline description string.
    debug!("Creating GStreamer Pipeline..");
    let pipeline = gst::parse_launch(&pipeline_description)?
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
                // Prevents reading twice.
                let local_buffer = buffer.to_vec();

                let is_match = detector.is_match(local_buffer.as_slice());

                {
                    // Save latest image bytes
                    let mut write_txn = LATEST_FRAME.write();
                    // Moves the local buffer
                    *write_txn = Some(local_buffer);
                    write_txn.commit();
                }

                if is_match {
                    debug!("Found slate image in video stream!");
                    FOUND_SLATE_COUNTER.inc();
                    action_sink.send(Event::Mode(VideoMode::Slate)).unwrap();
                } else {
                    FOUND_CONTENT_COUNTER.inc();
                    action_sink.send(Event::Mode(VideoMode::Content)).unwrap();
                    debug!("Did not find slate..");
                }
                SIMILARITY_EXECUTION_COUNTER.inc();

                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    Ok(pipeline)
}

pub fn main_loop(
    pipeline: gst::Pipeline,
    running: Arc<AtomicBool>,
    action_sink: Sender<Event>,
) -> Result<()> {
    pipeline.set_state(gst::State::Paused)?;

    let bus = pipeline
        .get_bus()
        .expect("Pipeline without bus. Shouldn't happen!");

    pipeline.set_state(gst::State::Playing)?;
    info!("Pipeline started...");

    while running.load(Ordering::SeqCst) {
        for msg in bus.iter_timed(gst::ClockTime::from_seconds(1)) {
            use gst::MessageView;

            match msg.view() {
                MessageView::AsyncDone(..) => {}
                MessageView::Eos(..) => {
                    // The End-of-stream message is posted when the stream is done, which in our case
                    // happens immediately after matching the slate image because we return
                    // gst::FlowError::Eos then.
                    running.store(false, Ordering::SeqCst);
                    info!("Got Eos message, done");
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
    }

    info!("Stopping pipeline gracefully!");
    action_sink.send(Event::Terminate)?;
    pipeline.set_state(gst::State::Null)?;

    Ok(())
}
