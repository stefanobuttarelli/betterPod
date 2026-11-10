use super::*;
use lofty::file::TaggedFileExt;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    sync::atomic::AtomicU64,
};

struct Fixture {
    root: PathBuf,
    audio: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bp-auto-cover-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("tools")).unwrap();
        let tool = root.join("tools/yt-dlp");
        fs::write(
            &tool,
            r#"#!/bin/sh
base=${0%/*}
metadata=no
output=
while [ "$#" -gt 0 ]; do
 case "$1" in -J) metadata=yes ;; -o) shift; output=$1 ;; esac
 shift
done
if [ "$metadata" = yes ]; then
 /bin/cat "$base/info.json"
else
 : > "$base/media-started"
 /bin/cp "$base/media.m4a" "${output%'%(ext)s'}m4a"
fi
"#,
        )
        .unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.join("tools/info.json"),serde_json::to_vec(&json!({"id":"first000001","title":"Fixture Song","artist":"Fixture Artist","album":"Fixture Album","webpage_url":"https://www.youtube.com/watch?v=first000001","formats":[{"format_id":"140","url":"https://example.invalid/never-fetched.m4a"}]})).unwrap()).unwrap();
        fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut bytes = ((body.len() + 8) as u32).to_be_bytes().to_vec();
            bytes.extend(kind);
            bytes.extend(body);
            bytes
        }
        let audio = b"Unchanged encoded audio payload in a local M4A fixture".to_vec();
        let mut bytes = boxed(b"ftyp", b"M4A \0\0\0\0isom");
        bytes.extend(boxed(b"moov", &[]));
        bytes.extend(boxed(b"mdat", &audio));
        fs::write(root.join("tools/media.m4a"), bytes).unwrap();
        Self { root, audio }
    }
    fn task(&self, url: String) -> Task {
        Task::DownloadWithAutomaticCover {
            request: DownloadRequest::YouTube {
                url: "https://www.youtube.com/watch?v=first000001".into(),
                video: false,
                split_chapters: false,
            },
            url,
        }
    }
    fn assert_cover(&self, output: Output) {
        let Output::Files(files) = output else {
            panic!("Automatic cover task returned no files")
        };
        assert_eq!(files.len(), 1);
        let bytes = fs::read(&files[0]).unwrap();
        assert!(bytes
            .windows(self.audio.len())
            .any(|part| part == self.audio));
        let read = lofty::probe::Probe::open(&files[0])
            .unwrap()
            .options(lofty::config::ParseOptions::new().read_properties(false))
            .read()
            .unwrap();
        let cover = &read.primary_tag().unwrap().pictures()[0];
        let image = image::load_from_memory(cover.data()).unwrap().to_rgb8();
        assert_eq!(
            image.width(),
            image.height(),
            "Automatic artwork must be square"
        );
        let pixel = image.get_pixel(0, 0);
        assert!(
            pixel[1] > 180 && pixel[0] < 70 && pixel[2] < 70,
            "Center crop must keep the green middle, not a red/blue edge"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn picture() -> Vec<u8> {
    let image = image::RgbImage::from_fn(160, 80, |x, _| {
        if x < 40 {
            image::Rgb([255, 0, 0])
        } else if x >= 120 {
            image::Rgb([0, 0, 255])
        } else {
            image::Rgb([0, 255, 0])
        }
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}
#[test]
fn automatic_cover_cache_miss_and_hit_embed_the_center_cover_in_m4a() {
    let fixture = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/cover.png", listener.local_addr().unwrap());
    let body = picture();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).unwrap() > 0);
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        stream.write_all(&body).unwrap();
    });
    fixture.assert_cover(
        execute(
            &fixture.root,
            None,
            &Context::standalone(),
            fixture.task(url.clone()),
        )
        .unwrap(),
    );
    server.join().unwrap(); // The same cover must now work with the server gone.
    fixture.assert_cover(
        execute(
            &fixture.root,
            None,
            &Context::standalone(),
            fixture.task(url.clone()),
        )
        .unwrap(),
    );
    let cancelled = Context::standalone();
    cancelled.cancel();
    assert_eq!(
        execute(&fixture.root, None, &cancelled, fixture.task(url)).unwrap_err(),
        "Cancelled"
    );
}
#[test]
fn automatic_cover_cancel_keeps_the_download_lane_and_never_starts_media() {
    let fixture = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/held-cover.png", listener.local_addr().unwrap());
    let body = picture();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        assert!(stream.read(&mut request).unwrap() > 0);
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let _ = stream.write_all(&body);
    });
    let mut services = Services::new(fixture.root.clone()).unwrap();
    let first = services.submit(fixture.task(url.clone())).unwrap();
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = services.submit(fixture.task(url)).unwrap();
    let browse = services.submit(Task::Downloaded).unwrap();
    let until = Instant::now() + Duration::from_millis(500);
    let mut browsed = false;
    while Instant::now() < until {
        if matches!(services.poll(),Some(Event::Finished{id,result:Ok(_)})if id==browse.id) {
            browsed = true;
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    first.cancel();
    second.cancel();
    release_tx.send(()).unwrap();
    server.join().unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    let mut cancelled = Vec::new();
    while Instant::now() < until && cancelled.len() < 2 {
        if let Some(Event::Finished {
            id,
            result: Err(error),
        }) = services.poll()
        {
            if id == first.id || id == second.id {
                assert!(error.contains("Cancelled"));
                cancelled.push(id);
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    services.shutdown();
    assert!(
        browsed,
        "Cover download jobs must leave interactive workers free"
    );
    assert_eq!(cancelled.len(), 2);
    assert!(
        !fixture.root.join("tools/media-started").exists(),
        "Cancelling artwork acquisition must prevent media download"
    );
}

#[test]
fn archive_then_youtube_downloads_are_fifo_after_success_failure_or_cancel() {
    for outcome in ["success", "failure", "cancel"] {
        let fixture = Fixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/first.wav", listener.local_addr().unwrap());
        let mut body = b"RIFF".to_vec();
        body.extend_from_slice(&436u32.to_le_bytes());
        body.extend_from_slice(b"WAVEfmt ");
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&22050u32.to_le_bytes());
        body.extend_from_slice(&44100u32.to_le_bytes());
        body.extend_from_slice(&2u16.to_le_bytes());
        body.extend_from_slice(&16u16.to_le_bytes());
        body.extend_from_slice(b"data");
        body.extend_from_slice(&400u32.to_le_bytes());
        body.resize(444, 0);
        let size = body.len();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let code = if outcome == "failure" {
                "500 Error"
            } else {
                "200 OK"
            };
            let _ = stream.write_all(
                format!("HTTP/1.1 {code}\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            );
            let _ = stream.write_all(&body);
        });
        let mut services = Services::new(fixture.root.clone()).unwrap();
        let mut album = OnlineItem::new(Provider::Archive);
        album.id = format!("queue-{outcome}");
        album.artist = "Fixture Artist".into();
        album.title = "First Album".into();
        album.raw =
            json!({"groups":{"WAV":[{"name":"01 First.wav","download_url":url,"size":size}]}});
        let first = services
            .submit(Task::Download(DownloadRequest::Archive {
                item: album,
                format: "WAV".into(),
            }))
            .unwrap();
        ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let second = services
            .submit(Task::Download(DownloadRequest::YouTube {
                url: "https://www.youtube.com/watch?v=first000001".into(),
                video: false,
                split_chapters: false,
            }))
            .unwrap();
        let cancelled = services
            .submit(Task::Download(DownloadRequest::File {
                url: "http://127.0.0.1:9/never-start".into(),
                name: "Cancelled.m4a".into(),
                category: "QA".into(),
                tags: None,
            }))
            .unwrap();
        cancelled.cancel();
        let browse = services.submit(Task::Downloaded).unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        let mut browsed = false;
        while Instant::now() < until {
            if let Some(event) = services.poll() {
                match event {
                    Event::Progress { id, .. } => assert_eq!(
                        id, first.id,
                        "Queued downloads cannot start before the first finishes"
                    ),
                    Event::Finished { id, result } => {
                        assert_eq!(id, browse.id);
                        result.unwrap();
                        browsed = true;
                        break;
                    }
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert!(
            browsed,
            "Browsing must remain available while downloads queue"
        );
        assert!(!fixture.root.join("tools/media-started").exists());
        if outcome == "cancel" {
            first.cancel();
        }
        release_tx.send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let mut finished = Vec::new();
        let mut second_started = false;
        while Instant::now() < until && finished.len() < 3 {
            if let Some(event) = services.poll() {
                match event {
                    Event::Progress { id, .. } => {
                        assert_ne!(id, cancelled.id, "A cancelled queued job must not start");
                        if id == second.id {
                            assert!(finished.contains(&first.id));
                            second_started = true;
                        }
                    }
                    Event::Finished { id, result } => {
                        if id == first.id {
                            if outcome == "success" {
                                assert!(matches!(result, Ok(Output::Files(_))), "{result:?}");
                            } else {
                                assert!(result.is_err());
                            }
                        } else if id == second.id {
                            assert!(matches!(result, Ok(Output::Files(_))), "{result:?}");
                        } else {
                            assert_eq!(id, cancelled.id);
                            assert!(result.unwrap_err().contains("Cancelled"));
                        }
                        finished.push(id);
                    }
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
        server.join().unwrap();
        services.shutdown();
        assert!(second_started);
        assert_eq!(finished, vec![first.id, second.id, cancelled.id]);
        assert!(fixture.root.join("tools/media-started").exists());
    }
}
