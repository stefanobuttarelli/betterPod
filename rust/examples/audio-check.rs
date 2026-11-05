//! Codec and transport checks with local fixtures and SDL's dummy output.
//! This does not test a DAC, Bluetooth or audible continuity.
use betterpod_rust::audio::{AudioCommand, AudioEngine};
use std::path::PathBuf;
use std::time::{Duration, Instant};
fn main() -> Result<(), String> {
    let sdl = sdl2::init()?;
    let _audio = sdl.audio()?;
    let mut engine = AudioEngine::new()?;
    for file in std::env::args_os().skip(1) {
        let path = PathBuf::from(file);
        let previous = engine.telemetry().callbacks;
        if !engine.command(AudioCommand::Play {
            path: path.clone(),
            sample_rate: 0,
            start: 0.0,
        }) {
            return Err("Play queue full".into());
        }
        wait(|| {
            let s = engine.snapshot();
            if let Some(e) = s.error {
                return Err(e);
            }
            Ok(s.playing && s.path.as_ref() == Some(&path))
        })?;
        std::thread::sleep(Duration::from_millis(600));
        let start = engine.snapshot();
        if start.position <= 0.2 || engine.telemetry().callbacks <= previous {
            return Err(format!("No advancing decoded playback: {}", path.display()));
        }
        engine.command(AudioCommand::Pause);
        wait(|| Ok(engine.snapshot().paused))?;
        let paused = engine.snapshot().position;
        std::thread::sleep(Duration::from_millis(180));
        if (engine.snapshot().position - paused).abs() > 0.05 {
            return Err("Paused position moved".into());
        }
        engine.command(AudioCommand::Resume);
        wait(|| Ok(engine.snapshot().playing))?;
        std::thread::sleep(Duration::from_millis(200));
        if engine.snapshot().position <= paused {
            return Err("Resume did not advance".into());
        }
        engine.command(AudioCommand::Seek(2.0));
        wait(|| {
            let s = engine.snapshot();
            if let Some(e) = s.error {
                return Err(e);
            }
            Ok(s.position >= 1.8)
        })?;
        println!(
            "PASS {}: native_rate={} callbacks={} paused/resumed/seeked",
            path.display(),
            engine.snapshot().rate,
            engine.telemetry().callbacks - previous
        );
    }
    engine.command(AudioCommand::Stop);
    wait(|| Ok(!engine.snapshot().playing))?;
    engine.shutdown();
    Ok(())
}
fn wait(mut condition: impl FnMut() -> Result<bool, String>) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if condition()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Timed out waiting for audio transport".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
