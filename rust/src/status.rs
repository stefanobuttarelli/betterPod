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
    /// The console battery's capacity, 0–100. `Watcher` delivers it already
    /// passed through a `BatteryGauge`.
    pub battery: Option<u8>,
    pub charging: bool,
    /// The battery reports Full, which the kernel does only on external power.
    pub charged: bool,
    pub external_power: bool,
    pub wifi: i8,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            battery: None,
            charging: false,
            charged: false,
            external_power: false,
            wifi: -1,
        }
    }
}

/// The header battery shows the level in steps of this many percent.
pub const BATTERY_STEP: u8 = 5;
/// A known level never draws less than this, so the red fill stays visible.
const BATTERY_MIN_FILL: u8 = 10;
/// The fill turns red at or below this level while not charging.
const BATTERY_LOW: u8 = 20;

/// Rounds a capacity to the nearest `BATTERY_STEP`.
fn battery_step(percent: u8) -> u8 {
    let percent = percent.min(100);
    (percent + BATTERY_STEP / 2) / BATTERY_STEP * BATTERY_STEP
}

/// Smooths the shown battery step. It holds while the reading stays within
/// three points of it, so a level that flickers across a rounding boundary
/// keeps one icon; a reading four or more points away moves it at once.
#[derive(Clone, Copy, Debug, Default)]
pub struct BatteryGauge {
    shown: Option<u8>,
}
impl BatteryGauge {
    pub fn update(&mut self, percent: Option<u8>) -> Option<u8> {
        self.shown = percent.map(|percent| {
            let percent = percent.min(100);
            match self.shown {
                Some(shown) if percent.abs_diff(shown) <= BATTERY_STEP / 2 + 1 => shown,
                _ => battery_step(percent),
            }
        });
        self.shown
    }
}

/// The symbol drawn over the battery fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryBadge {
    /// Charging.
    Bolt,
    /// On external power with a full battery.
    Plug,
}

