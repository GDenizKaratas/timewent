//! Running a child process with a hard deadline, so the 1 Hz loop can never stall on a hung
//! `osascript` (e.g. waiting on an Automation permission prompt or a beachballing browser).

use std::io::{self, Read};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::url_cache::QueryFailed;

/// A warm query measured ~110–140 ms, a cold first one ~300 ms; this leaves headroom.
pub const OSASCRIPT_TIMEOUT: Duration = Duration::from_millis(750);

/// How often the deadline loop checks the child; bounds the overshoot past the timeout.
const POLL_EVERY: Duration = Duration::from_millis(5);

#[derive(Debug)]
pub enum RunError {
    Spawn(io::Error),
    Wait(io::Error),
    TimedOut,
    Failed { status: ExitStatus, stderr: String },
}

/// osascript reports a missing Automation permission as errAEEventNotPermitted (-1743),
/// e.g. `execution error: Not authorized to send Apple events to Google Chrome. (-1743)`.
/// There is no structured channel for it, so stderr text is the signal.
fn is_automation_denial(stderr: &str) -> bool {
    stderr.contains("-1743") || stderr.contains("Not authorized")
}

impl From<RunError> for QueryFailed {
    fn from(e: RunError) -> Self {
        match e {
            RunError::TimedOut => QueryFailed::TimedOut,
            RunError::Failed { stderr, .. } if is_automation_denial(&stderr) => QueryFailed::Denied,
            RunError::Spawn(_) | RunError::Wait(_) | RunError::Failed { .. } => QueryFailed::Error,
        }
    }
}

/// Runs `osascript -e <script>` and returns its stdout.
pub fn osascript(script: &str) -> Result<String, RunError> {
    let mut cmd = Command::new("/usr/bin/osascript");
    cmd.arg("-e").arg(script);
    run_with_timeout(cmd, OSASCRIPT_TIMEOUT)
}

/// Spawns `cmd` (stdin null, stdout/stderr piped), kills it if it outlives `timeout`, and
/// returns stdout on a zero exit status, stderr with the status otherwise. The child is always
/// reaped. Output is read after exit: osascript writes a line or two, far below a pipe buffer.
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<String, RunError> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunError::Spawn)?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                // Kill can only fail if it already exited; wait reaps it either way.
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::TimedOut);
            }
            Ok(None) => thread::sleep(POLL_EVERY),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::Wait(e));
            }
        }
    };
    if !status.success() {
        let stderr = read_all(child.stderr.take()).unwrap_or_default();
        return Err(RunError::Failed { status, stderr });
    }
    read_all(child.stdout.take()).map_err(RunError::Wait)
}

fn read_all(pipe: Option<impl Read>) -> io::Result<String> {
    let mut out = Vec::new();
    if let Some(mut pipe) = pipe {
        pipe.read_to_end(&mut out)?;
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(script);
        c
    }

    #[test]
    fn returns_stdout_of_a_successful_child() {
        let out = run_with_timeout(sh("printf 'https://a.b/\\n'"), Duration::from_secs(5));
        assert_eq!(out.expect("ok"), "https://a.b/\n");
    }

    #[test]
    fn non_zero_exit_is_a_failure_carrying_stderr() {
        match run_with_timeout(
            sh("echo out; echo boom >&2; exit 1"),
            Duration::from_secs(5),
        ) {
            Err(RunError::Failed { stderr, .. }) => assert_eq!(stderr, "boom\n"),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn timeout_is_750_ms() {
        assert_eq!(OSASCRIPT_TIMEOUT, Duration::from_millis(750));
    }

    fn failed(stderr: &str) -> RunError {
        let status = Command::new("/usr/bin/false").status().expect("run false");
        RunError::Failed {
            status,
            stderr: stderr.into(),
        }
    }

    #[test]
    fn automation_denial_is_classified_as_denied() {
        // What osascript prints when the Automation permission is missing.
        let denied = "0:47: execution error: Not authorized to send Apple events to \
                      Google Chrome. (-1743)\n";
        assert_eq!(QueryFailed::from(failed(denied)), QueryFailed::Denied);
        assert_eq!(
            QueryFailed::from(failed("error (-1743)")),
            QueryFailed::Denied
        );
        assert_eq!(
            QueryFailed::from(failed("Not authorized to send Apple events")),
            QueryFailed::Denied
        );
    }

    #[test]
    fn a_timeout_is_classified_as_timed_out() {
        assert_eq!(QueryFailed::from(RunError::TimedOut), QueryFailed::TimedOut);
    }

    #[test]
    fn other_failures_are_classified_as_errors() {
        let script_error = "0:12: execution error: Can’t get window 1. Invalid index. (-1719)\n";
        assert_eq!(QueryFailed::from(failed(script_error)), QueryFailed::Error);
        assert_eq!(QueryFailed::from(failed("")), QueryFailed::Error);
        let spawn = RunError::Spawn(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(QueryFailed::from(spawn), QueryFailed::Error);
    }

    #[test]
    fn a_hung_child_is_killed_at_the_deadline() {
        let started = Instant::now();
        let res = run_with_timeout(sh("exec sleep 10"), Duration::from_millis(100));
        assert!(matches!(res, Err(RunError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn a_missing_program_is_a_spawn_error() {
        let res = run_with_timeout(
            Command::new("/nonexistent/timewent-x"),
            Duration::from_secs(1),
        );
        assert!(matches!(res, Err(RunError::Spawn(_))));
    }
}
