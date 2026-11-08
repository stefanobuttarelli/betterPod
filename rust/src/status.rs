//! Kernel status snapshots. No sysfs or network reads happen while drawing.
use std::{
    fs,
    path::Path,
    sync::{mpsc, Arc, Condvar, Mutex},
    thread,
    time::Duration,
};
#[derive(Clone, Copy, Debug)]
pub struct Status {
    pub battery: Option<u8>,
    pub charging: bool,
    pub external_power: bool,
    pub wifi: i8,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            battery: None,
            charging: false,
            external_power: false,
            wifi: -1,
        }
    }
}
pub struct Watcher {
    rx: mpsc::Receiver<Status>,
    stop: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Watcher {
    pub fn new() -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let signal = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("header-status".into())
            .spawn(move || loop {
                if let Ok(stop) = signal.0.lock() {
                    if *stop {
                        break;
                    }
                }
                let _ = tx.try_send(read_status(
                    Path::new("/sys"),
                    Path::new("/proc/net/wireless"),
                ));
                let stop = signal.0.lock().unwrap_or_else(|e| e.into_inner());
                let (stop, _) = signal
                    .1
                    .wait_timeout_while(stop, Duration::from_secs(2), |stop| !*stop)
                    .unwrap_or_else(|e| e.into_inner());
                if *stop {
                    break;
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            rx,
            stop,
            thread: Some(thread),
        })
    }
    pub fn poll(&self) -> Option<Status> {
        self.rx.try_recv().ok()
    }
    pub fn close(&mut self) {
        *self.stop.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.stop.1.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for Watcher {
    fn drop(&mut self) {
        self.close();
    }
}
fn trim(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap_or_default().trim().into()
}
/// The console's battery level now (screens drawn outside the app's loop,
/// such as the update runner).
pub fn current_battery() -> Option<u8> {
    read_status(Path::new("/sys"), Path::new("/proc/net/wireless")).battery
}
fn read_status(sys: &Path, wireless: &Path) -> Status {
    let mut state = Status::default();
    // The console's own battery first, by the names the Go app reads
    // (axp2202-battery on the H700); only without one, any battery that
    // isn't a device's (a Bluetooth headset or controller).
    let supply = sys.join("class/power_supply");
    let named = ["battery", "BAT0", "axp22-battery", "axp2202-battery"]
        .iter()
        .map(|name| supply.join(name))
        .find(|p| trim(p.join("capacity")).parse::<u8>().is_ok());
    if let Some(p) = &named {
        state.battery = trim(p.join("capacity")).parse::<u8>().ok().map(|v| v.min(100));
        state.charging = trim(p.join("status")) == "Charging";
        state.external_power |= matches!(trim(p.join("status")).as_str(), "Charging" | "Full");
    }
    if let Ok(entries) = fs::read_dir(&supply) {
        for entry in entries.flatten() {
            let p = entry.path();
            if trim(p.join("type")) == "Battery" {
                if named.is_some() {
                    continue;
                }
                // A connected Bluetooth headset or controller is a Battery
                // too ("scope" Device, e.g. hid-…-battery): only the
                // console's own counts (axp2202-battery on the H700), as in
                // the Go app. The headset's level showed as the console's.
                if trim(p.join("scope")) == "Device" {
                    continue;
                }
                if state.battery.is_some()
                    && !entry.file_name().to_string_lossy().starts_with("axp")
                {
                    continue;
                }
                state.battery = trim(p.join("capacity"))
                    .parse::<u8>()
                    .ok()
                    .map(|v| v.min(100));
                state.charging = trim(p.join("status")) == "Charging";
                state.external_power |=
                    matches!(trim(p.join("status")).as_str(), "Charging" | "Full");
            } else {
                state.external_power |= trim(p.join("online")) == "1";
            }
        }
    }
    if let Ok(entries) = fs::read_dir(sys.join("class/net")) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.join("wireless").exists() && !p.join("phy80211").exists() {
                continue;
            }
            if trim(p.join("carrier")) != "1"
                || matches!(
                    trim(p.join("operstate")).as_str(),
                    "down" | "dormant" | "lowerlayerdown"
                )
            {
                continue;
            }
            let iface = entry.file_name().to_string_lossy().into_owned();
            state.wifi = wireless_dbm(&trim(wireless), &iface)
                .map(wifi_bars)
                .unwrap_or(5);
            break;
        }
    }
    state
}
fn wireless_dbm(text: &str, iface: &str) -> Option<f64> {
    for line in text.lines() {
        let Some((name, values)) = line.split_once(':') else {
            continue;
        };
        if name.trim() != iface {
            continue;
        }
        let v: Vec<_> = values.split_whitespace().collect();
        if v.len() < 3 {
            continue;
        }
        let link = v[1].trim_end_matches('.').parse::<f64>().ok()?;
        let level = v[2].trim_end_matches('.').parse::<f64>().ok()?;
        if level < 0.0 && level > -110.0 {
            return Some(level);
        }
        if level > 0.0 && level <= 100.0 && link > 0.0 {
            return Some(-100.0 + level / 2.0);
        }
        if link > 0.0 {
            return Some(-100.0 + link / 70.0 * 60.0);
        }
    }
    None
}
fn wifi_bars(dbm: f64) -> i8 {
    if dbm >= -55.0 {
        5
    } else if dbm >= -63.0 {
        4
    } else if dbm >= -70.0 {
        3
    } else if dbm >= -77.0 {
        2
    } else if dbm >= -85.0 {
        1
    } else {
        0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proc_wireless_driver_encodings() {
        assert_eq!(
            wireless_dbm("wlan0: 0000 44. -66. 0.", "wlan0"),
            Some(-66.0)
        );
        assert_eq!(wireless_dbm("wlan0: 0000 70. 80. 0.", "wlan0"), Some(-60.0));
        assert_eq!(wireless_dbm("wlan1: 0000 70. -50. 0.", "wlan0"), None);
    }
    #[test]
    fn a_headset_battery_is_not_the_consoles() {
        let root = std::env::temp_dir().join(format!("bp-status-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let supply = root.join("class/power_supply");
        // the headset first, as directory order may put it
        for (name, kind, capacity, scope) in [
            ("hid-00:11:22-battery", "Battery", "60", Some("Device")),
            ("axp2202-battery", "Battery", "97", None),
            ("axp2202-usb", "USB", "", None),
        ] {
            let d = supply.join(name);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("type"), kind).unwrap();
            if !capacity.is_empty() {
                fs::write(d.join("capacity"), capacity).unwrap();
                fs::write(d.join("status"), "Discharging").unwrap();
            }
            if let Some(scope) = scope {
                fs::write(d.join("scope"), scope).unwrap();
            }
        }
        let state = read_status(&root, &root.join("no-wireless"));
        assert_eq!(state.battery, Some(97));
        assert!(!state.charging);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_battery_paths_the_go_app_reads_come_first() {
        let root = std::env::temp_dir().join(format!("bp-status-go-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let supply = root.join("class/power_supply");
        // another battery with a type, and the console's without one
        let other = supply.join("aaa-battery");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("type"), "Battery").unwrap();
        fs::write(other.join("capacity"), "40").unwrap();
        let console = supply.join("battery");
        fs::create_dir_all(&console).unwrap();
        fs::write(console.join("capacity"), "97\n").unwrap();
        fs::write(console.join("status"), "Charging\n").unwrap();
        let state = read_status(&root, &root.join("no-wireless"));
        assert_eq!(state.battery, Some(97));
        assert!(state.charging);
        let _ = fs::remove_dir_all(&root);
    }
}
