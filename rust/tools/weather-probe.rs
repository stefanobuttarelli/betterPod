//! Forecast-view visual probe with fixed offline data, not a live service test.
use betterpod_rust::{
    render::{Renderer, BLACK},
    weather::{draw_forecast, Forecast, Place},
};
use std::path::Path;
fn main() -> Result<(), String> {
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let attr = video.gl_attr();
    attr.set_context_profile(sdl2::video::GLProfile::GLES);
    attr.set_context_version(2, 0);
    let window = video
        .window("Rust weather visual probe", 640, 480)
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;
    let _context = window.gl_create_context()?;
    let mut r = Renderer::new(
        &video,
        Path::new("/repo/port/betterpod/assets/ui_font.ttf"),
        640,
        480,
    )?;
    let forecast:Forecast=serde_json::from_value(serde_json::json!({"fetched":1793880000u64,"utc_offset_seconds":3600,"timezone":"Europe/Rome","current":{"time":"2026-11-05T16:00","temperature_2m":18.0,"apparent_temperature":16.0,"relative_humidity_2m":64.0,"is_day":1,"weather_code":2,"wind_speed_10m":12.0,"wind_direction_10m":315.0},"daily":{"time":["2026-11-05","2026-11-06","2026-11-07","2026-11-08","2026-11-09","2026-11-10","2026-11-11","2026-11-12","2026-11-13","2026-11-14"],"weather_code":[2,61,3,73,95,0,1,2,61,3],"temperature_2m_max":[21,19,17,10,12,14,17,18,15,16],"temperature_2m_min":[12,11,9,2,3,5,8,9,7,8],"precipitation_probability_max":[0,70,10,60,80,0,0,10,50,10],"precipitation_sum":[0,2.5,0,1.3,12,0,0,0,2,0]}})).map_err(|e|e.to_string())?;
    for (name, code, day, fahrenheit, is_day) in [
        ("sun", 0, 0, false, true),
        ("cloud", 2, 0, false, true),
        ("rain", 63, 0, false, true),
        ("snow", 73, 0, false, true),
        ("storm", 95, 0, false, true),
        ("night", 0, 0, false, false),
        ("fahrenheit", 2, 0, true, true),
        ("future", 2, 7, false, true),
    ] {
        let mut f = forecast.clone();
        f.current.weather_code = code;
        f.current.is_day = u8::from(is_day);
        let place = Place {
            name: "Milan".into(),
            admin: "Lombardy".into(),
            country: "Italy".into(),
            lat: 45.46,
            lon: 9.19,
            forecast: Some(f),
        };
        r.begin(BLACK);
        draw_forecast(&mut r, &place, day, fahrenheit, 1.2);
        r.rect(0., 0., 640., 40., [0.88, 0.9, 0.93, 1.]);
        r.centered("Weather", 0., 7., 640., 22, [0.12, 0.14, 0.17, 1.]);
        r.finish();
        r.screenshot(Path::new(&format!("/out/weather-{name}.png")))?;
    }
    Ok(())
}
