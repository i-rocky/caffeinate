mod platform;

use std::env;
use std::fmt;
use std::io;
use std::process::{Command, ExitStatus};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};
use std::thread;
use std::time::{Duration, Instant};

use platform::InhibitFlags;

const USAGE: &str = "usage: caffeinate [-disum] [-t timeout] [-w pid] [utility [argument ...]]";
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static CTRL_HANDLER_INIT: OnceLock<Result<(), String>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default)]
struct Assertions {
    display: bool,
    idle: bool,
    disk: bool,
    system: bool,
    user_active: bool,
}

impl Assertions {
    fn any(self) -> bool {
        self.display || self.idle || self.disk || self.system || self.user_active
    }
}

#[derive(Debug, Clone)]
struct Options {
    assertions: Assertions,
    timeout_secs: Option<u64>,
    wait_pid: Option<u32>,
    utility: Vec<String>,
}

#[derive(Debug)]
enum Error {
    Usage(String),
    Runtime(String),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(s) => write!(f, "{s}"),
            Error::Runtime(s) => write!(f, "{s}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::Io(value)
    }
}

fn main() {
    match run(env::args().collect()) {
        Ok(code) => std::process::exit(code),
        Err(Error::Usage(msg)) => {
            eprintln!("{msg}");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
        Err(err) => {
            eprintln!("caffeinate: {err}");
            std::process::exit(1);
        }
    }
}

fn run(args: Vec<String>) -> Result<i32, Error> {
    let mut opts = parse_options(&args[1..])?;

    if !opts.assertions.any() {
        opts.assertions.idle = true;
    }

    if opts.assertions.user_active && opts.timeout_secs.is_none() && opts.utility.is_empty() {
        opts.timeout_secs = Some(5);
    }

    let effective = effective_options(opts);
    let prevent_system_sleep =
        resolve_system_sleep_assertion(effective.assertions.system, platform::on_ac_power());
    if effective.assertions.system && !prevent_system_sleep {
        eprintln!("caffeinate: warning: -s requested while on battery power; ignoring");
    }

    let flags = InhibitFlags {
        prevent_display_sleep: effective.assertions.display,
        prevent_idle_sleep: effective.assertions.idle || effective.assertions.disk,
        prevent_system_sleep,
        user_active: effective.assertions.user_active,
    };

    let _inhibitors = platform::activate(flags).map_err(Error::Runtime)?;

    if !effective.utility.is_empty() {
        run_utility(&effective.utility)
    } else {
        run_hold_loop(&effective)
    }
}

fn effective_options(mut opts: Options) -> Options {
    if !opts.utility.is_empty() {
        // Matches macOS caffeinate behavior: timeout and wait-on-pid are ignored
        // whenever a utility command is provided.
        opts.timeout_secs = None;
        opts.wait_pid = None;
    }
    opts
}

fn resolve_system_sleep_assertion(requested: bool, ac_power: Option<bool>) -> bool {
    if !requested {
        return false;
    }
    !matches!(ac_power, Some(false))
}

fn parse_options(args: &[String]) -> Result<Options, Error> {
    let mut assertions = Assertions::default();
    let mut timeout_secs: Option<u64> = None;
    let mut wait_pid: Option<u32> = None;
    let mut utility: Vec<String> = Vec::new();

    let mut i = 0;
    let mut parsing_options = true;

    while i < args.len() {
        let arg = &args[i];

        if !parsing_options {
            utility.extend_from_slice(&args[i..]);
            break;
        }

        if arg == "--" {
            parsing_options = false;
            i += 1;
            continue;
        }

        if !arg.starts_with('-') || arg == "-" {
            utility.extend_from_slice(&args[i..]);
            break;
        }

        let shorts = &arg[1..];
        if shorts.is_empty() {
            return Err(Error::Usage(format!("invalid option: {arg}")));
        }

        let mut chars = shorts.char_indices().peekable();
        while let Some((idx, ch)) = chars.next() {
            match ch {
                'd' => assertions.display = true,
                'i' => assertions.idle = true,
                'm' => assertions.disk = true,
                's' => assertions.system = true,
                'u' => assertions.user_active = true,
                't' => {
                    let value = if let Some((next_idx, _)) = chars.peek() {
                        shorts[*next_idx..].to_string()
                    } else {
                        i += 1;
                        args.get(i)
                            .ok_or_else(|| Error::Usage("-t requires a timeout value".to_string()))?
                            .to_string()
                    };
                    timeout_secs = Some(parse_u64("timeout", &value)?);
                    break;
                }
                'w' => {
                    let value = if let Some((next_idx, _)) = chars.peek() {
                        shorts[*next_idx..].to_string()
                    } else {
                        i += 1;
                        args.get(i)
                            .ok_or_else(|| Error::Usage("-w requires a pid value".to_string()))?
                            .to_string()
                    };
                    wait_pid = Some(parse_u32("pid", &value)?);
                    break;
                }
                _ => {
                    let _ = idx;
                    return Err(Error::Usage(format!("invalid option: -{ch}")));
                }
            }
        }

        i += 1;
    }

    Ok(Options {
        assertions,
        timeout_secs,
        wait_pid,
        utility,
    })
}

fn parse_u64(name: &str, value: &str) -> Result<u64, Error> {
    value
        .parse::<u64>()
        .map_err(|_| Error::Usage(format!("invalid {name}: {value}")))
}

fn parse_u32(name: &str, value: &str) -> Result<u32, Error> {
    value
        .parse::<u32>()
        .map_err(|_| Error::Usage(format!("invalid {name}: {value}")))
}

fn run_utility(argv: &[String]) -> Result<i32, Error> {
    let mut cmd = Command::new(&argv[0]);
    if argv.len() > 1 {
        cmd.args(&argv[1..]);
    }

    let status = cmd.status()?;
    Ok(exit_code(status))
}

fn ensure_ctrlc_handler() -> Result<(), Error> {
    let init = CTRL_HANDLER_INIT.get_or_init(|| {
        ctrlc::set_handler(|| {
            INTERRUPTED.store(true, Ordering::SeqCst);
        })
        .map_err(|e| e.to_string())
    });

    init.clone()
        .map_err(|e| Error::Runtime(format!("failed to install Ctrl-C handler: {e}")))
}

fn run_hold_loop(opts: &Options) -> Result<i32, Error> {
    ensure_ctrlc_handler()?;
    INTERRUPTED.store(false, Ordering::SeqCst);

    let deadline = opts
        .timeout_secs
        .map(|secs| Instant::now() + Duration::from_secs(secs));

    let pid_watch = opts.wait_pid.map(PidWatch::new);

    loop {
        if INTERRUPTED.load(Ordering::SeqCst) {
            break;
        }

        if let Some(watch) = &pid_watch {
            if !watch.is_alive() {
                break;
            }
        }

        if let Some(limit) = deadline {
            if Instant::now() >= limit {
                break;
            }
        }

        thread::sleep(Duration::from_millis(100));
    }

    Ok(0)
}

fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }

    1
}

