//! Bounded external tools. YouTube resolver children are the first processes
//! the kernel stops if RAM really runs out, so betterPod and its audio keep going.
use crate::online::Context;
use std::{
    fs,
    io::{self, Read},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MIB: u64 = 1024 * 1024;
const MEMORY_ERROR: &str = "Not enough free memory to open YouTube";
const POLL: Duration = Duration::from_millis(25);
#[derive(Clone, Copy, Default)]
struct MemoryUsage {
    available: u64,
    group_rss: u64,
    app_rss: u64,
}

pub fn run_command(
    context: &Context,
    command: &mut Command,
    timeout: Duration,
) -> Result<String, String> {
    run(
        context,
        command,
        timeout,
        false,
        64 * MIB as usize,
        memory_usage,
    )
}

pub(super) fn run_ytdlp(
    context: &Context,
    command: &mut Command,
    timeout: Duration,
) -> Result<String, String> {
    // No memory limits ahead of time: they refused videos that open fine on
    // the console (the Go app has none). The kernel stops these first instead.
    run(
        context,
        command,
        timeout,
        true,
        16 * MIB as usize,
        memory_usage,
    )
}

fn run(
    context: &Context,
    command: &mut Command,
    timeout: Duration,
    resolver: bool,
    output_limit: usize,
    mut probe: impl FnMut(Option<u32>) -> Result<MemoryUsage, String>,
) -> Result<String, String> {
    context.check()?;
    let operation = operation_name(command);
    let mut last_memory = MemoryUsage::default();
    if resolver {
        if let Ok(memory) = probe(None) {
            eprintln!(
                "[YouTube] {operation} starting: app={} MiB available={} MiB",
                memory.app_rss / MIB,
                memory.available / MIB
            );
            last_memory = memory;
        }
        protect_resolver(command);
    }
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().map_err(|error| {
        eprintln!("Start {:?}: {error}", command.get_program());
        if resolver && error.raw_os_error() == Some(libc::ENOMEM) {
            MEMORY_ERROR.to_owned()
        } else if resolver {
            format!("YouTube tools could not start: {error}. Update Tools in Settings → YouTube")
        } else {
            format!("Start {:?}: {error}", command.get_program())
        }
    })?;
    let mut group = ProcessGroup {
        child,
        stopped: false,
    };
    let mut stdout = group.child.stdout.take().ok_or("Missing command output")?;
    let mut stderr = group.child.stderr.take().ok_or("Missing command errors")?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let mut out = Capture::new(output_limit);
    let mut err = Capture::new(MIB as usize);
    let started = Instant::now();
    let mut last_probe = started - Duration::from_secs(1);
    let mut peak_group = 0;
    let status = loop {
        context.check()?;
        if started.elapsed() >= timeout {
            return Err("Command timed out".into());
        }
        // Measured for the log only; nothing is stopped for memory here.
        if resolver && last_probe.elapsed() >= Duration::from_millis(500) {
            if let Ok(memory) = probe(Some(group.child.id())) {
                peak_group = peak_group.max(memory.group_rss);
                last_memory = memory;
            }
            last_probe = Instant::now();
        }
        let out_done = out.drain(&mut stdout, Some(context))?;
        let err_done = err.drain(&mut stderr, Some(context))?;
        if let Some(status) = group.child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        wait_output(&stdout, !out_done, &stderr, !err_done)?;
    };
    // Even a failed/exited parent may leave Deno holding the pipe open. Kill the
    // whole owned group before draining, and never join a blocking reader thread.
    group.stop();
    let until = Instant::now() + Duration::from_millis(250);
    loop {
        let out_done = out.drain(&mut stdout, Some(context))?;
        let err_done = err.drain(&mut stderr, Some(context))?;
        if out_done && err_done {
            break;
        }
        if Instant::now() >= until {
            return Err("External tool did not close its output".into());
        }
        thread::sleep(POLL);
    }
    let errors = err.text();
    if resolver {
        eprintln!("[YouTube] {operation} finished: status={status} elapsed={} ms peak-group={} MiB app={} MiB available={} MiB", started.elapsed().as_millis(), peak_group / MIB, last_memory.app_rss / MIB, last_memory.available / MIB);
    }
    if !status.success() {
        // SIGKILL from the kernel: RAM really ran out and the tool was stopped first.
        if resolver && std::os::unix::process::ExitStatusExt::signal(&status) == Some(libc::SIGKILL)
        {
            return Err(MEMORY_ERROR.into());
        }
        return Err(command_error(&errors, resolver));
    }
    Ok(out.text())
}

fn protect_resolver(command: &mut Command) {
    #[cfg(target_os = "linux")]
    {
        // pre_exec forces std::process to fork the entire player. On the 1 GB
        // console that can fail with ENOMEM before yt-dlp even starts. Keep the
        // posix_spawn path: a tiny shell sets the inherited OOM priority, then
        // replaces itself with the tool in the same owned process group.
        // Program/arguments are positional parameters, never shell source.
        let mut child = Command::new("/bin/sh");
        child.args([
            "-c",
            "{ printf 1000 > /proc/self/oom_score_adj; } 2>/dev/null || :; exec \"$@\"",
            "betterpod-resolver",
        ]);
        child.arg(command.get_program()).args(command.get_args());
        if let Some(directory) = command.get_current_dir() {
            child.current_dir(directory);
        }
        for (key, value) in command.get_envs() {
            if let Some(value) = value {
                child.env(key, value);
            } else {
                child.env_remove(key);
            }
        }
        *command = child;
    }
}

fn command_error(errors: &str, resolver: bool) -> String {
    if resolver && v8_heap_failure(errors) {
        return MEMORY_ERROR.into();
    }
    errors
        .lines()
        .rev()
        .find(|s| !s.trim().is_empty())
        .unwrap_or("External tool failed")
        .to_string()
}

fn operation_name(command: &Command) -> &'static str {
    if command
        .get_args()
        .any(|arg| arg == "-J" || arg == "--dump-single-json")
    {
        "metadata"
    } else if command.get_args().any(|arg| arg == "--load-info-json") {
        "cached-media"
    } else {
        "media"
    }
}

