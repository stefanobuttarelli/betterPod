//! Real controller/render regressions for the reported console flows. All
//! songs, artwork, preferences and captures live in a private fixture tree.
use super::*;

fn wav(path: &std::path::Path) {
    let length = 44100u32 * 2 * 30;
    let mut bytes = Vec::with_capacity(length as usize + 44);
    bytes.extend(b"RIFF");
    bytes.extend((36 + length).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(44100u32.to_le_bytes());
    bytes.extend(88200u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(length.to_le_bytes());
    bytes.resize(44 + length as usize, 0);
    std::fs::write(path, bytes).unwrap();
}
fn capture(app: &mut App, r: &mut Renderer, out: &std::path::Path, name: &str, now: f64) {
    app.visual_time = now;
    app.visual_player_tick(now, 0.);
    app.draw(r, now);
    r.finish();
    r.screenshot(&out.join(format!("review-{name}.png")))
        .unwrap();
}

#[test]
#[ignore = "requires SDL/GLES; run rust/tools/check-app.sh"]
fn playback_saver_cache_and_navigation_regressions() {
    let assets = PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").expect("QA assets"));
    let out = PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_OUTPUT").expect("QA output"));
    let root = out.join(format!("private-player-review-{}", std::process::id()));
    let music = root.join("music");
    let data = root.join("data");
    std::fs::create_dir_all(&music).unwrap();
    let mut tracks = Vec::new();
    for i in 0..3 {
        let path = music.join(format!("{i}.wav"));
        wav(&path);
        let cover = root.join(format!("cover-{i}.png"));
        image::RgbaImage::from_pixel(
            64,
            64,
            image::Rgba([[220, 60, 60, 255], [60, 180, 90, 255], [50, 100, 220, 255]][i]),
        )
        .save(&cover)
        .unwrap();
        tracks.push(Track {
            path,
            cover: Some(cover),
            title: format!("Song {i}"),
            artist: "Fixture Artist".into(),
            album: format!("Album {i}"),
            duration: 30.,
            lossless: i == 1,
            ..Default::default()
        });
    }
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let _audio = sdl.audio().unwrap();
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Player regression QA", 640, 480)
        .opengl()
        .build()
        .unwrap();
    let _gl = window.gl_create_context().unwrap();
    let mut r = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
    let mut app = App::new(music, data.clone(), assets, None, &r).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.scanning {
        app.tick(0.01, &r);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    app.install_library(
        ScannedLibrary::new(Library::from_tracks(tracks.clone())),
        &r,
    );
    for track in &tracks {
        app.art.get(track.cover.as_deref());
    }
    while tracks
        .iter()
        .any(|t| app.art.palette(t.cover.as_deref()).is_none())
    {
        app.art.poll(&r);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let cached = app.art.get(tracks[0].cover.as_deref()).id;
    app.install_library(
        ScannedLibrary::new(Library::from_tracks(tracks.clone())),
        &r,
    );
    assert_eq!(
        app.art.get(tracks[0].cover.as_deref()).id,
        cached,
        "A rescan must retain unchanged GPU covers"
    );
    app.open(Screen::Lossless);
    assert_eq!(app.pages.last().unwrap().rows.len(), 1);
    assert_eq!(app.pages.last().unwrap().rows[0].label, "Album 1");
    capture(&mut app, &mut r, &out, "lossless", 2.);

    app.open(Screen::CoverFlow);
    app.cover_selected = 0;
    app.handle(Action::Select);
    app.visual_player_snap_state(3.);
    app.handle(Action::Select);
    assert_eq!(app.screen(), Screen::NowPlaying);
    // Observe before the decoder acknowledges Play: its old stopped snapshot
    // must not make the shared cover first travel to the paused pose.
    app.audio_snapshot.playing = false;
    app.audio_snapshot.paused = false;
    app.visual_player_tick(3.01, 0.);
    assert_eq!(app.player_visual.tilt.target(), 1.);
    assert_eq!(
        app.player_visual.covers[&0].pose.to.angle,
        now_playing_pose(1.).angle - 360.
    );
    app.visual_menu_tick(3.02);
    app.visual_time = 3.4;
    app.visual_menu_tick(3.4);
    assert_eq!(
        app.visual_menu_remote_state()["mini_pause"],
        0.,
        "Loading a requested song must not flash pause"
    );
    app.audio_snapshot.playing = false;
    app.audio_snapshot.paused = true;
    app.idle_play_pending = None;
    app.visual_player_tick(3.4, 0.);
    let mut angle = app.player_visual.covers[&0].pose.value.angle;
    for frame in 1..=35 {
        app.visual_player_tick(3.4 + frame as f64 / 60., 1. / 60.);
        let next = app.player_visual.covers[&0].pose.value.angle;
        assert!(
            (next - angle).abs() < 20.,
            "Pause during entry must not jump at the flip handoff"
        );
        angle = next;
    }
    app.audio_snapshot.paused = false;
    app.audio_snapshot.playing = true;
    app.idle_play_pending = None;
    app.visual_player_snap_state(4.);
    capture(&mut app, &mut r, &out, "now-playing", 4.);
    let volume = app.settings.volume;
    app.handle(Action::Right);
    assert_eq!(app.np_mode, 1);
    app.handle(Action::Right);
    assert_eq!(app.np_mode, 2);
    app.handle(Action::Down);
    assert_eq!(app.np_option, 1);
    app.handle(Action::Select);
    assert_ne!(app.settings.repeat, "off");
    assert_eq!(app.settings.volume, volume);
    capture(&mut app, &mut r, &out, "repeat", 5.);

    app.audio_snapshot.playing = false;
    app.audio_snapshot.paused = true;
    app.visual_player_tick(6., 0.);
    let mut previous = app.player_visual.covers[&0].pose.value.angle;
    for frame in 1..=48 {
        let now = 6. + frame as f64 / 60.;
        app.visual_player_tick(now, 1. / 60.);
        let angle = app.player_visual.covers[&0].pose.value.angle;
        assert!(
            (angle - previous).abs() < 3.,
            "Pause pose jumped at {now}: {previous} → {angle}"
        );
        previous = angle;
        if matches!(frame, 1 | 15 | 30 | 42) {
            capture(&mut app, &mut r, &out, &format!("pause-{frame}"), now);
        }
    }
    assert!((previous - (now_playing_pose(0.).angle - 360.)).abs() < 0.001);

    app.audio_snapshot.playing = true;
    app.audio_snapshot.paused = false;
    app.enter_saver();
    app.saver_action(SaverAction::PlayTrack(1));
    app.visual_player_tick(8., 0.);
    app.saver_action(SaverAction::Exit);
    app.visual_player_tick(9., 0.);
    assert_eq!(app.screen(), Screen::NowPlaying);
    assert_eq!(app.cover_selected, 1);
    assert_eq!(
        app.player_visual.current.as_ref().unwrap().cover,
        tracks[1].cover
    );
    app.handle(Action::Back);
    assert_eq!(app.screen(), Screen::CoverFlow);
    assert_eq!(
        app.cover_selected, 1,
        "Back must open the album selected in Cover Flip"
    );
    app.cover_card = None;
    app.cover_selected = 2;
    app.last_activity = Instant::now() - Duration::from_secs(11);
    app.visual_player_tick(10., 0.);
    assert_eq!(app.cover_selected, 1);

    app.open(Screen::Music);
    app.enter_saver();
    app.audio_snapshot.playing = false;
    app.audio_snapshot.paused = true;
    app.idle_play_pending = None;
    app.saver_action(SaverAction::Exit);
    assert_eq!(
        app.screen(),
        Screen::Music,
        "Paused music returns to the original page"
    );
    app.held = true;
    app.settings.screensaver_seconds = 1;
    app.last_activity = Instant::now() - Duration::from_secs(2);
    app.tick(0.01, &r);
    assert!(app.auto_saver, "Hold must allow the automatic screensaver");
    app.leave_saver();
    app.held = false;
    app.settings.screensaver_seconds = 0;

    app.notify("Hold On");
    capture(&mut app, &mut r, &out, "hold", 12.);
    capture(&mut app, &mut r, &out, "hold-visible", 12.3);
    app.toast = None;
    app.feedback = Feedback::new();
    app.visual_menu.status.battery = Some(82);
    app.open(Screen::WifiLibrary);
    for (i, phase) in ["off", "starting", "ready", "connected", "failed"]
        .iter()
        .enumerate()
    {
        app.wifi_library_fixture(phase);
        capture(
            &mut app,
            &mut r,
            &out,
            &format!("wifi-{phase}"),
            14. + i as f64,
        );
    }
    app.wifi_library_fixture("off");
    app.open(Screen::Albums);
    app.visual_time = 18.1;
    app.visual_menu_tick(18.1);
    app.handle(Action::Down);
    app.visual_time = 18.2;
    app.visual_menu_tick(18.2);
    assert_eq!(app.visual_menu_remote_state()["scroll_visible"], true);
    app.visual_time = 19.5;
    app.visual_menu_tick(19.5);
    assert_eq!(app.visual_menu_remote_state()["scroll_visible"], false);

    app.open(Screen::Settings);
    app.pages.last_mut().unwrap().selected = app
        .pages
        .last()
        .unwrap()
        .rows
        .iter()
        .position(|row| matches!(row.intent, Intent::SharingSettings))
        .unwrap();
    app.refresh_setting_preview();
    app.preview_focus = true;
    app.execute(Intent::SharingName);
    assert!(!app.keyboard.as_ref().unwrap().search);
    capture(&mut app, &mut r, &out, "sharing-enter", 20.);
    capture(&mut app, &mut r, &out, "sharing-enter-visible", 20.5);
    app.keyboard = None;
    let wide = root.join("wide-thumbnail.png");
    let mut pixels = image::RgbImage::from_pixel(160, 90, image::Rgb([40, 80, 160]));
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        if (x as i32 - 80).pow(2) + (y as i32 - 45).pow(2) < 30 * 30 {
            *pixel = image::Rgb([255, 255, 255]);
        }
    }
    pixels.save(&wide).unwrap();
    let mut video_item =
        betterpod_rust::online::OnlineItem::new(betterpod_rust::online::Provider::YouTube);
    video_item.url = "https://www.youtube.com/watch?v=abcdefghijk".into();
    video_item.title = "Wide thumbnail fixture".into();
    video_item.artist = "Fixture Artist".into();
    video_item.seconds = 7384.;
    video_item.cover = "https://example.invalid/wide.png".into();
    app.extras
        .qa_youtube_details(video_item, Some(wide.clone()));
    app.open(Screen::Extra);
    app.art.get(Some(&wide));
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.art.palette(Some(&wide)).is_none() {
        app.art.poll(&r);
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    capture(&mut app, &mut r, &out, "youtube-wide-thumbnail", 21.5);
    app.handle(Action::Down);
    assert_eq!(app.navigation_page().unwrap().rows[2].label, "Watch Video");
    app.handle(Action::Select);
    app.tick(0.01, &r);
    assert_eq!(
        app.screen(),
        Screen::Video,
        "Watch from Music search opens the full video loader"
    );
    capture(&mut app, &mut r, &out, "youtube-opening", 22.);
    app.handle(Action::Back);
    assert_eq!(app.screen(), Screen::Extra);
    assert_eq!(app.navigation_page().unwrap().selected, 2);
    // Retain visible GPU artwork under pressure, then invalidate an edited
    // file without evicting an unrelated album during the next scan.
    let retained = app.art.peek(tracks[1].cover.as_deref()).id;
    let deadline = Instant::now() + Duration::from_secs(10);
    for index in 0..44 {
        let cover = root.join(format!("pressure-{index}.png"));
        image::RgbImage::from_pixel(8, 8, image::Rgb([index, 100, 200]))
            .save(&cover)
            .unwrap();
        app.art.get(Some(&cover));
        while app.art.palette(Some(&cover)).is_none() {
            app.art.peek(tracks[1].cover.as_deref());
            app.art.poll(&r);
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
    }
    assert_eq!(app.art.peek(tracks[1].cover.as_deref()).id, retained);
    image::RgbImage::from_pixel(80, 80, image::Rgb([240, 240, 40]))
        .save(tracks[1].cover.as_ref().unwrap())
        .unwrap();
    app.install_library(ScannedLibrary::new(Library::from_tracks(tracks)), &r);
    assert!(app
        .art
        .missing(app.now_track.as_ref().unwrap().cover.as_deref()));
    let downloaded = data.join("downloads/YouTube/New Artist/New Album/01 New Song.wav");
    std::fs::create_dir_all(downloaded.parent().unwrap()).unwrap();
    wav(&downloaded);
    app.open(Screen::Albums);
    app.extras.qa_download_completion();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app
        .library
        .tracks
        .iter()
        .any(|track| track.path == downloaded)
    {
        app.tick(0.01, &r);
        assert!(
            Instant::now() < deadline,
            "Download completion did not refresh the library"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert!(app
        .page(Screen::Albums)
        .rows
        .iter()
        .any(|row| row.label == "New Album"));
    assert!(app
        .page(Screen::Artists)
        .rows
        .iter()
        .any(|row| row.label == "New Artist"));
    // Actual shutdown exercises renderer ownership while actors stop and flush.
    app.shutdown_animated(&mut r, &window).unwrap();
    assert!(data.join("settings.json").is_file());
    assert!(data.join("playback.json").is_file());
}
