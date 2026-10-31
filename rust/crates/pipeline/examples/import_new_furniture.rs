//! Runs `import_new_furniture` in the current directory with the answers given as arguments
//! (one per prompt, in order); once they run out, the terminal answers.
//!
//! `cargo run -p habbo-pipeline --example import_new_furniture -- y y 40001 1`

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use habbo_core::console::{self, Source};

struct Answers(Mutex<VecDeque<String>>);

impl Source for Answers {
    fn read_line(&self) -> Option<String> {
        let answer = self.0.lock().unwrap().pop_front();
        match answer {
            Some(answer) => {
                console::writeln(&format!("> {answer}"));
                Some(answer)
            }
            None => {
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) | Err(_) => None,
                    Ok(_) => Some(line.trim_end_matches(['\r', '\n']).to_string()),
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let answers: VecDeque<String> = std::env::args().skip(1).collect();
    console::set_source(Some(Arc::new(Answers(Mutex::new(answers)))));
    habbo_pipeline::import_new_furniture().await
}
