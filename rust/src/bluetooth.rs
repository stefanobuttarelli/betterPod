//! BlueZ commands run on one bounded worker, never on the rendering thread.
pub use crate::worker_task::{WorkerHandle, WorkerReply};
use crate::{online::Context, worker_task::WorkerRequest};
use serde::{Deserialize, Serialize};
use std::process::Command;
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
    /// Visible to other devices for three minutes, so a car can find the
    /// console and start the pairing from its own screen.
    Visible,
    Connect(String),
    Disconnect(String),
    Forget(String),
}
pub struct Bluetooth {
    tx: mpsc::SyncSender<WorkerRequest<Request>>,
    rx: mpsc::Receiver<WorkerReply<Result<State, String>>>,
}
impl Bluetooth {
    pub fn new() -> Self {
        let (tx, commands) = mpsc::sync_channel::<WorkerRequest<Request>>(8);
        let (results, rx) = mpsc::sync_channel(8);
        thread::Builder::new()
            .name("bluetooth".into())
            .spawn(move || {
                let mut cached = State::default();
                while let Ok(request) = commands.recv() {
                    if request
                        .handle
                        .as_ref()
                        .is_some_and(WorkerHandle::is_cancelled)
                    {
                        continue;
                    }
                    let context = request
                        .handle
                        .as_ref()
                        .map_or_else(Context::standalone, |handle| handle.context.clone());
                    let outcome = if matches!(request.value, Request::Header) {
                        cached.audio_connected = read_default_sink();
                        let mut sample = cached.clone();
                        sample.header_only = true;
                        Ok(sample)
                    } else {
                        execute(request.value, &context).and_then(|_| {
                            context.check()?;
                            refresh(&context)
                        })
                    };
                    if context.check().is_err() {
                        continue;
                    }
                    if let Ok(state) = &outcome {
                        cached = state.clone();
                    }
                    if let Some(handle) = request.handle.as_ref() {
                        if results
                            .send(WorkerReply::owned(handle, outcome, true))
                            .is_err()
                        {
                            return;
                        }
                    } else {
                        let _ = results.try_send(WorkerReply::background(outcome));
                    }
                }
            })
            .expect("Bluetooth worker");
        Self { tx, rx }
    }
    pub fn request(&self, request: Request) -> bool {
        self.tx
            .try_send(WorkerRequest {
                value: request,
                handle: None,
            })
            .is_ok()
    }
    pub fn start(&self, request: Request) -> Option<WorkerHandle> {
        let handle = WorkerHandle::new();
        self.tx
            .try_send(WorkerRequest {
                value: request,
                handle: Some(handle.clone()),
            })
            .ok()?;
        Some(handle)
    }
    pub fn poll(&self) -> Option<Result<State, String>> {
        self.poll_owned().map(|reply| reply.value)
    }
    pub fn poll_owned(&self) -> Option<WorkerReply<Result<State, String>>> {
        while let Ok(reply) = self.rx.try_recv() {
            if !reply.is_cancelled() {
                return Some(reply);
            }
        }
        None
    }
}
impl Default for Bluetooth {
    fn default() -> Self {
        Self::new()
    }
}
fn execute(request: Request, context: &Context) -> Result<(), String> {
    context.check()?;
    match request {
        Request::Header => Ok(()),
        Request::Refresh => Ok(()),
        Request::Power(on) => command_owned(
            context,
            &["power", if on { "on" } else { "off" }],
            Duration::from_secs(8),
        )
        .map(|_| ()),
        Request::Scan => command_owned(
            context,
            &["--timeout", "12", "scan", "on"],
            Duration::from_secs(16),
        )
        .map(|_| ()),
        Request::Visible => {
            for args in [
                &["pairable", "on"][..],
                &["discoverable-timeout", "180"],
                &["discoverable", "on"],
            ] {
                command_owned(context, args, Duration::from_secs(5))?;
            }
            Ok(())
        }
        Request::Connect(address) => {
            connect_commands_with(
                &address,
                context,
                |args, timeout| command_owned(context, args, timeout),
                |address| {
                    // Our agent shows and confirms the code a car asks for.
                    crate::bt_media::pair(address, Duration::from_secs(60), || {
                        context.check().is_err()
                    })
                },
                |duration| context.sleep(duration),
            )?;
            let executable =
                std::env::var("BETTERPOD_RUST_PACTL").unwrap_or_else(|_| "pactl".into());
            let started = Instant::now();
            let routed = route_sink_with_timing(
                &address,
                |args, timeout| run_output_owned(context, &executable, args, timeout),
                || started.elapsed(),
                |duration| {
                    let _ = context.sleep(duration);
                },
            );
            context.check()?;
            if let Err(error) = routed {
                eprintln!("Bluetooth audio routing: {error}");
            }
            Ok(())
        }
        Request::Disconnect(address) => {
            validate(&address)?;
            command_owned(context, &["disconnect", &address], Duration::from_secs(12)).map(|_| ())
        }
        Request::Forget(address) => {
            validate(&address)?;
            command_owned(context, &["remove", &address], Duration::from_secs(8)).map(|_| ())
        }
    }
}
#[cfg(test)]
fn connect_commands(
    address: &str,
    context: &Context,
    run: impl FnMut(&[&str], Duration) -> Result<String, String>,
) -> Result<(), String> {
    connect_commands_with(address, context, run, |_| None, |_| Ok(()))
}
/// The profile names in bluetoothctl's `info` or `show`.
fn profile_names(listing: &str) -> Vec<&str> {
    listing
        .lines()
        .filter_map(|line| line.trim().strip_prefix("UUID:"))
        .map(|uuid| uuid.rsplit_once(" (").map_or(uuid, |(name, _)| name).trim())
        .collect()
}
/// What the console shows a car before pairing: its class (a phone's, see
/// `bt_class`) and its own profiles. Strict cars also want the hands-free
/// audio gateway, which PipeWire provides.
fn log_console(show: &str) {
    let class = show
        .lines()
        .find_map(|line| line.trim().strip_prefix("Class:"))
        .map_or("unknown", str::trim);
    let profiles = profile_names(show);
    let gateway = profiles
        .iter()
        .any(|p| p.contains("Handsfree Audio Gateway"));
    eprintln!(
        "Bluetooth console: class {class}, hands-free gateway {}, offers: {}",
        if gateway { "yes" } else { "no" },
        profiles.join(", ")
    );
}
/// Plain words for a failed `bluetoothctl connect`; the full one is logged.
fn connect_error(error: &str) -> String {
    if error.contains("connection-refused") || error.contains("Connection refused") {
        return "The device refused the connection. Try again".into();
    }
    if error.contains("page-timeout") || error.contains("Host is down") {
        return "No answer from the device. Turn it on and try again".into();
    }
    if error.contains("InProgress") || error.contains("busy") {
        return "The device is still connecting. Try again".into();
    }
    if error.contains("profile-unavailable") {
        return "The device offers no audio the console can play to".into();
    }
    error.to_owned()
}
/// `pair` pairs through D-Bus with the app's agent; `None` (no system bus)
/// falls back to bluetoothctl, which cannot answer a code. `wait` sleeps
/// unless cancelled.
fn connect_commands_with(
    address: &str,
    context: &Context,
    mut run: impl FnMut(&[&str], Duration) -> Result<String, String>,
    pair: impl Fn(&str) -> Option<Result<(), String>>,
    wait: impl Fn(Duration) -> Result<(), String>,
) -> Result<(), String> {
    context.check()?;
    validate(address)?;
    let info = run(&["info", address], Duration::from_secs(5)).unwrap_or_default();
    context.check()?;
    // What the device offers (audio, remote control, hands-free): the log
    // shows why a car accepts or drops the console.
    let profiles = profile_names(&info);
    if !profiles.is_empty() {
        eprintln!("Bluetooth {address} offers: {}", profiles.join(", "));
    }
    if !info.contains("Paired: yes") {
        log_console(&run(&["show"], Duration::from_secs(5)).unwrap_or_default());
        context.check()?;
        match pair(address) {
            Some(result) => result?,
            None => {
                run(
                    &["--timeout", "20", "pair", address],
                    Duration::from_secs(24),
                )?;
            }
        }
        context.check()?;
    }
    run(&["trust", address], Duration::from_secs(5))?;
    context.check()?;
    let first = match run(&["connect", address], Duration::from_secs(18)) {
        Err(error) if !error.contains("AlreadyConnected") => error,
        _ => return context.check(),
    };
    eprintln!("Bluetooth connect {address}: {first}");
    context.check()?;
    // Right after pairing a car connects its own profiles (hands-free,
    // then audio), and ours fails meanwhile: "in progress", "busy" or
    // refused. Let the car finish, then connect what is left.
    wait(Duration::from_secs(3))?;
    let second = match run(&["connect", address], Duration::from_secs(18)) {
        Ok(_) => return context.check(),
        Err(error) if error.contains("AlreadyConnected") => return context.check(),
        Err(error) => error,
    };
    context.check()?;
    let info = run(&["info", address], Duration::from_secs(5)).unwrap_or_default();
    context.check()?;
    if info.contains("Connected: yes") {
        eprintln!("Bluetooth {address}: connected by the device ({second})");
        return Ok(());
    }
    eprintln!("Bluetooth connect {address}: {second}");
    Err(connect_error(&first))
}
fn refresh(context: &Context) -> Result<State, String> {
    let controller = command_owned(context, &["show"], Duration::from_secs(5))?;
    let listing = command_owned(context, &["devices"], Duration::from_secs(5))?;
    let mut state = State {
        header_only: false,
        audio_connected: read_default_sink_owned(context),
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
        let info =
            command_owned(context, &["info", address], Duration::from_secs(4)).unwrap_or_default();
        context.check()?;
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
    read_default_sink_owned(&Context::standalone())
}
fn read_default_sink_owned(context: &Context) -> Option<bool> {
    let executable = std::env::var("BETTERPOD_RUST_PACTL").unwrap_or_else(|_| "pactl".into());
    if let Ok(output) = run_output_owned(
        context,
        &executable,
        &["get-default-sink"],
        Duration::from_secs(3),
    ) {
        if !output.trim().is_empty() {
            return Some(sink_is_bluetooth(&output));
        }
    }
    run_output_owned(context, &executable, &["info"], Duration::from_secs(3))
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
#[cfg(test)]
/// Source btUseSink: the Bluetooth output appears asynchronously after connect.
/// Both detection and routing stay on this worker, with a bounded overall wait.
fn route_sink(
    address: &str,
    run: impl FnMut(&[&str], Duration) -> Result<String, String>,
) -> Result<(), String> {
    let started = Instant::now();
    route_sink_with_timing(address, run, || started.elapsed(), thread::sleep)
}
fn route_sink_with_timing(
    address: &str,
    mut run: impl FnMut(&[&str], Duration) -> Result<String, String>,
    mut elapsed: impl FnMut() -> Duration,
    mut sleep: impl FnMut(Duration),
) -> Result<(), String> {
    let budget = Duration::from_secs(8);
    for attempt in 0..16 {
        let remaining = budget.saturating_sub(elapsed());
        if remaining.is_zero() {
            break;
        }
        // A slow or temporarily unavailable Pulse server is a failed probe,
        // not a reason to abandon the remaining output-discovery budget.
        if let Ok(listing) = run(
            &["list", "short", "sinks"],
            Duration::from_secs(1).min(remaining),
        ) {
            if let Some(sink) = device_sink(&listing, address) {
                let remaining = budget.saturating_sub(elapsed());
                if remaining.is_zero() {
                    break;
                }
                return run(
                    &["set-default-sink", sink],
                    Duration::from_secs(2).min(remaining),
                )
                .map(|_| ());
            }
        }
        let remaining = budget.saturating_sub(elapsed());
        if attempt == 15 || remaining.is_zero() {
            break;
        }
        sleep(Duration::from_millis(500).min(remaining));
    }
    Err(format!(
        "No audio output for {address}; keeping the current output"
    ))
}
fn command_owned(context: &Context, args: &[&str], timeout: Duration) -> Result<String, String> {
    let executable =
        std::env::var("BETTERPOD_RUST_BLUETOOTHCTL").unwrap_or_else(|_| "bluetoothctl".into());
    run_output_owned(context, &executable, args, timeout)
}
fn run_output_owned(
    context: &Context,
    executable: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    context.check()?;
    let mut process = Command::new(executable);
    process.args(args);
    #[cfg(target_os = "linux")]
    if std::path::Path::new("/var/run/pulse/native").exists() {
        if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
            process.env("XDG_RUNTIME_DIR", "/var/run");
        }
        if std::env::var_os("PULSE_SERVER").is_none() {
            process.env("PULSE_SERVER", "unix:/var/run/pulse/native");
        }
    }
    let output = crate::downloads::run_command(context, &mut process, timeout)?;
    if output.contains("Failed to ") {
        return Err(output
            .lines()
            .find(|line| line.contains("Failed"))
            .unwrap_or("Bluetooth command failed")
            .to_owned());
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[test]
    fn cancelled_pairing_cannot_run_trust_connect_or_audio_routing() {
        let context = Context::standalone();
        let mut calls = Vec::new();
        let result = connect_commands("58:18:62:28:8E:DA", &context, |args, _| {
            calls.push(args.join(" "));
            if args.contains(&"pair") {
                context.cancel();
            }
            Ok(String::new())
        });
        assert_eq!(result, Err("Cancelled".into()));
        assert_eq!(
            calls,
            [
                "info 58:18:62:28:8E:DA",
                "show",
                "--timeout 20 pair 58:18:62:28:8E:DA"
            ]
        );
    }
    /// bluetoothctl's answers for a car: `connects` lists what each
    /// `connect` returns; `info` is the device's state after them.
    fn car_connect(
        connects: &[Result<&str, &str>],
        connected_after: bool,
    ) -> (Result<(), String>, Vec<String>) {
        let context = Context::standalone();
        let mut calls = Vec::new();
        let mut connects = connects.iter();
        let mut infos = 0;
        let result = connect_commands_with(
            "58:18:62:28:8E:DA",
            &context,
            |args, _| {
                calls.push(args.join(" "));
                match args[0] {
                    "info" => {
                        infos += 1;
                        Ok(if infos == 1 {
                            "Paired: no\n\tUUID: Handsfree  (0000111e-0000-1000-8000-00805f9b34fb)\n".into()
                        } else if connected_after {
                            "Paired: yes\nConnected: yes\n".into()
                        } else {
                            "Paired: yes\nConnected: no\n".into()
                        })
                    }
                    "connect" => connects
                        .next()
                        .expect("no more connect attempts")
                        .map(String::from)
                        .map_err(String::from),
                    _ => Ok(String::new()),
                }
            },
            |_| Some(Ok(())),
            |_| Ok(()),
        );
        (result, calls)
    }
    #[test]
    fn a_car_that_connects_itself_after_pairing_is_connected() {
        let busy = "Failed to connect: org.bluez.Error.InProgress br-connection-busy";
        // our second attempt finds the car's profiles already up
        let (result, calls) = car_connect(
            &[
                Err(busy),
                Err("Failed to connect: org.bluez.Error.AlreadyConnected"),
            ],
            false,
        );
        assert_eq!(result, Ok(()));
        assert_eq!(calls.iter().filter(|c| c.starts_with("connect")).count(), 2);
        // or still fails, while the car holds the link
        let (result, _) = car_connect(&[Err(busy), Err(busy)], true);
        assert_eq!(result, Ok(()));
        // or simply works the second time
        let (result, calls) = car_connect(&[Err(busy), Ok("Connection successful")], false);
        assert_eq!(result, Ok(()));
        assert_eq!(calls.last().unwrap(), "connect 58:18:62:28:8E:DA");
        // the first attempt working needs no second one
        let (result, calls) = car_connect(&[Ok("Connection successful")], false);
        assert_eq!(result, Ok(()));
        assert_eq!(
            calls,
            [
                "info 58:18:62:28:8E:DA",
                "show",
                "trust 58:18:62:28:8E:DA",
                "connect 58:18:62:28:8E:DA"
            ]
        );
    }
    #[test]
    fn a_refused_connection_reads_plainly() {
        let refused = "Failed to connect: org.bluez.Error.Failed br-connection-refused";
        let (result, _) = car_connect(&[Err(refused), Err(refused)], false);
        assert_eq!(
            result,
            Err("The device refused the connection. Try again".into())
        );
        assert!(connect_error(
            "Failed to connect: org.bluez.Error.Failed br-connection-page-timeout"
        )
        .contains("No answer"));
        assert_eq!(
            connect_error("Failed to connect: something new"),
            "Failed to connect: something new"
        );
        assert_eq!(
            profile_names("Controller X\n\tClass: 0x006c020c\n\tUUID: Handsfree Audio Gateway (0000111f-0000-1000-8000-00805f9b34fb)\n\tUUID: Audio Source              (0000110a-0000-1000-8000-00805f9b34fb)\n"),
            ["Handsfree Audio Gateway", "Audio Source"]
        );
    }
    #[test]
    fn cancelled_owned_command_never_launches_a_physical_program() {
        let context = Context::standalone();
        context.cancel();
        assert_eq!(
            run_output_owned(
                &context,
                "/a/program/that/must/not/start",
                &["power", "off"],
                Duration::from_secs(1)
            ),
            Err("Cancelled".into())
        );
        let handle = WorkerHandle::new();
        let reply = WorkerReply::owned(&handle, Ok::<_, String>(State::default()), true);
        handle.cancel();
        assert!(
            reply.is_cancelled(),
            "A queued completion must become stale when its owner is cancelled"
        );
    }
    #[test]
    fn routing_retries_a_failed_probe_before_selecting_the_matching_sink() {
        let elapsed = Cell::new(Duration::ZERO);
        let mut calls = Vec::new();
        route_sink_with_timing(
            "58:18:62:28:8E:DA",
            |args, _| {
                calls.push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
                match calls.len() {
                    1 => Err("Pulse server temporarily unavailable".into()),
                    2 => Ok("13 bluez_output.58_18_62_28_8E_DA.a2dp-sink pipewire\n".into()),
                    3 => Ok(String::new()),
                    _ => panic!("routing continued after selecting the output"),
                }
            },
            || elapsed.get(),
            |delay| elapsed.set(elapsed.get() + delay),
        )
        .unwrap();
        assert_eq!(calls[0], vec!["list", "short", "sinks"]);
        assert_eq!(calls[1], calls[0]);
        assert_eq!(
            calls[2],
            vec![
                "set-default-sink",
                "bluez_output.58_18_62_28_8E_DA.a2dp-sink"
            ]
        );
        assert_eq!(elapsed.get(), Duration::from_millis(500));
    }
    #[test]
    fn repeated_probe_timeouts_exhaust_the_budget_without_reporting_success() {
        let elapsed = Cell::new(Duration::ZERO);
        let mut timeouts = Vec::new();
        let error = route_sink_with_timing(
            "58:18:62:28:8E:DA",
            |args, timeout| {
                assert_eq!(args, ["list", "short", "sinks"]);
                assert!(!timeout.is_zero());
                timeouts.push(timeout);
                elapsed.set(elapsed.get() + timeout);
                Err("Pulse request timed out".into())
            },
            || elapsed.get(),
            |delay| elapsed.set(elapsed.get() + delay),
        )
        .unwrap_err();
        assert!(error.contains("No audio output"));
        assert_eq!(elapsed.get(), Duration::from_secs(8));
        assert!(timeouts.len() > 1 && timeouts.len() <= 16);
        assert_eq!(timeouts.last(), Some(&Duration::from_millis(500)));
    }
    #[test]
    fn unrelated_sinks_exhaust_the_attempt_limit_without_changing_the_output() {
        let elapsed = Cell::new(Duration::ZERO);
        let mut probes = 0;
        assert!(route_sink_with_timing(
            "58:18:62:28:8E:DA",
            |args, _| {
                assert_eq!(args, ["list", "short", "sinks"]);
                probes += 1;
                Ok("12 alsa_output.codec pipewire\n13 bluez_output.00_00_00_00_00_00.a2dp-sink pipewire\n".into())
            },
            || elapsed.get(),
            |delay| elapsed.set(elapsed.get() + delay),
        ).is_err());
        assert_eq!(probes, 16);
        assert!(elapsed.get() < Duration::from_secs(8));
    }
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
