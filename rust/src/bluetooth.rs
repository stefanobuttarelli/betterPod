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
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub powered: bool,
    pub devices: Vec<Device>,
}
#[derive(Clone, Debug)]
pub enum Request {
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
                while let Ok(request) = commands.recv() {
                    let outcome = execute(request).and_then(|_| refresh());
                    // Results are bounded; a hidden page cannot block the worker.
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
            command(&["connect", &address], Duration::from_secs(18)).map(|_| ())
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
fn command(args: &[&str], timeout: Duration) -> Result<String, String> {
    let executable =
        std::env::var("BETTERPOD_RUST_BLUETOOTHCTL").unwrap_or_else(|_| "bluetoothctl".into());
    let mut child = Command::new(executable)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
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
