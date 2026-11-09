//! Opt-in controller integration check using a real SDL/OpenGL renderer.
//! Run with rust/tools/check-app.sh; no product-only test routes are introduced.
use super::*;
fn settle(app: &mut App, r: &mut Renderer, window: &sdl2::video::Window) {
    for _ in 0..42 {
        app.tick(1.0 / 60.0, r);
        app.draw(r, app.visual_time);
        r.finish();
        window.gl_swap_window();
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn select(app: &mut App, label: &str) {
    let target = app
        .pages
        .last()
        .unwrap()
        .rows
        .iter()
        .position(|row| row.label == label)
        .unwrap_or_else(|| panic!("Missing menu row {label:?} on {:?}", app.screen()));
    let selected = app.pages.last().unwrap().selected;
    for _ in target..selected {
        app.handle(Action::Up);
    }
    for _ in selected..target {
        app.handle(Action::Down);
    }
    app.handle(Action::Select);
}
fn capture(
    app: &mut App,
    r: &mut Renderer,
    window: &sdl2::video::Window,
    out: &std::path::Path,
    name: &str,
) {
    settle(app, r, window);
    app.draw(r, app.visual_time);
    r.finish();
    r.screenshot(&out.join(format!("{name}.png"))).unwrap();
}
#[test]
#[ignore = "requires SDL/OpenGL, Xvfb and the audio fixture; run check-app.sh"]
fn controller_routes_and_real_screens() {
    let music =
        PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_MUSIC").expect("QA music fixture"));
    let assets = PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").expect("QA assets"));
    let out = PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_OUTPUT").expect("QA output"));
    let data = out.join(format!(
        "private-data-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&data).unwrap();
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let _audio = sdl.audio().unwrap();
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Rust controller QA", 640, 480)
        .opengl()
        .build()
        .unwrap();
    let _context = window.gl_create_context().unwrap();
    let mut r = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
    let mut app = App::new(music.clone(), data.clone(), assets, None, &r).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.scanning && Instant::now() < deadline {
        app.tick(1.0 / 60.0, &r);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!app.scanning);
    assert!(
        app.library.tracks.len() >= 4,
        "nonempty audio fixture required"
    );
    app.install_maintenance(
        MaintenanceController::new(
            crate::maintenance::Config {
                bootstrap: data.clone(),
                music: music.clone(),
                data: data.clone(),
                protected: vec![],
            },
            crate::updater::Config {
                installation: out.join("no-installation"),
                current_version: "0.1.0".into(),
                feed: None,
                beta_feed: None,
                allow_beta: false,
            },
        )
        .unwrap(),
    );
    capture(&mut app, &mut r, &window, &out, "01-home");
    select(&mut app, "Music");
    select(&mut app, "Cover Flow");
    assert_eq!(app.screen(), Screen::CoverFlow);
    app.handle(Action::Select);
    assert!(app.cover_card.is_some());
    capture(&mut app, &mut r, &window, &out, "02-coverflow-flip");
    app.handle(Action::Select);
    // Go CoverFlow.center enters its playing view from the selected card;
    // the Rust screen enum represents that nested playing state as NP.
    assert_eq!(app.screen(), Screen::NowPlaying);
    assert!(!app.queue.is_empty());
    settle(&mut app, &mut r, &window);
    // Go NowPlaying.OnCenter: progress → scrubber → options.
    app.handle(Action::Select);
    assert_eq!(app.np_mode, 1);
    app.handle(Action::Up);
    assert!(app.np_seek.is_some());
    capture(&mut app, &mut r, &window, &out, "03-nowplaying-scrub");
    app.handle(Action::Select);
    assert_eq!(app.np_mode, 2);
    let shuffle = app.settings.shuffle;
    app.handle(Action::Select);
    assert_ne!(app.settings.shuffle, shuffle);
    app.handle(Action::Back);
    app.handle(Action::Start);
    assert_eq!(app.screen(), Screen::CoverFlow);
    assert!(
        app.cover_card.is_some(),
        "START opens the original album card"
    );
    app.handle(Action::Back);
    assert_eq!(app.screen(), Screen::NowPlaying);
    app.handle(Action::BackHeld);
    assert_eq!(app.screen(), Screen::Home);
    select(&mut app, "Settings");
    let target = app
        .pages
        .last()
        .unwrap()
        .rows
        .iter()
        .position(|row| matches!(row.intent, Intent::Theme))
        .unwrap();
    for _ in 0..target {
        app.handle(Action::Down);
    }
    app.handle(Action::Right);
    assert!(app.preview_focus);
    app.handle(Action::Down);
    app.handle(Action::Select);
    capture(&mut app, &mut r, &window, &out, "04-settings-choice");
    app.handle(Action::Back);
    assert!(!app.preview_focus);
    app.handle(Action::BackHeld);
    select(&mut app, "Apps");
    select(&mut app, "2048");
    assert_eq!(app.screen(), Screen::Game);
    app.handle(Action::Start);
    capture(&mut app, &mut r, &window, &out, "05-game-menu");
    app.handle(Action::BackHeld);
    select(&mut app, "Apps");
    select(&mut app, "Puzzles");
    select(&mut app, PUZZLES[0].name);
    assert_eq!(app.screen(), Screen::Puzzle);
    capture(&mut app, &mut r, &window, &out, "06-puzzle");
    app.handle(Action::BackHeld);
    select(&mut app, "Settings");
    select(&mut app, "Screensaver");
    assert_eq!(app.screen(), Screen::Screensaver);
    app.settings.screensaver_style = "viz".into();
    select(&mut app, "Start Now");
    assert!(app.auto_saver);
    app.handle(Action::Right);
    app.handle(Action::Start);
    capture(&mut app, &mut r, &window, &out, "07-visualizer-clock");
    app.handle(Action::Back);
    settle(&mut app, &mut r, &window);
    assert!(!app.auto_saver, "Wait for the screensaver exit animation");
    app.handle(Action::BackHeld);
    select(&mut app, "Settings");
    select(&mut app, "Library");
    assert_eq!(app.screen(), Screen::Library);
    select(&mut app, "Missing Covers");
    assert_eq!(app.screen(), Screen::Curation);
    capture(&mut app, &mut r, &window, &out, "08-missing-covers");
    app.handle(Action::BackHeld);
    select(&mut app, "Settings");
    select(&mut app, "Library");
    select(&mut app, "Browse Files");
    assert_eq!(app.screen(), Screen::Files);
    capture(&mut app, &mut r, &window, &out, "09-file-browser");
    app.handle(Action::BackHeld);
    select(&mut app, "Settings");
    select(&mut app, "Library");
    assert!(app
        .pages
        .last()
        .unwrap()
        .rows
        .iter()
        .any(|row| row.label == "Songs"));
    capture(&mut app, &mut r, &window, &out, "10-library-statistics");
    select(&mut app, "Scanned Folders");
    assert_eq!(app.screen(), Screen::Files);
    capture(&mut app, &mut r, &window, &out, "11-scanned-folders");
    app.handle(Action::BackHeld);
    app.settings.screensaver_style = "cover".into();
    app.handle(Action::HoldDown);
    app.handle(Action::Options);
    app.handle(Action::HoldUp);
    assert!(app.auto_saver);
    assert!(!app.held);
    capture(&mut app, &mut r, &window, &out, "13-mosaic");
    app.handle(Action::Back);
    settle(&mut app, &mut r, &window);
    assert!(!app.auto_saver, "Wait for the screensaver exit animation");
    app.handle(Action::BackHeld);
    select(&mut app, "Music");
    select(&mut app, "Wi-Fi Library");
    assert_eq!(app.screen(), Screen::WifiLibrary);
    assert!(
        !app.wifi_library.active(),
        "Opening the page must not open a listener"
    );
    assert!(app
        .pages
        .last()
        .unwrap()
        .rows
        .iter()
        .any(|row| matches!(row.intent, Intent::StartWifiLibrary)));
    capture(&mut app, &mut r, &window, &out, "14-wifi-library-offline");
    app.handle(Action::Back);
    assert_eq!(app.screen(), Screen::Music);
    app.handle(Action::BackHeld);
    select(&mut app, "Settings");
    select(&mut app, "Wi-Fi Library");
    assert_eq!(app.screen(), Screen::WifiLibrary);
    assert!(!app.wifi_library.active());
    app.shutdown(&r).unwrap();
}

#[test]
#[ignore = "requires SDL/GLES, ffmpeg and an isolated WAV fixture; run check-app.sh"]
fn inline_preview_remote_pause_unplug_and_restore_use_real_decoder() {
    use betterpod_rust::online::{OnlineItem, Provider};
    use std::{
        collections::HashMap,
        io::{Read, Write},
        net::TcpListener,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicBool, Ordering},
    };
    let music =
        PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_MUSIC").expect("QA music fixture"));
    let assets = PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").expect("QA assets"));
    let out = PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_OUTPUT").expect("QA output"));
    let data = out.join(format!(
        "private-inline-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(data.join("fixture")).unwrap();
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let _audio = sdl.audio().unwrap();
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Rust Listen lifecycle regression", 640, 480)
        .opengl()
        .build()
        .unwrap();
    let _context = window.gl_create_context().unwrap();
    let mut renderer = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
    let mut app = App::new(music, data.clone(), assets, None, &renderer).unwrap();
    fn until(
        app: &mut App,
        r: &mut Renderer,
        window: &sdl2::video::Window,
        label: &str,
        predicate: impl Fn(&App) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            app.tick(0.01, r);
            assert!(
                app.extras
                    .preview_row_state()
                    .is_none_or(|state| state.error.is_none()),
                "{label}: {:?}",
                app.extras.preview_row_state()
            );
            if predicate(app) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Timed out {label}: mixer={:?}, player={:?}, cache={:?}, row={:?}",
                app.audio_snapshot,
                app.video.as_ref().map(|v| v.snapshot()),
                app.extras.preview_cache_snapshot(),
                app.extras.preview_row_state()
            );
            app.draw(r, app.visual_time);
            r.finish();
            window.gl_swap_window();
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    until(&mut app, &mut renderer, &window, "fixture scan", |app| {
        !app.scanning
    });
    let original = app
        .library
        .tracks
        .iter()
        .find(|track| track.path.extension().is_some_and(|ext| ext == "wav"))
        .expect("WAV fixture")
        .path
        .clone();
    let local = data.join("fixture/music.wav");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-stream_loop",
            "-1",
            "-i",
        ])
        .arg(&original)
        .args([
            "-t",
            "20",
            "-vn",
            "-c:a",
            "pcm_s16le",
            "-ar",
            "44100",
            "-ac",
            "2",
            "-y",
        ])
        .arg(&local)
        .status()
        .unwrap();
    assert!(status.success());
    let mut bodies = HashMap::new();
    for (name, length) in [
        ("first", 4.0),
        ("second", 3.0),
        ("short", 1.4),
        ("race", 4.0),
    ] {
        let path = data.join(format!("fixture/{name}.wav"));
        let status = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(&local)
            .args(["-t", &length.to_string(), "-c:a", "copy", "-y"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        bodies.insert(format!("/{name}.wav"), std::fs::read(path).unwrap());
    }
    let movie = data.join("fixture/movie.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=navy:size=160x120:rate=15",
            "-i",
        ])
        .arg(&local)
        .args([
            "-t",
            "4",
            "-c:v",
            "mpeg4",
            "-q:v",
            "7",
            "-c:a",
            "aac",
            "-b:a",
            "96k",
            "-movflags",
            "+faststart",
            "-y",
        ])
        .arg(&movie)
        .status()
        .unwrap();
    assert!(status.success());
    bodies.insert("/movie.mp4".into(), std::fs::read(&movie).unwrap());
    struct Server {
        url: String,
        first: Arc<AtomicBool>,
        race: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            self.first.store(true, Ordering::Release);
            self.race.store(true, Ordering::Release);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let first = Arc::new(AtomicBool::new(false));
    let race = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let (first_gate, race_gate, stopped) = (first.clone(), race.clone(), stop.clone());
    let worker = std::thread::spawn(move || {
        while !stopped.load(Ordering::Acquire) {
            let Ok((mut socket, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") && bytes.len() < 16384 {
                let n = socket.read(&mut buffer).unwrap_or(0);
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..n]);
            }
            if bytes.is_empty() {
                continue;
            }
            let request = String::from_utf8_lossy(&bytes);
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            let Some(body) = bodies.get(path) else {
                let _ = socket.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length:0\r\nConnection:close\r\n\r\n",
                );
                continue;
            };
            let gate = if path == "/first.wav" {
                Some(&first_gate)
            } else if path == "/race.wav" {
                Some(&race_gate)
            } else {
                None
            };
            while gate.is_some_and(|gate| !gate.load(Ordering::Acquire))
                && !stopped.load(Ordering::Acquire)
            {
                std::thread::sleep(Duration::from_millis(2));
            }
            if stopped.load(Ordering::Acquire) {
                break;
            }
            let range = request.lines().find_map(|line| {
                line.split_once(':')
                    .filter(|(key, _)| key.eq_ignore_ascii_case("range"))
                    .map(|(_, value)| value.trim())
            });
            let (start, end) = range
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.split_once('-'))
                .map(|(a, b)| {
                    (
                        a.parse::<usize>().unwrap(),
                        b.parse::<usize>()
                            .unwrap_or(body.len() - 1)
                            .min(body.len() - 1),
                    )
                })
                .unwrap_or((0, body.len() - 1));
            let response = if range.is_some() {
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\n",
                    body.len()
                )
            } else {
                "HTTP/1.1 200 OK\r\n".into()
            };
            let response = format!(
                "{response}Content-Length:{}\r\nAccept-Ranges:bytes\r\nConnection:close\r\n\r\n",
                end - start + 1
            );
            if socket.write_all(response.as_bytes()).is_ok() {
                let _ = socket.write_all(&body[start..=end]);
            }
        }
    });
    let server = Server {
        url,
        first,
        race,
        stop,
        worker: Some(worker),
    };
    let item = |name: &str, seconds: f64| {
        let mut item = OnlineItem::new(Provider::Archive);
        item.url = format!("{}/{}.wav", server.url, name);
        item.id = name.into();
        item.title = format!("Listen {name}");
        item.kind = "audio".into();
        item.seconds = seconds;
        item
    };
    app.install_library(
        ScannedLibrary::new(Library::from_tracks(vec![Track {
            path: local.clone(),
            title: "Own music".into(),
            artist: "QA".into(),
            album: "QA".into(),
            duration: 20.,
            sample_rate: 44100,
            bit_depth: 16,
            lossless: true,
            ..Default::default()
        }])),
        &renderer,
    );
    app.queue = Arc::new(vec![local.clone()]);
    app.queue_index = 0;
    app.play_current(1.0);
    until(&mut app, &mut renderer, &window, "own music plays", |app| {
        app.audio_snapshot.playing && app.audio_snapshot.position > 1.03
    });
    app.extras
        .qa_listen_page(vec![item("first", 4.), item("second", 3.)]);
    app.open(Screen::Extra);
    app.handle(Action::Select);
    until(
        &mut app,
        &mut renderer,
        &window,
        "Listen owns loading before response",
        |app| app.inline_preview && app.audio_snapshot.paused && app.extras.preview_waiting(),
    );
    assert!(!app.stream_restore.as_ref().unwrap().paused);
    assert!(app.extras.preview_row_state().unwrap().loading);
    server.first.store(true, Ordering::Release);
    until(
        &mut app,
        &mut renderer,
        &window,
        "real cached Listen plays",
        |app| {
            app.video
                .as_ref()
                .is_some_and(|v| v.snapshot().playing && v.snapshot().position > 0.03)
        },
    );
    assert_eq!(app.screen(), Screen::Extra);
    assert!(!app.video_open);
    assert!(!app.audio_snapshot.playing);
    let index = app.video_index;
    for action in [
        Action::MediaPlayPause,
        Action::MediaPlay,
        Action::MediaNext,
        Action::MediaPrevious,
    ] {
        app.handle(action);
    }
    for _ in 0..8 {
        app.tick(0.01, &renderer);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.video.as_ref().unwrap().snapshot().playing);
    assert_eq!(
        app.video_index, index,
        "generic transport must not pick Listen tracks"
    );
    app.handle(Action::Select);
    until(&mut app, &mut renderer, &window, "row A pauses", |app| {
        app.video.as_ref().unwrap().snapshot().paused
    });
    assert!(app.pending_stream_paused);
    app.handle(Action::Select);
    until(&mut app, &mut renderer, &window, "row A resumes", |app| {
        app.video.as_ref().unwrap().snapshot().playing
    });
    app.handle(Action::MediaUnplug);
    until(
        &mut app,
        &mut renderer,
        &window,
        "unplug pauses preview",
        |app| app.video.as_ref().unwrap().snapshot().paused,
    );
    assert!(app.stream_restore.as_ref().unwrap().paused);
    assert!(app.pending_stream_paused);
    app.handle(Action::Select);
    until(
        &mut app,
        &mut renderer,
        &window,
        "row A resumes after unplug",
        |app| app.video.as_ref().unwrap().snapshot().playing,
    );
    assert!(
        !app.pending_stream_paused,
        "explicit row resume must clear the pending pause intent"
    );
    assert!(app
        .video
        .as_ref()
        .unwrap()
        .command(VideoCommand::Seek(3.85)));
    until(
        &mut app,
        &mut renderer,
        &window,
        "prefetched next track plays after unplug-resume",
        |app| {
            app.video_index == 1
                && app.video.as_ref().unwrap().snapshot().playing
                && app.video.as_ref().unwrap().snapshot().position > 0.03
        },
    );
    assert!(!app.pending_stream_paused);
    app.handle(Action::MediaUnplug);
    until(
        &mut app,
        &mut renderer,
        &window,
        "next preview pauses safely",
        |app| app.video.as_ref().unwrap().snapshot().paused,
    );
    let saved_position = app.stream_restore.as_ref().unwrap().position;
    app.stop_inline_preview();
    until(
        &mut app,
        &mut renderer,
        &window,
        "stop restores paused own music",
        |app| {
            !app.inline_preview
                && app.stream_restore.is_none()
                && !app.video.as_ref().unwrap().snapshot().playing
        },
    );
    assert!(!app.audio_snapshot.playing);
    assert!(app.restored);
    assert_eq!(app.queue.as_ref(), &vec![local.clone()]);
    assert_eq!(app.restored_position, saved_position);

    // Actual delayed local resolver: old Listen must finish/restore own music
    // without canceling a different video's in-flight resolution.
    app.play_pause();
    until(
        &mut app,
        &mut renderer,
        &window,
        "own music resumes explicitly",
        |app| app.audio_snapshot.playing,
    );
    app.extras.qa_listen_page(vec![item("short", 1.4)]);
    app.handle(Action::Select);
    until(
        &mut app,
        &mut renderer,
        &window,
        "short preview plays",
        |app| app.inline_preview && app.video.as_ref().unwrap().snapshot().playing,
    );
    let saved_position = app.stream_restore.as_ref().unwrap().position;
    let tools = data.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let resolver = tools.join("yt-dlp");
    let movie_url = format!("{}/movie.mp4", server.url);
    let reply=serde_json::json!({"title":"Resolved QA movie","duration":4.0,"formats":[{"url":movie_url,"protocol":"http","vcodec":"mpeg4","acodec":"aac","height":120,"tbr":160}]}).to_string();
    std::fs::write(resolver.with_extension("json"), reply).unwrap();
    std::fs::write(
        &resolver,
        b"#!/bin/sh\nsleep 2\ncat -- \"${0%.json}.json\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&resolver, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut future = OnlineItem::new(Provider::YouTube);
    future.url = "https://youtube.com/watch?v=fixture-local-only".into();
    future.id = "fixture-local-only".into();
    future.title = "Resolved QA movie".into();
    future.kind = "video".into();
    future.seconds = 4.;
    app.extras
        .resolve_media(future.clone(), vec![future], 0, true, 0.);
    until(
        &mut app,
        &mut renderer,
        &window,
        "video opening coexists with old Listen",
        |app| app.inline_preview && app.visual_video_has_opening(),
    );
    until(
        &mut app,
        &mut renderer,
        &window,
        "old Listen EOF restores music without canceling resolver",
        |app| !app.inline_preview && app.audio_snapshot.playing && app.visual_video_has_opening(),
    );
    assert!(app.audio_snapshot.position + 0.15 >= saved_position);
    assert!(app.audio_snapshot.position < saved_position + 1.0);
    assert_eq!(app.audio_snapshot.path.as_ref(), Some(&local));
    until(
        &mut app,
        &mut renderer,
        &window,
        "delayed resolver still completes and actual movie plays",
        |app| {
            !app.visual_video_has_opening()
                && app.video_open
                && app.video.as_ref().unwrap().snapshot().playing
        },
    );
    assert_eq!(app.video_title, "Resolved QA movie");
    assert!(!app.inline_preview);
    app.handle(Action::Back);
    until(&mut app, &mut renderer, &window, "movie closes", |app| {
        app.screen() == Screen::Extra && !app.video_open
    });

    // Deterministically queue saved/direct Stream before an already completed
    // Listen Ready, so Extras.tick returns [Opening, Stream, stale Preview].
    app.play_pause();
    until(
        &mut app,
        &mut renderer,
        &window,
        "own music restarts for race",
        |app| app.audio_snapshot.playing,
    );
    app.extras.qa_listen_page(vec![item("race", 4.)]);
    app.handle(Action::Select);
    until(
        &mut app,
        &mut renderer,
        &window,
        "race preview waits in cache",
        |app| app.inline_preview && app.extras.preview_waiting(),
    );
    server.race.store(true, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.extras.qa_listen_worker_phase() != betterpod_rust::listen::Phase::Ready {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(3));
    }
    let mut direct = OnlineItem::new(Provider::Archive);
    direct.url = movie_url;
    direct.title = "Direct QA movie".into();
    direct.kind = "video".into();
    direct.seconds = 4.;
    app.extras
        .resolve_media(direct.clone(), vec![direct], 0, true, 0.);
    app.tick(0.01, &renderer);
    assert!(
        !app.inline_preview,
        "stale Ready must not retake audio after a newer full-screen Stream"
    );
    assert_eq!(app.video_title, "Direct QA movie");
    until(
        &mut app,
        &mut renderer,
        &window,
        "direct movie keeps ownership",
        |app| app.video_open && app.video.as_ref().unwrap().snapshot().playing,
    );
    assert!(!app.inline_preview);
    assert_eq!(app.video_title, "Direct QA movie");
    app.shutdown(&renderer).unwrap();
}

/// Regression for actual navigation (the static capture harness deliberately
/// snaps endpoints and therefore cannot catch a missing panel transition).
#[test]
#[ignore = "requires SDL/GLES; exercised by check-app.sh"]
fn custom_menu_entries_and_back_restore_visible_panels() {
    let music = PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_MUSIC").expect("QA music"));
    let assets = PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").expect("QA assets"));
    let out = PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_OUTPUT").expect("QA output"));
    let data = out.join(format!(
        "navigation-private-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&data).unwrap();
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let _audio = sdl.audio().unwrap();
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Custom menu navigation regression", 640, 480)
        .opengl()
        .build()
        .unwrap();
    let _context = window.gl_create_context().unwrap();
    let mut r = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
    let mut app = App::new(music.clone(), data.clone(), assets, None, &r).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.scanning && Instant::now() < deadline {
        app.tick(1. / 60., &r);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!app.scanning);
    app.settings.sharing_name = "QA".into();
    app.extras.set_sharing_name("QA".into());
    app.apply_setting(SettingValue::YouTube(true));
    app.install_maintenance(
        MaintenanceController::new(
            crate::maintenance::Config {
                bootstrap: data.clone(),
                music: music.clone(),
                data: data.clone(),
                protected: vec![],
            },
            crate::updater::Config {
                installation: out.join("no-installation"),
                current_version: "0.1.0".into(),
                feed: None,
                beta_feed: None,
                allow_beta: false,
            },
        )
        .unwrap(),
    );
    fn visible(app: &mut App, r: &mut Renderer, w: &sdl2::video::Window, label: &str) {
        settle(app, r, w);
        let rgba = r.capture_rgba();
        let count = rgba
            .pixels()
            .filter(|p| p[0] < 245 || p[1] < 245 || p[2] < 245)
            .count();
        assert!(
            count > 1000,
            "blank page at {label}: {}",
            app.remote_qa_state()
        );
        let page = app.navigation_page().unwrap();
        if page.split() {
            assert!(
                app.visual_menu_remote_state()["left"]
                    .as_f64()
                    .unwrap()
                    .abs()
                    < 0.001,
                "hidden split panel at {label}"
            );
        }
    }
    for path in [
        vec!["Podcasts"],
        vec!["Videos"],
        vec!["Music", "Playlists"],
        vec!["Music", "Receive"],
        vec!["Settings", "YouTube"],
        vec!["Apps", "Weather"],
        vec!["Settings", "Database"],
        vec!["Settings", "Library", "Missing Info"],
        vec!["Settings", "Library", "Missing Covers"],
    ] {
        app.handle(Action::BackHeld);
        settle(&mut app, &mut r, &window);
        for label in &path {
            select(&mut app, label);
            settle(&mut app, &mut r, &window);
        }
        visible(&mut app, &mut r, &window, &format!("open {path:?}"));
        if app.keyboard.is_some() || app.extras.is_typing() {
            app.handle(Action::BackHeld);
            settle(&mut app, &mut r, &window);
        }
        app.handle(Action::Back);
        visible(&mut app, &mut r, &window, &format!("back {path:?}"));
        assert!(
            app.visual_menu_remote_state()["views"]
                .as_array()
                .unwrap()
                .len()
                <= app.pages.len() + 1,
            "retained exited menu at {path:?}"
        );
    }
    app.handle(Action::BackHeld);
    settle(&mut app, &mut r, &window);
    // Repeated short Back must work without the long-Back reset previously
    // needed to recover the invisible parent.
    for _ in 0..12 {
        select(&mut app, "Podcasts");
        app.handle(Action::Back);
        visible(&mut app, &mut r, &window, "rapid podcast back");
    }
    // The cover's own animation must not leave the full-width player at
    // Cover Flow's hidden panel position. These use real, unsnapped inputs;
    // fixed scene captures would silently repair the stale panel state.
    for _ in 0..3 {
        app.handle(Action::BackHeld);
        settle(&mut app, &mut r, &window);
        select(&mut app, "Cover Flow");
        visible(&mut app, &mut r, &window, "cover flow");
        app.handle(Action::Select);
        visible(&mut app, &mut r, &window, "cover flow album card");
        app.handle(Action::Select);
        assert_eq!(app.screen(), Screen::NowPlaying);
        visible(&mut app, &mut r, &window, "cover flow song player");
        assert!(app.visual_menu_active_offset().abs() < 0.001);
        app.handle(Action::Back);
        assert_eq!(app.screen(), Screen::CoverFlow);
        visible(&mut app, &mut r, &window, "back to album card");
        app.handle(Action::Select);
        visible(&mut app, &mut r, &window, "resume album song player");
        app.handle(Action::Start);
        assert_eq!(app.screen(), Screen::CoverFlow);
        visible(&mut app, &mut r, &window, "player flipped to album card");
        app.handle(Action::Back);
        assert_eq!(app.screen(), Screen::NowPlaying);
        visible(&mut app, &mut r, &window, "back from flipped album card");
        assert!(app.visual_menu_active_offset().abs() < 0.001);
    }
    app.shutdown(&r).unwrap();
}

#[test]
#[ignore = "requires SDL/GLES and the copied music fixture"]
fn playlist_full_page_draws_its_reserved_mini_player() {
    let music = PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_MUSIC").unwrap());
    let assets = PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").unwrap());
    let out = PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_OUTPUT").unwrap());
    let data = out.join(format!(
        "playlist-mini-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&data).unwrap();
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let _audio = sdl.audio().unwrap();
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Rust playlist viewport regression", 640, 480)
        .opengl()
        .build()
        .unwrap();
    let _context = window.gl_create_context().unwrap();
    let mut r = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
    let mut app = App::new(music, data.clone(), assets, None, &r).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.scanning && Instant::now() < deadline {
        app.tick(1. / 60., &r);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!app.scanning && !app.library.tracks.is_empty());
    app.settings.theme = "light".into();
    let track = app.library.tracks[0].clone();
    app.queue = Arc::new(vec![track.path.clone()]);
    app.now_track = Some(track);
    app.execute(Intent::Playlist(betterpod_rust::playlists::Playlist {
        name: "Playlist viewport".into(),
        path: data.join("playlists/viewport.m3u8"),
        tracks: app
            .library
            .tracks
            .iter()
            .take(16)
            .map(|t| t.path.clone())
            .collect(),
        read_only: false,
    }));
    capture(
        &mut app,
        &mut r,
        &window,
        &out,
        "rust-playlist-full-with-mini",
    );
    let page = app.navigation_page().unwrap();
    assert_eq!(page.kind, ViewKind::Full);
    assert!(page.mini);
    assert_eq!(app.visual_menu_content_height(&page), 324.0);
    let (wanted, shown) = app.visual_menu_qa_mini_state();
    assert!(wanted && shown > 0.99, "reserved player not drawn: {shown}");
    let image = r.capture_rgba();
    let bottom_ink = image
        .enumerate_pixels()
        .filter(|(_, y, p)| *y >= 380 && (p[0] < 210 || p[1] < 210 || p[2] < 210))
        .count();
    assert!(
        bottom_ink > 1000,
        "bottom viewport is empty despite its reserved mini-player"
    );
    assert_eq!(r.diagnostics()["gl_errors"], serde_json::json!([]));
    assert!(!app
        .reference_home_rows()
        .iter()
        .any(|row| row.label == "Downloads"));
    // A real local failure exercises the start/finish lifecycle without any
    // provider or console dependency. The UI owns queued state immediately.
    app.extras
        .download(betterpod_rust::downloads::DownloadRequest::File {
            url: "http://127.0.0.1:9/offline-fixture".into(),
            name: "Local download fixture".into(),
            category: "QA".into(),
            tags: None,
        });
    let music = app.reference_music_rows();
    let downloads = music.iter().find(|row| row.label == "Downloads").unwrap();
    assert_eq!(downloads.visual.value, "1");
    let until = Instant::now() + Duration::from_secs(3);
    while app.extras.active_download_count() > 0 && Instant::now() < until {
        app.tick(0.01, &r);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.extras.active_download_count(), 0);
    let recent = app
        .reference_music_rows()
        .into_iter()
        .find(|row| row.label == "Downloads")
        .expect("Recent jobs must remain reachable from Music");
    assert_eq!(recent.visual.value, "Recent");
    app.execute(recent.intent);
    let page = app.navigation_page().unwrap();
    assert_eq!(page.title, "Downloads");
    assert!(page.rows.iter().any(|row| row.label == "Recent downloads"));
    app.handle(Action::Select);
    assert_eq!(app.extras.title(), "Download");
    assert!(app.extras.has_custom_visual());
    capture(
        &mut app,
        &mut r,
        &window,
        &out,
        "rust-reopened-download-failure",
    );
    app.handle(Action::Back);
    assert_eq!(app.navigation_page().unwrap().title, "Downloads");
    app.handle(Action::Options);
    capture(
        &mut app,
        &mut r,
        &window,
        &out,
        "rust-download-history-options",
    );
    assert!(app
        .extras
        .visual_options()
        .unwrap()
        .rows
        .iter()
        .any(|row| row.label == "Remove from List"));
    app.handle(Action::Down);
    app.handle(Action::Select);
    capture(
        &mut app,
        &mut r,
        &window,
        &out,
        "rust-download-history-clear-confirmation",
    );
    app.handle(Action::Select);
    capture(
        &mut app,
        &mut r,
        &window,
        &out,
        "rust-download-history-empty",
    );
    assert_eq!(app.extras.active_download_count(), 0);
    assert!(!app.extras.has_downloads());
    let cached_home = app
        .pages
        .iter()
        .find(|page| page.screen == Screen::Home)
        .unwrap();
    assert!(
        !cached_home.rows.iter().any(|row| row.label == "Downloads"),
        "Clearing failed history must refresh cached Home even when active count stays zero"
    );
    app.shutdown(&r).unwrap();
}
