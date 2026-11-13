//! Opt-in, loopback-only integration of the real cover worker. Environment
//! endpoint overrides require a dedicated test process.
use super::*;
use crate::online::{Event, Output, Services, Task};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let address = url.clone();
        let mut image = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(8, 8)
            .write_to(&mut image, image::ImageFormat::Png)
            .unwrap();
        let cover = image.into_inner();
        let worker = thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // On macOS accepted sockets inherit O_NONBLOCK from
                        // the listener; wait for the actual request bytes.
                        stream.set_nonblocking(false).unwrap();
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let mut data = [0_u8; 8192];
                        let length = stream.read(&mut data).unwrap_or(0);
                        let request = String::from_utf8_lossy(&data[..length])
                            .lines()
                            .next()
                            .unwrap_or("")
                            .to_string();
                        seen.lock().unwrap().push(request.clone());
                        let (body, kind) = if request.contains(" /ws/2/release") {
                            (br#"{"release-groups":[]}"#.to_vec(), "application/json")
                        } else if request.contains(" /search?") {
                            (serde_json::to_vec(&serde_json::json!({"results":[{"collectionId":1,"collectionName":"Rumours","artistName":"Fleetwood Mac","artworkUrl100":format!("{address}/cover-100x100.png")}]})).unwrap(),"application/json")
                        } else if request.contains(" /cover-1200x1200.png ") {
                            (cover.clone(), "image/png")
                        } else {
                            (b"{}".to_vec(), "application/json")
                        };
                        let header=format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                        let _ = stream.write_all(header.as_bytes());
                        let _ = stream.write_all(&body);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            stop,
            requests,
            worker: Some(worker),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Environment {
    fn local(url: &str) -> Self {
        let mut saved = Vec::new();
        for key in [
            "BETTERPOD_RUST_MB_BASE",
            "BETTERPOD_RUST_ITUNES_BASE",
            "BETTERPOD_RUST_DEEZER_BASE",
            "BETTERPOD_RUST_WIKI_BASE",
        ] {
            saved.push((key, std::env::var_os(key)));
            std::env::set_var(key, url);
        }
        Self(saved)
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value)
            } else {
                std::env::remove_var(key)
            }
        }
    }
}
#[test]
#[ignore = "loopback-only HTTP integration; run alone with --test-threads=1"]
fn actual_cover_search_uses_source_matcher_and_persists_only_private_artwork() {
    let server = Server::new();
    let _environment = Environment::local(&server.url);
    let root = std::env::temp_dir().join(format!("betterpod-cover-http-{}", std::process::id()));
    let music = root.join("music");
    let data = root.join("data");
    fs::create_dir_all(&music).unwrap();
    let song = music.join("unchanged.mp3");
    let original = b"synthetic original music bytes; never retag or replace";
    fs::write(&song, original).unwrap();
    let song = fs::canonicalize(song).unwrap();
    let mut services = Services::new_with_music(data.clone(), music).unwrap();
    let job = services
        .submit(Task::FindMissingCovers {
            albums: vec![CoverQuery {
                paths: vec![song.clone()],
                album: "Rumours [Deluxe Edition]".into(),
                artist: "Unknown Artist".into(),
                ..Default::default()
            }],
        })
        .unwrap();
    let limit = Instant::now() + Duration::from_secs(10);
    let mut report = None;
    let mut finished = false;
    while !finished {
        if let Some(event) = services.poll() {
            match event {
                Event::Progress { id, status, .. } => {
                    assert_eq!(id, job.id);
                    if let Some(state) = CoverSearchProgress::from_status(&status) {
                        report = Some(state);
                    }
                }
                Event::Finished { id, result } => {
                    assert_eq!(id, job.id);
                    match result.unwrap() {
                        Output::CurationApplied { matched, total } => {
                            assert_eq!(
                                (matched, total),
                                (1, 1),
                                "report={report:?}, requests={:?}",
                                server.requests.lock().unwrap()
                            )
                        }
                        _ => panic!("wrong cover-search output"),
                    };
                    finished = true;
                }
            }
        } else {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(Instant::now() < limit, "loopback cover search timed out");
    }
    services.shutdown();
    let state =
        report.expect("the cumulative source view report must be delivered before Finished");
    assert!(state.finished && !state.cancelled);
    assert_eq!((state.found, state.failed, state.errors), (1, 0, 0));
    let manifest = read(&data);
    let selected = manifest.tracks[&song].cover.as_ref().unwrap();
    assert!(selected.starts_with(fs::canonicalize(&data).unwrap()));
    assert!(selected.is_file());
    assert_eq!(fs::read(&song).unwrap(), original);
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].contains("Rumours"));
    assert!(!requests
        .iter()
        .any(|s| s.contains("Unknown") || s.contains("Deluxe")));
    assert!(requests[2].contains("/cover-1200x1200.png"));
    drop(requests);
    fs::remove_dir_all(root).unwrap();
}
