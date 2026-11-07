use super::*;
use std::{net::TcpListener, time::Instant};
struct Fixture {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let Ok((mut socket, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                while !bytes.windows(4).any(|b| b == b"\r\n\r\n") && bytes.len() < 16384 {
                    let n = socket.read(&mut buffer).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..n]);
                }
                let request = String::from_utf8_lossy(&bytes);
                if request.is_empty() {
                    continue;
                }
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                seen.lock().unwrap().push(path.clone());
                if path == "/fail" {
                    let _ = socket.write_all(
                        b"HTTP/1.1 503 Unavailable\r\nContent-Length:0\r\nConnection:close\r\n\r\n",
                    );
                    continue;
                }
                if path == "/slow" {
                    let _ = socket.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length:2097152\r\nConnection:close\r\n\r\n",
                    );
                    for _ in 0..2048 {
                        if socket.write_all(&[0x55; 1024]).is_err() {
                            break;
                        }
                        thread::sleep(Duration::from_millis(2));
                    }
                    continue;
                }
                let body = path.as_bytes().repeat(8192);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length:{}\r\nConnection:close\r\n\r\n",
                    body.len()
                );
                if socket.write_all(header.as_bytes()).is_ok() {
                    let _ = socket.write_all(&body);
                }
            }
        });
        let root = std::env::temp_dir().join(format!(
            "betterpod-listen-qa-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        Self {
            url,
            requests,
            stop,
            worker: Some(worker),
            root,
        }
    }
    fn item(&self, path: &str) -> OnlineItem {
        let mut item = OnlineItem::new(online::Provider::Archive);
        item.url = format!("{}{path}", self.url);
        item.title = path.into();
        item.seconds = 42.;
        item
    }
    fn wait(&self, mut predicate: impl FnMut() -> bool) {
        let until = Instant::now() + Duration::from_secs(8);
        while !predicate() {
            assert!(Instant::now() < until, "Listen fixture timed out");
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn real_whole_file_prefetch_is_only_current_and_next_then_reuses_next() {
    let fixture = Fixture::new();
    let mut client = ListenClient::new(fixture.root.clone()).unwrap();
    client
        .start(
            vec![
                fixture.item("/zero"),
                fixture.item("/one"),
                fixture.item("/two"),
            ],
            0,
        )
        .unwrap();
    let mut first = None;
    fixture.wait(|| {
        assert_ne!(
            client.snapshot().phase,
            Phase::Failed,
            "{:?}",
            client.snapshot()
        );
        first = first.take().or_else(|| client.take_ready());
        first.is_some()
    });
    let first = first.unwrap();
    assert_eq!(first.index, 0);
    assert_eq!(
        fs::read(&first.source.video_url).unwrap(),
        b"/zero".repeat(8192)
    );
    fixture.wait(|| fixture.requests.lock().unwrap().len() >= 2);
    thread::sleep(Duration::from_millis(70));
    assert_eq!(*fixture.requests.lock().unwrap(), vec!["/zero", "/one"]);
    client.advance().unwrap();
    let mut next = None;
    fixture.wait(|| {
        next = next.take().or_else(|| client.take_ready());
        next.is_some()
    });
    let next = next.unwrap();
    assert_eq!(next.index, 1);
    assert_eq!(
        fs::read(&next.source.video_url).unwrap(),
        b"/one".repeat(8192)
    );
    fixture.wait(|| fixture.requests.lock().unwrap().len() == 3);
    assert_eq!(
        *fixture.requests.lock().unwrap(),
        vec!["/zero", "/one", "/two"]
    );
    assert!(
        !Path::new(&first.source.video_url).exists(),
        "played cache file is removed"
    );
    client.shutdown();
    assert!(
        !Path::new(&next.source.video_url).exists(),
        "joined shutdown removes private cache"
    );
}
#[test]
fn failed_preview_is_retained_and_retry_performs_another_request() {
    let fixture = Fixture::new();
    let mut client = ListenClient::new(fixture.root.clone()).unwrap();
    let queue = vec![fixture.item("/fail")];
    client.start(queue.clone(), 0).unwrap();
    fixture.wait(|| client.snapshot().phase == Phase::Failed);
    let first = client.snapshot();
    assert!(first.error.is_some());
    assert!(client.take_ready().is_none());
    thread::sleep(Duration::from_millis(50));
    assert_eq!(client.snapshot().phase, Phase::Failed);
    client.retry(queue, 0).unwrap();
    fixture.wait(|| fixture.requests.lock().unwrap().len() >= 2);
    fixture.wait(|| client.snapshot().phase == Phase::Failed);
    assert_eq!(fixture.requests.lock().unwrap().len(), 2);
    client.stop();
    client.shutdown();
    assert_eq!(client.snapshot().phase, Phase::Idle);
}
#[test]
fn last_item_end_does_not_restart_or_fetch_more_and_shutdown_is_idempotent() {
    let fixture = Fixture::new();
    let mut client = ListenClient::new(fixture.root.clone()).unwrap();
    client.start(vec![fixture.item("/only")], 0).unwrap();
    fixture.wait(|| {
        assert_ne!(
            client.snapshot().phase,
            Phase::Failed,
            "{:?}",
            client.snapshot()
        );
        client.take_ready().is_some()
    });
    client.advance().unwrap();
    fixture.wait(|| client.snapshot().phase == Phase::Ended);
    thread::sleep(Duration::from_millis(70));
    assert_eq!(client.snapshot().phase, Phase::Ended);
    assert_eq!(fixture.requests.lock().unwrap().len(), 1);
    client.shutdown();
    client.shutdown();
}
#[test]
fn in_flight_download_cancels_and_joins_before_private_cache_cleanup() {
    let fixture = Fixture::new();
    let mut client = ListenClient::new(fixture.root.clone()).unwrap();
    client.start(vec![fixture.item("/slow")], 0).unwrap();
    fixture.wait(|| client.snapshot().done > 0);
    let started = Instant::now();
    client.shutdown();
    assert!(started.elapsed() < Duration::from_secs(2));
    let tmp = fixture.root.join("tmp");
    assert_eq!(
        fs::read_dir(tmp).unwrap().count(),
        0,
        "joined worker removes its session after the request exits"
    );
}

/// Actual AAC packets, actual HTTP byte ranges and actual copy-only remuxes.
/// This opt-in check runs inside the offline Linux fixture image with FFmpeg.
#[test]
#[ignore = "Requires the local FFmpeg fixture executable"]
fn native_sidx_parts_preserve_every_compressed_aac_packet_exactly_once() {
    let root = std::env::temp_dir().join(format!(
        "betterpod-listen-aac-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("source.m4a");
    let ffmpeg = downloads::ffmpeg_path(&root);
    let status = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=442:sample_rate=48000",
            "-t",
            "45",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof+global_sidx",
            "-frag_duration",
            "5000000",
        ])
        .arg(&input)
        .status()
        .unwrap();
    assert!(status.success());
    let original = fs::read(&input).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/source.m4a", listener.local_addr().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let worker = thread::spawn(move || {
        while !stopped.load(Ordering::Acquire) {
            let Ok((mut socket, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(2));
                continue;
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
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
            let range = request
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("range"))
                        .map(|(_, v)| v.trim())
                })
                .unwrap();
            let (start, end) = range
                .strip_prefix("bytes=")
                .unwrap()
                .split_once('-')
                .unwrap();
            let start = start.parse::<usize>().unwrap();
            let end = end.parse::<usize>().unwrap().min(original.len() - 1);
            assert!(start <= end);
            let body = &original[start..=end];
            let header=format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length:{}\r\nConnection:close\r\n\r\n",original.len(),body.len());
            if socket.write_all(header.as_bytes()).is_ok() {
                let _ = socket.write_all(body);
            }
        }
    });
    let context = Context::standalone();
    let source = StreamSource {
        video_url: url,
        audio_url: None,
        headers: Vec::new(),
        duration: 45.,
        title: "AAC fixture".into(),
        available_at: None,
        chapters: 0,
    };
    let plan = mp4::open(&context, &source)
        .unwrap()
        .expect("FFmpeg fixture must contain global SIDX");
    assert_eq!(plan.parts.len(), 2);
    assert!(plan.parts[0].duration >= 20. && plan.parts[0].duration < 25.1);
    fn packets(ffmpeg: &Path, path: &Path) -> Vec<u8> {
        let output = Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
            .arg(path)
            .args(["-map", "0:a:0", "-c:a", "copy", "-f", "data", "pipe:1"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
    let expected = packets(&ffmpeg, &input);
    let mut actual = Vec::new();
    for index in 0..plan.parts.len() {
        let path = root.join(format!("part-{index}.mka"));
        mp4::fetch(&context, &root, &source, &plan, index, &path, |_, _| {}).unwrap();
        actual.extend(packets(&ffmpeg, &path));
    }
    stop.store(true, Ordering::Release);
    worker.join().unwrap();
    assert!(!expected.is_empty());
    assert_eq!(
        actual, expected,
        "part grouping must neither transcode, duplicate nor omit any AAC packet"
    );
    fs::remove_dir_all(root).unwrap();
}
