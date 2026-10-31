//! `FfdecInvocation`: spawns FFDec the right way for the current OS. Windows ships a native
//! ffdec-cli.exe; Linux / macOS launch the shipped ffdec-cli.jar through `java -jar`.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use tokio::process::Command;

/// FFDec is a short-lived JVM launched once per file, so its cold start dominates.
/// C1-only JIT and the serial GC cut both the per-file CPU cost and the startup latency.
/// Passed via JAVA_TOOL_OPTIONS so it reaches both the bundled-JVM exe and `java -jar`.
const JVM_FAST_START_OPTIONS: &str = "-XX:TieredStopAtLevel=1 -XX:+UseSerialGC";

pub fn tools_dir() -> PathBuf {
    PathBuf::from("Tools").join("ffdec")
}

/// A command that runs FFDec with these arguments, stdout/stderr piped, no console window.
pub fn command<I, S>(args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = if cfg!(windows) {
        Command::new(tools_dir().join("ffdec-cli.exe"))
    } else {
        let mut java = Command::new("java");
        java.arg("-jar").arg(tools_dir().join("ffdec-cli.jar"));
        java
    };
    command
        .args(args)
        .env("JAVA_TOOL_OPTIONS", JVM_FAST_START_OPTIONS)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// The result of one FFDec run.
pub struct FfdecOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl FfdecOutput {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// Runs FFDec and waits up to `timeout`. `Ok(None)` when it timed out (the process is killed).
pub async fn run<I, S>(args: I, timeout: Duration) -> Result<Option<FfdecOutput>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let child = command(args).spawn()?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(output) => {
            let output = output?;
            Ok(Some(FfdecOutput {
                exit_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            }))
        }
        // Dropping the future drops the child, and kill_on_drop ends it.
        Err(_) => Ok(None),
    }
}
