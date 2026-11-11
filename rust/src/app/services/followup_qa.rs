//! Regressions for console reports; fixtures never use a live service or music.
use super::*;
use std::time::{Duration, Instant};

fn fixture(name: &str) -> (PathBuf, Extras) {
    let root = std::env::temp_dir().join(format!("bp-followup-{name}-{}", std::process::id()));
    std::fs::create_dir_all(root.join("music")).unwrap();
    let extras = Extras::new(root.join("music"), root.join("data")).unwrap();
    (root, extras)
}

#[test]
fn imported_playlist_y_delete_requires_confirmation_and_preserves_songs() {
    let (root, mut extras) = fixture("playlist");
    let song = root.join("music/song.flac");
    std::fs::write(&song, b"original audio").unwrap();
    let path = root.join("music/Imported.m3u8");
    std::fs::write(&path, b"#EXTM3U\nsong.flac\n").unwrap();
    let list = Playlist {
        name: "Imported".into(),
        path: path.clone(),
        tracks: vec![song.clone()],
        read_only: true,
    };
    extras.open(ExtraPage::Playlists);
    extras.apply_playlist_listing(vec![list]);
    extras.pages.last_mut().unwrap().selected = 1;
    extras.handle(Action::Options);
    let page = extras.pages.last_mut().unwrap();
    page.selected = page
        .rows
        .iter()
        .position(|row| row.title == "Delete Playlist")
        .unwrap();
    extras.handle(Action::Select);
    assert!(path.exists());
    assert!(extras.pages.last().unwrap().role == PageRole::Sheet);
    extras.handle(Action::Back);
    assert!(path.exists(), "Back must leave the playlist intact");
    extras.handle(Action::Options);
    let page = extras.pages.last_mut().unwrap();
    page.selected = page
        .rows
        .iter()
        .position(|row| row.title == "Delete Playlist")
        .unwrap();
    extras.handle(Action::Select);
    extras.handle(Action::Select);
    let deadline = Instant::now() + Duration::from_secs(3);
    while path.exists() || !extras.lists.is_empty() {
        extras.tick(0.01);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(std::fs::read(song).unwrap(), b"original audio");
    extras.shutdown();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn large_archive_rows_share_album_and_queue_instead_of_copying_them_per_track() {
    let (root, mut extras) = fixture("archive");
    let mut item = OnlineItem::new(Provider::Archive);
    item.id = "large-album".into();
    item.raw = serde_json::json!({"groups":{"FLAC":(0..500).map(|i| serde_json::json!({"name":format!("{i}.flac"),"download_url":format!("https://example.invalid/{i}"),"size":1000})).collect::<Vec<_>>()}});
    let rows = extras.archive_rows(&item);
    let mut previous_queue = None;
    let mut previous_album = None;
    let mut tracks = 0;
    for row in &rows {
        if let Choice::Preview(_, queue, _) = &row.choice {
            if let Some(previous) = previous_queue {
                assert!(Arc::ptr_eq(previous, queue));
            }
            previous_queue = Some(queue);
        }
        for option in &row.options {
            if let Choice::ArchiveTrack(album, _, _) = &option.choice {
                if let Some(previous) = previous_album {
                    assert!(Arc::ptr_eq(previous, album));
                }
                previous_album = Some(album);
                tracks += 1;
            }
        }
    }
    assert_eq!(tracks, 500);
    extras.shutdown();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_find_info_keeps_page_query_and_retry_in_place() {
    let (root, mut extras) = fixture("info-error");
    extras.push(
        "Find Info Online",
        vec![Item::new("Manual query", Choice::CombinedSearch)],
    );
    extras.pages.last_mut().unwrap().role = PageRole::Combined;
    let owner = extras.pages.last().unwrap().key;
    extras.combined = Some(CombinedState {
        query: "Manual query".into(),
        automatic_query: "Original album".into(),
        cover: false,
        cover_album: String::new(),
        cover_artist: String::new(),
        cover_paths: Arc::new(Vec::new()),
        cover_context: None,
        manual: true,
        generation: extras.generation,
        hits: Vec::new(),
        pending: 4,
        errors: Vec::new(),
        split_video: None,
    });
    let mut invalid = OnlineItem::new(Provider::Archive);
    invalid.id = "/invalid".into(); // Real worker failure without network access.
    for _ in 0..4 {
        extras.task(
            Task::Details(invalid.clone()),
            Purpose::Combined {
                generation: extras.generation,
            },
        );
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while extras.combined.as_ref().unwrap().pending > 0 {
        let effects = extras.tick(0.01);
        assert!(!effects.iter().any(|effect| matches!(effect, Effect::Exit)));
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(extras.pages.last().unwrap().key, owner);
    assert_eq!(extras.pages.last().unwrap().rows[0].title, "Manual query");
    assert!(matches!(
        extras.pages.last().unwrap().rows[1].choice,
        Choice::CombinedRetry
    ));
    assert!(extras.foreground_busy_row().is_none());
    extras.services.shutdown();
    extras.pages.last_mut().unwrap().selected = 1;
    extras.handle(Action::Select);
    assert_eq!(extras.combined.as_ref().unwrap().query, "Manual query");
    assert_eq!(extras.pages.last().unwrap().key, owner);
    extras.shutdown();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn video_search_tool_failure_offers_repair_and_back_keeps_the_query() {
    let (root, mut extras) = fixture("video-repair");
    extras.tools_state.ready = true;
    extras.open(ExtraPage::Videos);
    extras.handle(Action::Select);
    assert_eq!(extras.videos.watch_depth, Some(extras.pages.len()));
    extras.keyboard = None;
    extras.searches.clear();
    extras.searches.push(SearchState {
        generation: extras.generation,
        provider: Provider::YouTube,
        metadata: false,
        query: "My video".into(),
        items: Vec::new(),
        total: 0,
        page: 1,
        busy: false,
        error: "YouTube tools could not start: Permission denied".into(),
    });
    let owner = extras.pages.last().unwrap().key;
    extras.update_search_page(owner, Provider::YouTube, false, false);
    let page = extras.pages.last_mut().unwrap();
    assert!(page
        .rows
        .iter()
        .any(|row| row.title == "Couldn't search · Try Again"));
    assert!(
        page.rows[0].gear.is_none(),
        "Videos search stays video-only"
    );
    page.selected = page
        .rows
        .iter()
        .position(|row| row.title == "Update YouTube Tools")
        .unwrap();
    extras.handle(Action::Select);
    extras.handle(Action::Back);
    assert_eq!(extras.pages.last().unwrap().key, owner);
    assert_eq!(extras.pages.last().unwrap().rows[0].title, "My video");
    extras.shutdown();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires SDL/GLES; run the console follow-up captures"]
fn console_followup_input_and_captures() {
    use crate::app::{App, Intent, Row, Screen};
    let assets = PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").unwrap());
    let out = PathBuf::from(std::env::var_os("BETTERPOD_RUST_QA_OUTPUT").unwrap());
    let root = out.join(format!("private-followup-{}", std::process::id()));
    std::fs::create_dir_all(root.join("music")).unwrap();
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();
    let _audio = sdl.audio().unwrap();
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Console follow-up", 640, 480)
        .opengl()
        .build()
        .unwrap();
    let _gl = window.gl_create_context().unwrap();
    let mut r = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
    let mut app = App::new(root.join("music"), root.join("data"), assets, None, &r).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.scanning {
        app.tick(0.01, &r);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let capture = |app: &mut App, r: &mut Renderer, name: &str, now: f64| {
        app.draw(r, now);
        r.finish();
        r.screenshot(&out.join(format!("followup-{name}.png")))
            .unwrap();
        let light = r.capture_rgba();
        app.settings.theme = "dark".into();
        app.draw(r, now);
        r.finish();
        r.screenshot(&out.join(format!("followup-{name}-dark.png")))
            .unwrap();
        if name.starts_with("share-") && name != "share-back" && name != "share-name" {
            let dark = r.capture_rgba();
            assert!(dark.get_pixel(10, 400)[0] < 50, "Dark studio floor: {name}");
            assert!(
                light.get_pixel(10, 400)[0] > 180,
                "Light studio floor: {name}"
            );
        }
        app.settings.theme = "light".into();
        if name == "share-received" {
            let dark = r.capture_rgba();
            assert_eq!(light.get_pixel(320, 200), dark.get_pixel(320, 200));
            assert_eq!(dark.get_pixel(320, 200).0, [230, 90, 40, 255]);
        }
    };
    app.open(Screen::SearchHub);
    let lossless = app
        .pages
        .last()
        .unwrap()
        .rows
        .iter()
        .position(|row| row.label == "Lossless")
        .unwrap();
    app.pages.last_mut().unwrap().selected = lossless;
    app.visual_menu_snap_state(1.);
    capture(&mut app, &mut r, "search-lossless", 1.);
    app.handle(Action::Select);
    assert_eq!(app.screen(), Screen::Lossless);
    app.handle(Action::Back);
    assert_eq!(app.screen(), Screen::SearchHub);

    app.extras.set_sharing_name(String::new());
    app.extras.open(ExtraPage::Receive);
    app.open(Screen::Extra);
    app.visual_menu_snap_state(2.);
    app.draw(&mut r, 2.);
    r.finish();
    capture(&mut app, &mut r, "share-name", 2.5);
    app.extras.keyboard.as_mut().unwrap().0.query = "Fixture Pod".into();
    app.handle(Action::Start);
    capture(&mut app, &mut r, "share-receive", 3.5);
    assert_eq!(app.pages.last().unwrap().title, "Receive");
    assert!(!app.pages.last().unwrap().split());

    let info = ShareInfo {
        album: "A Very Long Album Title That Must Stay Inside Its Column".into(),
        artist: "A Very Long Artist Name and Collaborators".into(),
        songs: 12,
        ..Default::default()
    };
    app.extras.pages.clear();
    app.extras.push("Share", Vec::new());
    app.extras.share_view = Some(ShareVisual::send(info, None, 4.));
    app.extras
        .share_view
        .as_mut()
        .unwrap()
        .event(&share::Event::Code("12345678".into()), 4.);
    capture(&mut app, &mut r, "share-preparing", 4.);
    assert_eq!(app.pages.last().unwrap().title, "Share");
    app.extras.share_view.as_mut().unwrap().event(
        &share::Event::Progress {
            done: 0,
            total: 100,
            phase: "Waiting for receiver".into(),
            peer: None,
        },
        5.,
    );
    for (name, now) in [("share-waiting", 5.), ("share-float", 6.25)] {
        capture(&mut app, &mut r, name, now);
    }
    app.extras
        .share_view
        .as_mut()
        .unwrap()
        .event(&share::Event::Sent, 7.);
    for (name, now) in [
        ("share-return-start", 7.),
        ("share-return-middle", 7.3),
        ("share-sent", 7.7),
    ] {
        capture(&mut app, &mut r, name, now);
    }
    app.handle(Action::Back);
    app.tick(0.01, &r);
    assert_ne!(app.screen(), Screen::Extra);
    capture(&mut app, &mut r, "share-back", 8.5);

    app.open(Screen::Songs);
    app.pages.last_mut().unwrap().rows = Arc::new(
        (0..20)
            .map(|i| Row::new(format!("Song {i}"), Intent::Noop))
            .collect(),
    );
    app.visual_menu_snap_state(10.);
    capture(&mut app, &mut r, "scroll-expired", 12.);
    assert_eq!(app.visual_menu_remote_state()["scroll_visible"], false);
    Arc::make_mut(&mut app.pages.last_mut().unwrap().rows)
        .push(Row::new("Loaded song", Intent::Noop));
    capture(&mut app, &mut r, "scroll-loaded", 12.1);
    assert_eq!(app.visual_menu_remote_state()["scroll_visible"], true);
    capture(&mut app, &mut r, "scroll-faded", 14.);
    assert_eq!(app.visual_menu_remote_state()["scroll_visible"], false);
    app.handle(Action::Down);
    capture(&mut app, &mut r, "scroll-navigation", 14.1);
    assert_eq!(app.visual_menu_remote_state()["scroll_visible"], true);
    // Exercise themed error, transfer and completion states without networking.
    app.extras.pages.clear();
    app.extras.push("Receive", Vec::new());
    app.extras.share_view = Some(ShareVisual::receive(20.));
    app.open(Screen::Extra);
    app.visual_menu_snap_state(20.);
    app.extras
        .share_view
        .as_mut()
        .unwrap()
        .handle(Action::Up, 20.);
    app.extras
        .share_view
        .as_mut()
        .unwrap()
        .handle(Action::Select, 20.);
    capture(&mut app, &mut r, "share-connecting", 21.);
    app.extras.share_view.as_mut().unwrap().event(
        &share::Event::Error("Connection interrupted. Try again.".into()),
        22.,
    );
    capture(&mut app, &mut r, "share-receive-error", 22.);
    let cover = root.join("fixture-cover.png");
    image::RgbaImage::from_pixel(150, 150, image::Rgba([230, 90, 40, 255]))
        .save(&cover)
        .unwrap();
    app.art.get(Some(&cover));
    let deadline = Instant::now() + Duration::from_secs(3);
    while !app.art.poll(&r) {
        assert!(
            Instant::now() < deadline,
            "Fixture cover must finish decoding"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    app.extras.share_view.as_mut().unwrap().event(
        &share::Event::Preview {
            info: Some(ShareInfo {
                album: "Midnight Sessions".into(),
                artist: "The Night Quartet".into(),
                songs: 12,
                ..Default::default()
            }),
            cover: Some(cover.clone()),
        },
        23.,
    );
    for (name, phase, at) in [
        ("share-receiving", "Receiving", 23.),
        ("share-saving", "Saving", 24.),
    ] {
        app.extras.share_view.as_mut().unwrap().event(
            &share::Event::Progress {
                done: 50,
                total: 100,
                phase: phase.into(),
                peer: None,
            },
            at,
        );
        capture(&mut app, &mut r, name, at + 0.5);
    }
    app.extras.share_view.as_mut().unwrap().event(
        &share::Event::Received {
            paths: vec![root.join("music/song.flac")],
            info: None,
        },
        25.,
    );
    capture(&mut app, &mut r, "share-received", 27.);
    app.extras
        .share_view
        .as_mut()
        .unwrap()
        .handle(Action::Right, 27.);
    capture(&mut app, &mut r, "share-receive-another", 27.1);
    app.handle(Action::Select);
    capture(&mut app, &mut r, "share-reopened", 28.);
    assert_eq!(app.pages.last().unwrap().title, "Receive");
    app.shutdown(&r).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
