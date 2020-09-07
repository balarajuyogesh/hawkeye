use crate::models::{Action, HttpAuth, HttpCall, VideoMode};
use crate::video_stream::Event;
use color_eyre::Result;
use log::{debug, error, info};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crate::models;
#[cfg(test)]
use sn_fake_clock::FakeClock as Instant;
#[cfg(not(test))]
use std::time::Instant;

/// Abstract behaviour of an Hawkeye Action.
///
/// New actions can implement this trait and will be ready to be used with video watchers.
impl Action {
    fn execute(&mut self) -> Result<()> {
        match self {
            Action::HttpCall(a) => a.execute(),

            #[cfg(test)]
            Action::FakeAction(a) => a.execute(),
        }
    }
}

/// Represents a sequence of video modes.
#[derive(Clone, Eq, PartialEq)]
pub struct Transition(VideoMode, VideoMode);

impl Transition {
    pub fn new(start: VideoMode, end: VideoMode) -> Self {
        Self(start, end)
    }
}

/// Manages the execution of an `Action` based on a flow of `VideoMode`s.
///
/// The `ActionExecutor` abstracts the logic of execution that is inherent to all `Action` types.
pub struct ActionExecutor {
    transition: Transition,
    action: Action,
    last_mode: Option<VideoMode>,
    last_call: Option<Instant>,
}

impl ActionExecutor {
    /// Creates a new `ActionExecutor` instance
    pub fn new(transition: Transition, action: Action) -> Self {
        Self {
            transition,
            action,
            last_mode: None,
            last_call: None,
        }
    }

    // Manage the execution of an action based on the provided video mode.
    pub fn execute(&mut self, mode: VideoMode) {
        if let Some(result) = self.call_action(mode) {
            match result {
                Ok(_) => self.last_call = Some(Instant::now()),
                Err(err) => error!(
                    "Error while processing action in mode {:?}: {:#}",
                    mode, err
                ),
            }
        }
    }

    /// Executes the action if the video mode matches the transition and if the action is
    /// allowed to run.
    fn call_action(&mut self, mode: VideoMode) -> Option<Result<()>> {
        match self.last_mode {
            None => {
                self.last_mode = Some(mode);
                None
            }
            Some(last_mode) => {
                if Transition(last_mode, mode) == self.transition && self.allowed_to_run() {
                    Some(self.action.execute())
                } else {
                    None
                }
            }
        }
    }

    /// Check if the action is allowed to run within the timeframe it was called.
    ///
    /// We need to limit the action frequency since the source of video mode does not guarantee the
    /// ordering of events.
    fn allowed_to_run(&self) -> bool {
        match &self.last_call {
            None => true,
            Some(last_call) => last_call.elapsed() > Duration::from_secs(5),
        }
    }
}

impl From<models::Transition> for Vec<ActionExecutor> {
    fn from(transition: models::Transition) -> Self {
        let target_transition = Transition(transition.from, transition.to);
        transition
            .actions
            .into_iter()
            .map(|action| ActionExecutor::new(target_transition.clone(), action))
            .collect()
    }
}

pub struct Runtime {
    receiver: Receiver<Event>,
    actions: Vec<ActionExecutor>,
}

impl Runtime {
    pub fn new(receiver: Receiver<Event>, processors: Vec<ActionExecutor>) -> Self {
        Runtime {
            receiver,
            actions: processors,
        }
    }

    pub fn run_blocking(&mut self) -> Result<()> {
        loop {
            match self.receiver.recv()? {
                Event::Terminate => break,
                Event::Mode(mode) => {
                    for p in self.actions.iter_mut() {
                        p.execute(mode);
                    }
                }
            }
        }
        Ok(())
    }
}

