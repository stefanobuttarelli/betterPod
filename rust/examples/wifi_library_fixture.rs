//! Disposable integration fixture for the Mac client's actual Rust LAN server.
//! Pairing details are emitted once on stdout for the test subprocess only.
use betterpod_rust::{
    library::Library,
    wifi_library::{Config, Controller, Event},
};
use serde_json::json;
use std::{
    fs,
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<(), String> {
    let paths = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if paths.len() != 2 {
        return Err("Usage: wifi_library_fixture <temporary music> <temporary data>".into());
    }
    let music = fs::canonicalize(&paths[0]).map_err(|e| e.to_string())?;
    let data = fs::canonicalize(&paths[1]).map_err(|e| e.to_string())?;
    let root = music.parent().ok_or("Missing fixture root")?;
    let temporary = fs::canonicalize(std::env::temp_dir()).map_err(|e| e.to_string())?;
    if data.parent() != Some(root)
        || music == data
        || !root.starts_with(&temporary)
        || fs::read(root.join(".betterpod-wifi-test-fixture"))
            .ok()
            .as_deref()
            != Some(b"betterpod-wifi-library-test-v1")
    {
        return Err("The fixture only opens marked, disposable temporary directories".into());
    }
    let library = Arc::new(Library::load_or_scan(&music, &data, None)?);
    if library.tracks.is_empty() {
        return Err("The disposable fixture has no readable tracks".into());
    }
    let total = library.tracks.len();
    let mut controller = Controller::new(Config {
        music: music.clone(),
        data: data.clone(),
        name: "Library interop fixture".into(),
    });
    controller.start(library, Vec::new())?;

    let stopped = Arc::new(AtomicBool::new(false));
    let stop_reader = stopped.clone();
    thread::spawn(move || {
        let _ = io::stdin().lock().lines().next();
        stop_reader.store(true, Ordering::Release);
    });
    let until = Instant::now() + Duration::from_secs(120);
    let mut ready = false;
    while !stopped.load(Ordering::Acquire) && Instant::now() < until {
        while let Some(event) = controller.poll() {
            match event {
                Event::Ready { code, port, id, .. } => {
                    // The check runner reads this private pipe and never logs it.
                    println!(
                        "{}",
                        json!({"v":1,"host":"127.0.0.1","port":port,"id":id,"code":code,"total":total})
                    );
                    io::stdout().flush().map_err(|e| e.to_string())?;
                    ready = true;
                }
                Event::Changed { .. } => {
                    let library = Arc::new(Library::load_or_scan(&music, &data, None)?);
                    controller.update_snapshot(library, Vec::new());
                }
                Event::Error(error) => {
                    controller.shutdown();
                    return Err(error);
                }
                Event::Stopped => {
                    return if ready {
                        Ok(())
                    } else {
                        Err("Fixture stopped before becoming ready".into())
                    }
                }
                Event::Connected { .. } | Event::Disconnected { .. } => {}
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    controller.shutdown();
    if ready {
        Ok(())
    } else {
        Err("Fixture timed out before becoming ready".into())
    }
}
