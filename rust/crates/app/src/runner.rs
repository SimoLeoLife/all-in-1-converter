//! `OperationRunner`: runs one operation with the console redirected — every line it prints
//! goes to the window, and every prompt it reads waits for the answer box.

use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use habbo_core::console::{self, Sink, Source};

use crate::catalog::OperationDefinition;

pub enum RunnerEvent {
    Output(String),
    Finished(OperationResult),
}

#[derive(Debug, Clone)]
pub struct OperationResult {
    pub succeeded: bool,
    pub finished_at: chrono::DateTime<chrono::Local>,
    pub error: Option<String>,
    pub elapsed: Duration,
}

/// Lines typed in the window; `read_line` blocks until one arrives.
#[derive(Default)]
pub struct InputQueue {
    lines: Mutex<std::collections::VecDeque<String>>,
    ready: Condvar,
}

impl InputQueue {
    pub fn submit(&self, value: String) {
        self.lines.lock().unwrap().push_back(value);
        self.ready.notify_one();
    }

    pub fn reset(&self) {
        self.lines.lock().unwrap().clear();
    }
}

struct QueueSource(Arc<InputQueue>);

impl Source for QueueSource {
    fn read_line(&self) -> Option<String> {
        let mut lines = self.0.lines.lock().unwrap();
        loop {
            if let Some(line) = lines.pop_front() {
                return Some(line);
            }
            lines = self.0.ready.wait(lines).unwrap();
        }
    }
}

struct ChannelSink {
    sender: mpsc::Sender<RunnerEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Sink for ChannelSink {
    fn write(&self, text: &str) {
        let _ = self.sender.send(RunnerEvent::Output(text.to_string()));
        (self.wake)();
    }
}

pub struct OperationRunner {
    input: Arc<InputQueue>,
    runtime: tokio::runtime::Handle,
}

impl OperationRunner {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        Self { input: Arc::new(InputQueue::default()), runtime }
    }

    /// Starts the operation on the runtime; events arrive on the returned channel.
    /// `wake` is called whenever an event is queued (the window repaints).
    pub fn start(&self, operation: &'static OperationDefinition, wake: Arc<dyn Fn() + Send + Sync>) -> mpsc::Receiver<RunnerEvent> {
        let (sender, receiver) = mpsc::channel();
        let input = self.input.clone();
        input.reset();
        let finished_sender = sender.clone();
        let finished_wake = wake.clone();

        self.runtime.spawn(async move {
            let clock = Instant::now();
            let previous_sink = console::set_sink(Arc::new(ChannelSink { sender, wake }));
            let previous_source = console::set_source(Some(Arc::new(QueueSource(input.clone()))));

            // A panic in an operation must not take the window down: run it as its own task.
            let outcome = tokio::spawn((operation.action)()).await;
            let (succeeded, error) = match outcome {
                Ok(Ok(())) => (true, None),
                Ok(Err(error)) => {
                    console::writeln(&format!("\nError: {error:#}"));
                    (false, Some(format!("{error:#}")))
                }
                Err(join_error) => {
                    let message = if join_error.is_panic() { "the operation crashed".to_string() } else { join_error.to_string() };
                    console::writeln(&format!("\nError: {message}"));
                    (false, Some(message))
                }
            };

            input.reset();
            console::set_sink(previous_sink);
            console::set_source(previous_source);
            let _ = finished_sender.send(RunnerEvent::Finished(OperationResult {
                succeeded,
                finished_at: chrono::Local::now(),
                error,
                elapsed: clock.elapsed(),
            }));
            finished_wake();
        });
        receiver
    }

    pub fn submit_input(&self, value: String) {
        self.input.submit(value);
    }
}
