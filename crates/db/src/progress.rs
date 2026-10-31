//! The batch progress bar FixItemSettings and SetOfferID redraw every 500 ms from a
//! `System.Timers.Timer`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use habbo_core::out;

/// The C# `consoleLock`: the timer and the final lines never interleave.
pub static CONSOLE_LOCK: Mutex<()> = Mutex::new(());

pub fn build_progress_bar(processed: usize, total: usize, bar_width: usize) -> String {
    let fraction = processed as f64 / total as f64;
    let filled = ((fraction * bar_width as f64) as i64).clamp(0, bar_width as i64) as usize;
    format!("[{}{}]", "▓".repeat(filled), "-".repeat(bar_width - filled))
}

pub fn progress_line(processed: usize, total: usize) -> String {
    let percent = processed as f64 / total as f64 * 100.0;
    format!(
        "\r{} {}% ({processed}/{total} batches processed)",
        build_progress_bar(processed, total, 50),
        crate::dotnet::format_fixed(percent, 2)
    )
}

/// A running timer; dropping it stops it (the C# `using var timer`).
pub struct ProgressTimer {
    pub processed: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl ProgressTimer {
    pub fn start(total: usize) -> Self {
        let processed = Arc::new(AtomicUsize::new(0));
        let counter = processed.clone();
        let task = tokio::spawn(async move {
            let period = Duration::from_millis(500);
            let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            loop {
                interval.tick().await;
                let _guard = CONSOLE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                out!("{}", progress_line(counter.load(Ordering::SeqCst), total));
            }
        });
        Self { processed, task }
    }

    pub fn increment(&self) {
        self.processed.fetch_add(1, Ordering::SeqCst);
    }

    pub fn stop(&self) {
        self.task.abort();
    }
}

impl Drop for ProgressTimer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// `Partition(source, size)`.
pub fn partition<T: Clone>(source: &[T], size: usize) -> Vec<Vec<T>> {
    source.chunks(size).map(|c| c.to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar() {
        assert_eq!(build_progress_bar(1, 4, 8), "[▓▓------]");
        assert_eq!(progress_line(1, 3), format!("\r[{}{}] 33.33% (1/3 batches processed)", "▓".repeat(16), "-".repeat(34)));
        assert_eq!(partition(&[1, 2, 3, 4, 5], 2), vec![vec![1, 2], vec![3, 4], vec![5]]);
    }
}