fn v8_heap_failure(errors: &str) -> bool {
    let lower = errors.to_ascii_lowercase();
    lower.contains("heap out of memory")
        || lower.contains("reached heap limit")
        || lower.contains("javascript out of memory")
}

struct ProcessGroup {
    child: Child,
    stopped: bool,
}
impl ProcessGroup {
    fn signal(&self, signal: i32) -> bool {
        unsafe { libc::kill(-(self.child.id() as i32), signal) == 0 }
    }
    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        if self.signal(libc::SIGTERM) {
            let until = Instant::now() + Duration::from_millis(100);
            while self.signal(0) && Instant::now() < until {
                let _ = self.child.try_wait();
                thread::sleep(POLL);
            }
            // The parent can already be reaped while a child ignores SIGTERM.
            self.signal(libc::SIGKILL);
        }
        let until = Instant::now() + Duration::from_millis(250);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < until {
            thread::sleep(POLL);
        }
    }
}
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.stop();
    }
}

// Wake as soon as either pipe has data. A fixed sleep throttles large JSON
// responses when the child fills its pipe between nonblocking reads. Closed
// pipes are disabled so POLLHUP cannot turn a quiet child into a busy loop.
fn wait_output(
    stdout: &impl AsRawFd,
    out_open: bool,
    stderr: &impl AsRawFd,
    err_open: bool,
) -> Result<(), String> {
    let mut fds = [
        libc::pollfd {
            fd: if out_open { stdout.as_raw_fd() } else { -1 },
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: if err_open { stderr.as_raw_fd() } else { -1 },
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let result = unsafe {
        libc::poll(
            fds.as_mut_ptr(),
            fds.len() as libc::nfds_t,
            POLL.as_millis() as i32,
        )
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error.to_string());
        }
    }
    Ok(())
}

fn nonblocking(file: &impl AsRawFd) -> Result<(), String> {
    let fd = file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(())
}