impl HttpCall {
    fn execute(&mut self) -> Result<()> {
        let start_api_call = Instant::now();

        let method = self.method.to_string();
        let mut request = ureq::request(&method, self.url.as_str());

        request.timeout_connect(500);

        if let Some(HttpAuth::Basic { username, password }) = &self.authorization {
            request.auth(username, password);
        }

        if let Some(timeout) = &self.timeout {
            request.timeout(Duration::from_secs(*timeout as u64));
        }

        if let Some(headers) = &self.headers {
            for (k, v) in headers.iter() {
                request.set(k, v);
            }
        }

        let response = match self.body.as_ref() {
            Some(data) => request.send_string(data),
            None => request.call(),
        };
        if response.ok() {
            debug!(
                "Successfully called backend API {}",
                response.into_string()?
            );
        } else {
            debug!(
                "Error while calling backend API ({}): {}",
                response.status(),
                response.into_string()?
            );
        }

        info!(
            "HTTP call to backend API took: {}ms",
            start_api_call.elapsed().as_millis()
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models;
    use crate::models::{FakeAction, HttpAuth, HttpMethod};
    use mockito::{mock, server_url, Matcher};
    use sn_fake_clock::FakeClock;
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::mpsc::channel;

    fn sleep(d: Duration) {
        FakeClock::advance_time(d.as_millis() as u64);
    }

    #[test]
    fn executor_slate_action_called_when_transition_content_to_slate() {
        let called = Rc::new(Cell::new(false));
        let fake_action = FakeAction {
            called: called.clone(),
            execute_returns: Some(Ok(())),
        };
        let mut executor = ActionExecutor::new(
            Transition::new(VideoMode::Content, VideoMode::Slate),
            Action::FakeAction(fake_action),
        );
        executor.execute(VideoMode::Content);
        // Didn't call since it was the first state found
        assert_eq!(called.get(), false);

        executor.execute(VideoMode::Slate);
        // Must be called since we had a state transition that matches what we defined in the executor
        assert_eq!(called.get(), true);
    }

    #[test]
    fn executor_slate_action_cannot_be_called_twice_in_short_timeframe() {
        let called = Rc::new(Cell::new(false));
        let fake_action = FakeAction {
            called: called.clone(),
            execute_returns: Some(Ok(())),
        };
        let mut executor = ActionExecutor::new(
            Transition::new(VideoMode::Content, VideoMode::Slate),
            Action::FakeAction(fake_action),
        );
        executor.execute(VideoMode::Content);
        executor.execute(VideoMode::Slate);
        // Must be called since we had a state transition that matches what we defined in the executor
        assert_eq!(called.get(), true);
        // Reset state of our mock to "not called"
        called.set(false);
        executor.execute(VideoMode::Content);
        executor.execute(VideoMode::Slate);
        assert_eq!(called.get(), false);
    }

    #[test]
    fn executor_slate_action_can_be_called_twice_after_some_time_passes() {
        let called = Rc::new(Cell::new(false));
        let fake_action = FakeAction {
            called: called.clone(),
            execute_returns: Some(Ok(())),
        };
        let mut executor = ActionExecutor::new(
            Transition::new(VideoMode::Content, VideoMode::Slate),
            Action::FakeAction(fake_action),
        );
        executor.execute(VideoMode::Content);
        executor.execute(VideoMode::Slate);
        // Must be called since we had a state transition that matches what we defined in the executor
        assert_eq!(called.get(), true);
        // Reset state of our mock to "not called"
        called.set(false);

        // Move time forward over the delay
        sleep(Duration::from_secs(10));

        executor.execute(VideoMode::Content);
        executor.execute(VideoMode::Slate);
        assert_eq!(called.get(), true);
    }

    #[test]
    fn runtime_calls_action_executor_with_video_mode() {
        let called = Rc::new(Cell::new(false));
        let fake_action = FakeAction {
            called: called.clone(),
            execute_returns: Some(Ok(())),
        };
        let mut executor = ActionExecutor::new(
            Transition::new(VideoMode::Content, VideoMode::Slate),
            Action::FakeAction(fake_action),
        );
        // Prepare executor to be ready in the next call with `VideoMode::Slate`
        executor.execute(VideoMode::Content);
        assert_eq!(called.get(), false);

        let (s, r) = channel();
        // Pile up some events for the runtime to consume
        s.send(Event::Mode(VideoMode::Slate)).unwrap();
        s.send(Event::Terminate).unwrap();

        let mut runtime = Runtime::new(r, vec![executor]);
        runtime.run_blocking().expect("Should run successfully!");

        // Check the action was called
        assert_eq!(called.get(), true);
    }

    #[test]
    fn action_http_call_performs_request() {
        let path = "/do-something";
        let req_body = "{\"duration\":20}";

        let server = mock("POST", path)
            .match_body(req_body)
            .match_header("content-type", "application/json")
            .match_header("authorization", Matcher::Any)
            .with_status(202)
            .create();

        let mut action = HttpCall {
            method: HttpMethod::POST,
            url: format!("{}{}", server_url(), path),
            description: None,
            authorization: Some(HttpAuth::Basic {
                username: "user".to_string(),
                password: "pass".to_string(),
            }),
            headers: Some(
                [("content-type", "application/json")]
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect::<HashMap<String, String>>(),
            ),
            body: Some(req_body.to_string()),
            retries: None,
            timeout: None,
        };

        action.execute().expect("Should execute successfully!");
        assert!(server.matched());
    }

    #[test]
    fn build_executor_from_models() {
        let transition = models::Transition {
            from: models::VideoMode::Content,
            to: models::VideoMode::Slate,
            actions: vec![models::Action::HttpCall(HttpCall {
                description: Some("Trigger AdBreak using API".to_string()),
                method: HttpMethod::POST,
                url: "http://non-existent.cbsi.com/v1/organization/cbsa/channel/sl/ad-break"
                    .to_string(),
                authorization: Some(HttpAuth::Basic {
                    username: "dev_user".to_string(),
                    password: "something".to_string(),
                }),
                headers: Some(
                    [("content-type", "application/json")]
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect::<HashMap<String, String>>(),
                ),
                body: Some("{\"duration\":320}".to_string()),
                retries: Some(3),
                timeout: Some(10),
            })],
        };

        let _executors: Vec<ActionExecutor> = transition.into();
    }
}
