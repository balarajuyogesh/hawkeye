use crate::video_stream::{Event, VideoMode};
use color_eyre::Result;
use log::{debug, error, info};
use std::sync::mpsc::Receiver;
use std::time::Duration;

#[cfg(test)]
use sn_fake_clock::FakeClock as Instant;
#[cfg(not(test))]
use std::time::Instant;

/// Abstract behaviour of an Hawkeye Action.
///
/// New actions can implement this trait and will be ready to be used with video watchers.
pub trait Action {
    fn execute(&mut self) -> Result<()> {
        Ok(())
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
    action: Box<dyn Action>,
    last_mode: Option<VideoMode>,
    last_call: Option<Instant>,
}

impl ActionExecutor {
    /// Creates a new `ActionExecutor` instance
    pub fn new(transition: Transition, action: Box<dyn Action>) -> Self {
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

pub struct HttpCall {
    url: String,
    method: String,
    username: String,
    password: String,
    payload: Option<String>,
}

impl HttpCall {
    pub fn new(
        url: String,
        method: String,
        username: String,
        password: String,
        payload: String,
    ) -> Self {
        HttpCall {
            url,
            method,
            username,
            password,
            payload: Some(payload),
        }
    }
}

impl Action for HttpCall {
    fn execute(&mut self) -> Result<()> {
        let start_api_call = Instant::now();

        let response = ureq::request(self.method.as_str(), self.url.as_str())
            .auth(self.username.as_str(), self.password.as_str())
            .timeout_connect(500)
            .set("content-type", "application/json")
            .timeout(Duration::from_secs(10))
            .send_string(self.payload.as_ref().unwrap().clone().as_str());

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
    use sn_fake_clock::FakeClock;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::mpsc::channel;

    struct FakeAction {
        called: Rc<Cell<bool>>,
        execute_returns: Option<Result<()>>,
    }

    impl Action for FakeAction {
        fn execute(&mut self) -> Result<()> {
            self.called.set(true);
            if let Some(result) = self.execute_returns.take() {
                Ok(result?)
            } else {
                Err(color_eyre::eyre::eyre!(
                    "No return value provided for mock!"
                ))
            }
        }
    }

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
            Box::new(fake_action),
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
            Box::new(fake_action),
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
            Box::new(fake_action),
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
            Box::new(fake_action),
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
}
