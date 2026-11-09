//! Opt-in local visual fixtures. Included only under cfg(test); no runtime API.
use super::*;
#[path = "app_visual_games_qa.rs"]
mod games_qa;
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
struct FixtureTrack {
    file: String,
    cover: String,
    title: String,
    artist: String,
    album: String,
    number: u32,
    duration: f64,
}
#[derive(Deserialize)]
struct Scene {
    id: String,
    screen: String,
    selected: String,
    time_ms: u64,
    #[serde(default)]
    motion: Option<String>,
}
#[derive(Deserialize)]
struct Manifest {
    tracks: Vec<FixtureTrack>,
    scenes: Vec<Scene>,
    position: f64,
    volume: f32,
    battery: u8,
    wifi: i8,
    clock: String,
}
#[derive(Serialize)]
struct CaptureState<'a> {
    id: &'a str,
    screen: &'a str,
    selected: String,
    scroll: f32,
    rows: Vec<String>,
    time_ms: u64,
    track: String,
    position: f64,
    duration: f64,
    playing: bool,
    lossless: bool,
    mini_wanted: bool,
    mini_shown: f64,
    control_mode: String,
    clock: String,
    battery: u8,
    wifi: i8,
    theme: &'a str,
    effects: &'a str,
    tracks: usize,
    albums: usize,
}
fn is_game_scene(screen: &str) -> bool {
    matches!(
        screen,
        "game2048" | "gamebrick" | "gamesolitaire" | "gamewords"
    )
}
fn selected(page: &mut Page, label: &str) -> Result<(), String> {
    if !label.is_empty() {
        page.selected = page
            .rows
            .iter()
            .position(|r| r.label == label)
            .ok_or_else(|| format!("Rust scene missing row {label:?} on {:?}", page.screen))?;
    }
    page.scroll = 0.0;
    page.target_scroll = 0.0;
    page.reveal_selection();
    page.enter = 0.0;
    Ok(())
}
fn preload(app: &mut App, r: &Renderer) -> Result<(), String> {
    let covers = app
        .library
        .albums
        .iter()
        .filter_map(|a| a.cover.clone())
        .collect::<Vec<_>>();
    for path in &covers {
        app.art.get(Some(path));
    }
    let until = Instant::now() + Duration::from_secs(15);
    while covers.iter().any(|p| app.art.palette(Some(p)).is_none()) {
        app.art.poll(r);
        if Instant::now() > until {
            return Err(
                "Visual fixture artwork was not ready; no placeholder capture accepted".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}
#[test]
#[ignore = "requires local fixed-state fixtures and GLES2; run rust/tools/visual-parity.sh"]
fn capture_static_scenes() -> Result<(), String> {
    let manifest = PathBuf::from(
        std::env::var_os("BETTERPOD_VISUAL_MANIFEST").ok_or("Missing visual manifest")?,
    );
    let fixtures =
        PathBuf::from(std::env::var_os("BETTERPOD_VISUAL_FIXTURES").ok_or("Missing fixtures")?);
    let out = PathBuf::from(std::env::var_os("BETTERPOD_VISUAL_OUTPUT").ok_or("Missing output")?);
    let assets =
        PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").ok_or("Missing assets")?);
    let model: Manifest =
        serde_json::from_slice(&std::fs::read(manifest).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let _audio = sdl.audio()?;
    video
        .gl_attr()
        .set_context_profile(sdl2::video::GLProfile::GLES);
    video.gl_attr().set_context_version(2, 0);
    let window = video
        .window("Rust fixed visual fixtures", 640, 480)
        .opengl()
        .build()
        .map_err(|e| e.to_string())?;
    let _context = window.gl_create_context()?;
    let mut renderer = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480)?;
    let mut app = App::new(
        fixtures.join("music"),
        out.join("private-data"),
        assets,
        None,
        &renderer,
    )?;
    app.install_maintenance(MaintenanceController::new(
        crate::maintenance::Config {
            bootstrap: app.data_dir.clone(),
            music: fixtures.join("music"),
            data: app.data_dir.clone(),
            protected: Vec::new(),
        },
        crate::updater::Config {
            installation: out.join("fixture-ports"),
            current_version: "1.1".into(),
            feed: None,
            beta_feed: None,
            allow_beta: false,
        },
    )?);
    let deadline = Instant::now() + Duration::from_secs(15);
    while app.scanning {
        app.tick(0.0, &renderer);
        if Instant::now() > deadline {
            return Err("Fixture scan timeout".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let tracks = model
        .tracks
        .iter()
        .map(|t| Track {
            path: fixtures.join("music").join(&t.file),
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            album_artist: t.artist.clone(),
            track: t.number,
            duration: t.duration,
            sample_rate: 44100,
            bit_depth: 16,
            lossless: true,
            cover: Some(fixtures.join("music").join(&t.cover)),
            ..Track::default()
        })
        .collect();
    app.install_library(ScannedLibrary::new(Library::from_tracks(tracks)), &renderer);
    preload(&mut app, &renderer)?;
    app.settings.theme = "light".into();
    app.settings.effects = "full".into();
    app.settings.shuffle = false;
    app.settings.repeat = "off".into();
    app.settings.youtube = false;
    app.settings.hide_podcasts = false;
    app.settings.volume = model.volume;
    app.settings.screensaver_seconds = 0;
    app.settings.screensaver_dim = 0;
    app.settings.sharing_name.clear();
    app.settings.screensaver_style = "cover".into();
    app.queue = Arc::new(app.library.tracks.iter().map(|t| t.path.clone()).collect());
    app.queue_index = 0;
    app.now_track = app.library.tracks.first().cloned();
    app.idle_stopped = false;
    app.restored = false;
    eprintln!(
        "TEXT_WIDTH automatic={}",
        renderer.text_width_weighted("Dark from 20:00 to 7:00", 24., 400)
    );
    for scene in &model.scenes {
        app.visual_time = if scene.motion.is_some() {
            1.0
        } else {
            scene.time_ms as f64 / 1000.0
        };
        app.clock = model.clock.clone();
        app.audio_snapshot = AudioSnapshot {
            path: app.queue.first().cloned(),
            playing: true,
            paused: false,
            loading: false,
            position: model.position,
            duration: model.tracks[0].duration,
            volume: model.volume,
            rate: 44100,
            ..AudioSnapshot::default()
        };
        app.feedback = Feedback::new();
        app.keyboard = None;
        app.visual_menu = MenuVisual::new();
        app.player_visual = PlayerVisual::new();
        app.visual_player_set_phase_epoch(296.0);
        app.visual_menu
            .seed_preview_order((0..app.library.albums.len()).collect(), 0.0);
        app.visual_menu.status = app_visual_menu::HeaderStatus {
            battery: Some(model.battery),
            charging: false,
            external_power: false,
            bluetooth: false,
            headphones: false,
            wifi: model.wifi,
        };
        app.pages.clear();
        app.pages.push(app.page(Screen::Home));
        app.options = None;
        app.toast = None;
        app.cover_selected = 0;
        app.cover_position = 0.0;
        app.cover_card = None;
        app.cover_flip = 0.0;
        app.cover_from_np = false;
        app.np_mode = 0;
        app.np_option = 0;
        app.np_seek = None;
        app.auto_saver = false;
        app.screen_off = false;
        app.preview_focus = false;
        if let Some(route) = scene.motion.as_deref() {
            selected(app.pages.last_mut().unwrap(), "Music")?;
            if route != "home-music" {
                app.pages.push(app.page(Screen::Music));
                selected(app.pages.last_mut().unwrap(), "Albums")?;
            }
            app.visual_menu_snap_state(1.0);
            app.visual_player_snap_state(1.0);
            app.draw(&mut renderer, 1.0);
            renderer.finish();
            app.handle(if route == "music-home" {
                Action::Back
            } else {
                Action::Select
            });
            app.visual_time = scene.time_ms as f64 / 1000.0;
        } else {
            match scene.screen.as_str() {
                "home" => {}
                "music" => app.pages.push(app.page(Screen::Music)),
                "albums" | "sheet" => {
                    app.pages.push(app.page(Screen::Music));
                    app.pages.push(app.page(Screen::Albums));
                }
                "song-options" => {
                    app.pages.push(app.page(Screen::Songs));
                    selected(app.pages.last_mut().unwrap(), &model.tracks[0].title)?;
                    app.show_options();
                }
                "album-options" => {
                    app.pages.push(app.page(Screen::Albums));
                    selected(app.pages.last_mut().unwrap(), &model.tracks[0].album)?;
                    app.show_album_options(0);
                }
                other if other.starts_with("videos-") => {
                    let page = app.extras.visual_videos_fixture(other)?;
                    app.pages.push(page);
                }
                other if other.starts_with("youtube-") => {
                    let page = app.extras.visual_youtube_fixture(other)?;
                    app.pages.push(page);
                }
                other if other.starts_with("video-") => {
                    app.visual_video_fixture(other, &renderer)?;
                }
                "settings" => app.pages.push(app.page(Screen::Settings)),
                "artists" => app.pages.push(app.page(Screen::Artists)),
                "artist" => app.pages.push(app.page(Screen::Artist(0))),
                "album" => app.pages.push(app.page(Screen::Album(0))),
                "songs" => app.pages.push(app.page(Screen::Songs)),
                "upnext" => app.pages.push(app.page(Screen::Queue)),
                "screensaver" => app.pages.push(app.page(Screen::Screensaver)),
                "games" => app.pages.push(app.page(Screen::Apps)),
                "searchhub" => app.pages.push(app.page(Screen::SearchHub)),
                "search" => {
                    app.search_query.clear();
                    app.search_rows = Arc::new(vec![app_search::search_field("")]);
                    app.pages.push(app.page(Screen::Search));
                    let mut keyboard = Keyboard::new("");
                    keyboard.snap_visible(app.visual_time);
                    app.keyboard = Some(keyboard);
                    app.keyboard_context = KeyboardContext::Library;
                    app.ensure_selection_viewport();
                }
                "nowplaying" => app.pages.push(app.page(Screen::NowPlaying)),
                "nowplaying-card" => {
                    app.pages.push(app.page(Screen::NowPlaying));
                    app.pages.push(app.page(Screen::CoverFlow));
                    app.cover_from_np = true;
                    app.cover_card = Some(0);
                }
                "coverflow-card" => {
                    app.pages.push(app.page(Screen::CoverFlow));
                    app.cover_card = Some(0);
                }
                "coverflow" => app.pages.push(app.page(Screen::CoverFlow)),
                other if other.starts_with("curation-covers-") => {
                    app.curation = Curation::visual_fixture(other, Arc::clone(&app.library))?;
                    let page = app
                        .curation
                        .visual_page()
                        .ok_or("Missing curation fixture page")?;
                    app.pages.push(page);
                }
                other if other.starts_with("maintenance-") => {
                    app.maintenance = Some(MaintenanceController::visual_fixture(
                        other,
                        fixtures.join("music"),
                        app.data_dir.clone(),
                        Arc::clone(&app.library),
                    )?);
                    app.pages.push(app.page(Screen::Maintenance));
                }
                other => {
                    if !games_qa::seed(&mut app, other, &fixtures)? {
                        return Err(format!("Unsupported visual scene {other}"));
                    }
                }
            }
        }
        if scene.screen == "sheet" {
            let mut next = Row::new("Play Next", Intent::Noop);
            next.visual.icon = "queue_next".into();
            let mut later = Row::new("Play Later", Intent::Noop);
            later.visual.icon = "queue_last".into();
            app.options = Some(Page {
                screen: Screen::Albums,
                kind: ViewKind::Full,
                mini: false,
                title: model.tracks[0].title.clone(),
                rows: Arc::new(vec![next, later, Row::new("Cancel", Intent::CloseOptions)]),
                selected: 0,
                scroll: 0.0,
                target_scroll: 0.0,
                enter: 0.0,
            });
        } else if scene.motion.is_none()
            && !is_game_scene(&scene.screen)
            && !matches!(
                scene.screen.as_str(),
                "coverflow"
                    | "coverflow-card"
                    | "nowplaying-card"
                    | "song-options"
                    | "album-options"
            )
        {
            selected(app.pages.last_mut().unwrap(), &scene.selected)?;
        }
        if scene.screen == "settings" {
            app.refresh_setting_preview();
        }
        if matches!(
            scene.screen.as_str(),
            "sheet" | "song-options" | "album-options"
        ) {
            let now = app.visual_time;
            app.visual_time = 0.;
            app.draw(&mut renderer, 0.);
            renderer.finish();
            app.visual_time = now;
        }
        if scene.motion.is_none() {
            app.visual_menu_snap_state(app.visual_time);
            app.visual_player_snap_state(app.visual_time);
        }
        // Both draws use the same explicit endpoint. No tick polls clocks/audio here.
        app.draw(&mut renderer, app.visual_time);
        renderer.finish();
        app.draw(&mut renderer, app.visual_time);
        renderer.finish();
        renderer.screenshot(&out.join(format!("{}.png", scene.id)))?;
        let selection = if matches!(scene.screen.as_str(), "coverflow-card" | "nowplaying-card") {
            app.library.tracks[0].title.clone()
        } else if scene.screen == "coverflow" {
            app.library.albums[app.cover_selected].name.clone()
        } else if matches!(
            scene.screen.as_str(),
            "sheet" | "song-options" | "album-options"
        ) {
            app.options
                .as_ref()
                .and_then(|p| p.rows.get(p.selected))
                .map(|r| r.label.clone())
                .unwrap_or_default()
        } else if scene.screen == "nowplaying"
            || is_game_scene(&scene.screen)
            || scene.screen.starts_with("curation-covers-")
            || scene.screen.starts_with("video-")
        {
            String::new()
        } else {
            app.pages
                .last()
                .and_then(|p| p.rows.get(p.selected))
                .map(|r| r.label.clone())
                .unwrap_or_default()
        };
        let state = CaptureState {
            id: &scene.id,
            screen: &scene.screen,
            selected: selection,
            scroll: app.pages.last().map(|page| page.scroll / 2.).unwrap_or(0.),
            rows: if is_game_scene(&scene.screen)
                || scene.screen.starts_with("curation-covers-")
                || scene.screen.starts_with("video-")
                || matches!(
                    scene.screen.as_str(),
                    "coverflow" | "nowplaying" | "coverflow-card" | "nowplaying-card"
                ) {
                Vec::new()
            } else if matches!(
                scene.screen.as_str(),
                "sheet" | "song-options" | "album-options"
            ) {
                app.options
                    .as_ref()
                    .map(|p| p.rows.iter().map(|r| r.label.clone()).collect())
                    .unwrap_or_default()
            } else {
                app.pages
                    .last()
                    .map(|p| p.rows.iter().map(|r| r.label.clone()).collect())
                    .unwrap_or_default()
            },
            time_ms: scene.time_ms,
            track: app.now_track.as_ref().unwrap().title.clone(),
            position: app.audio_snapshot.position,
            duration: app.audio_snapshot.duration,
            playing: app.audio_snapshot.playing,
            lossless: app.now_track.as_ref().is_some_and(|track| track.lossless),
            mini_wanted: app.visual_menu_qa_mini_state().0,
            mini_shown: app.visual_menu_qa_mini_state().1,
            control_mode:
                if matches!(scene.screen.as_str(), "coverflow-card" | "nowplaying-card") {
                    "card".into()
                } else if app.screen() == Screen::NowPlaying {
                    match app.np_mode {
                        1 => "scrub",
                        2 => "options",
                        _ => "progress",
                    }
                } else {
                    "none"
                }
                .into(),
            clock: app.clock.clone(),
            battery: app.visual_menu.status.battery.unwrap_or_default(),
            wifi: app.visual_menu.status.wifi,
            theme: "light",
            effects: "full",
            tracks: app.library.tracks.len(),
            albums: app.library.albums.len(),
        };
        std::fs::write(
            out.join(format!("{}.state.json", scene.id)),
            serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        eprintln!("CAPTURE {}", scene.id);
    }
    app.shutdown(&renderer)?;
    Ok(())
}
