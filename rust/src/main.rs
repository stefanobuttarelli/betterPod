mod app;
mod remote_qa;
pub use betterpod_rust::files;
pub use betterpod_rust::{
    art, audio, bluetooth, games, input, library, pcm, platform, playlists, puzzles, render,
    storage, timing,
};
pub use betterpod_rust::{curation, saver};
pub use betterpod_rust::{downloads, metadata, online, podcasts};
pub use betterpod_rust::{maintenance, updater};
pub use betterpod_rust::{share, video, weather};

use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    if let Some(result) = betterpod_rust::frontend_audio::run_cli(std::env::args_os().skip(1)) {
        if let Err(error) = result {
            eprintln!("Frontend audio: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = run() {
        eprintln!("betterPod Rust: {error}");
        std::process::exit(1);
    }
}
fn env_path(name: &str, default: impl Into<PathBuf>) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| default.into())
}
fn run() -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let assets = env_path(
        "BETTERPOD_RUST_ASSETS_DIR",
        if cwd.join("assets").is_dir() {
            cwd.join("assets")
        } else {
            cwd.join("port/betterpod/assets")
        },
    );
    let default_data = env_path("BETTERPOD_RUST_DATA_DIR", cwd.join("build/rust/local-conf"));
    let bootstrap = env_path("BETTERPOD_RUST_BOOTSTRAP_DIR", default_data.clone());
    let default_music = env_path("BETTERPOD_RUST_MUSIC_DIR", cwd.join("music"));
    let (mut music, mut data) =
        maintenance::resolve_paths(&bootstrap, &default_music, &default_data);
    let import = std::env::var_os("BETTERPOD_RUST_IMPORT_CACHE").map(PathBuf::from);
    let exit_after = std::env::var("BETTERPOD_RUST_EXIT_AFTER_SECONDS")
        .ok()
        .and_then(|s| s.parse::<f64>().ok());
    let benchmark = std::env::var("BETTERPOD_RUST_BENCHMARK").is_ok_and(|s| s == "1");
    let screenshot = std::env::var_os("BETTERPOD_RUST_SCREENSHOT").map(PathBuf::from);
    let target = std::env::var("BETTERPOD_RUST_FPS")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(60.0)
        .clamp(15.0, 120.0);
    eprintln!(
        "betterPod Rust {} / independent experimental app",
        env!("CARGO_PKG_VERSION")
    );
    eprintln!("Music: {} / data: {}", music.display(), data.display());
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let audio_subsystem = sdl.audio()?;
    platform::install_signal_handlers();
    let attr = video.gl_attr();
    attr.set_double_buffer(true);
    attr.set_depth_size(0);
    attr.set_stencil_size(0);
    if cfg!(target_os = "macos") {
        attr.set_context_profile(sdl2::video::GLProfile::Core);
        attr.set_context_version(3, 2);
    } else {
        attr.set_context_profile(sdl2::video::GLProfile::GLES);
        attr.set_context_version(2, 0);
    }
    let mut builder = video.window("betterPod Rust", 640, 480);
    builder.opengl().position_centered();
    if cfg!(target_os = "linux")
        && std::env::var("BETTERPOD_RUST_FULLSCREEN").is_ok_and(|s| s == "1")
    {
        builder.fullscreen_desktop();
    }
    let requested_samples = std::env::var("BETTERPOD_RUST_MSAA")
        .ok()
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|n| matches!(n, 0 | 2 | 4))
        .unwrap_or(4);
    let create_context = |samples: u8| -> Result<_, String> {
        attr.set_multisample_buffers(u8::from(samples > 0));
        attr.set_multisample_samples(samples);
        let window = builder.build().map_err(|e| e.to_string())?;
        let context = window.gl_create_context()?;
        Ok((window, context))
    };
    let (window, context) = match create_context(requested_samples) {
        Ok(pair) => pair,
        Err(error) if requested_samples > 0 => {
            eprintln!("MSAA {requested_samples} unavailable ({error}); using ordinary framebuffer");
            create_context(0)?
        }
        Err(error) => return Err(error),
    };
    // SDL's macOS display-link can wait indefinitely when a test window is
    // occluded or the display sleeps. Keep local Mac pacing on our own clock.
    let request_vsync =
        std::env::var("BETTERPOD_RUST_VSYNC").map_or(!cfg!(target_os = "macos"), |s| s != "0");
    let interval = if request_vsync {
        sdl2::video::SwapInterval::VSync
    } else {
        sdl2::video::SwapInterval::Immediate
    };
    let vsync = request_vsync && video.gl_set_swap_interval(interval).is_ok();
    if !request_vsync {
        video.gl_set_swap_interval(interval)?;
    }
    let display_hz = window
        .display_index()
        .ok()
        .and_then(|index| video.current_display_mode(index).ok())
        .map_or(0, |mode| mode.refresh_rate);
    // At the display rate, swap already owns the deadline. A second timer
    // can miss the next vblank and lock motion to every other refresh.
    let swap_paces_frames = vsync && display_hz > 0 && target >= display_hz as f64 - 1.;
    let (width, height) = window.drawable_size();
    let mut renderer = render::Renderer::new(
        &video,
        &assets.join("ui_font.ttf"),
        width as i32,
        height as i32,
    )?;
    eprintln!(
        "Renderer: {} / swap interval accepted: {vsync} / nominal target: {target} / display: {display_hz} Hz / swap pacing: {swap_paces_frames} / MSAA: {} samples / artwork anisotropy: {}x",
        renderer.backend, renderer.samples, renderer.art_anisotropy
    );
    let mut input = input::Input::new(&sdl)?;
    let installation = env_path("BETTERPOD_RUST_INSTALLATION", cwd.parent().unwrap_or(&cwd));
    let stable = installation.join("betterpod");
    let mut protected = Vec::new();
    if stable.join("betterpod").is_file() {
        protected.push(stable);
    }
    if let Some(path) = import.as_ref().and_then(|p| p.parent()) {
        protected.push(path.to_path_buf());
    }
    if let Some(path) = std::env::var_os("BETTERPOD_RUST_PROTECTED_DATA") {
        protected.push(path.into());
    }
    maintenance::validate_private_data(&bootstrap, &data, &protected)?;
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let update_config = updater::Config {
        installation,
        current_version: env!("CARGO_PKG_VERSION").into(),
        // The published Rust channel (GitHub release "rust-latest", made by
        // .github/workflows/rust.yml) on the console build only (its
        // packages are AArch64 Linux); an empty variable turns it off.
        feed: std::env::var("BETTERPOD_RUST_UPDATE_FEED")
            .ok()
            .or_else(|| {
                cfg!(all(target_os = "linux", target_arch = "aarch64"))
                    .then(|| updater::DEFAULT_FEED.to_string())
            }),
        // the beta channel ("rust-beta"), read when Beta Versions is on
        beta_feed: std::env::var("BETTERPOD_RUST_BETA_UPDATE_FEED")
            .ok()
            .or_else(|| {
                cfg!(all(target_os = "linux", target_arch = "aarch64"))
                    .then(|| updater::DEFAULT_BETA_FEED.to_string())
            }),
        allow_beta: false,
    };
    if updater::recover_installation_if_needed(&update_config)? {
        eprintln!("Recovered an interrupted Rust update; reopen the app before playback");
        return Ok(());
    }
    let started = Instant::now();
    let mut previous = started;
    let mut last_present = started;
    let mut clock = timing::FrameClock::new(target);
    let mut metrics = if benchmark || std::env::var_os("BETTERPOD_RUST_METRICS").is_some() {
        Some(timing::Metrics::new())
    } else {
        None
    };
    let mut frame_count = 0u64;
    let mut black_shown = false;
    let mut pending_transfer: Option<(PathBuf, std::sync::Arc<library::Library>, bool)> = None;
    let mut remote_qa = remote_qa::RemoteQa::from_env()?;
    let trace_input = std::env::var_os("BETTERPOD_RUST_QA_TRACE_INPUT").is_some();
    loop {
        maintenance::validate_private_data(&bootstrap, &data, &protected)?;
        let recovered = maintenance::recover_transferred_clients(&maintenance::Config {
            bootstrap: bootstrap.clone(),
            music: music.clone(),
            data: data.clone(),
            protected: protected.clone(),
        })?;
        if recovered > 0 {
            eprintln!("Recovered saved library paths for {recovered} Rust transfer(s)");
        }
        let mut app = app::App::new(
            music.clone(),
            data.clone(),
            assets.clone(),
            import.clone(),
            &renderer,
        )?;
        let mut controller = app::maintenance_ui::Controller::new(
            maintenance::Config {
                bootstrap: bootstrap.clone(),
                music: music.clone(),
                data: data.clone(),
                protected: protected.clone(),
            },
            update_config.clone(),
        )?;
        if let Some((manifest, library, visible)) = pending_transfer.take() {
            controller.adopt_verified_transfer(manifest, library, visible)?;
        }
        app.install_maintenance(controller);
        while app.running && !platform::quit_requested() {
            let frame = Instant::now();
            let dt = frame.duration_since(previous).as_secs_f32().min(0.1);
            previous = frame;
            if let Some(qa) = remote_qa.as_mut() {
                qa.before_frame(&mut app);
            }
            for action in input.poll() {
                if trace_input {
                    eprintln!("QA input {action:?}");
                }
                app.handle(action);
                if !app.running {
                    break;
                }
            }
            let elapsed = started.elapsed().as_secs_f64();
            if benchmark {
                app.benchmark(elapsed);
            }
            app.tick(dt, &renderer);
            if let Some(limit) = exit_after {
                if elapsed >= limit {
                    app.user_quit = true;
                    app.running = false;
                }
            }
            if app.screen_off {
                if !black_shown {
                    renderer.begin(render::BLACK);
                    renderer.finish();
                    window.gl_swap_window();
                    black_shown = true;
                }
                if let Some(qa) = remote_qa.as_mut() {
                    renderer.begin(render::BLACK);
                    renderer.finish();
                    qa.after_frame(&mut app, &renderer);
                }
                std::thread::sleep(Duration::from_millis(100));
                clock.reset();
                continue;
            }
            black_shown = false;
            app.draw(&mut renderer, elapsed);
            renderer.finish();
            if let Some(qa) = remote_qa.as_mut() {
                qa.after_frame(&mut app, &renderer);
            }
            if !app.running {
                if let Some(path) = &screenshot {
                    renderer.screenshot(path)?;
                }
            }
            let present = Instant::now();
            window.gl_swap_window();
            let completed = Instant::now();
            frame_count += 1;
            if let Some(metrics) = &mut metrics {
                let snap = &app.audio_snapshot;
                metrics.push(timing::FrameSample {
                    phase: app.phase,
                    elapsed,
                    work_ms: completed.duration_since(frame).as_secs_f64() * 1000.0,
                    present_ms: completed.duration_since(present).as_secs_f64() * 1000.0,
                    gap_ms: completed.duration_since(last_present).as_secs_f64() * 1000.0,
                    playing: snap.playing,
                    position: snap.position,
                    audio: app.audio_telemetry(),
                    video: app.video_telemetry(),
                });
            }
            last_present = completed;
            if !swap_paces_frames {
                clock.wait(completed);
            }
        }
        let maintenance_dark = app.dark();
        let maintenance_library = app.maintenance_library();
        // Preserve opt-in measurements even if a peripheral stalls in shutdown.
        // This runs at an exit/maintenance barrier, never during frame drawing.
        if let Some(metrics) = &metrics {
            let path = env_path("BETTERPOD_RUST_METRICS", data.join("frames.csv"));
            if let Err(error) = metrics.save(&path) {
                eprintln!("Metrics snapshot before shutdown: {error}");
            }
        }
        let shutdown = app.shutdown_animated(&mut renderer, &window);
        let user_quit = app.user_quit;
        let request = app.maintenance_request.take();
        // Drop joins every remaining client and Store even if a peripheral
        // cleanup failed; metadata finalization always follows this barrier.
        drop(app);
        let Some(request) = request else {
            shutdown?;
            break;
        };
        if let app::maintenance_ui::Request::FinalizeTransfer {
            manifest, visible, ..
        } = request
        {
            (music, data) = maintenance::resolve_paths(&bootstrap, &default_music, &default_data);
            maintenance::validate_private_data(&bootstrap, &data, &protected)?;
            app::maintenance_ui::prepare_verified_transfer_clients(
                maintenance::Config {
                    bootstrap: bootstrap.clone(),
                    music: music.clone(),
                    data: data.clone(),
                    protected: protected.clone(),
                },
                manifest.clone(),
            )?;
            shutdown?;
            if user_quit || platform::quit_requested() {
                break;
            }
            pending_transfer = Some((manifest, maintenance_library, visible));
            clock.reset();
            previous = Instant::now();
            last_present = previous;
            black_shown = false;
            continue;
        }
        shutdown?;
        if user_quit || platform::quit_requested() {
            break;
        }
        match app::maintenance_ui::run(
            request,
            &mut renderer,
            &mut input,
            &window,
            &assets,
            maintenance_dark,
            maintenance_library,
        ) {
            Ok(true) => {
                eprintln!("Update complete; reopen betterPod Rust to run the new binary");
                break;
            }
            Ok(false) => {}
            Err(error) => eprintln!("Maintenance: {error}"),
        }
        (music, data) = maintenance::resolve_paths(&bootstrap, &default_music, &default_data);
        clock.reset();
        previous = Instant::now();
        last_present = previous;
        black_shown = false;
    }
    if let Some(metrics) = &metrics {
        let path = env_path("BETTERPOD_RUST_METRICS", data.join("frames.csv"));
        metrics.save(&path)?;
        eprintln!(
            "Metrics: {} samples written to {}",
            metrics.samples.len(),
            path.display()
        );
    }
    eprintln!(
        "Clean exit: {frame_count} frames, {:.3}s",
        started.elapsed().as_secs_f64()
    );
    drop(renderer);
    drop(context);
    drop(window);
    drop(audio_subsystem);
    drop(sdl);
    Ok(())
}
