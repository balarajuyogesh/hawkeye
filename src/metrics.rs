use lazy_static::lazy_static;
use log::debug;
use prometheus::{self, Encoder, TextEncoder};
use prometheus::{register_histogram, register_int_counter, Histogram, IntCounter};

lazy_static! {
    pub static ref FOUND_SLATE_COUNTER: IntCounter = register_int_counter!(
        "slate_found_in_stream",
        "Number of times a slate image was found in the stream"
    )
    .unwrap();
    pub static ref FOUND_CONTENT_COUNTER: IntCounter = register_int_counter!(
        "content_found_in_stream",
        "Number of times the content was found in the stream"
    )
    .unwrap();
    pub static ref SIMILARITY_EXECUTION_COUNTER: IntCounter = register_int_counter!(
        "similarity_execution",
        "Number of times we searched for slate in the stream"
    )
    .unwrap();
    pub static ref HTTP_CALL_DURATION: Histogram = register_histogram!(
        "http_call_action_execution_seconds",
        "Seconds it took to execute the HTTP call"
    )
    .unwrap();
    pub static ref HTTP_CALL_SUCCESS_COUNTER: IntCounter = register_int_counter!(
        "http_call_success",
        "Number of times the HTTP call executed successfully"
    )
    .unwrap();
    pub static ref HTTP_CALL_ERROR_COUNTER: IntCounter = register_int_counter!(
        "http_call_error",
        "Number of times the HTTP call returned an HTTP error status code"
    )
    .unwrap();
}

pub fn get_metric_contents() -> String {
    debug!("Metrics endpoint called!");
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();

    let metric_families = prometheus::gather();
    encoder.encode(&metric_families, &mut buffer).unwrap();

    String::from_utf8(buffer).unwrap()
}
