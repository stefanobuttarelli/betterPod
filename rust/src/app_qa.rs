//! Opt-in controller integration check using a real SDL/OpenGL renderer.
//! Run with rust/tools/check-app.sh; no product-only test routes are introduced.
use super::*;
fn settle(app: &mut App, r: &mut Renderer, window: &sdl2::video::Window) {
    for frame in 0..18 {
        app.tick(1.0 / 60.0, r);
        app.draw(r, frame as f64 / 60.0);
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
    app.draw(r, 1.0);
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
    let data = out.join("private-data");
    std::fs::create_dir_all(&data).unwrap();
    let profile_library = Library::from_tracks(
        (0..10_000)
            .map(|i| Track {
                path: format!("/profile/album{}/song{i}.wav", i / 20).into(),
                title: format!("Song {i}"),
                artist: format!("Artist {}", i % 100),
                album: format!("Album {}", i / 20),
                ..Default::default()
            })
            .collect(),
    );
    let started = Instant::now();
    let projections = ScannedLibrary::new(profile_library);
    let prepared = started.elapsed();
    let started = Instant::now();
    for _ in 0..10_000 {
        std::hint::black_box(Arc::clone(&projections.songs));
    }
    eprintln!(
        "10k-track row preparation {:?} (worker); 10k cached navigation clones {:?}",
        prepared,
        started.elapsed()
    );
    drop(projections);
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
    assert_eq!(app.screen(), Screen::NowPlaying);
    app.handle(Action::Select);
    assert_eq!(app.np_mode, 1);
    app.handle(Action::Right);
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
    app.handle(Action::Start);
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
    select(&mut app, "Apps");
    select(&mut app, "Visualizer");
    assert_eq!(app.screen(), Screen::Visualizer);
    app.handle(Action::Right);
    app.handle(Action::Start);
    capture(&mut app, &mut r, &window, &out, "07-visualizer-clock");
    app.handle(Action::BackHeld);
    select(&mut app, "Settings");
    select(&mut app, "Missing Covers");
    assert_eq!(app.screen(), Screen::Curation);
    capture(&mut app, &mut r, &window, &out, "08-missing-covers");
    app.handle(Action::BackHeld);
    select(&mut app, "Settings");
    select(&mut app, "Browse Files");
    assert_eq!(app.screen(), Screen::Files);
    capture(&mut app, &mut r, &window, &out, "09-file-browser");
    app.handle(Action::BackHeld);
    select(&mut app, "Settings");
    select(&mut app, "Library Information");
    capture(&mut app, &mut r, &window, &out, "10-library-statistics");
    app.handle(Action::BackHeld);
    select(&mut app, "Settings");
    select(&mut app, "Scanned Folders");
    capture(&mut app, &mut r, &window, &out, "11-scanned-folders");
    app.handle(Action::BackHeld);
    select(&mut app, "Settings");
    select(&mut app, "Maintenance");
    assert_eq!(app.screen(), Screen::Maintenance);
    app.handle(Action::Select);
    assert_eq!(app.maintenance.as_ref().unwrap().title(), "Confirm");
    app.handle(Action::Back);
    assert!(app.maintenance_request.is_none());
    capture(&mut app, &mut r, &window, &out, "12-maintenance-cancel");
    app.handle(Action::BackHeld);
    app.handle(Action::HoldDown);
    app.handle(Action::Options);
    app.handle(Action::HoldUp);
    assert!(app.auto_saver);
    assert!(!app.held);
    capture(&mut app, &mut r, &window, &out, "13-mosaic");
    app.handle(Action::Back);
    app.shutdown(&r).unwrap();
}
