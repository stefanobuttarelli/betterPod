use super::*;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "betterpod-frontend-audio-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn journal(&self) -> PathBuf {
        self.0.join("handoff.json")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn process(pid: u32, name: &str, stopped: bool) -> Process {
    Process {
        pid,
        start: u64::from(pid) * 100,
        name: name.into(),
        stopped,
    }
}
fn client(index: u32, pid: u32) -> Value {
    json!({"index": index,"properties":{"application.process.id":pid.to_string(),"pipewire.sec.pid":pid.to_string(),"object.serial":format!("client-{index}")}})
}
fn input(index: u32, client: u32, node: u32, sink: u32) -> Value {
    // Actual ES sink-inputs omit application.process.id. Ownership must be
    // joined through the Pulse client, not guessed from application labels.
    json!({"index":index,"client":client,"sink":sink,"mute":false,"properties":{"application.name":"emulationstation","object.id":node.to_string(),"object.serial":format!("stream-{index}")}})
}
struct Fake {
    processes: BTreeMap<u32, Process>,
    clients: Vec<Value>,
    inputs: Vec<Value>,
    sinks: Vec<Value>,
    routes: BTreeMap<(u32, String), i64>,
    default_sink: u32,
    cookie: String,
    events: Vec<String>,
    fail_once: Option<String>,
}
impl Fake {
    fn new() -> Self {
        Self {
            processes: [
                process(100, "launcher", false),
                process(200, "emulationstatio", false),
                process(201, "emulationstation", false),
                process(300, "unrelated-player", false),
            ]
            .into_iter()
            .map(|p| (p.pid, p))
            .collect(),
            clients: vec![client(10, 200), client(99, 300)],
            inputs: vec![input(20, 10, 30, 2), input(88, 99, 89, 2)],
            sinks: vec![
                json!({"index":1,"name":"alsa_output.internal","properties":{"object.id":"101","object.serial":"10001","alsa.card_name":"audiocodec"}}),
                json!({"index":2,"name":"bluez_output.Sony","properties":{"object.id":"102","object.serial":"10002"}}),
            ],
            routes: BTreeMap::new(),
            default_sink: 1,
            cookie: "server-one".into(),
            events: Vec::new(),
            fail_once: None,
        }
    }
    fn route(&self, node: u32, key: &str) -> Option<i64> {
        self.routes.get(&(node, key.into())).copied()
    }
    fn set_route(&mut self, node: u32, key: &str, value: Option<i64>) {
        if let Some(value) = value {
            self.routes.insert((node, key.into()), value);
        } else {
            self.routes.remove(&(node, key.into()));
        }
        let desired = self
            .route(node, "target.object")
            .filter(|n| *n >= 0)
            .and_then(|n| {
                self.sinks
                    .iter()
                    .find(|s| property(s, "object.serial") == n.to_string())
            })
            .or_else(|| {
                self.route(node, "target.node")
                    .filter(|n| *n >= 0)
                    .and_then(|n| {
                        self.sinks
                            .iter()
                            .find(|s| number(&s["properties"]["object.id"]) == Some(n as u32))
                    })
            })
            .and_then(|s| number(&s["index"]))
            .unwrap_or(self.default_sink);
        for input in &mut self.inputs {
            if number(&input["properties"]["object.id"]) == Some(node) {
                input["sink"] = json!(desired);
            }
        }
    }
    fn owned(&self) -> &Value {
        self.inputs.iter().find(|i| i["index"] == 20).unwrap()
    }
    fn signal_name(signal: i32) -> &'static str {
        if signal == libc::SIGCONT {
            "CONT"
        } else if signal == libc::SIGSTOP {
            "STOP"
        } else {
            "OTHER"
        }
    }
    fn metadata_text(&self, filter: Option<(u32, &str)>) -> String {
        self.routes
            .iter()
            .filter(|((node, key), _)| filter.is_none_or(|(n, k)| *node == n && key == k))
            .map(|((node, key), value)| {
                format!("update: id:{node} key:'{key}' value:'{value}' type:'Spa:Id'\n")
            })
            .collect()
    }
}
impl Host for Fake {
    fn run(&mut self, tool: &str, args: &[String]) -> Result<String, String> {
        let event = format!("{tool} {}", args.join(" "));
        self.events.push(event.clone());
        if self.fail_once.as_deref() == Some(&event) {
            self.fail_once = None;
            return Err("Injected graph operation failure".into());
        }
        let a: Vec<_> = args.iter().map(String::as_str).collect();
        match (tool, a.as_slice()) {
            ("pactl", ["info"]) => Ok(format!(
                "Server Name: PulseAudio (on PipeWire)\nCookie: {}\n",
                self.cookie
            )),
            ("pactl", ["-f", "json", "list", what]) => serde_json::to_string(match *what {
                "clients" => &self.clients,
                "sink-inputs" => &self.inputs,
                "sinks" => &self.sinks,
                _ => panic!("Unexpected pactl list {what}"),
            })
            .map_err(|e| e.to_string()),
            ("pactl", ["set-sink-input-mute", index, mute]) => {
                let index: u32 = index.parse().unwrap();
                self.inputs
                    .iter_mut()
                    .find(|i| number(&i["index"]) == Some(index))
                    .ok_or("Input disappeared")?["mute"] = json!(*mute == "1");
                Ok(String::new())
            }
            ("pactl", ["move-sink-input", index, name]) => {
                let sink = self
                    .sinks
                    .iter()
                    .find(|s| s["name"] == *name)
                    .and_then(|s| number(&s["index"]))
                    .ok_or("Sink disappeared")?;
                let index: u32 = index.parse().unwrap();
                self.inputs
                    .iter_mut()
                    .find(|i| number(&i["index"]) == Some(index))
                    .ok_or("Input disappeared")?["sink"] = json!(sink);
                Ok(String::new())
            }
            ("pw-metadata", ["-n", "default"]) => Ok(self.metadata_text(None)),
            ("pw-metadata", ["-n", "default", node, key]) => {
                Ok(self.metadata_text(Some((node.parse().unwrap(), key))))
            }
            ("pw-metadata", ["-n", "default", "-d", node, key]) => {
                self.set_route(node.parse().unwrap(), key, None);
                Ok(String::new())
            }
            ("pw-metadata", ["-n", "default", node, key, value, "Spa:Id"]) => {
                self.set_route(node.parse().unwrap(), key, Some(value.parse().unwrap()));
                Ok(String::new())
            }
            _ => panic!("Unexpected host call: {event}"),
        }
    }
    fn process(&self, pid: u32) -> Option<Process> {
        self.processes.get(&pid).cloned()
    }
    fn signal(&mut self, expected: &Process, signal: i32) -> Result<(), String> {
        // Match the real host's PID/start-time/name guard before signaling.
        if !same_process(self, expected) {
            return Ok(());
        }
        self.events.push(format!(
            "signal {} {}",
            expected.pid,
            Self::signal_name(signal)
        ));
        self.processes.get_mut(&expected.pid).unwrap().stopped = signal == libc::SIGSTOP;
        Ok(())
    }
}

