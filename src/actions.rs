use crate::video_stream::VideoMode;
use color_eyre::Result;
use log::{debug, error, info};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use reqwest::Method;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

pub struct HttpCallManager {
    client: Client,
    url: String,
    method: Method,
    username: String,
    password: String,
    payload: String,
    last_call: Option<Instant>,
    last_mode: Option<VideoMode>,
    receiver: Receiver<Option<VideoMode>>,
}

impl HttpCallManager {
    pub fn new(
        url: String,
        method: Method,
        username: String,
        password: String,
        payload: String,
        receiver: Receiver<Option<VideoMode>>,
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
            last_mode: None,
            receiver,
        }
    }

    pub fn run_blocking(&mut self) -> Result<()> {
        loop {
            match self.receiver.recv()? {
                None => break,
                Some(mode) => {
                    if self.last_call.is_some()
                        && self.last_call.as_ref().unwrap().elapsed() < Duration::from_secs(10)
                    {
                        debug!("Wait at least 10 seconds before any kind of transitioning..");
                        continue;
                    }
                    debug!("{:?} {:?}", &self.last_mode, &mode);
                    match (&self.last_mode, &mode) {
                        (Some(VideoMode::Content), VideoMode::Slate) => {
                            self.transition_to_slate();
                        }
                        (Some(VideoMode::Slate), VideoMode::Content) => {
                            self.transition_to_content();
                        }
                        (None, VideoMode::Slate) => {
                            self.transition_to_slate();
                        }
                        (None, VideoMode::Content) => {
                            self.transition_to_content();
                        }
                        (Some(VideoMode::Slate), VideoMode::Slate) => {}
                        (Some(VideoMode::Content), VideoMode::Content) => {}
                    }
                    debug!("last_mode = {:?}", &mode);
                    self.last_mode = Some(mode);
                }
            }
        }
        Ok(())
    }

    fn transition_to_slate(&mut self) {
        let start_api_call = Instant::now();

        match self
            .client
            .request(self.method.clone(), &self.url)
            .basic_auth(&self.username, Some(&self.password))
            .header(CONTENT_TYPE, "application/json")
            .body(self.payload.clone())
            .timeout(Duration::from_secs(10))
            .send()
        {
            Ok(response) => match response.error_for_status() {
                Ok(_) => {
                    if let Some(last_call) = &self.last_call {
                        info!(
                            "Transitioning to SLATE after: {}ms",
                            last_call.elapsed().as_millis()
                        );
                    }
                    self.last_call = Some(Instant::now());
                }
                Err(err) => error!(
                    "transition_to_slate: Received error from the backend API: {}",
                    err
                ),
            },
            Err(err) => error!(
                "transition_to_slate: Problem while calling backend API: {}",
                err
            ),
        }

        info!(
            "HTTP call to backend API took: {}ms",
            start_api_call.elapsed().as_millis()
        );
    }

    fn transition_to_content(&mut self) {
        let start_api_call = Instant::now();

        match self
            .client
            .request(Method::DELETE, &self.url)
            .basic_auth(&self.username, Some(&self.password))
            .header(CONTENT_TYPE, "application/json")
            .body(self.payload.clone())
            .timeout(Duration::from_secs(10))
            .send()
        {
            Ok(response) => match response.error_for_status() {
                Ok(_) => {
                    if let Some(last_call) = &self.last_call {
                        info!(
                            "Transitioning to CONTENT after: {}ms",
                            last_call.elapsed().as_millis()
                        );
                    }
                    self.last_call = Some(Instant::now());
                }
                Err(err) => error!(
                    "transition_to_content: Received error from the backend API: {}",
                    err
                ),
            },
            Err(err) => error!(
                "transition_to_content: Problem while calling backend API: {}",
                err
            ),
        }

        info!(
            "HTTP call to backend API took: {}ms",
            start_api_call.elapsed().as_millis()
        );
    }
}
