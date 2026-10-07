//! Prints real samples as JSONL (one `serde_json::to_string(&Sample)` per line) for manual
//! checks and fixture recording. Diagnostics, including per-call `sample()` cost, go to stderr.
//!
//! usage: timewent-probe-dump [--seconds N (10)] [--interval-ms MS (1000)]

use std::process::ExitCode;
use std::time::Duration;

struct Args {
    seconds: u64,
    interval_ms: u64,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        seconds: 10,
        interval_ms: 1000,
    };
    while let Some(flag) = args.next() {
        let target = match flag.as_str() {
            "--seconds" => &mut parsed.seconds,
            "--interval-ms" => &mut parsed.interval_ms,
            other => return Err(format!("unknown argument `{other}`")),
        };
        let value = args.next().ok_or(format!("{flag} needs a value"))?;
        *target = value
            .parse()
            .map_err(|_| format!("{flag}: `{value}` is not a non-negative integer"))?;
    }
    if parsed.interval_ms == 0 {
        return Err("--interval-ms must be > 0".into());
    }
    Ok(parsed)
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("timewent-probe-dump: {e}");
            eprintln!("usage: timewent-probe-dump [--seconds N] [--interval-ms MS]");
            return ExitCode::from(2);
        }
    };
    run(&args)
}

#[cfg(target_os = "macos")]
fn run(args: &Args) -> ExitCode {
    use std::io::Write;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};
    use timewent_probe::{permissions, pump_main_run_loop, MacProbe, Probe};

    let trusted = permissions().accessibility;
    eprintln!(
        "accessibility: {} ({})",
        if trusted { "trusted" } else { "NOT trusted" },
        if trusted {
            "window titles available"
        } else {
            "window_title will be null; grant the parent terminal in System Settings → Privacy & Security → Accessibility"
        }
    );

    let interval = Duration::from_millis(args.interval_ms);
    let count = (args.seconds * 1000).div_ceil(args.interval_ms);
    let mut probe = MacProbe::new();
    let mut costs_us: Vec<u128> = Vec::new();
    let start = Instant::now();
    let mut stdout = std::io::stdout().lock();
    for i in 0..count {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        let t = Instant::now();
        let sample = probe.sample(now_ms);
        costs_us.push(t.elapsed().as_micros());
        let line = serde_json::to_string(&sample).expect("Sample serializes");
        if writeln!(stdout, "{line}")
            .and_then(|_| stdout.flush())
            .is_err()
        {
            break; // stdout closed (e.g. piped into `head`)
        }
        // Fixed-rate schedule; pumping also delivers frontmost-app changes to NSWorkspace.
        let next = interval * u32::try_from(i + 1).unwrap_or(u32::MAX);
        pump_main_run_loop(next.saturating_sub(start.elapsed()));
    }

    costs_us.sort_unstable();
    if let Some(&max) = costs_us.last() {
        let avg = costs_us.iter().sum::<u128>() / costs_us.len() as u128;
        let median = costs_us[costs_us.len() / 2];
        eprintln!(
            "sample() cost over {} calls: avg {avg} µs, median {median} µs, max {max} µs",
            costs_us.len()
        );
    }
    ExitCode::SUCCESS
}

#[cfg(not(target_os = "macos"))]
fn run(_args: &Args) -> ExitCode {
    eprintln!("timewent-probe-dump: only macOS has a real probe");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_are_ten_seconds_at_one_hertz() {
        let a = parse(&[]).expect("defaults");
        assert_eq!((a.seconds, a.interval_ms), (10, 1000));
    }

    #[test]
    fn flags_override_defaults() {
        let a = parse(&["--interval-ms", "250", "--seconds", "15"]).expect("flags");
        assert_eq!((a.seconds, a.interval_ms), (15, 250));
    }

    #[test]
    fn bad_input_is_an_error() {
        assert!(parse(&["--seconds"]).is_err());
        assert!(parse(&["--seconds", "-1"]).is_err());
        assert!(parse(&["--interval-ms", "0"]).is_err());
        assert!(parse(&["--verbose"]).is_err());
    }
}