#[test]
fn joins_multiple_frontend_clients_by_pid_and_pins_even_an_internal_default() {
    let temp = Scratch::new();
    let path = temp.journal();
    let mut host = Fake::new();
    host.inputs[0]["sink"] = json!(1);
    host.clients
        .extend([client(11, 200), client(12, 201), client(13, 200)]);
    host.clients.last_mut().unwrap()["properties"]["pipewire.sec.pid"] = json!(300);
    host.inputs.extend([
        input(21, 11, 31, 1),
        input(22, 12, 32, 1),
        input(23, 13, 33, 2),
    ]);
    let unrelated = host.inputs[1].clone();
    let untrusted = host.inputs.last().unwrap().clone();
    assert!(host.inputs[0]["properties"]["application.process.id"].is_null());
    prepare(&path, 100, &[200, 201, 200], &mut host).unwrap();
    let plan = load(&path).unwrap().unwrap();
    assert_eq!(plan.processes.len(), 2);
    assert_eq!(plan.streams.len(), 3);
    for node in [30, 31, 32] {
        assert_eq!(host.route(node, "target.node"), Some(101));
        assert_eq!(host.route(node, "target.object"), Some(10001));
    }
    for index in [20, 21, 22] {
        let input = host.inputs.iter().find(|i| i["index"] == index).unwrap();
        assert_eq!(input["mute"], true);
        assert_eq!(input["sink"], 1);
    }
    assert_eq!(host.inputs[1], unrelated);
    assert_eq!(host.inputs.last().unwrap(), &untrusted);
    assert_eq!(&host.events[..2], ["signal 200 CONT", "signal 201 CONT"]);
    assert_eq!(
        &host.events[host.events.len() - 2..],
        ["signal 200 STOP", "signal 201 STOP"]
    );
    assert!(host
        .events
        .iter()
        .any(|e| e == "pactl move-sink-input 20 alsa_output.internal"));
}

