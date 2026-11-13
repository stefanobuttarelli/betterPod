//! Park only the frontend's audio before freezing its process. A stopped
//! PipeWire client cannot acknowledge Bluetooth graph changes.
use crate::{downloads, online::Context, storage::atomic_write};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, ffi::OsString, fs, path::Path, process::Command, time::Duration};

const LIMIT: u64 = 256 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Process {
    pid: u32,
    start: u64,
    name: String,
    stopped: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Stream {
    index: u32,
    client: u32,
    client_serial: String,
    serial: String,
    node: u32,
    pid: u32,
    mute: bool,
    sink: String,
    target_node: Option<i64>,
    target_object: Option<i64>,
}
#[derive(Debug, Serialize, Deserialize)]
struct Plan {
    version: u32,
    owner: Process,
    processes: Vec<Process>,
    streams: Vec<Stream>,
    cookie: Option<String>,
    released: bool,
}

trait Host {
    fn run(&mut self, tool: &str, args: &[String]) -> Result<String, String>;
    fn process(&self, pid: u32) -> Option<Process>;
    fn signal(&mut self, process: &Process, signal: i32) -> Result<(), String>;
    fn owns_display(&self, process: &Process) -> Result<bool, String>;
}
struct System;
impl Host for System {
    fn owns_display(&self, process: &Process) -> Result<bool, String> {
        let descriptors = fs::read_dir(format!("/proc/{}/fd", process.pid))
            .map_err(|e| format!("Cannot inspect frontend display ownership: {e}"))?;
        for entry in descriptors {
            let entry = entry.map_err(|e| e.to_string())?;
            match fs::read_link(entry.path()) {
                Ok(path) if display_device(&path) => return Ok(true),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("Cannot inspect frontend display descriptor: {e}")),
            }
        }
        Ok(false)
    }
    fn run(&mut self, tool: &str, args: &[String]) -> Result<String, String> {
        let mut command = Command::new(tool);
        command.args(args).env("LC_ALL", "C");
        let output = downloads::run_command(&Context::standalone(), &mut command, TIMEOUT)?;
        if output.len() > 2 * 1024 * 1024 {
            return Err("Frontend audio response is too large".into());
        }
        Ok(output)
    }
    fn process(&self, pid: u32) -> Option<Process> {
        process_stat(pid, &fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
    }
    fn signal(&mut self, process: &Process, signal: i32) -> Result<(), String> {
        if !same_process(self, process) {
            return Ok(());
        }
        if unsafe { libc::kill(process.pid as i32, signal) } < 0 {
            return Err(format!(
                "Frontend signal failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
}

/// The helper path returns before SDL, UI, library or audio initialization.
pub fn run_cli(args: impl Iterator<Item = OsString>) -> Option<Result<(), String>> {
    let args: Vec<_> = args.collect();
    if args.first()?.to_str()? != "--frontend-audio" {
        return None;
    }
    Some((|| {
        if args.len() < 4 {
            return Err(
                "Expected --frontend-audio prepare|release|finish STATE OWNER_PID [ES_PID...]"
                    .into(),
            );
        }
        let mode = args[1].to_str().ok_or("Invalid frontend audio operation")?;
        let path = Path::new(&args[2]);
        let owner = args[3]
            .to_str()
            .and_then(|s| s.parse().ok())
            .ok_or("Invalid launcher PID")?;
        let pids = args[4..]
            .iter()
            .map(|s| {
                s.to_str()
                    .and_then(|s| s.parse().ok())
                    .ok_or("Invalid frontend PID".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut host = System;
        match mode {
            "prepare" => prepare(path, owner, &pids, &mut host),
            "release" => release_owned(path, owner, false, &mut host),
            "finish" => release_owned(path, owner, true, &mut host),
            _ => Err("Unknown frontend audio operation".into()),
        }
    })())
}

fn call(host: &mut impl Host, tool: &str, args: &[&str]) -> Result<String, String> {
    host.run(
        tool,
        &args.iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
    )
}
fn json(host: &mut impl Host, what: &str) -> Result<Vec<Value>, String> {
    serde_json::from_str(&call(host, "pactl", &["-f", "json", "list", what])?)
        .map_err(|e| format!("Invalid frontend audio {what}: {e}"))
}
fn number(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|n| n.try_into().ok())
        .or_else(|| value.as_str()?.parse().ok())
}
fn property(value: &Value, key: &str) -> String {
    match &value["properties"][key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}
fn process_stat(pid: u32, stat: &str) -> Option<Process> {
    let (_, rest) = stat.split_once('(')?;
    let (name, rest) = rest.rsplit_once(')')?;
    let fields: Vec<_> = rest.split_whitespace().collect();
    Some(Process {
        pid,
        start: fields.get(19)?.parse().ok()?,
        name: name.into(),
        stopped: matches!(*fields.first()?, "T" | "t"),
    })
}
fn same_process(host: &impl Host, expected: &Process) -> bool {
    host.process(expected.pid)
        .is_some_and(|p| p.start == expected.start && p.name == expected.name)
}
fn frontend(process: &Process) -> bool {
    matches!(
        process.name.as_str(),
        "emulationstation" | "emulationstatio"
    )
}
fn display_device(path: &Path) -> bool {
    let name = path.to_string_lossy();
    ["/dev/fb", "/dev/dri/card"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit()))
    })
}

fn require_released_display(host: &impl Host, process: &Process) -> Result<(), String> {
    if host.owns_display(process)? {
        return Err("EmulationStation still owns the display. Start betterPod Rust from Ports or EmulationStation's local /launch API; do not run its launcher directly over SSH.".into());
    }
    Ok(())
}
fn cookie(host: &mut impl Host) -> Result<Option<String>, String> {
    Ok(call(host, "pactl", &["info"])?
        .lines()
        .find_map(|line| line.strip_prefix("Cookie:").map(|s| s.trim().into())))
}
fn metadata(text: &str, node: u32, key: &str) -> Result<Option<i64>, String> {
    let prefix = format!("update: id:{node} key:'{key}' value:'");
    for line in text.lines() {
        if let Some(value) = line.trim().strip_prefix(&prefix) {
            let value = value
                .split_once('\'')
                .ok_or("Invalid frontend route metadata")?
                .0;
            return value
                .parse()
                .map(Some)
                .map_err(|_| "Invalid frontend route metadata value".into());
        }
    }
    Ok(None)
}
fn set_metadata(
    host: &mut impl Host,
    node: u32,
    key: &str,
    value: Option<i64>,
) -> Result<(), String> {
    let node = node.to_string();
    if let Some(value) = value {
        call(
            host,
            "pw-metadata",
            &["-n", "default", &node, key, &value.to_string(), "Spa:Id"],
        )?;
    } else {
        call(host, "pw-metadata", &["-n", "default", "-d", &node, key])?;
    }
    let text = call(host, "pw-metadata", &["-n", "default", &node, key])?;
    if metadata(&text, node.parse().unwrap(), key)? != value {
        return Err("Frontend audio route metadata did not update".into());
    }
    Ok(())
}
fn client_pid<'a>(input: &Value, clients: &'a [Value]) -> Option<(u32, &'a Value)> {
    let index = number(&input["client"])?;
    let client = clients
        .iter()
        .find(|c| number(&c["index"]) == Some(index))?;
    // PipeWire's authenticated peer PID takes precedence over an application's
    // self-reported PID, which another client could copy into its properties.
    let pid = number(&client["properties"]["pipewire.sec.pid"])
        .or_else(|| number(&client["properties"]["application.process.id"]))?;
    Some((pid, client))
}
fn identity(
    input: &Value,
    clients: &[Value],
    processes: &[Process],
) -> Result<Option<Stream>, String> {
    let Some((pid, client)) = client_pid(input, clients) else {
        if !processes.is_empty()
            && [
                property(input, "node.name"),
                property(input, "application.name"),
            ]
            .iter()
            .any(|name| name.to_ascii_lowercase().contains("emulationstation"))
        {
            return Err("Cannot verify frontend audio owner".into());
        }
        return Ok(None);
    };
    if !processes.iter().any(|p| p.pid == pid) {
        return Ok(None);
    }
    if number(&client["properties"]["application.process.id"])
        .is_some_and(|declared| declared != pid)
    {
        return Err("Conflicting frontend audio process identity".into());
    }
    let index = number(&input["index"]).ok_or("Missing frontend stream index")?;
    let node = number(&input["properties"]["object.id"]).ok_or("Missing frontend node ID")?;
    let serial = property(input, "object.serial");
    let client_serial = property(client, "object.serial");
    if serial.is_empty() || client_serial.is_empty() {
        return Err("Missing frontend audio identity".into());
    }
    Ok(Some(Stream {
        index,
        client: number(&client["index"]).unwrap(),
        client_serial,
        serial,
        node,
        pid,
        mute: input["mute"]
            .as_bool()
            .ok_or("Missing frontend mute state")?,
        sink: String::new(),
        target_node: None,
        target_object: None,
    }))
}
fn matches_stream(
    input: &Value,
    clients: &[Value],
    expected: &Stream,
    processes: &[Process],
    host: &impl Host,
) -> bool {
    if !processes
        .iter()
        .any(|p| p.pid == expected.pid && same_process(host, p))
    {
        return false;
    }
    identity(input, clients, processes)
        .ok()
        .flatten()
        .is_some_and(|now| {
            now.index == expected.index
                && now.client == expected.client
                && now.client_serial == expected.client_serial
                && now.serial == expected.serial
                && now.node == expected.node
                && now.pid == expected.pid
        })
}

fn check_stream(
    host: &mut impl Host,
    stream: &Stream,
    processes: &[Process],
) -> Result<(), String> {
    let inputs = json(host, "sink-inputs")?;
    let clients = json(host, "clients")?;
    if inputs
        .iter()
        .any(|input| matches_stream(input, &clients, stream, processes, host))
    {
        Ok(())
    } else {
        Err("Frontend stream changed before audio operation".into())
    }
}

fn settle_stream(
    host: &mut impl Host,
    stream: &Stream,
    processes: &[Process],
    sink: u32,
    mute: bool,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    loop {
        let inputs = json(host, "sink-inputs")?;
        let clients = json(host, "clients")?;
        let input = inputs
            .iter()
            .find(|input| matches_stream(input, &clients, stream, processes, host))
            .ok_or("Frontend stream changed during audio handoff")?;
        if number(&input["sink"]) == Some(sink) && input["mute"].as_bool() == Some(mute) {
            return Ok(());
        }
        if started.elapsed() >= TIMEOUT {
            return Err("Frontend audio handoff did not settle".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
fn save(path: &Path, plan: &Plan) -> Result<(), String> {
    atomic_write(path, &serde_json::to_vec(plan).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn load(path: &Path) -> Result<Option<Plan>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > LIMIT {
        return Err("Unsafe frontend audio journal".into());
    }
    let plan: Plan = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if plan.version != 1 || plan.processes.len() > 16 || plan.streams.len() > 256 {
        return Err("Unsupported frontend audio journal".into());
    }
    Ok(Some(plan))
}

fn prepare(path: &Path, owner: u32, pids: &[u32], host: &mut impl Host) -> Result<(), String> {
    if pids.len() > 16 {
        return Err("Too many frontend processes".into());
    }
    if let Some(mut stale) = load(path)? {
        if same_process(host, &stale.owner) {
            return Err("Another launcher owns frontend audio".into());
        }
        release(path, &mut stale, host)?;
        finish(path, &stale, host)?;
    }
    let owner = host.process(owner).ok_or("Launcher process disappeared")?;
    let mut seen = BTreeSet::new();
    let processes = pids
        .iter()
        .filter(|&&pid| seen.insert(pid))
        .map(|&pid| {
            let p = host.process(pid).ok_or("Frontend process disappeared")?;
            if !frontend(&p) {
                return Err("Refusing to suspend a non-frontend process");
            }
            Ok(p)
        })
        .collect::<Result<Vec<_>, &str>>()?;
    // SIGSTOP is safe only after the native launcher has released scanout.
    // Freezing a live Mali framebuffer owner can stall both applications, and
    // a raw SSH launch also bypasses configgen's controller setup.
    for process in &processes {
        require_released_display(host, process)?;
    }
    let mut plan = Plan {
        version: 1,
        owner,
        processes,
        streams: Vec::new(),
        cookie: None,
        released: false,
    };
    save(path, &plan)?;
    let result = (|| {
        // A caller may already have stopped ES. Let its audio client answer all
        // control requests before taking snapshots or changing any graph links.
        for p in &plan.processes {
            host.signal(p, libc::SIGCONT)?;
        }
        if plan.processes.is_empty() {
            return Ok(());
        }
        let inputs = json(host, "sink-inputs")?;
        let clients = json(host, "clients")?;
        let sinks = json(host, "sinks")?;
        let target = sinks
            .iter()
            .filter(|s| {
                s["name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with("alsa_output."))
            })
            .min_by_key(|s| property(s, "alsa.card_name") != "audiocodec")
            .ok_or("No stable internal audio sink for frontend")?;
        let target_name = target["name"]
            .as_str()
            .ok_or("Missing internal sink name")?;
        let target_node =
            number(&target["properties"]["object.id"]).ok_or("Missing internal sink ID")?;
        let target_serial: i64 = property(target, "object.serial")
            .parse()
            .map_err(|_| "Missing internal sink serial")?;
        let route = call(host, "pw-metadata", &["-n", "default"])?;
        plan.cookie = cookie(host)?;
        for input in &inputs {
            if let Some(mut stream) = identity(input, &clients, &plan.processes)? {
                let sink = sinks
                    .iter()
                    .find(|s| number(&s["index"]) == number(&input["sink"]))
                    .ok_or("Frontend sink disappeared")?;
                stream.sink = sink["name"]
                    .as_str()
                    .ok_or("Missing frontend sink name")?
                    .into();
                stream.target_node = metadata(&route, stream.node, "target.node")?;
                stream.target_object = metadata(&route, stream.node, "target.object")?;
                plan.streams.push(stream);
            }
        }
        if plan.streams.len() > 256 {
            return Err("Too many frontend audio streams".into());
        }
        save(path, &plan)?;
        for stream in &plan.streams {
            let index = stream.index.to_string();
            check_stream(host, stream, &plan.processes)?;
            call(host, "pactl", &["set-sink-input-mute", &index, "1"])?;
            check_stream(host, stream, &plan.processes)?;
            call(host, "pactl", &["move-sink-input", &index, target_name])?;
            // Moving to the current default clears preference in Pulse. Explicit
            // per-node metadata is necessary when Bluetooth is connected later.
            check_stream(host, stream, &plan.processes)?;
            set_metadata(host, stream.node, "target.node", Some(target_node as i64))?;
            check_stream(host, stream, &plan.processes)?;
            set_metadata(host, stream.node, "target.object", Some(target_serial))?;
            settle_stream(
                host,
                stream,
                &plan.processes,
                number(&target["index"]).ok_or("Missing internal sink index")?,
                true,
            )?;
        }
        let inputs = json(host, "sink-inputs")?;
        let clients = json(host, "clients")?;
        for stream in &plan.streams {
            let input = inputs
                .iter()
                .find(|i| matches_stream(i, &clients, stream, &plan.processes, host))
                .ok_or("Frontend stream changed during handoff")?;
            if input["mute"] != true || number(&input["sink"]) != number(&target["index"]) {
                return Err("Frontend audio handoff did not settle".into());
            }
        }
        for input in &inputs {
            if let Some(found) = identity(input, &clients, &plan.processes)? {
                if !plan
                    .streams
                    .iter()
                    .any(|s| s.serial == found.serial && s.client_serial == found.client_serial)
                {
                    return Err("New frontend stream appeared during handoff".into());
                }
            }
        }
        for p in &plan.processes {
            if !same_process(host, p) {
                return Err("Frontend restarted during handoff".into());
            }
            host.signal(p, libc::SIGSTOP)?;
            require_released_display(host, p)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let cleanup = release(path, &mut plan, host).and_then(|_| finish(path, &plan, host));
        return Err(match cleanup {
            Ok(()) => error,
            Err(restore) => format!("{error}; frontend restore: {restore}"),
        });
    }
    Ok(())
}

fn release_owned(path: &Path, owner: u32, done: bool, host: &mut impl Host) -> Result<(), String> {
    let Some(mut plan) = load(path)? else {
        return Ok(());
    };
    if plan.owner.pid != owner || !same_process(host, &plan.owner) {
        return Err("Frontend journal belongs to a different launcher".into());
    }
    release(path, &mut plan, host)?;
    if done {
        finish(path, &plan, host)?;
    }
    Ok(())
}
fn release(path: &Path, plan: &mut Plan, host: &mut impl Host) -> Result<(), String> {
    // This must precede every PipeWire operation, even if a previous release
    // partially failed or the child app is still blocked while shutting down.
    for p in &plan.processes {
        host.signal(p, libc::SIGCONT)?;
    }
    if plan.released {
        return Ok(());
    }
    if !plan.streams.is_empty() {
        let current_cookie = cookie(host)?;
        if plan.cookie.is_none() || current_cookie == plan.cookie {
            let inputs = json(host, "sink-inputs")?;
            let clients = json(host, "clients")?;
            let sinks = json(host, "sinks")?;
            for stream in &plan.streams {
                if !inputs
                    .iter()
                    .any(|input| matches_stream(input, &clients, stream, &plan.processes, host))
                {
                    continue;
                }
                let preferred = stream.target_object.is_some_and(|n| n >= 0)
                    || stream.target_node.is_some_and(|n| n >= 0);
                let sink = preferred
                    .then(|| {
                        sinks
                            .iter()
                            .find(|s| s["name"].as_str() == Some(&stream.sink))
                    })
                    .flatten();
                let target_node = if preferred {
                    sink.and_then(|s| number(&s["properties"]["object.id"]))
                        .map(i64::from)
                } else {
                    stream.target_node
                };
                let target_object = if preferred {
                    sink.and_then(|s| property(s, "object.serial").parse().ok())
                } else {
                    stream.target_object
                };
                check_stream(host, stream, &plan.processes)?;
                set_metadata(host, stream.node, "target.node", target_node)?;
                check_stream(host, stream, &plan.processes)?;
                set_metadata(host, stream.node, "target.object", target_object)?;
                check_stream(host, stream, &plan.processes)?;
                call(
                    host,
                    "pactl",
                    &[
                        "set-sink-input-mute",
                        &stream.index.to_string(),
                        if stream.mute { "1" } else { "0" },
                    ],
                )?;
            }
        }
    }
    plan.released = true;
    save(path, plan)
}
fn finish(path: &Path, plan: &Plan, host: &mut impl Host) -> Result<(), String> {
    // The launcher calls finish only after wait(app), so restoring a caller's
    // originally suspended frontend cannot hold up the app's audio shutdown.
    for p in &plan.processes {
        if p.stopped {
            host.signal(p, libc::SIGSTOP)?;
        }
    }
    fs::remove_file(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod qa;
