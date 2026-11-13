//! Native forecast composition: physical coordinates ported from ij_weather_view.go.
use super::super::feedback::rounded_gradient;
use super::*;
use crate::render::Color;
use betterpod_rust::{
    motion::{Anim, Ease},
    weather::{self, Place},
};
use chrono::{Datelike, Timelike};
#[derive(Clone)]
pub(super) struct ForecastVisual {
    place: Option<(i64, i64)>,
    first: Anim,
    first_to: usize,
    #[cfg(test)]
    pub epoch: Option<i64>,
}
impl ForecastVisual {
    pub(super) fn new() -> Self {
        Self {
            place: None,
            first: Anim::new(0.),
            first_to: 0,
            #[cfg(test)]
            epoch: None,
        }
    }
    pub(super) fn draw(
        &mut self,
        r: &mut Renderer,
        place: &Place,
        selected: usize,
        fahrenheit: bool,
        loading: bool,
        error: &str,
        now: f64,
    ) {
        let identity = ((place.lat * 100000.) as i64, (place.lon * 100000.) as i64);
        if self.place != Some(identity) {
            self.place = Some(identity);
            self.first.snap(0.);
            self.first_to = 0;
        }
        let forecast = place.forecast.as_ref().filter(|f| f.days() > 0);
        let selected = selected.min(forecast.map_or(0, |f| f.days().saturating_sub(1)));
        let day = selected > 0 || forecast.is_none_or(|f| f.current.is_day != 0);
        let code = forecast.map_or(0, |f| {
            if selected == 0 {
                f.current.weather_code
            } else {
                f.daily.weather_code[selected]
            }
        });
        let stops = weather::sky(code, day);
        r.push_transform(0., 40., 1., 1., 1.);
        r.push_clip(0., 0., 640., 440.);
        r.gradient(0., 0., 640., 242., stops[0], stops[1]);
        r.gradient(0., 242., 640., 198., stops[1], stops[2]);
        r.gradient(0., 0., 640., 220., [1., 1., 1., 0.16], [1., 1., 1., 0.]);
        if !day {
            for index in 0..36 {
                let x = (index as f32 * 97.3) % 320.;
                let y = (index as f32 * 41.7) % 118.;
                let a = 0.25 + 0.5 * (index as f32 * 0.37 % 1.);
                r.rect((x * 2.).round(), (y * 2.).round(), 2., 2., [1., 1., 1., a]);
            }
        }
        let wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0., |d| d.as_secs_f64());
        #[cfg(test)]
        let wall = self.epoch.map(|epoch| epoch as f64 + now).unwrap_or(wall);
        let unix = wall as i64;
        let local = chrono::DateTime::<chrono::Utc>::from_timestamp(
            unix + forecast.map_or(0, |f| f.utc_offset_seconds as i64),
            0,
        );
        let time = local
            .map(|t| format!("{} {:02}:{:02}", t.format("%a"), t.hour(), t.minute()))
            .unwrap_or_default();
        let tw = if forecast.is_some() {
            r.text_width_weighted(&time, 24., 600)
        } else {
            0.
        };
        if forecast.is_some() {
            shadow_text(r, &time, 640. - tw - 20., 36., 24., 600, WHITE, tw + 1.);
        }
        shadow_text(
            r,
            &place.name,
            20.,
            38.,
            32.,
            700,
            WHITE,
            (640. - tw - 60.).max(0.),
        );
        let mut sub = place.location();
        let status = if loading {
            "Updating…".into()
        } else if !error.is_empty() && forecast.is_some() {
            format!("Couldn't update: {error}")
        } else if let Some(f) = forecast {
            format!("Updated {}", ago(unix.saturating_sub(f.fetched as i64)))
        } else {
            String::new()
        };
        if !status.is_empty() {
            if !sub.is_empty() {
                sub.push_str(" · ");
            }
            sub.push_str(&status);
        }
        shadow_text(r, &sub, 20., 66., 22., 500, [1., 1., 1., 0.85], 600.);
        weather::draw_reference_icon(r, code, day, 16., 64., 168., wall.rem_euclid(3.) / 3.);
        let Some(f) = forecast else {
            shadow_text(
                r,
                if !error.is_empty() && !loading {
                    "Couldn't load the forecast"
                } else {
                    "Loading the forecast…"
                },
                208.,
                144.,
                30.,
                700,
                WHITE,
                412.,
            );
            shadow_text(
                r,
                if !error.is_empty() && !loading {
                    error
                } else {
                    "From Open-Meteo"
                },
                208.,
                180.,
                22.,
                500,
                [1., 1., 1., 0.85],
                412.,
            );
            r.pop_clip();
            r.pop_transform();
            return;
        };
        let temperature = |value| weather::temperature(value, fahrenheit);
        let big = temperature(if selected == 0 {
            f.current.temperature_2m
        } else {
            f.daily.temperature_2m_max[selected]
        });
        let bw = r.text_width_weighted(&big, 88., 700);
        shadow_text(r, &big, 204., 164., 88., 700, WHITE, 420.);
        shadow_text(
            r,
            &format!("↑ {}", temperature(f.daily.temperature_2m_max[selected])),
            204. + bw + 20.,
            118.,
            30.,
            600,
            WHITE,
            (416. - bw - 20.).max(0.),
        );
        shadow_text(
            r,
            &format!("↓ {}", temperature(f.daily.temperature_2m_min[selected])),
            204. + bw + 20.,
            158.,
            30.,
            600,
            [1., 1., 1., 0.85],
            (416. - bw - 20.).max(0.),
        );
        shadow_text(r, weather::label(code), 204., 206., 28., 700, WHITE, 420.);
        let details = if selected == 0 {
            let compass = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"]
                [((f.current.wind_direction_10m.rem_euclid(360.) / 45.).round() as usize) % 8];
            let speed = if fahrenheit {
                f.current.wind_speed_10m * 0.621371
            } else {
                f.current.wind_speed_10m
            };
            format!(
                "Feels {} · Humidity {:.0}% · {compass} {:.0} {}",
                temperature(f.current.apparent_temperature),
                f.current.relative_humidity_2m,
                speed.round(),
                if fahrenheit { "mph" } else { "km/h" }
            )
        } else {
            let mut parts = Vec::new();
            let rain = f.rain_text(selected);
            if !rain.is_empty() {
                parts.push(rain);
            }
            if let Some(s) = f.daily.sunrise.get(selected).and_then(|s| s.get(11..16)) {
                parts.push(format!("Sunrise {s}"));
            }
            if let Some(s) = f.daily.sunset.get(selected).and_then(|s| s.get(11..16)) {
                parts.push(format!("Sunset {s}"));
            }
            parts.join(" · ")
        };
        shadow_text(r, &details, 204., 236., 20., 500, [1., 1., 1., 0.88], 420.);
        let target = if selected < self.first_to {
            selected
        } else if selected >= self.first_to + 5 {
            selected - 4
        } else {
            self.first_to
        };
        if target != self.first_to {
            self.first_to = target;
            self.first.set(target as f64, 0.260, Ease::Css, 0., now);
        }
        let first = self.first.value(now) as f32;
        r.push_clip(7., 248., 626., 188.);
        for index in 0..f.days() {
            let x = 15. + (index as f32 - first) * 124.;
            if x > 640. || x + 114. < 0. {
                continue;
            }
            let y = 256. - if index == selected { 4. } else { 0. };
            tile(r, f, index, index == selected, fahrenheit, x, y);
        }
        r.pop_clip();
        if self.first_to > 0 {
            r.triangle([[3.5, 335.], [10.5, 328.], [10.5, 342.]], [1., 1., 1., 0.9]);
        }
        if self.first_to + 5 < f.days() {
            r.triangle(
                [[636.5, 335.], [629.5, 328.], [629.5, 342.]],
                [1., 1., 1., 0.9],
            );
        }
        r.pop_clip();
        r.pop_transform();
    }
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.]
}
fn shadow_text(
    r: &mut Renderer,
    value: &str,
    x: f32,
    base: f32,
    size: f32,
    weight: u16,
    color: Color,
    width: f32,
) {
    let value = super::super::visual_menu::ellipsize(r, value, size, weight, width);
    r.text_baseline(
        &value,
        x + 1.,
        base + 2.,
        size,
        weight,
        [0., 20. / 255., 50. / 255., 0.45 * color[3]],
        width,
    );
    r.text_baseline(&value, x, base, size, weight, color, width);
}
fn center(r: &mut Renderer, value: &str, cx: f32, base: f32, size: f32, weight: u16, color: Color) {
    let width = r.text_width_weighted(value, size, weight);
    r.text_baseline(
        value,
        cx - width / 2.,
        base,
        size,
        weight,
        color,
        width + 1.,
    );
}
fn ago(seconds: i64) -> String {
    if seconds < 60 {
        "just now".into()
    } else if seconds < 3600 {
        format!("{} min ago", seconds / 60)
    } else {
        format!("{} h ago", seconds / 3600)
    }
}
fn tile(
    r: &mut Renderer,
    f: &weather::Forecast,
    index: usize,
    selected: bool,
    fahrenheit: bool,
    x: f32,
    y: f32,
) {
    rounded_gradient(
        r,
        x,
        y + 4.,
        114.,
        172.,
        18.,
        &[
            (0., [0., 20. / 255., 50. / 255., 0.18]),
            (1., [0., 20. / 255., 50. / 255., 0.28]),
        ],
    );
    let (top, bottom) = if selected {
        ([1., 1., 1., 0.98], [214. / 255., 234. / 255., 1., 0.95])
    } else {
        (
            [1., 1., 1., 0.88],
            [225. / 255., 236. / 255., 248. / 255., 0.78],
        )
    };
    if selected {
        r.rounded_rect(
            x - 3.,
            y - 3.,
            120.,
            178.,
            21.,
            [70. / 255., 170. / 255., 1., 0.95],
        );
    }
    rounded_gradient(r, x, y, 114., 172., 18., &[(0., top), (1., bottom)]);
    rounded_gradient(
        r,
        x + 3.,
        y + 3.,
        108.,
        69.24,
        16.,
        &[(0., [1., 1., 1., 0.85]), (1., [1., 1., 1., 0.15])],
    );
    let day = if index == 0 {
        "TODAY".into()
    } else {
        chrono::NaiveDate::parse_from_str(&f.daily.time[index], "%Y-%m-%d")
            .map(|date| {
                format!(
                    "{} {}",
                    date.format("%a").to_string().to_uppercase(),
                    date.day()
                )
            })
            .unwrap_or_default()
    };
    center(r, &day, x + 57., y + 26., 20., 700, rgb(59, 79, 107));
    weather::draw_reference_icon(
        r,
        f.daily.weather_code[index],
        true,
        x + 19.,
        y + 32.,
        76.,
        -1.,
    );
    let high = weather::temperature(f.daily.temperature_2m_max[index], fahrenheit);
    let low = weather::temperature(f.daily.temperature_2m_min[index], fahrenheit);
    let hw = r.text_width_weighted(&high, 28., 700);
    let lw = r.text_width_weighted(&low, 24., 600);
    let sx = x + (114. - hw - 8. - lw) / 2.;
    r.text_baseline(&high, sx, y + 136., 28., 700, rgb(216, 38, 27), hw + 1.);
    r.text_baseline(
        &low,
        sx + hw + 8.,
        y + 136.,
        24.,
        600,
        rgb(31, 95, 200),
        lw + 1.,
    );
    if let Some(chance) = f
        .daily
        .precipitation_probability_max
        .get(index)
        .copied()
        .flatten()
        .filter(|chance| *chance >= 10.)
    {
        let label = format!("{chance:.0}%");
        center(r, &label, x + 62., y + 162., 18., 600, rgb(61, 106, 166));
        r.circle(x + 34., y + 154., 4., rgb(61, 106, 166));
        r.triangle(
            [
                [x + 30., y + 154.],
                [x + 38., y + 154.],
                [x + 34., y + 146.],
            ],
            rgb(61, 106, 166),
        );
    }
}