#[test]
fn release_restores_follow_default_metadata_and_original_mute_after_cont() {
    for original in [None, Some(-1)] {
        let temp = Scratch::new();
        let path = temp.journal();
        let mut host = Fake::new();
        host.set_route(30, "target.node", original);
        host.set_route(30, "target.object", original);
        host.inputs[0]["mute"] = json!(true);
        prepare(&path, 100, &[200], &mut host).unwrap();
        host.events.clear();
        host.default_sink = 2;
        release_owned(&path, 100, false, &mut host).unwrap();
        assert_eq!(host.events.first().unwrap(), "signal 200 CONT");
        assert!(!host.events.iter().any(|e| e.ends_with(" STOP")));
        assert_eq!(host.route(30, "target.node"), original);
        assert_eq!(host.route(30, "target.object"), original);
        assert_eq!(host.owned()["mute"], true);
        assert_eq!(host.owned()["sink"], 2);
        assert!(!host.process(200).unwrap().stopped);
        assert!(load(&path).unwrap().unwrap().released);
        host.events.clear();
        release_owned(&path, 100, true, &mut host).unwrap();
        assert_eq!(host.events, ["signal 200 CONT"]);
        assert!(!path.exists());
    }
}

#[test]
fn explicit_route_rebinds_the_original_sink_after_its_node_ids_change() {
    let temp = Scratch::new();
    let path = temp.journal();
    let mut host = Fake::new();
    host.set_route(30, "target.node", Some(102));
    host.set_route(30, "target.object", Some(10002));
    prepare(&path, 100, &[200], &mut host).unwrap();
    host.sinks[1]["index"] = json!(7);
    host.sinks[1]["properties"]["object.id"] = json!(202);
    host.sinks[1]["properties"]["object.serial"] = json!(20002);
    host.events.clear();
    release_owned(&path, 100, true, &mut host).unwrap();
    assert_eq!(host.events.first().unwrap(), "signal 200 CONT");
    assert_eq!(host.route(30, "target.node"), Some(202));
    assert_eq!(host.route(30, "target.object"), Some(20002));
    assert_eq!(host.owned()["sink"], 7);
    assert_eq!(host.owned()["mute"], false);
    assert!(!path.exists());
}

#[test]
fn reused_stream_client_process_or_server_identities_are_never_restored() {
    for change in [
        "stream-serial",
        "client-serial",
        "client-index",
        "node",
        "pid",
        "process-start",
        "cookie",
    ] {
        let temp = Scratch::new();
        let path = temp.journal();
        let mut host = Fake::new();
        prepare(&path, 100, &[200], &mut host).unwrap();
        match change {
            "stream-serial" => host.inputs[0]["properties"]["object.serial"] = json!("replacement"),
            "client-serial" => {
                host.clients[0]["properties"]["object.serial"] = json!("replacement")
            }
            "client-index" => {
                host.inputs[0]["client"] = json!(11);
                host.clients.push(client(11, 200));
            }
            "node" => host.inputs[0]["properties"]["object.id"] = json!(300),
            "pid" => host.clients[0]["properties"]["application.process.id"] = json!(300),
            "process-start" => host.processes.get_mut(&200).unwrap().start += 1,
            "cookie" => host.cookie = "new-server".into(),
            _ => unreachable!(),
        }
        host.events.clear();
        release_owned(&path, 100, true, &mut host).unwrap();
        assert!(
            !host.events.iter().any(|e| e.starts_with("pactl set-")
                || e.starts_with("pactl move-")
                || e.starts_with("pw-metadata")),
            "Mutated reused identity: {change}: {:?}",
            host.events
        );
        if change == "process-start" {
            assert!(!host.events.iter().any(|e| e.starts_with("signal 200 ")));
        }
        assert!(!path.exists());
    }
}

