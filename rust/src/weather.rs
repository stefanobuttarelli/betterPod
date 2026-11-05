//! Open-Meteo weather with a single bounded worker. Cached forecasts remain
//! available offline; no HTTP request, JSON parse or write runs while drawing.
use crate::{render::Renderer, storage};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
const MAX_BYTES: usize = 4 * 1024 * 1024;
const STALE_SECONDS: u64 = 30 * 60;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Current {
    pub time: String,
    pub temperature_2m: f64,
    pub apparent_temperature: f64,
    pub relative_humidity_2m: f64,
    pub is_day: u8,
    pub weather_code: i32,
    pub wind_speed_10m: f64,
    pub wind_direction_10m: f64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Daily {
    pub time: Vec<String>,
    pub weather_code: Vec<i32>,
    pub temperature_2m_max: Vec<f64>,
    pub temperature_2m_min: Vec<f64>,
    pub precipitation_probability_max: Vec<Option<f64>>,
    pub precipitation_sum: Vec<Option<f64>>,
    pub sunrise: Vec<String>,
    pub sunset: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Forecast {
    pub fetched: u64,
    pub utc_offset_seconds: i32,
    pub timezone: String,
    pub current: Current,
    pub daily: Daily,
}
impl Forecast {
    pub fn days(&self) -> usize {
        self.daily
            .time
            .len()
            .min(self.daily.weather_code.len())
            .min(self.daily.temperature_2m_max.len())
            .min(self.daily.temperature_2m_min.len())
    }
    pub fn is_stale(&self) -> bool {
        now().saturating_sub(self.fetched) > STALE_SECONDS
    }
    pub fn rain_text(&self, day: usize) -> String {
        let code = self.daily.weather_code.get(day).copied().unwrap_or(0);
        let chance = self
            .daily
            .precipitation_probability_max
            .get(day)
            .copied()
            .flatten();
        let mm = self.daily.precipitation_sum.get(day).copied().flatten();
        let word = if matches!(code, 71..=77 | 85 | 86) {
            "Snow"
        } else {
            "Rain"
        };
        let amount = mm.filter(|n| *n >= 0.1).map(|n| {
            if n < 1.0 {
                "<1 mm".into()
            } else {
                format!("{:.0} mm", n.round())
            }
        });
        match (chance.filter(|n| *n > 0.0), amount) {
            (Some(p), Some(a)) => format!("{word} {:.0}% / {a}", p.round()),
            (Some(p), None) => format!("{word} {:.0}%", p.round()),
            (None, Some(a)) if code >= 51 => format!("{word} {a}"),
            _ => {
                if code < 51 && chance == Some(0.0) {
                    "Rain 0%".into()
                } else {
                    String::new()
                }
            }
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Place {
    pub name: String,
    pub admin: String,
    pub country: String,
    pub lat: f64,
    pub lon: f64,
    pub forecast: Option<Forecast>,
}
impl Place {
    pub fn location(&self) -> String {
        [self.admin.as_str(), self.country.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty() && *s != self.name)
            .collect::<Vec<_>>()
            .join(", ")
    }
    fn valid(&self) -> bool {
        !self.name.trim().is_empty()
            && self.name.len() <= 300
            && self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherSnapshot {
    pub fahrenheit: bool,
    pub places: Vec<Place>,
}
#[derive(Clone, Debug)]
pub enum WeatherCommand {
    Search(String),
    Add(Place),
    Remove(usize),
    Refresh(usize),
    Units(bool),
}
#[derive(Clone, Debug)]
pub enum WeatherEvent {
    State(WeatherSnapshot),
    SearchResults { query: String, places: Vec<Place> },
    Error { context: String, message: String },
}
pub struct Weather {
    tx: Option<mpsc::SyncSender<WeatherCommand>>,
    rx: mpsc::Receiver<WeatherEvent>,
    latest: WeatherSnapshot,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Weather {
    pub fn new(data: PathBuf) -> Result<Self, String> {
        let (tx, commands) = mpsc::sync_channel(16);
        let (events, rx) = mpsc::sync_channel(64);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = std::thread::Builder::new()
            .name("weather".into())
            .spawn(move || worker(data, commands, events, worker_stop))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            tx: Some(tx),
            rx,
            latest: WeatherSnapshot::default(),
            stop,
            worker: Some(worker),
        })
    }
    pub fn command(&self, command: WeatherCommand) -> bool {
        self.tx
            .as_ref()
            .is_some_and(|tx| tx.try_send(command).is_ok())
    }
    pub fn poll(&mut self) -> Option<WeatherEvent> {
        let event = self.rx.try_recv().ok()?;
        if let WeatherEvent::State(state) = &event {
            self.latest = state.clone();
        }
        Some(event)
    }
    /// Barrier for database switches/resets. Cancels queued work, drains the
    /// bounded event channel, and joins so no old cache write can race mutation.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            while !worker.is_finished() {
                let _ = self.rx.recv_timeout(Duration::from_millis(20));
            }
            let _ = worker.join();
        }
        while self.rx.try_recv().is_ok() {}
    }
    pub fn snapshot(&self) -> &WeatherSnapshot {
        &self.latest
    }
}
impl Drop for Weather {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
fn worker(
    data: PathBuf,
    commands: mpsc::Receiver<WeatherCommand>,
    events: mpsc::SyncSender<WeatherEvent>,
    stop: Arc<AtomicBool>,
) {
    let path = data.join("weather.json");
    let mut state = std::fs::metadata(&path)
        .ok()
        .filter(|m| m.is_file() && m.len() <= MAX_BYTES as u64)
        .and_then(|_| std::fs::read(&path).ok())
        .and_then(|bytes| serde_json::from_slice::<WeatherSnapshot>(&bytes).ok())
        .unwrap_or_default();
    state.places.retain(Place::valid);
    state.places.truncate(50);
    for place in &mut state.places {
        if place.forecast.as_ref().is_some_and(|f| f.days() == 0) {
            place.forecast = None;
        }
    }
    if events.send(WeatherEvent::State(state.clone())).is_err() {
        return;
    }
    while !stop.load(Ordering::Acquire) {
        let command = match commands.recv_timeout(Duration::from_millis(200)) {
            Ok(c) => c,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return,
        };
        let mut changed = false;
        let mut refresh = None;
        let result = match command {
            WeatherCommand::Search(query) => {
                let query = query.trim().to_owned();
                if query.len() < 2 || query.len() > 200 {
                    Err(("search".into(), "Enter between 2 and 200 characters".into()))
                } else {
                    match search(&query, &stop) {
                        Ok(places) => {
                            if events
                                .send(WeatherEvent::SearchResults { query, places })
                                .is_err()
                            {
                                return;
                            }
                            Ok(())
                        }
                        Err(e) => Err(("search".into(), e)),
                    }
                }
            }
            WeatherCommand::Add(place) => {
                if !place.valid() {
                    Err(("add".into(), "Invalid place coordinates".into()))
                } else if state.places.len() >= 50 {
                    Err(("add".into(), "At most 50 saved places".into()))
                } else {
                    let index = state.places.iter().position(|p| {
                        (p.lat - place.lat).abs() < 0.0001 && (p.lon - place.lon).abs() < 0.0001
                    });
                    refresh = Some(if let Some(i) = index {
                        i
                    } else {
                        state.places.push(place);
                        changed = true;
                        state.places.len() - 1
                    });
                    Ok(())
                }
            }
            WeatherCommand::Remove(index) => {
                if index < state.places.len() {
                    state.places.remove(index);
                    changed = true;
                    Ok(())
                } else {
                    Err(("remove".into(), "Place is no longer saved".into()))
                }
            }
            WeatherCommand::Refresh(index) => {
                if index < state.places.len() {
                    refresh = Some(index);
                    Ok(())
                } else {
                    Err(("refresh".into(), "Place is no longer saved".into()))
                }
            }
            WeatherCommand::Units(fahrenheit) => {
                state.fahrenheit = fahrenheit;
                changed = true;
                Ok(())
            }
        };
        if stop.load(Ordering::Acquire) {
            return;
        }
        if changed {
            if let Err(e) = save(&path, &state) {
                let _ = events.send(WeatherEvent::Error {
                    context: "save".into(),
                    message: e,
                });
            }
            if events.send(WeatherEvent::State(state.clone())).is_err() {
                return;
            }
            changed = false;
        }
        if let Err((context, message)) = result {
            if events
                .send(WeatherEvent::Error { context, message })
                .is_err()
            {
                return;
            }
        }
        if let Some(index) = refresh {
            if stop.load(Ordering::Acquire) {
                return;
            }
            match fetch(&state.places[index], &stop) {
                Ok(forecast) => {
                    state.places[index].forecast = Some(forecast);
                    changed = true;
                }
                Err(message) => {
                    let _ = events.send(WeatherEvent::Error {
                        context: format!("forecast:{index}"),
                        message,
                    });
                }
            }
        }
        if stop.load(Ordering::Acquire) {
            return;
        }
        if changed {
            if let Err(message) = save(&path, &state) {
                let _ = events.send(WeatherEvent::Error {
                    context: "save".into(),
                    message,
                });
            }
            if events.send(WeatherEvent::State(state.clone())).is_err() {
                return;
            }
        }
    }
}
fn save(path: &std::path::Path, state: &WeatherSnapshot) -> Result<(), String> {
    storage::atomic_write(path, &serde_json::to_vec(state).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn get_json<T: for<'de> Deserialize<'de>>(url: &str, stop: &AtomicBool) -> Result<T, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_per_read(Some(Duration::from_secs(8)))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", crate::online::USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?;
    let mut reader = response.body_mut().as_reader();
    let mut data = Vec::new();
    let mut buffer = [0; 16384];
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if data.len() + n > MAX_BYTES {
            return Err("Weather response is too large".into());
        }
        data.extend_from_slice(&buffer[..n]);
    }
    serde_json::from_slice(&data).map_err(|e| format!("Invalid weather response: {e}"))
}
#[derive(Deserialize)]
struct GeoResponse {
    #[serde(default)]
    results: Vec<GeoPlace>,
}
#[derive(Deserialize)]
struct GeoPlace {
    name: String,
    #[serde(default)]
    admin1: String,
    #[serde(default)]
    country: String,
    latitude: f64,
    longitude: f64,
}
fn base(variable: &str, default: &str) -> String {
    std::env::var(variable)
        .unwrap_or_else(|_| default.into())
        .trim_end_matches('/')
        .into()
}
fn search(query: &str, stop: &AtomicBool) -> Result<Vec<Place>, String> {
    let url = format!(
        "{}/v1/search?name={}&count=20&language=en&format=json",
        base(
            "BETTERPOD_RUST_METEO_GEO",
            "https://geocoding-api.open-meteo.com"
        ),
        urlencoding::encode(query)
    );
    let result: GeoResponse = get_json(&url, stop)?;
    Ok(result
        .results
        .into_iter()
        .map(|p| Place {
            name: p.name,
            admin: p.admin1,
            country: p.country,
            lat: p.latitude,
            lon: p.longitude,
            forecast: None,
        })
        .filter(Place::valid)
        .take(20)
        .collect())
}
fn fetch(place: &Place, stop: &AtomicBool) -> Result<Forecast, String> {
    let url=format!("{}/v1/forecast?latitude={:.5}&longitude={:.5}&timezone=auto&current=temperature_2m,relative_humidity_2m,apparent_temperature,is_day,weather_code,wind_speed_10m,wind_direction_10m&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,precipitation_sum,sunrise,sunset&forecast_days=10",base("BETTERPOD_RUST_METEO_API","https://api.open-meteo.com"),place.lat,place.lon);
    let mut forecast: Forecast = get_json(&url, stop)?;
    if forecast.days() == 0 {
        return Err("Forecast did not contain any complete days".into());
    }
    forecast.fetched = now();
    Ok(forecast)
}
pub fn temperature(c: f64, fahrenheit: bool) -> String {
    format!("{:.0}°", if fahrenheit { c * 9.0 / 5.0 + 32.0 } else { c })
}
pub fn wind(kmh: f64, direction: f64, fahrenheit: bool) -> String {
    let degrees = if direction.is_finite() {
        direction.rem_euclid(360.0)
    } else {
        0.0
    };
    let compass =
        ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][((degrees / 45.0).round() as usize) % 8];
    format!(
        "{:.0} {} {compass}",
        if fahrenheit { kmh * 0.621371 } else { kmh },
        if fahrenheit { "mph" } else { "km/h" }
    )
}
pub fn label(code: i32) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly Clear",
        2 => "Partly Cloudy",
        3 => "Cloudy",
        45 => "Fog",
        48 => "Freezing Fog",
        51 => "Light Drizzle",
        53 => "Drizzle",
        55 => "Heavy Drizzle",
        56 | 57 => "Freezing Drizzle",
        61 => "Light Rain",
        63 => "Rain",
        65 => "Heavy Rain",
        66 | 67 => "Freezing Rain",
        71 => "Light Snow",
        73 => "Snow",
        75 => "Heavy Snow",
        77 => "Snow Grains",
        80 => "Showers",
        81 => "Rain Showers",
        82 => "Heavy Showers",
        85 | 86 => "Snow Showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm, Hail",
        _ => "Cloudy",
    }
}
pub fn draw_icon(
    r: &mut Renderer,
    code: i32,
    is_day: bool,
    x: f32,
    y: f32,
    size: f32,
    elapsed: f32,
) {
    let cx = x + size * 0.5;
    let cy = y + size * 0.43;
    if code <= 2 {
        let sun_x = if code == 2 { x + size * 0.66 } else { cx };
        let sun_y = if code == 2 { y + size * 0.30 } else { cy };
        if is_day {
            for i in 0..10 {
                let a = i as f32 * std::f32::consts::TAU / 10.0 + elapsed * 0.1;
                r.line(
                    sun_x + size * 0.22 * a.cos(),
                    sun_y + size * 0.22 * a.sin(),
                    sun_x + size * 0.30 * a.cos(),
                    sun_y + size * 0.30 * a.sin(),
                    size * 0.035,
                    [1.0, 0.8, 0.24, 1.0],
                );
            }
            r.circle(sun_x, sun_y, size * 0.18, [1.0, 0.78, 0.16, 1.0]);
        } else {
            r.circle(sun_x, sun_y, size * 0.23, [0.93, 0.95, 0.99, 1.0]);
            r.circle(
                sun_x + size * 0.10,
                sun_y - size * 0.09,
                size * 0.22,
                [0.11, 0.19, 0.35, 1.0],
            );
        }
    }
    if code >= 2 {
        let sway = (elapsed * 0.55).sin() * size * 0.025;
        let c = [0.88, 0.92, 0.97, 1.0];
        r.rect(
            x + size * 0.2 + sway,
            y + size * 0.48,
            size * 0.62,
            size * 0.19,
            c,
        );
        for (a, b, rad) in [(0.27, 0.48, 0.16), (0.47, 0.39, 0.23), (0.68, 0.47, 0.18)] {
            r.circle(x + size * a + sway, y + size * b, size * rad, c);
        }
    }
    if matches!(code, 45 | 48) {
        for i in 0..3 {
            r.line(
                x + size * 0.1,
                y + size * (0.71 + i as f32 * 0.09),
                x + size * 0.9,
                y + size * (0.71 + i as f32 * 0.09),
                size * 0.025,
                [0.82, 0.87, 0.93, 0.75],
            );
        }
    } else if code >= 51 && code < 95 {
        for i in 0..4 {
            let phase = (elapsed * 0.7 + i as f32 * 0.27).fract();
            let xx = x + size * (0.23 + i as f32 * 0.17);
            let yy = y + size * (0.62 + phase * 0.30);
            if matches!(code, 71..=77 | 85 | 86) {
                r.circle(xx, yy, size * 0.027, [1.; 4]);
            } else {
                r.line(
                    xx,
                    yy,
                    xx - size * 0.04,
                    yy + size * 0.1,
                    size * 0.025,
                    [0.35, 0.7, 1.0, 0.9],
                );
            }
        }
    }
    if code >= 95 {
        let c = [1.0, 0.85, 0.2, 1.0];
        r.triangle(
            [
                [cx, cy + size * 0.1],
                [cx - size * 0.13, cy + size * 0.36],
                [cx + size * 0.04, cy + size * 0.31],
            ],
            c,
        );
        r.triangle(
            [
                [cx - size * 0.02, cy + size * 0.27],
                [cx + size * 0.13, cy + size * 0.25],
                [cx - size * 0.08, cy + size * 0.58],
            ],
            c,
        );
    }
}
pub fn draw_forecast(r: &mut Renderer, place: &Place, day: usize, fahrenheit: bool, elapsed: f32) {
    let Some(f) = place.forecast.as_ref().filter(|f| f.days() > 0) else {
        r.rect(0., 40., 640., 440., [0.14, 0.26, 0.43, 1.]);
        r.centered("No cached forecast yet", 20., 210., 600., 24, [1.; 4]);
        r.centered("A: refresh", 20., 256., 600., 18, [1.; 4]);
        return;
    };
    let day = day.min(f.days().saturating_sub(1));
    let is_day = day != 0 || f.current.is_day != 0;
    let top = if is_day {
        [0.13, 0.49, 0.83, 1.]
    } else {
        [0.05, 0.09, 0.21, 1.]
    };
    let bottom = if is_day {
        [0.48, 0.72, 0.88, 1.]
    } else {
        [0.15, 0.24, 0.39, 1.]
    };
    r.gradient(0., 40., 640., 440., top, bottom);
    r.text(&place.name, 18., 55., 26, [1.; 4], 440.);
    r.text(&place.location(), 18., 88., 16, [0.85, 0.92, 1., 1.], 560.);
    if let Some(local) = chrono::DateTime::<chrono::Utc>::from_timestamp(
        now() as i64 + f.utc_offset_seconds as i64,
        0,
    ) {
        r.text(
            &local.format("%a %H:%M").to_string(),
            490.0,
            61.0,
            18,
            [0.9, 0.95, 1.0, 1.0],
            140.0,
        );
    }
    let code = if day == 0 {
        f.current.weather_code
    } else {
        f.daily.weather_code[day]
    };
    draw_icon(r, code, is_day, 24., 124., 160., elapsed);
    let temp = if day == 0 {
        f.current.temperature_2m
    } else {
        f.daily.temperature_2m_max[day]
    };
    r.text(
        &temperature(temp, fahrenheit),
        222.,
        117.,
        48,
        [1.; 4],
        230.,
    );
    r.text(label(code), 222., 179., 26, [1.; 4], 400.);
    let detail = if day == 0 {
        format!(
            "Feels like {} / {}",
            temperature(f.current.apparent_temperature, fahrenheit),
            wind(
                f.current.wind_speed_10m,
                f.current.wind_direction_10m,
                fahrenheit
            )
        )
    } else {
        format!(
            "High {} / Low {}",
            temperature(f.daily.temperature_2m_max[day], fahrenheit),
            temperature(f.daily.temperature_2m_min[day], fahrenheit)
        )
    };
    r.text(&detail, 222., 217., 18, [0.91, 0.96, 1., 1.], 400.);
    r.text(
        &f.rain_text(day),
        222.,
        248.,
        18,
        [0.91, 0.96, 1., 1.],
        400.,
    );
    let first = day.saturating_sub(2).min(f.days().saturating_sub(5));
    for i in first..(first + 5).min(f.days()) {
        let x = 12. + (i - first) as f32 * 124.;
        r.rect(
            x,
            296.,
            116.,
            130.,
            if i == day {
                [0.07, 0.28, 0.52, 0.85]
            } else {
                [1., 1., 1., 0.14]
            },
        );
        let date = if i == 0 {
            "Today"
        } else {
            f.daily.time[i].get(5..).unwrap_or(&f.daily.time[i])
        };
        r.centered(date, x + 3., 306., 110., 16, [1.; 4]);
        draw_icon(r, f.daily.weather_code[i], true, x + 31., 330., 54., 0.);
        r.centered(
            &format!(
                "{}  {}",
                temperature(f.daily.temperature_2m_max[i], fahrenheit),
                temperature(f.daily.temperature_2m_min[i], fahrenheit)
            ),
            x + 3.,
            393.,
            110.,
            18,
            [1.; 4],
        );
    }
    let age = now().saturating_sub(f.fetched) / 60;
    let status = if age == 0 {
        "Updated just now".into()
    } else {
        format!("Updated {age} minutes ago")
    };
    r.text(&status, 16., 443., 16, [0.88, 0.94, 1., 1.], 280.);
    r.text(
        "Arrows: day / A: refresh",
        345.,
        443.,
        16,
        [0.88, 0.94, 1., 1.],
        285.,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_daily_vectors_are_bounded() {
        let mut f = Forecast::default();
        f.daily.time = vec!["2026-11-05".into(); 10];
        f.daily.weather_code = vec![0; 8];
        f.daily.temperature_2m_max = vec![20.; 9];
        f.daily.temperature_2m_min = vec![10.; 7];
        assert_eq!(f.days(), 7);
    }
    #[test]
    fn precipitation_text_does_not_claim_dry_weather_for_a_wet_icon() {
        let mut f = Forecast::default();
        f.daily.weather_code = vec![61, 0];
        f.daily.precipitation_probability_max = vec![Some(0.), Some(0.)];
        f.daily.precipitation_sum = vec![Some(1.4), Some(0.)];
        assert_eq!(f.rain_text(0), "Rain 1 mm");
        assert_eq!(f.rain_text(1), "Rain 0%");
    }
    #[test]
    fn units_and_compass_are_consistent() {
        assert_eq!(temperature(0., true), "32°");
        assert_eq!(temperature(20., false), "20°");
        assert_eq!(wind(16.09344, 90., true), "10 mph E");
        assert_eq!(wind(10., -45., false), "10 km/h NW");
    }
    #[test]
    fn invalid_coordinates_are_not_saved_places() {
        assert!(!Place {
            name: "Bad".into(),
            lat: 91.,
            ..Place::default()
        }
        .valid());
        assert!(!Place {
            name: "Bad".into(),
            lon: f64::NAN,
            ..Place::default()
        }
        .valid());
    }
    #[test]
    fn old_forecasts_are_retained_but_marked_stale() {
        let f = Forecast {
            fetched: 1,
            ..Forecast::default()
        };
        assert!(f.is_stale());
        let f = Forecast {
            fetched: now(),
            ..Forecast::default()
        };
        assert!(!f.is_stale());
    }
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    use std::{fs, io::Write, net::TcpListener, thread, time::Instant};
    fn dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "betterpod-weather-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn wait(weather: &mut Weather, check: impl Fn(&WeatherEvent) -> bool) -> WeatherEvent {
        let began = Instant::now();
        loop {
            if let Some(event) = weather.poll() {
                if check(&event) {
                    return event;
                }
            }
            assert!(
                began.elapsed() < Duration::from_secs(3),
                "weather event timed out"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn units_are_persisted_on_the_worker_and_restored_offline() {
        let data = dir();
        let mut weather = Weather::new(data.clone()).unwrap();
        wait(&mut weather, |e| matches!(e, WeatherEvent::State(_)));
        assert!(weather.command(WeatherCommand::Units(true)));
        wait(
            &mut weather,
            |e| matches!(e,WeatherEvent::State(s)if s.fahrenheit),
        );
        assert!(
            serde_json::from_slice::<WeatherSnapshot>(
                &fs::read(data.join("weather.json")).unwrap()
            )
            .unwrap()
            .fahrenheit
        );
        drop(weather);
        let mut restored = Weather::new(data.clone()).unwrap();
        wait(
            &mut restored,
            |e| matches!(e,WeatherEvent::State(s)if s.fahrenheit),
        );
        drop(restored);
        fs::remove_dir_all(data).unwrap();
    }
    #[test]
    fn geocoding_forecast_and_cached_places_work_with_local_http() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.local_addr().unwrap());
        std::env::set_var("BETTERPOD_RUST_METEO_GEO", &address);
        std::env::set_var("BETTERPOD_RUST_METEO_API", &address);
        let server_thread = thread::spawn(move || {
            for turn in 0..2 {
                let (mut stream, _) = server.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut bytes = [0; 1024];
                while !request.windows(4).any(|p| p == b"\r\n\r\n") {
                    let n = stream.read(&mut bytes).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&bytes[..n]);
                }
                let request = String::from_utf8_lossy(&request);
                let body = if turn == 0 {
                    assert!(request.starts_with("GET /v1/search?name=New%20York"));
                    r#"{"results":[{"name":"New York","admin1":"New York","country":"United States","latitude":40.7,"longitude":-74.0}]}"#
                } else {
                    assert!(request
                        .starts_with("GET /v1/forecast?latitude=40.70000&longitude=-74.00000"));
                    r#"{"utc_offset_seconds":-18000,"timezone":"America/New_York","current":{"time":"2026-11-05T12:00","temperature_2m":14,"is_day":1,"weather_code":2},"daily":{"time":["2026-11-05","2026-11-06"],"weather_code":[2,61],"temperature_2m_max":[16,15],"temperature_2m_min":[8,7],"precipitation_probability_max":[0,50],"precipitation_sum":[0,2.1]}}"#
                };
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
        });
        let data = dir();
        let mut weather = Weather::new(data.clone()).unwrap();
        wait(&mut weather, |e| matches!(e, WeatherEvent::State(_)));
        assert!(weather.command(WeatherCommand::Search("New York".into())));
        let WeatherEvent::SearchResults { query, mut places } = wait(&mut weather, |e| {
            matches!(e, WeatherEvent::SearchResults { .. })
        }) else {
            unreachable!()
        };
        assert_eq!(query, "New York");
        assert_eq!(places.len(), 1);
        assert!(weather.command(WeatherCommand::Add(places.remove(0))));
        wait(
            &mut weather,
            |e| matches!(e,WeatherEvent::State(s)if s.places.first().and_then(|p|p.forecast.as_ref()).is_some_and(|f|f.days()==2)),
        );
        assert_eq!(
            weather.snapshot().places[0]
                .forecast
                .as_ref()
                .unwrap()
                .current
                .temperature_2m,
            14.0
        );
        drop(weather);
        server_thread.join().unwrap();
        std::env::remove_var("BETTERPOD_RUST_METEO_GEO");
        std::env::remove_var("BETTERPOD_RUST_METEO_API");
        let mut restored = Weather::new(data.clone()).unwrap();
        wait(
            &mut restored,
            |e| matches!(e,WeatherEvent::State(s)if s.places.len()==1),
        );
        assert_eq!(
            restored.snapshot().places[0]
                .forecast
                .as_ref()
                .unwrap()
                .days(),
            2
        );
        drop(restored);
        fs::remove_dir_all(data).unwrap();
    }
    #[test]
    fn shutdown_joins_even_when_bounded_event_queue_is_full() {
        let data = dir();
        let mut weather = Weather::new(data.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut accepted = 0;
        while accepted < 80 && Instant::now() < deadline {
            if weather.command(WeatherCommand::Units(accepted % 2 == 0)) {
                accepted += 1;
            } else {
                thread::sleep(Duration::from_millis(2));
            }
        }
        assert!(accepted >= 64);
        let start = Instant::now();
        weather.shutdown();
        assert!(start.elapsed() < Duration::from_secs(2));
        fs::write(data.join("weather.json"), b"reset marker").unwrap();
        thread::sleep(Duration::from_millis(25));
        assert_eq!(
            fs::read(data.join("weather.json")).unwrap(),
            b"reset marker"
        );
        assert!(!weather.command(WeatherCommand::Units(true)));
        drop(weather);
        fs::remove_dir_all(data).unwrap();
    }
}
