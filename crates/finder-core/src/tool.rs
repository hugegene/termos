//! Running a helper program, such as poppler's `pdftoppm`, with a time limit, a cancel check and a cap on how
//! much of its output is read. Killing the program is how a preview is cancelled, and a crash in it can't take
//! the caller down.

use std::fmt;
use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// How often the cancel check and the time limit are looked at while a program runs.
const POLL: Duration = Duration::from_millis(10);

#[derive(Debug)]
pub enum ToolError {
    /// The program isn't installed. Holds the message to show.
    Missing(String),
    /// The program failed. Holds the first line it wrote to stderr, or its exit status.
    Failed(String),
    /// It ran past its time limit and was stopped.
    TimedOut,
    /// The caller no longer wanted the result, so it was stopped.
    Cancelled,
    Io(io::Error),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolError::Missing(msg) | ToolError::Failed(msg) => f.write_str(msg),
            ToolError::TimedOut => f.write_str("took too long and was stopped"),
            ToolError::Cancelled => f.write_str("cancelled"),
            ToolError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ToolError {}

/// What a program wrote to stdout, and whether reading stopped at the cap.
#[derive(Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub truncated: bool,
}

/// Runs `cmd` and returns its stdout. Reading stops after `max_out` bytes (the program is then stopped and the
/// output marked truncated). The program is also stopped when `cancel` returns true or `timeout` passes.
/// `missing` is the message for when the program isn't installed.
pub fn run(
    mut cmd: Command,
    max_out: usize,
    timeout: Option<Duration>,
    cancel: &dyn Fn() -> bool,
    missing: &str,
) -> Result<Output, ToolError> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(ToolError::Missing(missing.to_owned())),
        Err(e) => return Err(ToolError::Io(e)),
    };
    let start = Instant::now();
    let over = || cancel().then_some(ToolError::Cancelled).or_else(|| timed_out(start, timeout));

    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let res = (&mut stdout).take(max_out as u64 + 1).read_to_end(&mut buf).map(|_| buf);
        let _ = tx.send(res);
    });
    let errors = thread::spawn(move || {
        let mut first = Vec::new();
        let _ = (&mut stderr).take(4096).read_to_end(&mut first);
        // Keep draining, so a chatty program never blocks on a full pipe.
        let _ = io::copy(&mut stderr, &mut io::sink());
        first
    });

    let read = loop {
        // Checked first too, so a request that's already stale (or a zero time limit) stops at once.
        if let Some(e) = over() {
            break Err(e);
        }
        match rx.recv_timeout(POLL) {
            Ok(res) => break res.map_err(ToolError::Io),
            Err(RecvTimeoutError::Disconnected) => break Err(ToolError::Io(io::Error::other("output reader stopped"))),
            Err(RecvTimeoutError::Timeout) => {}
        }
    };
    let mut stdout = match read {
        Ok(out) => out,
        Err(e) => {
            stop(&mut child);
            return Err(e);
        }
    };
    if stdout.len() > max_out {
        stop(&mut child);
        stdout.truncate(max_out);
        return Ok(Output { stdout, truncated: true });
    }
    // Its stdout is closed, so it's finishing; still honour the limits while it does.
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if let Some(e) = over() {
                    stop(&mut child);
                    return Err(e);
                }
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) => {
                stop(&mut child);
                return Err(ToolError::Io(e));
            }
        }
    };
    if status.success() {
        return Ok(Output { stdout, truncated: false });
    }
    let first = errors.join().unwrap_or_default();
    let text = String::from_utf8_lossy(&first);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty());
    Err(ToolError::Failed(line.map_or_else(|| status.to_string(), str::to_owned)))
}

fn timed_out(start: Instant, timeout: Option<Duration>) -> Option<ToolError> {
    timeout.is_some_and(|t| start.elapsed() >= t).then_some(ToolError::TimedOut)
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn sh(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", script]);
        c
    }

    #[test]
    fn returns_output() {
        let out = run(sh("printf hello"), 100, None, &|| false, "").unwrap();
        assert_eq!(out.stdout, b"hello");
        assert!(!out.truncated);
    }

    #[test]
    fn caps_output() {
        let out = run(sh("yes"), 1000, None, &|| false, "").unwrap();
        assert_eq!(out.stdout.len(), 1000);
        assert!(out.truncated);
    }

    #[test]
    fn reports_the_first_error_line() {
        match run(sh("echo >&2; echo 'bad file' >&2; echo more >&2; exit 3"), 100, None, &|| false, "") {
            Err(ToolError::Failed(msg)) => assert_eq!(msg, "bad file"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn missing_program() {
        let cmd = Command::new("tb-no-such-program");
        match run(cmd, 100, None, &|| false, "needs it") {
            Err(ToolError::Missing(msg)) => assert_eq!(msg, "needs it"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cancel_stops_it_quickly() {
        let flag = AtomicBool::new(false);
        let start = Instant::now();
        thread::scope(|s| {
            s.spawn(|| {
                thread::sleep(Duration::from_millis(30));
                flag.store(true, Ordering::Relaxed);
            });
            let res = run(sh("exec sleep 5"), 100, None, &|| flag.load(Ordering::Relaxed), "");
            assert!(matches!(res, Err(ToolError::Cancelled)), "{res:?}");
        });
        // Far less than `sleep`'s 5 s. Loose, because shared CI runners are slow: macOS took 170 ms once.
        assert!(start.elapsed() < Duration::from_secs(1), "{:?}", start.elapsed());
    }

    #[test]
    fn time_limit_stops_it() {
        let start = Instant::now();
        let res = run(sh("exec sleep 5"), 100, Some(Duration::from_millis(50)), &|| false, "");
        assert!(matches!(res, Err(ToolError::TimedOut)), "{res:?}");
        // Far less than `sleep`'s 5 s. Loose, because shared CI runners are slow: macOS took 170 ms once.
        assert!(start.elapsed() < Duration::from_secs(1), "{:?}", start.elapsed());
    }
}