struct Capture {
    bytes: Vec<u8>,
    limit: usize,
    line: String,
    last_progress: Instant,
}
impl Capture {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            line: String::new(),
            last_progress: Instant::now() - Duration::from_secs(1),
        }
    }
    /// Bound work per poll as well as retained bytes, including a noisy child.
    fn drain(&mut self, reader: &mut impl Read, context: Option<&Context>) -> Result<bool, String> {
        let mut buffer = [0u8; 8192];
        for _ in 0..32 {
            let n = match reader.read(&mut buffer) {
                Ok(0) => return Ok(true),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.to_string()),
            };
            if self.bytes.len().saturating_add(n) > self.limit {
                return Err("External tool output was too large".into());
            }
            self.bytes.extend_from_slice(&buffer[..n]);
            if let Some(context) = context {
                for part in String::from_utf8_lossy(&buffer[..n]).split_inclusive('\n') {
                    if self.line.len() < 65536 {
                        self.line.push_str(part);
                    }
                    if part.ends_with('\n') {
                        if let Some(progress) = self.line.trim().strip_prefix("BP_PROGRESS|") {
                            let mut fields = progress.split('|');
                            let bytes = |s: &str| {
                                s.trim()
                                    .parse::<f64>()
                                    .ok()
                                    .filter(|n| n.is_finite() && *n >= 0.)
                                    .map(|n| n as u64)
                            };
                            let done = fields.next().and_then(bytes).unwrap_or(0);
                            let total = fields
                                .next()
                                .and_then(bytes)
                                .filter(|n| *n > 0)
                                .or_else(|| fields.next().and_then(bytes).filter(|n| *n > 0));
                            if self.last_progress.elapsed() > Duration::from_millis(200) {
                                context.progress(done, total, "Downloading YouTube");
                                self.last_progress = Instant::now();
                            }
                        }
                        self.line.clear();
                    }
                }
            }
        }
        Ok(false)
    }
    fn text(self) -> String {
        // Avoid a second full-size allocation for valid UTF-8 metadata.
        String::from_utf8(self.bytes)
            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
    }
}

fn memory_usage(group: Option<u32>) -> Result<MemoryUsage, String> {
    let text = fs::read_to_string("/proc/meminfo")
        .map_err(|_| "Unable to check free memory for YouTube".to_string())?;
    let value = |key: &str| {
        text.lines().find_map(|line| {
            line.strip_prefix(key)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()?
                .checked_mul(1024)
        })
    };
    let mut usage = MemoryUsage {
        available: value("MemAvailable:").ok_or("Unable to check free memory for YouTube")?,
        group_rss: 0,
        app_rss: fs::read_to_string("/proc/self/stat")
            .ok()
            .and_then(|stat| resident_pages(&stat))
            .unwrap_or(0)
            .saturating_mul(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(0) as u64),
    };
    if let Some(group) = group {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page_size <= 0 {
            return Err("Unable to check YouTube memory use".into());
        }
        for entry in fs::read_dir("/proc").map_err(|e| e.to_string())?.flatten() {
            if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
                continue;
            }
            // Process exit races are expected. Read TGIDs only, so threads are not counted twice.
            let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            if let Some(pages) = group_resident_pages(&stat, group) {
                usage.group_rss = usage.group_rss.saturating_add(pages * page_size as u64);
            }
        }
    }
    Ok(usage)
}

fn group_resident_pages(stat: &str, group: u32) -> Option<u64> {
    // comm may contain spaces and parentheses; numeric fields start after its final ')'.
    let mut fields = stat.rsplit_once(')')?.1.split_whitespace();
    if fields.next()? == "Z" || fields.nth(1)?.parse::<u32>().ok()? != group {
        return None;
    }
    fields.nth(18)?.parse().ok()
}

