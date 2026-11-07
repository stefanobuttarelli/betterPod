//! Opt-in live decoder/controller proof. All media is generated in private
//! test data, SDL audio is dummy and no resolver/network request is required.
use super::*;
fn until(
    app: &mut App,
    r: &mut Renderer,
    window: &sdl2::video::Window,
    label: &str,
    predicate: impl Fn(&App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.tick(0.01, r);
        if predicate(app) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{label}: audio={:?}, video={:?}",
            app.audio_snapshot,
            app.video.as_ref().map(|v| v.snapshot())
        );
        let now = app.visual_time;
        app.draw(r, now);
        r.finish();
        window.gl_swap_window();
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn local_item(path: &std::path::Path, title: &str) -> betterpod_rust::online::OnlineItem {
    let mut item =
        betterpod_rust::online::OnlineItem::new(betterpod_rust::online::Provider::Archive);
    item.url = path.to_string_lossy().into_owned();
    item.id = title.into();
    item.title = title.into();
    item.kind = "video".into();
    item.seconds = 20.;
    item
}
#[test]
#[ignore = "requires SDL/GLES and ffmpeg; isolated dummy-audio local controller test"]
fn fullscreen_video_source_controls_and_opening_cancel_use_real_decoder() {
    let assets = PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").expect("QA assets"));
    let out = PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_OUTPUT").expect("QA output"));
    let root = out.join(format!(
        "private-fullscreen-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let music = root.join("music");
    let data = root.join("data");
    std::fs::create_dir_all(&music).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let song = music.join("own-music.wav");
    let movie = data.join("synthetic-video.mp4");
    assert!(std::process::Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100",
            "-t",
            "20",
            "-ac",
            "2",
            "-c:a",
            "pcm_s16le",
            "-y"
        ])
        .arg(&song)
        .status()
        .unwrap()
        .success());
    assert!(std::process::Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=660:sample_rate=44100",
            "-t",
            "20",
            "-c:v",
            "mpeg4",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-ac",
            "2",
            "-y"
        ])
        .arg(&movie)
        .status()
        .unwrap()
        .success());
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let _audio = sdl.audio().unwrap();
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Full-screen Video source regression", 640, 480)
        .opengl()
        .build()
        .unwrap();
    let _context = window.gl_create_context().unwrap();
    let mut renderer = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
    let mut app = App::new(music, data, assets, None, &renderer).unwrap();
    until(&mut app, &mut renderer, &window, "fixture scan", |app| {
        !app.scanning
    });
    app.queue = Arc::new(vec![song.clone()]);
    app.queue_index = 0;
    app.play_current(1.);
    until(&mut app, &mut renderer, &window, "own music plays", |app| {
        app.audio_snapshot.playing && app.audio_snapshot.position > 1.03
    });
    let opening = local_item(&movie, "Opening test");
    app.visual_video_opening(opening.clone(), vec![opening], 0, 0.);
    app.handle(Action::MediaPlayPause);
    for _ in 0..3 {
        app.tick(0.01, &renderer);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        app.audio_snapshot.playing,
        "opening transport must not toggle the user's music"
    );
    app.handle(Action::Back);
    assert!(app.audio_snapshot.playing);
    assert!(!app.visual_video_waiting());
    assert!(app.stream_restore.is_none());
    app.extras
        .open(ExtraPage::Online(betterpod_rust::online::Provider::Archive));
    app.extras.cancel_typing();
    app.open(Screen::Extra);
    let audio = StreamSource {
        video_url: song.to_string_lossy().into_owned(),
        audio_url: None,
        headers: Vec::new(),
        duration: 20.,
        title: "Existing Listen".into(),
        available_at: None,
        chapters: 0,
    };
    app.begin_stream(
        audio,
        false,
        "existing-listen".into(),
        0.,
        Vec::new(),
        0,
        true,
        &renderer,
    )
    .unwrap();
    until(
        &mut app,
        &mut renderer,
        &window,
        "Listen plays before opening Video",
        |app| {
            app.pending_stream.is_none() && app.video.as_ref().is_some_and(|v| v.snapshot().playing)
        },
    );
    let generation = app.video.as_ref().unwrap().snapshot().generation;
    let opening = local_item(&movie, "Cancel over Listen");
    app.visual_video_opening(opening.clone(), vec![opening], 0, 0.);
    assert!(app.inline_preview);
    assert_eq!(app.video_origin, "existing-listen");
    app.handle(Action::MediaPlayPause);
    app.handle(Action::Back);
    assert!(app.inline_preview);
    assert_eq!(app.screen(), Screen::Extra);
    assert_eq!(app.video_origin, "existing-listen");
    assert_eq!(
        app.video.as_ref().unwrap().snapshot().generation,
        generation
    );
    assert!(
        app.video.as_ref().unwrap().snapshot().playing,
        "cancelled resolver must not stop the old Listen"
    );
    app.stop_inline_preview();
    until(
        &mut app,
        &mut renderer,
        &window,
        "Listen normal stop restores own music",
        |app| app.audio_snapshot.playing && app.stream_restore.is_none(),
    );
    let item = local_item(&movie, "Local full-screen movie");
    let source = StreamSource {
        video_url: item.url.clone(),
        audio_url: None,
        headers: Vec::new(),
        duration: 20.,
        title: item.title.clone(),
        available_at: None,
        chapters: 0,
    };
    app.begin_stream(
        source,
        true,
        item.url.clone(),
        0.,
        vec![item],
        0,
        false,
        &renderer,
    )
    .unwrap();
    until(
        &mut app,
        &mut renderer,
        &window,
        "movie picture and sound start",
        |app| {
            app.video_texture.is_some()
                && app.pending_stream.is_none()
                && app.video.as_ref().is_some_and(|v| {
                    let s = v.snapshot();
                    s.playing && !s.paused && s.position > 0.03
                })
        },
    );
    assert_eq!(app.screen(), Screen::Video);
    assert!(!app.audio_snapshot.playing);
    assert!(
        app.stream_restore.is_some(),
        "Videos retain the user's music position"
    );
    app.video_visual.controls.snap(0.);
    app.video_visual.until = -1.;
    app.handle(Action::Select);
    assert_eq!(app.video_visual.controls.target(), 1.);
    assert!(
        !app.video.as_ref().unwrap().snapshot().paused,
        "first A reveals hidden controls"
    );
    app.handle(Action::Select);
    until(&mut app, &mut renderer, &window, "second A pauses", |app| {
        app.video.as_ref().unwrap().snapshot().paused
    });
    app.handle(Action::Start);
    until(&mut app, &mut renderer, &window, "Start resumes", |app| {
        let s = app.video.as_ref().unwrap().snapshot();
        s.playing && !s.paused
    });
    app.handle(Action::Left);
    app.handle(Action::Right);
    app.video_visual.seeking = None;
    app.handle(Action::Right);
    assert!(app.video_visual.seeking.is_some());
    app.screen_off = true;
    until(
        &mut app,
        &mut renderer,
        &window,
        "seek flushes with screen blanked",
        |app| {
            app.video_visual.seeking.is_none()
                && app.video.as_ref().unwrap().snapshot().position > 9.
        },
    );
    app.screen_off = false;
    app.handle(Action::Options);
    assert!(app
        .options
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|row| row.label == "Watch from the Start"));
    app.options = None;
    app.video
        .as_ref()
        .unwrap()
        .command(VideoCommand::Seek(19.85));
    until(
        &mut app,
        &mut renderer,
        &window,
        "movie reaches its real end",
        |app| app.video.as_ref().unwrap().snapshot().ended,
    );
    app.handle(Action::Select);
    until(
        &mut app,
        &mut renderer,
        &window,
        "A replays ended movie",
        |app| {
            let s = app.video.as_ref().unwrap().snapshot();
            s.playing && !s.ended && s.position < 2.
        },
    );
    let invalid = StreamSource {
        video_url: root
            .join("missing-video.mp4")
            .to_string_lossy()
            .into_owned(),
        audio_url: None,
        headers: Vec::new(),
        duration: 20.,
        title: "Failed decoder attempt".into(),
        available_at: None,
        chapters: 0,
    };
    app.video.as_ref().unwrap().command(VideoCommand::Open {
        source: invalid,
        start: 0.,
        audio_only: false,
    });
    until(
        &mut app,
        &mut renderer,
        &window,
        "real decoder error reaches UI",
        |app| app.video.as_ref().unwrap().snapshot().error.is_some(),
    );
    app.handle(Action::Select);
    until(
        &mut app,
        &mut renderer,
        &window,
        "A retries the original local source",
        |app| {
            !app.video_visual.resolving
                && app.pending_stream.is_none()
                && app.video.as_ref().is_some_and(|v| {
                    let s = v.snapshot();
                    s.error.is_none() && s.playing
                })
        },
    );
    app.handle(Action::Back);
    until(
        &mut app,
        &mut renderer,
        &window,
        "full Video closes without resuming own music",
        |app| {
            !app.video_open
                && app.stream_restore.is_none()
                && !app.video.as_ref().unwrap().snapshot().playing
        },
    );
    assert!(!app.audio_snapshot.playing);
    assert!(app.restored);
    assert_eq!(app.queue.as_ref(), &vec![song]);
    app.shutdown(&renderer).unwrap();
}