#[test]
fn partial_handoff_failure_rolls_back_before_any_frontend_stop() {
    let temp = Scratch::new();
    let path = temp.journal();
    let mut host = Fake::new();
    host.set_route(30, "target.node", Some(102));
    host.set_route(30, "target.object", Some(10002));
    host.fail_once = Some("pw-metadata -n default 30 target.object 10001 Spa:Id".into());
    let error = prepare(&path, 100, &[200], &mut host).unwrap_err();
    assert!(error.contains("Injected"));
    assert!(!host.events.iter().any(|e| e.ends_with(" STOP")));
    assert_eq!(host.route(30, "target.node"), Some(102));
    assert_eq!(host.route(30, "target.object"), Some(10002));
    assert_eq!(host.owned()["mute"], false);
    assert_eq!(host.owned()["sink"], 2);
    assert!(!path.exists());
    let first_mutation = host
        .events
        .iter()
        .position(|e| e.starts_with("pactl set-"))
        .unwrap();
    assert!(host
        .events
        .iter()
        .enumerate()
        .any(|(i, e)| i > first_mutation && e == "signal 200 CONT"));
}

#[test]
fn originally_stopped_frontend_stays_resumed_until_finish_after_release() {
    let temp = Scratch::new();
    let path = temp.journal();
    let mut host = Fake::new();
    host.processes.get_mut(&200).unwrap().stopped = true;
    prepare(&path, 100, &[200], &mut host).unwrap();
    assert!(host.process(200).unwrap().stopped);
    host.events.clear();
    release_owned(&path, 100, false, &mut host).unwrap();
    assert!(!host.process(200).unwrap().stopped);
    assert_eq!(host.events[0], "signal 200 CONT");
    assert!(!host.events.iter().any(|e| e.ends_with(" STOP")));
    host.events.clear();
    release_owned(&path, 100, true, &mut host).unwrap();
    assert_eq!(host.events, ["signal 200 CONT", "signal 200 STOP"]);
    assert!(host.process(200).unwrap().stopped);
    assert!(!path.exists());
}

#[test]
fn live_journal_owner_blocks_takeover_but_dead_owner_is_recovered() {
    let temp = Scratch::new();
    let path = temp.journal();
    let mut host = Fake::new();
    prepare(&path, 100, &[200], &mut host).unwrap();
    host.processes.insert(101, process(101, "launcher", false));
    host.events.clear();
    assert!(prepare(&path, 101, &[200], &mut host)
        .unwrap_err()
        .contains("Another launcher"));
    assert!(host.events.is_empty());
    host.processes.remove(&100);
    prepare(&path, 101, &[200], &mut host).unwrap();
    let plan = load(&path).unwrap().unwrap();
    assert_eq!(plan.owner.pid, 101);
    assert_eq!(plan.streams.len(), 1);
    assert!(!plan.streams[0].mute);
    assert_eq!(host.events[0], "signal 200 CONT");
    let restored = host
        .events
        .iter()
        .position(|e| e == "pactl set-sink-input-mute 20 0")
        .unwrap();
    let muted = host
        .events
        .iter()
        .position(|e| e == "pactl set-sink-input-mute 20 1")
        .unwrap();
    assert!(restored < muted);
    release_owned(&path, 101, true, &mut host).unwrap();
    assert!(!host.process(200).unwrap().stopped);
}

#[test]
fn owner_pid_reuse_and_non_frontend_processes_cannot_gain_control() {
    let temp = Scratch::new();
    let path = temp.journal();
    let mut host = Fake::new();
    assert!(prepare(&path, 100, &[300], &mut host)
        .unwrap_err()
        .contains("non-frontend"));
    assert!(host.events.is_empty());
    assert!(!path.exists());
    prepare(&path, 100, &[200], &mut host).unwrap();
    host.events.clear();
    assert!(release_owned(&path, 999, true, &mut host).is_err());
    assert!(host.events.is_empty());
    host.processes.get_mut(&100).unwrap().start += 1;
    assert!(release_owned(&path, 100, true, &mut host).is_err());
    assert!(host.events.is_empty());
}

#[test]
fn missing_internal_sink_fails_safely_before_stop() {
    let temp = Scratch::new();
    let path = temp.journal();
    let mut host = Fake::new();
    host.sinks.remove(0);
    assert!(prepare(&path, 100, &[200], &mut host)
        .unwrap_err()
        .contains("internal"));
    assert!(!host
        .events
        .iter()
        .any(|e| e.ends_with(" STOP") || e.starts_with("pactl set-")));
    assert_eq!(host.owned()["mute"], false);
    assert!(!host.process(200).unwrap().stopped);
    assert!(!path.exists());
}
