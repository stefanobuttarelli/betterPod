//! Bounded external tools. Resolver children must never compete with audio for all RAM.
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
// Supported by the bundled Deno 2.9.8 / V8 15.0.245.2 runtime. yt-dlp passes
// DENO_V8_FLAGS through to Deno; an address-space rlimit would break V8's reservations.
const DENO_HEAP_FLAGS: &str = "--max-old-space-size=64,--max-semi-space-size=4";
const POLL: Duration = Duration::from_millis(25);

#[derive(Clone, Copy)]
struct MemoryBudget {
    group_rss: u64,
    reserve: u64,
    preflight: u64,
}
const RESOLVER_MEMORY: MemoryBudget = MemoryBudget {
    group_rss: 224 * MIB,
    reserve: 96 * MIB,
    preflight: 160 * MIB,
};
#[derive(Default)]
struct MemoryUsage {
    total: u64,
    available: u64,
    group_rss: u64,
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
        None,
        64 * MIB as usize,
        memory_usage,
    )
}

pub(super) fn run_ytdlp(
    context: &Context,
    command: &mut Command,
    timeout: Duration,
) -> Result<String, String> {
    // Desktop development is unaffected. The supported H700 console has 1 GiB.
    let constrained = cfg!(all(target_os = "linux", target_arch = "aarch64"))
        || (cfg!(target_os = "linux")
            && memory_usage(None).is_ok_and(|m| m.total <= 2 * 1024 * MIB));
    configure_runtime(command, constrained);
    run(
        context,
        command,
        timeout,
        constrained.then_some(RESOLVER_MEMORY),
        16 * MIB as usize,
        memory_usage,
    )
}

fn configure_runtime(command: &mut Command, constrained: bool) {
    if constrained {
        // Own these flags rather than appending an inherited, incompatible heap limit.
        command.env("DENO_V8_FLAGS", DENO_HEAP_FLAGS);
    }
}

fn run(
    context: &Context,
    command: &mut Command,
    timeout: Duration,
    budget: Option<MemoryBudget>,
    output_limit: usize,
    mut probe: impl FnMut(Option<u32>) -> Result<MemoryUsage, String>,
) -> Result<String, String> {
    context.check()?;
    if let Some(budget) = budget {
        if probe(None)?.available < budget.preflight {
            return Err(MEMORY_ERROR.into());
        }
    }
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|e| format!("Start {:?}: {e}", command.get_program()))?;
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
    let status = loop {
        context.check()?;
        if started.elapsed() >= timeout {
            return Err("Command timed out".into());
        }
        if let Some(budget) = budget {
            if last_probe.elapsed() >= Duration::from_millis(100) {
                let memory = probe(Some(group.child.id()))?;
                if memory.group_rss > budget.group_rss || memory.available < budget.reserve {
                    eprintln!(
                        "[YouTube] resolver memory limit: group={} MiB available={} MiB",
                        memory.group_rss / MIB,
                        memory.available / MIB
                    );
                    return Err(MEMORY_ERROR.into());
                }
                last_probe = Instant::now();
            }
        }
        out.drain(&mut stdout, Some(context))?;
        err.drain(&mut stderr, None)?;
        if let Some(status) = group.child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        thread::sleep(POLL);
    };
    // Even a failed/exited parent may leave Deno holding the pipe open. Kill the
    // whole owned group before draining, and never join a blocking reader thread.
    group.stop();
    let until = Instant::now() + Duration::from_millis(250);
    loop {
        let out_done = out.drain(&mut stdout, Some(context))?;
        let err_done = err.drain(&mut stderr, None)?;
        if out_done && err_done {
            break;
        }
        if Instant::now() >= until {
            return Err("External tool did not close its output".into());
        }
        thread::sleep(POLL);
    }
    let errors = err.text();
    if !status.success() {
        return Err(command_error(&errors, budget.is_some()));
    }
    Ok(out.text())
}