fn resident_pages(stat: &str) -> Option<u64> {
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(21)?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "betterpod-resolver-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn command(&self, script: &str) -> Command {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script, "resolver-test"]);
            command.arg(self.0.join("pid")).arg(self.0.join("ready"));
            command
        }
        fn pid(&self) -> u32 {
            fs::read_to_string(self.0.join("pid"))
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn ample(_: Option<u32>) -> Result<MemoryUsage, String> {
        Ok(MemoryUsage {
            available: 512 * MIB,
            group_rss: 0,
            app_rss: 128 * MIB,
        })
    }
    fn is_running(pid: u32) -> bool {
        #[cfg(target_os = "linux")]
        {
            fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|s| s.rsplit_once(')').map(|(_, rest)| !rest.starts_with(" Z")))
                .unwrap_or(false)
        }
        #[cfg(not(target_os = "linux"))]
        unsafe {
            libc::kill(pid as i32, 0) == 0
        }
    }
    fn assert_stopped(pid: u32) {
        let until = Instant::now() + Duration::from_secs(1);
        while is_running(pid) && Instant::now() < until {
            thread::sleep(POLL);
        }
        assert!(!is_running(pid), "process {pid} survived resolver cleanup");
    }

    #[test]
    fn large_output_and_early_closed_stdout_are_drained_without_stalling() {
        let fixture = Fixture::new();
        let output = run_command(
            &Context::standalone(),
            &mut fixture.command("head -c 4194304 /dev/zero"),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(output.len(), 4 * 1024 * 1024);
        assert!(output.bytes().all(|byte| byte == 0));
        let error = run_command(
            &Context::standalone(),
            &mut fixture.command("exec 1>&-; sleep 0.1; printf 'late failure' >&2; exit 1"),
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert_eq!(error, "late failure");
    }

    #[test]
    fn captures_success_output_and_preserves_tool_errors() {
        let fixture = Fixture::new();
        let output = run_command(
            &Context::standalone(),
            &mut fixture.command("printf '{\"title\":\"Music\"}'; printf 'warning' >&2"),
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(output, "{\"title\":\"Music\"}");
        assert_eq!(
            run_command(
                &Context::standalone(),
                &mut fixture.command("printf 'detail\\nresolver failed\\n' >&2; exit 7"),
                Duration::from_secs(2)
            ),
            Err("resolver failed".into())
        );
    }

    #[test]
    fn captures_complete_large_output_while_draining_stderr() {
        let fixture = Fixture::new();
        let output = run_command(
            &Context::standalone(),
            &mut fixture.command(
                "head -c 131072 /dev/zero | tr '\\000' e >&2 & \
                 head -c 524288 /dev/zero | tr '\\000' x; wait; printf 'end'",
            ),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(output.len(), 524288 + 3);
        assert!(output[..524288].bytes().all(|b| b == b'x'));
        assert!(output.ends_with("end"));
    }

    #[test]
    fn parent_failure_kills_term_ignoring_child_holding_output_pipe() {
        let fixture = Fixture::new();
        let mut command = fixture.command(
            "(trap '' TERM; echo ready > \"$2\"; while :; do sleep 1; done) & \
             printf '%s' \"$!\" > \"$1\"; \
             while [ ! -f \"$2\" ]; do sleep 0.01; done; \
             echo 'resolver failed' >&2; exit 7",
        );
        let started = Instant::now();
        assert_eq!(
            run_command(&Context::standalone(), &mut command, Duration::from_secs(4)),
            Err("resolver failed".into())
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_stopped(fixture.pid());
    }

    #[test]
    fn timeout_kills_entire_group_even_when_parent_ignores_term() {
        let fixture = Fixture::new();
        let mut command = fixture.command("trap '' TERM; echo $$ > \"$1\"; sleep 60 & wait");
        let started = Instant::now();
        assert_eq!(
            run_command(
                &Context::standalone(),
                &mut command,
                Duration::from_millis(150)
            ),
            Err("Command timed out".into())
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_stopped(fixture.pid());
    }

    #[test]
    fn cancellation_does_not_wait_for_inherited_pipes() {
        let fixture = Fixture::new();
        let context = Context::standalone();
        let cancel = context.clone();
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            cancel.cancel();
        });
        let started = Instant::now();
        let result = run_command(
            &context,
            &mut fixture.command("trap '' TERM; echo $$ > \"$1\"; sleep 60 & wait"),
            Duration::from_secs(4),
        );
        worker.join().unwrap();
        assert_eq!(result, Err("Cancelled".into()));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_stopped(fixture.pid());
    }

    #[test]
    fn v8_heap_failure_has_concise_resource_error() {
        let fixture = Fixture::new();
        assert_eq!(
            run(
                &Context::standalone(),
                &mut fixture.command(
                    "echo 'FATAL ERROR: Reached heap limit Allocation failed - JavaScript heap out of memory' >&2; \
                     echo 'raw V8 stack trace' >&2; exit 1"
                ),
                Duration::from_secs(2),
                true,
                1024,
                ample
            ),
            Err(MEMORY_ERROR.into())
        );
    }

    #[test]
    fn low_memory_no_longer_refuses_and_children_are_stopped_first() {
        let fixture = Fixture::new();
        let result = run(
            &Context::standalone(),
            &mut fixture.command("cat /proc/self/oom_score_adj"),
            Duration::from_secs(2),
            true,
            1024,
            |group| {
                let mut memory = ample(group)?;
                memory.available = 10 * MIB;
                memory.group_rss = 900 * MIB;
                Ok(memory)
            },
        );
        assert_eq!(result.unwrap().trim(), "1000");
    }

    #[test]
    fn resolver_spawn_preserves_literal_arguments_environment_and_directory() {
        let fixture = Fixture::new();
        let directory = std::env::temp_dir();
        let mut command = fixture.command("printf '%s\\n' \"$BP_LITERAL\"; printf '%s\\n' \"$3\"; pwd; cat /proc/self/oom_score_adj");
        let literal = "spaces ' quotes ; $(do-not-run) `do-not-run`";
        command
            .arg(literal)
            .env("BP_LITERAL", literal)
            .current_dir(&directory);
        let output = run(
            &Context::standalone(),
            &mut command,
            Duration::from_secs(2),
            true,
            4096,
            ample,
        )
        .unwrap();
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines[0], literal);
        assert_eq!(lines[1], literal);
        assert_eq!(
            std::fs::canonicalize(lines[2]).unwrap(),
            std::fs::canonicalize(directory).unwrap()
        );
        assert_eq!(lines[3], "1000");
    }

    #[test]
    fn protected_resolver_cancellation_stops_its_entire_process_group() {
        let fixture = Fixture::new();
        let context = Context::standalone();
        let cancel = context.clone();
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            cancel.cancel();
        });
        let result = run(
            &context,
            &mut fixture.command("trap '' TERM; echo $$ > \"$1\"; sleep 60 & wait"),
            Duration::from_secs(4),
            true,
            4096,
            ample,
        );
        worker.join().unwrap();
        assert_eq!(result, Err("Cancelled".into()));
        assert_stopped(fixture.pid());
    }

    #[test]
    fn a_resolver_killed_by_the_kernel_reports_memory() {
        let fixture = Fixture::new();
        assert_eq!(
            run(
                &Context::standalone(),
                &mut fixture.command("kill -KILL $$"),
                Duration::from_secs(2),
                true,
                1024,
                ample
            ),
            Err(MEMORY_ERROR.into())
        );
    }

    #[test]
    fn operation_names_do_not_log_media_urls() {
        let mut command = Command::new("yt-dlp");
        command.args(["-J", "https://www.youtube.com/watch?v=first000001"]);
        assert_eq!(operation_name(&command), "metadata");
        let mut command = Command::new("yt-dlp");
        command.args(["--load-info-json", "/private/info.json"]);
        assert_eq!(operation_name(&command), "cached-media");
    }

    #[test]
    fn output_limit_stops_tool_without_retaining_unbounded_metadata() {
        let fixture = Fixture::new();
        assert_eq!(
            run(
                &Context::standalone(),
                &mut fixture.command("printf '1234567890'"),
                Duration::from_secs(2),
                false,
                8,
                ample
            ),
            Err("External tool output was too large".into())
        );
    }

    #[test]
    fn proc_rss_parser_handles_parentheses_groups_and_zombies() {
        let mut fields = vec!["0"; 22];
        fields[0] = "R";
        fields[2] = "123";
        fields[21] = "456";
        let stat = format!("42 (Deno (worker) name) {}", fields.join(" "));
        assert_eq!(group_resident_pages(&stat, 123), Some(456));
        assert_eq!(group_resident_pages(&stat, 124), None);
        fields[0] = "Z";
        assert_eq!(
            group_resident_pages(&format!("42 (deno) {}", fields.join(" ")), 123),
            None
        );
    }
}