/// How the header battery is drawn for one status.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BatteryIcon {
    /// Share of the fill image to draw, 0.0–1.0.
    pub fill: f32,
    /// Draw the red fill instead of the green one.
    pub low: bool,
    pub badge: Option<BatteryBadge>,
}
impl BatteryIcon {
    /// An unknown level draws full, as the Go app does. A known level fills in
    /// proportion to the real charge, in `BATTERY_STEP` steps.
    pub fn new(level: Option<u8>, charging: bool, charged: bool) -> Self {
        let step = level.map(battery_step);
        let full = step.is_none_or(|step| step >= 100);
        let badge = if charging && !full {
            Some(BatteryBadge::Bolt)
        } else if charging || charged {
            Some(BatteryBadge::Plug)
        } else {
            None
        };
        Self {
            fill: step.map_or(1.0, |step| f32::from(step.max(BATTERY_MIN_FILL)) / 100.0),
            low: !charging && step.is_some_and(|step| step <= BATTERY_LOW),
            badge,
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
        let mut gauge = BatteryGauge::default();
        let thread = thread::Builder::new()
            .name("header-status".into())
            .spawn(move || loop {
                if let Ok(stop) = signal.0.lock() {
                    if *stop {
                        break;
                    }
                }
                let mut status = current();
                status.battery = gauge.update(status.battery);
                let _ = tx.try_send(status);
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
/// The console's status now, unsmoothed. Screens drawn outside the app's
/// loop, such as the update runner, read it directly.
pub fn current() -> Status {
    read_status(Path::new("/sys"), Path::new("/proc/net/wireless"), Path::new("/tmp"))
}
fn read_battery(supply: &Path, state: &mut Status) {
    state.battery = trim(supply.join("capacity"))
        .parse::<u8>()
        .ok()
        .filter(|v| *v <= 100);
    let status = trim(supply.join("status"));
    state.charging = status == "Charging";
    state.charged = status == "Full";
    state.external_power |= state.charging || state.charged;
}
fn read_status(sys: &Path, wireless: &Path, tmp: &Path) -> Status {
    let mut state = Status::default();
    // Prefer the known console paths even when firmware omits their type.
    let supply = sys.join("class/power_supply");
    let named = ["battery", "BAT0", "axp22-battery", "axp2202-battery"]
        .iter()
        .map(|name| supply.join(name))
        .find(|p| {
            trim(p.join("scope")) != "Device"
                && trim(p.join("capacity"))
                    .parse::<u8>()
                    .is_ok_and(|v| v <= 100)
        });
    if let Some(p) = &named {
        read_battery(p, &mut state);
    }
    if let Ok(entries) = fs::read_dir(&supply) {
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (!name.starts_with("axp"), name)
        });
        for entry in entries {
            let p = entry.path();
            // A connected headset/controller must not replace the console level.
            if trim(p.join("scope")) == "Device" {
                continue;
            }
            if trim(p.join("type")) == "Battery" && state.battery.is_none() {
                read_battery(&p, &mut state);
            } else {
                state.external_power |= trim(p.join("online")) == "1";
            }
        }
    }
    // Knulli's BatteryPlus estimates the level from the voltage and writes
    // it here; Knulli's own screens show this number (knulli-battery-check).
    // The PMIC's capacity read above can be far lower (55% against 93%).
    if let Some(pct) = trim(tmp.join("battery.percent"))
        .parse::<u8>()
        .ok()
        .filter(|v| *v <= 100)
    {
        state.battery = Some(pct);
    }
    let status = trim(tmp.join("battery.status"));
    if !status.is_empty() {
        state.charging = status == "Charging";
        state.charged = status == "Full";
        state.external_power |= state.charging || state.charged;
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
    fn battery_levels_round_to_five_percent_steps() {
        let steps: Vec<u8> = [0, 2, 3, 7, 8, 49, 82, 83, 97, 98, 100, 150]
            .into_iter()
            .map(battery_step)
            .collect();
        assert_eq!(steps, [0, 0, 5, 5, 10, 50, 80, 85, 95, 100, 100, 100]);
    }
    #[test]
    fn battery_gauge_ignores_one_point_flicker_but_follows_real_changes() {
        let mut gauge = BatteryGauge::default();
        let shown: Vec<_> = [77, 78, 77, 78, 79, 77, 76, 40, 41]
            .into_iter()
            .map(|percent| gauge.update(Some(percent)))
            .collect();
        let expected = [75, 75, 75, 75, 80, 80, 75, 40, 40].map(Some);
        assert_eq!(shown, expected);
        assert_eq!(gauge.update(None), None);
        assert_eq!(gauge.update(Some(63)), Some(65));
    }
    #[test]
    fn battery_icon_fills_in_proportion_to_the_real_level() {
        let icon = |level, charging, charged| BatteryIcon::new(level, charging, charged);
        // The Go mapping (level / 85) showed 82% as 96% full.
        assert_eq!(icon(Some(82), false, false).fill, 0.8);
        assert_eq!(icon(Some(50), false, false).fill, 0.5);
        assert_eq!(icon(Some(3), false, false).fill, 0.1);
        assert_eq!(icon(None, false, false).fill, 1.0);
        assert!(icon(Some(20), false, false).low);
        assert!(!icon(Some(20), true, false).low);
        assert!(!icon(Some(25), false, false).low);
        assert!(!icon(None, false, false).low);
    }
    #[test]
    fn battery_icon_shows_a_bolt_while_charging_and_a_plug_when_full() {
        let badge = |level, charging, charged| BatteryIcon::new(level, charging, charged).badge;
        assert_eq!(badge(Some(60), false, false), None);
        assert_eq!(badge(Some(60), true, false), Some(BatteryBadge::Bolt));
        assert_eq!(badge(Some(99), true, false), Some(BatteryBadge::Plug));
        assert_eq!(badge(Some(100), false, true), Some(BatteryBadge::Plug));
        assert_eq!(badge(None, true, false), Some(BatteryBadge::Plug));
    }
    #[test]
    fn a_full_battery_on_external_power_is_charged_not_charging() {
        let root = std::env::temp_dir().join(format!("bp-status-full-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let battery = root.join("class/power_supply/axp2202-battery");
        fs::create_dir_all(&battery).unwrap();
        fs::write(battery.join("capacity"), "100\n").unwrap();
        fs::write(battery.join("status"), "Full\n").unwrap();
        let state = read_status(&root, &root.join("no-wireless"), &root.join("no-tmp"));
        assert_eq!(state.battery, Some(100));
        assert!(state.charged && !state.charging && state.external_power);
        let _ = fs::remove_dir_all(&root);
    }
    #[test]
    fn system_battery_wins_over_peripheral_and_secondary_supplies() {
        let root = std::env::temp_dir().join(format!("bp-battery-{}", std::process::id()));
        for (name, scope, capacity) in [
            ("battery", "System", "82"),
            ("controller", "Device", "50"),
            ("secondary", "System", "40"),
        ] {
            let p = root.join("class/power_supply").join(name);
            fs::create_dir_all(&p).unwrap();
            for (file, value) in [
                ("type", "Battery"),
                ("scope", scope),
                ("capacity", capacity),
                ("status", "Discharging"),
            ] {
                fs::write(p.join(file), value).unwrap();
            }
        }
        assert_eq!(read_status(&root, &root.join("wireless"), &root.join("no-tmp")).battery, Some(82));
        fs::remove_dir_all(root).unwrap();
    }
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
        let state = read_status(&root, &root.join("no-wireless"), &root.join("no-tmp"));
        assert_eq!(state.battery, Some(97));
        assert!(!state.charging);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn knulli_batteryplus_level_wins_over_the_pmic() {
        let root = std::env::temp_dir().join(format!("bp-status-plus-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let console = root.join("class/power_supply/axp2202-battery");
        fs::create_dir_all(&console).unwrap();
        fs::write(console.join("type"), "Battery").unwrap();
        fs::write(console.join("capacity"), "55\n").unwrap();
        fs::write(console.join("status"), "Discharging\n").unwrap();
        let tmp = root.join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let wireless = root.join("no-wireless");
        assert_eq!(read_status(&root, &wireless, &tmp).battery, Some(55));
        fs::write(tmp.join("battery.percent"), "93\n").unwrap();
        assert_eq!(read_status(&root, &wireless, &tmp).battery, Some(93));
        fs::write(tmp.join("battery.percent"), "junk").unwrap();
        assert_eq!(read_status(&root, &wireless, &tmp).battery, Some(55));
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
        let state = read_status(&root, &root.join("no-wireless"), &root.join("no-tmp"));
        assert_eq!(state.battery, Some(97));
        assert!(state.charging);
        let _ = fs::remove_dir_all(&root);
    }
}
