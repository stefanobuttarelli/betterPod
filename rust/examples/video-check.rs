//! Local A/V transport fixture using generated media and dummy audio.
use betterpod_rust::{
    online::StreamSource,
    video::{VideoCommand, VideoPlayer},
};
use std::time::{Duration, Instant};
fn source(path: &str, audio: Option<String>, seconds: f64) -> StreamSource {
    StreamSource {
        video_url: path.into(),
        audio_url: audio,
        headers: Vec::new(),
        duration: seconds,
        title: format!("Local fixture: {path}"),
        available_at: None,
    }
}
fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("Pass a local video fixture")?;
    let silent = std::env::args()
        .nth(2)
        .ok_or("Pass a silent video fixture")?;
    let sdl = sdl2::init()?;
    let _audio = sdl.audio()?;
    let mut video = VideoPlayer::new()?;
    assert!(video.command(VideoCommand::Open {
        source: source(&path, None, 6.0),
        start: 0.0,
        audio_only: false
    }));
    wait(|| {
        let state = video.snapshot();
        if let Some(error) = state.error {
            return Err(error);
        }
        Ok(state.playing && video.frame().is_some())
    })?;
    std::thread::sleep(Duration::from_millis(600));
    let state = video.snapshot();
    assert!(state.position > 0.3, "Audio clock did not advance");
    assert!(video.command(VideoCommand::Pause(true)));
    wait(|| Ok(video.snapshot().paused))?;
    let paused = video.snapshot().position;
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        (video.snapshot().position - paused).abs() < 0.03,
        "Paused video advanced"
    );
    assert!(video.command(VideoCommand::Pause(false)));
    wait(|| Ok(video.snapshot().playing))?;
    assert!(video.command(VideoCommand::Seek(2.0)));
    let old_generation = state.generation;
    wait(|| {
        let s = video.snapshot();
        if let Some(error) = s.error {
            return Err(error);
        }
        Ok(!s.loading && s.position >= 1.9 && s.generation > old_generation)
    })?;
    wait(|| Ok(video.frame().is_some()))?;
    assert!(video.command(VideoCommand::Stop));
    wait(|| {
        let s = video.snapshot();
        Ok(!s.playing && !s.loading)
    })?;
    println!("PASS local A/V: frames, audio clock, pause/resume, seek, stop");
    let before = video.snapshot().generation;
    // Immediate seek may supersede opening before the worker reads Open.
    assert!(video.command(VideoCommand::Open {
        source: source(&path, None, 6.0),
        start: 0.0,
        audio_only: false
    }));
    assert!(video.command(VideoCommand::Seek(1.0)));
    let generation = before;
    wait(|| {
        let s = video.snapshot();
        if let Some(e) = s.error {
            if s.generation > generation {
                return Err(e);
            }
        }
        Ok(s.generation > before && s.playing && s.position >= 0.99 && video.frame().is_some())
    })?;
    println!("PASS queued Open + Seek retains source");
    let before = video.snapshot().generation;
    assert!(video.command(VideoCommand::Open {
        source: source(&silent, None, 2.0),
        start: 0.0,
        audio_only: false
    }));
    assert!(video.command(VideoCommand::Seek(0.4)));
    wait(|| {
        let s = video.snapshot();
        if s.generation > before {
            if let Some(e) = s.error {
                return Err(e);
            }
        }
        Ok(s.generation > before
            && s.duration == 2.0
            && s.playing
            && s.position >= 0.39
            && video.frame().is_some())
    })?;
    println!("PASS Open another source + queued Seek replaces the active source");
    let before = video.snapshot().generation;
    assert!(video.command(VideoCommand::Open {
        source: source(&silent, None, 2.0),
        start: 0.0,
        audio_only: false
    }));
    wait(|| {
        let s = video.snapshot();
        if let Some(e) = s.error {
            return Err(e);
        }
        Ok(s.generation > before && s.playing && s.position > 0.05 && video.frame().is_some())
    })?;
    println!("PASS deliberately silent video has a wall clock and decoded frames");
    let before = video.snapshot().generation;
    assert!(video.command(VideoCommand::Open {
        source: source(&path, Some("/does-not-exist/invalid-audio.m4a".into()), 6.0),
        start: 0.0,
        audio_only: false
    }));
    wait(|| {
        let s = video.snapshot();
        Ok(s.generation > before
            && s.error
                .as_ref()
                .is_some_and(|e| e.contains("No audio samples"))
            && !s.playing
            && !s.loading)
    })?;
    println!("PASS broken separate audio is reported instead of silently playing video");
    let before = video.snapshot().generation;
    assert!(video.command(VideoCommand::Open {
        source: source(&silent, None, 2.0),
        start: 0.0,
        audio_only: true
    }));
    wait(|| {
        let s = video.snapshot();
        Ok(s.generation > before
            && s.error
                .as_ref()
                .is_some_and(|e| e.contains("No audio samples"))
            && !s.playing
            && !s.loading)
    })?;
    println!("PASS Listen rejects a source without audio");
    let started = Instant::now();
    video.shutdown();
    assert!(started.elapsed() < Duration::from_secs(2));
    println!("PASS stop/join barrier");
    Ok(())
}
fn wait(mut condition: impl FnMut() -> Result<bool, String>) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(12);
    loop {
        if condition()? {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err("Timed out waiting for video transport".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
