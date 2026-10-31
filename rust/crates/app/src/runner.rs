//! `OperationRunner`: runs one operation with the console redirected — every line it prints
//! goes to the window, and every prompt it reads waits for the answer box.

use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use habbo_core::console::{self, Color, Sink, Source};
use tokio::task::AbortHandle;

use crate::catalog::OperationDefinition;

pub enum RunnerEvent {
    Output(String),
    /// A colour change, in order with the output (`None` = default colour).
    Color(Option<Color>),
    Finished(OperationResult),
}

#[derive(Debug, Clone)]
pub struct OperationResult {
    pub succeeded: bool,
    pub cancelled: bool,
    pub finished_at: chrono::DateTime<chrono::Local>,
    pub error: Option<String>,
    pub elapsed: Duration,
}

#[derive(Default)]
struct InputState {
    lines: std::collections::VecDeque<String>,
    cancelled: bool,
}

/// Lines typed in the window; `read_line` blocks until one arrives (or the run is cancelled).
#[derive(Default)]
pub struct InputQueue {
    state: Mutex<InputState>,
    ready: Condvar,
}

impl InputQueue {
    pub fn submit(&self, value: String) {
        self.state.lock().unwrap().lines.push_back(value);
        self.ready.notify_one();
    }

    fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.lines.clear();
        state.cancelled = false;
    }

    fn cancel(&self) {
        self.state.lock().unwrap().cancelled = true;
        self.ready.notify_all();
    }
}

struct QueueSource(Arc<InputQueue>);

impl Source for QueueSource {
    fn read_line(&self) -> Option<String> {
        let mut state = self.0.state.lock().unwrap();
        loop {
            if state.cancelled {
                return None; // end of input: a waiting prompt gives up
            }
            if let Some(line) = state.lines.pop_front() {
                return Some(line);
            }
            state = self.0.ready.wait(state).unwrap();
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

    fn set_color(&self, color: Option<Color>) {
        let _ = self.sender.send(RunnerEvent::Color(color));
    }
}

pub struct OperationRunner {
    input: Arc<InputQueue>,
    runtime: tokio::runtime::Handle,
    running: Arc<Mutex<Option<AbortHandle>>>,
}

impl OperationRunner {
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        Self { input: Arc::new(InputQueue::default()), runtime, running: Arc::new(Mutex::new(None)) }
    }

    /// Starts the operation on the runtime; events arrive on the returned channel.
    /// `wake` is called whenever an event is queued (the window repaints).
    /// `answers` are queued before the run starts: prompts read them first, in order (a queued
    /// or dropped-file run answers its own "(H) or (I)" question this way).
    pub fn start(
        &self,
        operation: &'static OperationDefinition,
        answers: Vec<String>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> mpsc::Receiver<RunnerEvent> {
        let (sender, receiver) = mpsc::channel();
        let input = self.input.clone();
        input.reset();
        for answer in answers {
            input.submit(answer);
        }
        let finished_sender = sender.clone();
        let finished_wake = wake.clone();
        let running = self.running.clone();

        self.runtime.spawn(async move {
            let clock = Instant::now();
            let previous_sink = console::set_sink(Arc::new(ChannelSink { sender, wake }));
            let previous_source = console::set_source(Some(Arc::new(QueueSource(input.clone()))));

            // A panic in an operation must not take the window down: run it as its own task.
            let task = tokio::spawn((operation.action)());
            *running.lock().unwrap() = Some(task.abort_handle());
            let outcome = task.await;
            *running.lock().unwrap() = None;

            let mut cancelled = false;
            let (succeeded, error) = match outcome {
                Ok(Ok(())) => (true, None),
                Ok(Err(error)) => {
                    console::writeln(&format!("\nError: {error:#}"));
                    (false, Some(format!("{error:#}")))
                }
                Err(join_error) if join_error.is_cancelled() => {
                    cancelled = true;
                    console::writeln("\n⏹ Cancelled. Work already written to disk stays; a running step may finish in the background.");
                    (false, Some("cancelled".to_string()))
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
                cancelled,
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

    /// Stops the running operation at its next await point; a prompt waiting for input ends.
    /// CPU work already handed to a worker thread finishes on its own.
    pub fn cancel(&self) {
        self.input.cancel();
        if let Some(handle) = self.running.lock().unwrap().as_ref() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::OperationCategory;
    use futures::FutureExt;

    fn op(id: &'static str, action: crate::catalog::Action) -> &'static OperationDefinition {
        Box::leak(Box::new(OperationDefinition {
            id,
            category: OperationCategory::General,
            title: id,
            description: "",
            action,
            requires_input: true,
            is_destructive: false,
        }))
    }

    fn finish(receiver: &mpsc::Receiver<RunnerEvent>) -> OperationResult {
        loop {
            match receiver.recv_timeout(Duration::from_secs(10)).expect("the run did not finish") {
                RunnerEvent::Finished(result) => return result,
                _ => continue,
            }
        }
    }

    #[test]
    fn cancel_stops_a_long_operation_and_a_waiting_prompt() {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let runner = OperationRunner::new(runtime.handle().clone());
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});

        // A long download-like wait.
        let sleeper = op("test.sleep", || async { tokio::time::sleep(Duration::from_secs(60)).await; Ok(()) }.boxed());
        let events = runner.start(sleeper, vec![], wake.clone());
        std::thread::sleep(Duration::from_millis(200));
        runner.cancel();
        let result = finish(&events);
        assert!(result.cancelled && !result.succeeded);

        // A prompt waiting for an answer.
        let asker = op("test.ask", || async {
            let answer = tokio::task::spawn_blocking(habbo_core::console::read_line_opt).await?;
            anyhow::ensure!(answer.is_none(), "the prompt should end without an answer");
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(())
        }
        .boxed());
        let events = runner.start(asker, vec![], wake.clone());
        std::thread::sleep(Duration::from_millis(200));
        runner.cancel();
        assert!(finish(&events).cancelled);

        // Answers still reach the next run.
        let echo = op("test.echo", || async {
            let answer = tokio::task::spawn_blocking(habbo_core::console::read_line).await?;
            anyhow::ensure!(answer == "42");
            Ok(())
        }
        .boxed());
        let events = runner.start(echo, vec![], wake.clone());
        std::thread::sleep(Duration::from_millis(100));
        runner.submit_input("42".into());
        assert!(finish(&events).succeeded);

        // Pre-filled answers.
        let events = runner.start(echo, vec!["42".into()], wake);
        assert!(finish(&events).succeeded);
    }
}