fn command_error(errors: &str, resolver: bool) -> String {
    if resolver {
        let lower = errors.to_ascii_lowercase();
        if lower.contains("heap out of memory")
            || lower.contains("reached heap limit")
            || lower.contains("javascript out of memory")
        {
            return MEMORY_ERROR.into();
        }
    }
    errors
        .lines()
        .rev()
        .find(|s| !s.trim().is_empty())
        .unwrap_or("External tool failed")
        .to_string()
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
                            let done = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                            let total = fields.next().and_then(|s| s.parse().ok());
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
        total: value("MemTotal:").ok_or("Unable to check free memory for YouTube")?,
        available: value("MemAvailable:").ok_or("Unable to check free memory for YouTube")?,
        group_rss: 0,
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
    let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    if fields.first()? == &"Z" || fields.get(2)?.parse::<u32>().ok()? != group {
        return None;
    }
    fields.get(21)?.parse().ok()
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
            total: 1024 * MIB,
            available: 512 * MIB,
            group_rss: 0,
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
    fn preflight_refuses_before_starting_child() {
        let fixture = Fixture::new();
        let mut command = fixture.command("echo started > \"$1\"");
        let result = run(
            &Context::standalone(),
            &mut command,
            Duration::from_secs(2),
            Some(RESOLVER_MEMORY),
            1024,
            |group| {
                assert!(group.is_none());
                let mut memory = ample(None)?;
                memory.available = RESOLVER_MEMORY.preflight - 1;
                Ok(memory)
            },
        );
        assert_eq!(result, Err(MEMORY_ERROR.into()));
        assert!(!fixture.0.join("pid").exists());
    }

    #[test]
    fn group_rss_and_free_memory_reserve_each_stop_child() {
        for rss_limit in [true, false] {
            let fixture = Fixture::new();
            let mut pid = None;
            let result = run(
                &Context::standalone(),
                &mut fixture.command("trap '' TERM; sleep 60 & wait"),
                Duration::from_secs(2),
                Some(RESOLVER_MEMORY),
                1024,
                |group| {
                    let mut memory = ample(group)?;
                    if let Some(group) = group {
                        pid = Some(group);
                        if rss_limit {
                            memory.group_rss = RESOLVER_MEMORY.group_rss + 1;
                        } else {
                            memory.available = RESOLVER_MEMORY.reserve - 1;
                        }
                    }
                    Ok(memory)
                },
            );
            assert_eq!(result, Err(MEMORY_ERROR.into()));
            assert_stopped(pid.unwrap());
        }
    }

    #[test]
    fn runtime_probe_failure_still_cleans_up_started_process() {
        let fixture = Fixture::new();
        let mut pid = None;
        let result = run(
            &Context::standalone(),
            &mut fixture.command("sleep 60 & wait"),
            Duration::from_secs(2),
            Some(RESOLVER_MEMORY),
            1024,
            |group| {
                if let Some(group) = group {
                    pid = Some(group);
                    Err("Unable to check YouTube memory use".into())
                } else {
                    ample(None)
                }
            },
        );
        assert_eq!(result, Err("Unable to check YouTube memory use".into()));
        assert_stopped(pid.unwrap());
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
                Some(RESOLVER_MEMORY),
                1024,
                ample
            ),
            Err(MEMORY_ERROR.into())
        );
    }

    #[test]
    fn supported_deno_flags_reach_child_without_conflicting_inherited_limit() {
        let fixture = Fixture::new();
        let mut command = fixture.command("printf '%s' \"$DENO_V8_FLAGS\"");
        command.env("DENO_V8_FLAGS", "--max-heap-size=9999");
        configure_runtime(&mut command, true);
        assert_eq!(
            run_command(&Context::standalone(), &mut command, Duration::from_secs(2)).unwrap(),
            DENO_HEAP_FLAGS
        );
    }

    #[test]
    fn output_limit_stops_tool_without_retaining_unbounded_metadata() {
        let fixture = Fixture::new();
        assert_eq!(
            run(
                &Context::standalone(),
                &mut fixture.command("printf '1234567890'"),
                Duration::from_secs(2),
                None,
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