#[derive(Debug, Clone)]
struct PidWatch {
    pid: u32,
    #[cfg(target_os = "linux")]
    start_time_ticks: Option<u64>,
}

impl PidWatch {
    fn new(pid: u32) -> Self {
        Self {
            pid,
            #[cfg(target_os = "linux")]
            start_time_ticks: linux_pid_snapshot(pid).map(|s| s.start_time_ticks),
        }
    }

    fn is_alive(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            let Some(current) = linux_pid_snapshot(self.pid) else {
                return false;
            };
            if current.state == 'Z' || current.state == 'X' {
                return false;
            }
            self.start_time_ticks.is_some()
                && Some(current.start_time_ticks) == self.start_time_ticks
        }

        #[cfg(target_os = "windows")]
        {
            return platform::is_process_alive(self.pid);
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = self.pid;
            false
        }
    }
}

#[cfg(target_os = "linux")]
struct LinuxPidSnapshot {
    state: char,
    start_time_ticks: u64,
}

#[cfg(target_os = "linux")]
fn linux_pid_snapshot(pid: u32) -> Option<LinuxPidSnapshot> {
    let stat_path = format!("/proc/{pid}/stat");
    let content = std::fs::read_to_string(stat_path).ok()?;

    // /proc/<pid>/stat has format: pid (comm) state ... starttime ...
    // starttime is field #22; parse by skipping the executable name in parens.
    let right_paren = content.rfind(')')?;
    let tail = content.get(right_paren + 2..)?;
    let fields: Vec<&str> = tail.split_whitespace().collect();

    // In tail, field #3 in original stat is index 0 (state).
    // starttime (#22) is index 19.
    let state = fields.first()?.chars().next()?;
    let start_time_ticks = fields.get(19)?.parse::<u64>().ok()?;

    Some(LinuxPidSnapshot {
        state,
        start_time_ticks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn normalize(mut opts: Options, ac_power: Option<bool>) -> Options {
        if !opts.assertions.any() {
            opts.assertions.idle = true;
        }
        if opts.assertions.user_active && opts.timeout_secs.is_none() && opts.utility.is_empty() {
            opts.timeout_secs = Some(5);
        }
        let mut effective = effective_options(opts);
        effective.assertions.system =
            resolve_system_sleep_assertion(effective.assertions.system, ac_power);
        effective
    }

    fn parse(args: &[&str]) -> Options {
        let argv = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        parse_options(&argv).expect("parse should succeed")
    }

    #[test]
    fn defaults_to_idle_when_no_assertions() {
        let opts = normalize(parse(&[]), None);
        assert!(opts.assertions.idle);
    }

    #[test]
    fn parses_combined_flags_and_values() {
        let opts = parse(&["-dismu", "-t", "15", "-w123"]);
        assert!(opts.assertions.display);
        assert!(opts.assertions.idle);
        assert!(opts.assertions.disk);
        assert!(opts.assertions.system);
        assert!(opts.assertions.user_active);
        assert_eq!(opts.timeout_secs, Some(15));
        assert_eq!(opts.wait_pid, Some(123));
    }

    #[test]
    fn parses_attached_numeric_values() {
        let opts = parse(&["-t10", "-w42"]);
        assert_eq!(opts.timeout_secs, Some(10));
        assert_eq!(opts.wait_pid, Some(42));
    }

    #[test]
    fn parses_split_numeric_values() {
        let opts = parse(&["-t", "10", "-w", "42"]);
        assert_eq!(opts.timeout_secs, Some(10));
        assert_eq!(opts.wait_pid, Some(42));
    }

    #[test]
    fn parses_option_with_tail_after_t() {
        let opts = parse(&["-dt10"]);
        assert!(opts.assertions.display);
        assert_eq!(opts.timeout_secs, Some(10));
    }

    #[test]
    fn parses_option_with_tail_after_w() {
        let opts = parse(&["-iw777"]);
        assert!(opts.assertions.idle);
        assert_eq!(opts.wait_pid, Some(777));
    }

    #[test]
    fn utility_consumes_remaining_args() {
        let opts = parse(&["-d", "sleep", "5", "-t", "100"]);
        assert!(opts.assertions.display);
        assert_eq!(opts.utility, vec!["sleep", "5", "-t", "100"]);
    }

    #[test]
    fn dash_dash_stops_option_parsing() {
        let opts = parse(&["-d", "--", "-t", "100"]);
        assert!(opts.assertions.display);
        assert_eq!(opts.utility, vec!["-t", "100"]);
    }

    #[test]
    fn single_dash_is_utility_name() {
        let opts = parse(&["-"]);
        assert_eq!(opts.utility, vec!["-"]);
    }

    #[test]
    fn utility_ignores_timeout_and_wait() {
        let opts = parse(&["-t", "10", "-w", "22", "echo", "hi"]);
        let effective = effective_options(opts);
        assert!(effective.timeout_secs.is_none());
        assert!(effective.wait_pid.is_none());
    }

    #[test]
    fn user_activity_defaults_to_five_seconds() {
        let opts = normalize(parse(&["-u"]), None);
        assert_eq!(opts.timeout_secs, Some(5));
    }

    #[test]
    fn user_activity_does_not_override_explicit_timeout() {
        let opts = normalize(parse(&["-u", "-t", "9"]), None);
        assert_eq!(opts.timeout_secs, Some(9));
    }

    #[test]
    fn system_sleep_requires_ac_power_only() {
        let on_ac = normalize(parse(&["-s"]), Some(true));
        let on_battery = normalize(parse(&["-s"]), Some(false));
        let unknown = normalize(parse(&["-s"]), None);
        assert!(on_ac.assertions.system);
        assert!(!on_battery.assertions.system);
        assert!(unknown.assertions.system);
    }

    #[test]
    fn resolve_system_sleep_assertion_helper() {
        assert!(!resolve_system_sleep_assertion(false, Some(true)));
        assert!(resolve_system_sleep_assertion(true, Some(true)));
        assert!(!resolve_system_sleep_assertion(true, Some(false)));
        assert!(resolve_system_sleep_assertion(true, None));
    }

    #[test]
    fn invalid_short_option_is_error() {
        let argv = vec!["-x".to_string()];
        let err = parse_options(&argv).expect_err("should fail");
        assert!(matches!(err, Error::Usage(_)));
    }

    #[test]
    fn missing_timeout_value_is_error() {
        let argv = vec!["-t".to_string()];
        let err = parse_options(&argv).expect_err("should fail");
        match err {
            Error::Usage(msg) => assert!(msg.contains("-t requires a timeout value")),
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn missing_wait_pid_value_is_error() {
        let argv = vec!["-w".to_string()];
        let err = parse_options(&argv).expect_err("should fail");
        match err {
            Error::Usage(msg) => assert!(msg.contains("-w requires a pid value")),
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn invalid_timeout_value_is_error() {
        let argv = vec!["-t".to_string(), "abc".to_string()];
        let err = parse_options(&argv).expect_err("should fail");
        match err {
            Error::Usage(msg) => assert!(msg.contains("invalid timeout")),
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn invalid_pid_value_is_error() {
        let argv = vec!["-w".to_string(), "abc".to_string()];
        let err = parse_options(&argv).expect_err("should fail");
        match err {
            Error::Usage(msg) => assert!(msg.contains("invalid pid")),
            _ => panic!("unexpected error type"),
        }
    }

    #[test]
    fn run_utility_returns_child_exit_code() {
        #[cfg(unix)]
        {
            let argv = vec!["sh".to_string(), "-c".to_string(), "exit 7".to_string()];
            let code = run_utility(&argv).expect("utility should run");
            assert_eq!(code, 7);
        }

        #[cfg(windows)]
        {
            let argv = vec!["cmd".to_string(), "/C".to_string(), "exit /b 7".to_string()];
            let code = run_utility(&argv).expect("utility should run");
            assert_eq!(code, 7);
        }
    }

    #[test]
    fn hold_loop_respects_timeout() {
        let opts = Options {
            assertions: Assertions::default(),
            timeout_secs: Some(1),
            wait_pid: None,
            utility: vec![],
        };

        let start = Instant::now();
        let code = run_hold_loop(&opts).expect("hold loop should complete");
        let elapsed = start.elapsed();

        assert_eq!(code, 0);
        assert!(elapsed >= Duration::from_millis(900));
        assert!(elapsed <= Duration::from_millis(2500));
    }

    #[test]
    fn hold_loop_exits_immediately_for_dead_pid() {
        let opts = Options {
            assertions: Assertions::default(),
            timeout_secs: None,
            wait_pid: Some(999_999),
            utility: vec![],
        };

        let start = Instant::now();
        let code = run_hold_loop(&opts).expect("hold loop should complete");
        let elapsed = start.elapsed();

        assert_eq!(code, 0);
        assert!(elapsed <= Duration::from_millis(400));
    }

    #[test]
    #[cfg(unix)]
    fn hold_loop_waits_for_pid_exit() {
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 1")
            .spawn()
            .expect("child should spawn");

        let opts = Options {
            assertions: Assertions::default(),
            timeout_secs: None,
            wait_pid: Some(child.id()),
            utility: vec![],
        };

        let start = Instant::now();
        let code = run_hold_loop(&opts).expect("hold loop should complete");
        let elapsed = start.elapsed();
        let _ = child.wait();

        assert_eq!(code, 0);
        assert!(elapsed >= Duration::from_millis(800));
        assert!(elapsed <= Duration::from_millis(2500));
    }

    #[test]
    #[cfg(unix)]
    fn hold_loop_stops_at_earliest_of_timeout_or_wait_pid() {
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 5")
            .spawn()
            .expect("child should spawn");

        let opts = Options {
            assertions: Assertions::default(),
            timeout_secs: Some(1),
            wait_pid: Some(child.id()),
            utility: vec![],
        };

        let start = Instant::now();
        let code = run_hold_loop(&opts).expect("hold loop should complete");
        let elapsed = start.elapsed();
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(code, 0);
        assert!(elapsed >= Duration::from_millis(900));
        assert!(elapsed <= Duration::from_millis(2500));
    }
}
