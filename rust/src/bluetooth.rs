//! BlueZ commands run on one bounded worker, never on the rendering thread.
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub address: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    pub battery: Option<u8>,
    #[serde(default)]
    pub icon: String,
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub header_only: bool,
    pub audio_connected: Option<bool>,
    pub powered: bool,
    pub devices: Vec<Device>,
}
#[derive(Clone, Debug)]
pub enum Request {
    Header,
    Refresh,
    Power(bool),
    Scan,
    Connect(String),
    Disconnect(String),
    Forget(String),
}
pub struct Bluetooth {
    tx: mpsc::SyncSender<Request>,
    rx: mpsc::Receiver<Result<State, String>>,
}
impl Bluetooth {
    pub fn new() -> Self {
        let (tx, commands) = mpsc::sync_channel(8);
        let (results, rx) = mpsc::sync_channel(8);
        thread::Builder::new()
            .name("bluetooth".into())
            .spawn(move || {
                let mut cached = State::default();
                while let Ok(request) = commands.recv() {
                    let outcome = if matches!(request, Request::Header) {
                        cached.audio_connected = read_default_sink();
                        let mut sample = cached.clone();
                        sample.header_only = true;
                        Ok(sample)
                    } else {
                        execute(request).and_then(|_| refresh())
                    };
                    if let Ok(state) = &outcome {
                        cached = state.clone();
                    }
                    let _ = results.try_send(outcome);
                }
            })
            .expect("Bluetooth worker");
        Self { tx, rx }
    }
    pub fn request(&self, request: Request) -> bool {
        self.tx.try_send(request).is_ok()
    }
    pub fn poll(&self) -> Option<Result<State, String>> {
        self.rx.try_recv().ok()
    }
}
impl Default for Bluetooth {
    fn default() -> Self {
        Self::new()
    }
}
fn execute(request: Request) -> Result<(), String> {
    match request {
        Request::Header => Ok(()),
        Request::Refresh => Ok(()),
        Request::Power(on) => command(
            &["power", if on { "on" } else { "off" }],
            Duration::from_secs(8),
        )
        .map(|_| ()),
        Request::Scan => {
            command(&["--timeout", "12", "scan", "on"], Duration::from_secs(16)).map(|_| ())
        }
        Request::Connect(address) => {
            validate(&address)?;
            let info = command(&["info", &address], Duration::from_secs(5)).unwrap_or_default();
            if !info.contains("Paired: yes") {
                command(
                    &["--timeout", "20", "pair", &address],
                    Duration::from_secs(24),
                )?;
            }
            command(&["trust", &address], Duration::from_secs(5))?;
            command(&["connect", &address], Duration::from_secs(18))?;
            let executable =
                std::env::var("BETTERPOD_RUST_PACTL").unwrap_or_else(|_| "pactl".into());
            if let Err(error) = route_sink(&address, |args, timeout| {
                run_output(&executable, args, timeout)
            }) {
                eprintln!("Bluetooth audio routing: {error}");
            }
            Ok(())
        }
        Request::Disconnect(address) => {
            validate(&address)?;
            command(&["disconnect", &address], Duration::from_secs(12)).map(|_| ())
        }
        Request::Forget(address) => {
            validate(&address)?;
            command(&["remove", &address], Duration::from_secs(8)).map(|_| ())
        }
    }
}
fn refresh() -> Result<State, String> {
    let controller = command(&["show"], Duration::from_secs(5))?;
    let listing = command(&["devices"], Duration::from_secs(5))?;
    let mut state = State {
        header_only: false,
        audio_connected: read_default_sink(),
        powered: controller.contains("Powered: yes"),
        devices: Vec::new(),
    };
    for line in listing.lines() {
        let Some(rest) = line.trim().strip_prefix("Device ") else {
            continue;
        };
        let Some((address, name)) = rest.split_once(' ') else {
            continue;
        };
        if validate(address).is_err() {
            continue;
        }
        let info = command(&["info", address], Duration::from_secs(4)).unwrap_or_default();
        state.devices.push(Device {
            address: address.to_owned(),
            name: name.trim().to_owned(),
            paired: info.contains("Paired: yes"),
            connected: info.contains("Connected: yes"),
            battery: parse_battery(&info),
            icon: info
                .lines()
                .find_map(|line| {
                    line.trim()
                        .strip_prefix("Icon:")
                        .map(|s| s.trim().to_string())
                })
                .unwrap_or_default(),
        });
        if state.devices.len() >= 128 {
            break;
        }
    }
    state.devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.paired.cmp(&a.paired))
            .then(a.name.cmp(&b.name))
    });
    Ok(state)
}
fn parse_battery(info: &str) -> Option<u8> {
    info.lines()
        .find(|line| line.contains("Battery Percentage:"))
        .and_then(|line| line.rsplit_once('('))
        .and_then(|(_, n)| n.trim_end_matches(')').trim().parse::<u8>().ok())
        .map(|n| n.min(100))
}
fn validate(address: &str) -> Result<(), String> {
    if address.len() == 17
        && address.bytes().enumerate().all(|(i, b)| {
            if i % 3 == 2 {
                b == b':'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        Ok(())
    } else {
        Err("Invalid Bluetooth device address".into())
    }
}
fn sink_is_bluetooth(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("bluez_output.") || value.starts_with("bluez_sink.")
}
fn read_default_sink() -> Option<bool> {
    let executable = std::env::var("BETTERPOD_RUST_PACTL").unwrap_or_else(|_| "pactl".into());
    if let Ok(output) = run_output(&executable, &["get-default-sink"], Duration::from_secs(3)) {
        if !output.trim().is_empty() {
            return Some(sink_is_bluetooth(&output));
        }
    }
    run_output(&executable, &["info"], Duration::from_secs(3))
        .ok()?
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("Default Sink:")
                .map(sink_is_bluetooth)
        })
}
fn device_sink<'a>(listing: &'a str, address: &str) -> Option<&'a str> {
    let id = address.replace(':', "_").to_ascii_uppercase();
    listing
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .find(|sink| sink.starts_with("bluez_") && sink.to_ascii_uppercase().contains(&id))
}
/// Source btUseSink: the Bluetooth output appears asynchronously after connect.
/// Both detection and routing stay on this worker, with a bounded overall wait.
fn route_sink(
    address: &str,
    mut run: impl FnMut(&[&str], Duration) -> Result<String, String>,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(8);
    for attempt in 0..16 {
        let listing = run(&["list", "short", "sinks"], Duration::from_secs(1))?;
        if let Some(sink) = device_sink(&listing, address) {
            run(&["set-default-sink", sink], Duration::from_secs(2))?;
            return Ok(());
        }
        if attempt == 15 || Instant::now() >= deadline {
            break;
        }
        thread::sleep(
            Duration::from_millis(500).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    Err(format!(
        "No audio output for {address}; keeping the current output"
    ))
}
fn command(args: &[&str], timeout: Duration) -> Result<String, String> {
    let executable =
        std::env::var("BETTERPOD_RUST_BLUETOOTHCTL").unwrap_or_else(|_| "bluetoothctl".into());
    run_output(&executable, args, timeout)
}
fn run_output(executable: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut process = Command::new(executable);
    process
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    #[cfg(target_os = "linux")]
    if std::path::Path::new("/var/run/pulse/native").exists() {
        if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
            process.env("XDG_RUNTIME_DIR", "/var/run");
        }
        if std::env::var_os("PULSE_SERVER").is_none() {
            process.env("PULSE_SERVER", "unix:/var/run/pulse/native");
        }
    }
    let mut child = process
        .spawn()
        .map_err(|e| format!("Bluetooth unavailable: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(1024 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(1024 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Err("Bluetooth did not respond in time".into());
            }
        }
    };
    let output = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    let error = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    if !status.success() || output.contains("Failed to ") {
        return Err(if error.trim().is_empty() {
            output
                .lines()
                .find(|l| l.contains("Failed"))
                .unwrap_or("Bluetooth command failed")
                .to_owned()
        } else {
            error.trim().to_owned()
        });
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connect_routes_only_matching_bluetooth_sink_with_structured_arguments() {
        let listing = "12 alsa_output.platform.codec pipewire
13 bluez_output.58_18_62_28_8E_DA.a2dp-sink pipewire
14 bluez_output.00_00_00_00_00_00.a2dp-sink pipewire
";
        assert!(device_sink(listing, "11:11:11:11:11:11").is_none());
        let mut calls = Vec::new();
        route_sink("58:18:62:28:8e:da", |args, _| {
            calls.push(args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>());
            if args[0] == "list" {
                Ok(listing.into())
            } else {
                Ok(String::new())
            }
        })
        .unwrap();
        assert_eq!(
            calls,
            vec![
                vec!["list", "short", "sinks"],
                vec![
                    "set-default-sink",
                    "bluez_output.58_18_62_28_8E_DA.a2dp-sink"
                ]
            ]
        );
    }
    #[test]
    fn audio_header_follows_actual_default_sink() {
        assert!(sink_is_bluetooth("bluez_output.58_18.a2dp-sink\n"));
        assert!(sink_is_bluetooth("bluez_sink.58_18.a2dp_sink"));
        assert!(!sink_is_bluetooth(
            "alsa_output.platform.codec.stereo-fallback"
        ));
        assert!(!sink_is_bluetooth("headphones connected"));
    }
    #[test]
    fn address_is_a_device_not_a_command() {
        assert!(validate("58:18:62:28:8E:DA").is_ok());
        for bad in ["../phone", "58:18:62:28:8E:DA;reboot", "--help", ""] {
            assert!(validate(bad).is_err());
        }
    }
    #[test]
    fn battery_decodes_bluez_hex_and_decimal() {
        assert_eq!(parse_battery("Battery Percentage: 0x4e (78)"), Some(78));
        assert_eq!(parse_battery("Connected: yes"), None);
    }
}
