use super::*;
use lofty::file::TaggedFileExt;
use std::{
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture {
    root: PathBuf,
    audio: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bp-youtube-album-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("tools")).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let tool = root.join("tools/yt-dlp");
        fs::write(&tool, r#"#!/bin/sh
base=${0%/*}
printf '%s\n' "$@" >> "$base/invocations"
metadata=no
output=
url=
info=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -J) metadata=yes ;;
    -o) shift; output=$1 ;;
    --load-info-json) shift; info=$1 ;;
  esac
  url=$1
  shift
done
if [ -n "$info" ]; then
  case "$(/bin/cat "$info")" in
    *'"id":"first000001"'*) url='https://www.youtube.com/watch?v=first000001' ;;
    *'"id":"second00001"'*) url='https://www.youtube.com/watch?v=second00001' ;;
  esac
  if [ -f "$base/stale-info" ]; then echo 'HTTP Error 403: Forbidden' >&2; exit 1; fi
fi
if [ "$metadata" = yes ]; then
  case "$url" in
    *'playlist?list='*) /bin/cat "$base/playlist.json" ;;
    *'first000001'*) /bin/cat "$base/first.json" ;;
    *'second00001'*) /bin/cat "$base/second.json" ;;
    *) exit 21 ;;
  esac
else
  printf '%s\n' "$url" >> "$base/media-invocations"
  case "$url" in
    *'second00001'*) if [ -f "$base/cancel-second" ]; then : > "$base/second-started"; exec /bin/sleep 30; fi ;;
    *'playlist?list='*) exit 22 ;;
  esac
  /bin/cp "$base/media.m4a" "${output%'%(ext)s'}m4a"
fi
"#).unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
        let playlist = json!({"_type":"playlist","id":"PLfixture","title":"Album - A / ../ shared album","channel":"Main / Artist","entries":[
            {"id":"first000001","title":"Same title","url":"first000001"},
            {"id":"private0001","title":"[Private video]"},
            {"id":"second00001","title":"Same title","url":"second00001"}]});
        for (name, value) in [
            ("playlist", playlist),
            (
                "first",
                json!({"id":"first000001","title":"Same title","artist":"Singer One","webpage_url":"https://www.youtube.com/watch?v=first000001","formats":[{"format_id":"140","url":"https://example.invalid/first.m4a"}]}),
            ),
            (
                "second",
                json!({"id":"second00001","title":"Same title","artist":"Singer Two","webpage_url":"https://www.youtube.com/watch?v=second00001","formats":[{"format_id":"140","url":"https://example.invalid/second.m4a"}]}),
            ),
        ] {
            fs::write(
                root.join(format!("tools/{name}.json")),
                serde_json::to_vec(&value).unwrap(),
            )
            .unwrap();
        }
        fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut bytes = ((body.len() + 8) as u32).to_be_bytes().to_vec();
            bytes.extend(kind);
            bytes.extend(body);
            bytes
        }
        let audio = b"original encoded audio payload for each playlist track".to_vec();
        let mut bytes = boxed(b"ftyp", b"M4A \0\0\0\0isom");
        bytes.extend(boxed(b"moov", &[]));
        bytes.extend(boxed(b"mdat", &audio));
        fs::write(root.join("tools/media.m4a"), bytes).unwrap();
        Self { root, audio }
    }
    fn run(&self, context: &Context) -> Result<Vec<PathBuf>, String> {
        youtube_download(
            context,
            &self.root,
            "https://www.youtube.com/playlist?list=PLfixture",
            false,
            false,
            None,
        )
    }
    fn files(&self) -> Vec<PathBuf> {
        fn walk(path: &Path, out: &mut Vec<PathBuf>) {
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        walk(&entry.path(), out);
                    } else if entry.path().extension().is_some_and(|v| v == "m4a") {
                        out.push(entry.path());
                    }
                }
            }
        }
        let mut files = Vec::new();
        walk(&self.root.join("downloads"), &mut files);
        files.sort();
        files
    }
    fn calls(&self, id: &str) -> usize {
        fs::read_to_string(self.root.join("tools/invocations"))
            .unwrap()
            .lines()
            .filter(|line| *line == format!("https://www.youtube.com/watch?v={id}"))
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn payload(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).unwrap();
    let mut offset = 0;
    while offset + 8 <= bytes.len() {
        let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        assert!(size >= 8 && offset + size <= bytes.len());
        if &bytes[offset + 4..offset + 8] == b"mdat" {
            return bytes[offset + 8..offset + size].to_vec();
        }
        offset += size;
    }
    panic!("missing source media");
}

#[test]
fn playlist_download_publishes_ordered_tracks_in_one_album_without_overwriting() {
    let fixture = Fixture::new();
    let cover = fixture.root.join("cover.png");
    image::RgbaImage::new(2, 2).save(&cover).unwrap();
    let files = youtube_download(
        &Context::standalone(),
        &fixture.root,
        "https://www.youtube.com/playlist?list=PLfixture",
        false,
        false,
        Some(&cover),
    )
    .unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].parent(), files[1].parent());
    assert_ne!(files[0], files[1]);
    assert!(files[0].starts_with(fixture.root.join("downloads")));
    assert!(files[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("01 "));
    assert!(files[1]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("02 "));
    for (index, path) in files.iter().enumerate() {
        let tags = metadata::read_tag_fields(path).unwrap();
        assert_eq!(tags.album.as_deref(), Some("A / ../ shared album"));
        assert_eq!(tags.album_artist.as_deref(), Some("Main / Artist"));
        assert_eq!(
            tags.artist.as_deref(),
            Some(if index == 0 {
                "Singer One"
            } else {
                "Singer Two"
            })
        );
        assert_eq!(tags.track, Some(index as u32 + 1));
        assert_eq!(tags.track_total, Some(2));
        assert_eq!(payload(path), fixture.audio);
        let read = lofty::probe::Probe::open(path)
            .unwrap()
            .options(lofty::config::ParseOptions::new().read_properties(false))
            .read()
            .unwrap();
        assert_eq!(
            read.primary_tag().unwrap().pictures()[0].data(),
            fs::read(&cover).unwrap()
        );
    }
    let first = fs::read(&files[0]).unwrap();
    let repeated = fixture.run(&Context::standalone()).unwrap();
    assert_eq!(repeated, files);
    assert_eq!(fixture.files().len(), 2);
    assert_eq!(
        fixture.calls("first000001"),
        1,
        "A verified completed track must not be resolved again"
    );
    assert_eq!(fixture.calls("second00001"), 1);
    assert_eq!(
        fs::read(&files[0]).unwrap(),
        first,
        "A second download never replaces the first album's files"
    );
}

#[test]
fn cancelling_a_playlist_preserves_completed_tracks_and_cleans_the_current_stage() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("tools/cancel-second"), b"").unwrap();
    let context = Context::standalone();
    let worker_context = context.clone();
    let root = fixture.root.clone();
    let worker = thread::spawn(move || {
        youtube_download(
            &worker_context,
            &root,
            "https://www.youtube.com/playlist?list=PLfixture",
            false,
            false,
            None,
        )
    });
    let until = Instant::now() + Duration::from_secs(5);
    while !fixture.root.join("tools/second-started").exists() && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        fixture.root.join("tools/second-started").exists(),
        "Second track never started"
    );
    context.cancel();
    let error = worker.join().unwrap().unwrap_err();
    assert!(error.contains("Cancelled"));
    let files = fixture.files();
    assert_eq!(files.len(), 1);
    assert_eq!(payload(&files[0]), fixture.audio);
    let pending = fixture.root.join("tmp/youtube/0");
    assert!(
        fs::read_dir(&pending)
            .map(|entries| entries.count())
            .unwrap_or(0)
            == 0,
        "An interrupted track left a reusable staging source"
    );
    let first = fs::read(&files[0]).unwrap();
    fs::remove_file(fixture.root.join("tools/cancel-second")).unwrap();
    let retried = fixture.run(&Context::standalone()).unwrap();
    assert_eq!(retried.len(), 2);
    assert_eq!(retried[0], files[0]);
    assert_eq!(fixture.files().len(), 2);
    assert_eq!(fs::read(&files[0]).unwrap(), first);
    assert_eq!(
        fixture.calls("first000001"),
        1,
        "Retry must reuse the completed first track"
    );
    assert_eq!(
        fixture.calls("second00001"),
        1,
        "Retry must reuse the interrupted track's bounded metadata cache"
    );
    let media = fs::read_to_string(fixture.root.join("tools/media-invocations")).unwrap();
    assert_eq!(
        media
            .lines()
            .filter(|line| line.contains("first000001"))
            .count(),
        1
    );
    assert_eq!(
        media
            .lines()
            .filter(|line| line.contains("second00001"))
            .count(),
        2
    );
}

#[test]
fn playlist_retries_keep_repeated_video_positions_distinct() {
    let fixture = Fixture::new();
    let path = fixture.root.join("tools/playlist.json");
    let mut playlist: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let repeated = playlist["entries"][0].clone();
    playlist["entries"].as_array_mut().unwrap().push(repeated);
    fs::write(&path, serde_json::to_vec(&playlist).unwrap()).unwrap();
    let files = fixture.run(&Context::standalone()).unwrap();
    assert_eq!(files.len(), 3);
    assert_ne!(files[0], files[2]);
    assert_eq!(metadata::read_tag_fields(&files[0]).unwrap().track, Some(1));
    assert_eq!(metadata::read_tag_fields(&files[2]).unwrap().track, Some(3));
    assert_eq!(fixture.run(&Context::standalone()).unwrap(), files);
    assert_eq!(fixture.files().len(), 3);
    assert_eq!(fixture.calls("first000001"), 1);
}

#[test]
fn playlist_retries_reject_changed_missing_and_symlinked_tracks_without_overwriting() {
    let fixture = Fixture::new();
    let files = fixture.run(&Context::standalone()).unwrap();
    let mut changed = fs::read(&files[0]).unwrap();
    *changed.last_mut().unwrap() ^= 1;
    fs::write(&files[0], &changed).unwrap();
    fs::remove_file(&files[1]).unwrap();
    let retried = fixture.run(&Context::standalone()).unwrap();
    assert_ne!(
        retried[0], files[0],
        "A changed file must be preserved and replaced by a fresh destination"
    );
    assert_eq!(fs::read(&files[0]).unwrap(), changed);
    assert_eq!(
        retried[1], files[1],
        "A missing track may safely reuse its absent destination"
    );
    assert_eq!(payload(&retried[0]), fixture.audio);
    assert_eq!(payload(&retried[1]), fixture.audio);
    let outside = fixture.root.join("outside.m4a");
    fs::write(&outside, b"keep this file").unwrap();
    fs::remove_file(&retried[1]).unwrap();
    std::os::unix::fs::symlink(&outside, &retried[1]).unwrap();
    let next = fixture.run(&Context::standalone()).unwrap();
    assert_ne!(next[1], retried[1]);
    assert!(fs::symlink_metadata(&retried[1])
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&outside).unwrap(), b"keep this file");
    fs::remove_file(&next[1]).unwrap();
    std::os::unix::fs::symlink(fixture.root.join("absent.m4a"), &next[1]).unwrap();
    let dangling = fixture.run(&Context::standalone()).unwrap();
    assert_ne!(
        dangling[1], next[1],
        "A dangling symlink must also remain untouched"
    );
    assert!(fs::symlink_metadata(&next[1])
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn playlist_receipts_refuse_traversal_and_symlinked_manifests() {
    let fixture = Fixture::new();
    let files = fixture.run(&Context::standalone()).unwrap();
    let manifest = files[0].parent().unwrap().join(".youtube-receipts.json");
    let mut receipts: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    for receipt in receipts["tracks"].as_object_mut().unwrap().values_mut() {
        receipt["file"] = json!("../outside.m4a");
    }
    fs::write(&manifest, serde_json::to_vec(&receipts).unwrap()).unwrap();
    let retried = fixture.run(&Context::standalone()).unwrap();
    assert!(retried
        .iter()
        .all(|path| path.parent() == files[0].parent()));
    assert!(retried.iter().all(|path| !files.contains(path)));
    let outside = fixture.root.join("outside.json");
    fs::write(&outside, b"preserve").unwrap();
    fs::remove_file(&manifest).unwrap();
    std::os::unix::fs::symlink(&outside, &manifest).unwrap();
    assert!(fixture
        .run(&Context::standalone())
        .unwrap_err()
        .contains("unsafe"));
    assert_eq!(fs::read(outside).unwrap(), b"preserve");
}

#[test]
fn playlist_video_download_is_refused_before_any_media_is_written() {
    let fixture = Fixture::new();
    let error = youtube_download(
        &Context::standalone(),
        &fixture.root,
        "https://www.youtube.com/playlist?list=PLfixture",
        true,
        false,
        None,
    )
    .unwrap_err();
    assert!(error.contains("individual video"));
    assert!(fixture.files().is_empty());
    assert!(!fixture.root.join("tools/invocations").exists());
}

#[test]
fn full_album_details_then_download_resolves_once_and_keeps_only_slim_ui_metadata() {
    let fixture = Fixture::new();
    let metadata = fixture.root.join("tools/first.json");
    let mut full: Value = serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
    full["title"] = json!("ANRI - Timely!! (1983) FULL ALBUM");
    full["chapters"] = json!([{"title":"CAT'S EYE","start_time":0,"end_time":240},{"title":"WINDY SUMMER","start_time":240,"end_time":480}]);
    let long = "x".repeat(8192);
    full["formats"] = json!((0..256).map(|index| json!({"format_id":index.to_string(),"url":format!("https://example.invalid/audio?signature={long}"),"vcodec":"none","acodec":"mp4a.40.2","http_headers":{"User-Agent":long}})).collect::<Vec<_>>());
    full["automatic_captions"] = json!({"en":[{"url":long}]});
    full["subtitles"] = json!({"en":[{"url":long}]});
    fs::write(&metadata, serde_json::to_vec(&full).unwrap()).unwrap();
    let full_bytes = serde_json::to_vec(&full).unwrap().len();
    let item = crate::online::youtube_item(
        json!({"id":"first000001","url":"https://www.youtube.com/watch?v=first000001"}),
        crate::online::Provider::YouTube,
    );
    let details = crate::online::details(&Context::standalone(), &fixture.root, item).unwrap();
    let slim_bytes = serde_json::to_vec(&details).unwrap().len();
    assert!(
        details.raw.get("formats").is_none()
            && details.raw.get("subtitles").is_none()
            && details.raw.get("automatic_captions").is_none()
    );
    assert_eq!(details.raw["chapters"].as_array().unwrap().len(), 2);
    assert!(full_bytes > 4 * 1024 * 1024 && slim_bytes < 2048);
    eprintln!("Full-album metadata fixture: full={full_bytes} bytes, UI projection={slim_bytes} bytes, five UI copies={} bytes", slim_bytes * 5);
    let files = youtube_download(
        &Context::standalone(),
        &fixture.root,
        &details.url,
        false,
        false,
        None,
    )
    .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(payload(&files[0]), fixture.audio);
    assert_eq!(
        fixture.calls("first000001"),
        1,
        "Details -> Download must not repeat metadata extraction"
    );
    let calls = fs::read_to_string(fixture.root.join("tools/invocations")).unwrap();
    assert_eq!(calls.lines().filter(|line| *line == "-J").count(), 1);
    assert_eq!(
        calls
            .lines()
            .filter(|line| *line == "--load-info-json")
            .count(),
        1
    );
}

#[test]
fn stale_cached_urls_retry_the_original_video_once_without_changing_audio() {
    let fixture = Fixture::new();
    let url = "https://www.youtube.com/watch?v=first000001";
    let _ = youtube_video_info(&Context::standalone(), &fixture.root, url).unwrap();
    fs::write(fixture.root.join("tools/stale-info"), b"").unwrap();
    let files = youtube_download(
        &Context::standalone(),
        &fixture.root,
        url,
        false,
        false,
        None,
    )
    .unwrap();
    assert_eq!(payload(&files[0]), fixture.audio);
    let calls = fs::read_to_string(fixture.root.join("tools/invocations")).unwrap();
    assert_eq!(
        calls
            .lines()
            .filter(|line| *line == "--load-info-json")
            .count(),
        1
    );
    assert_eq!(fixture.calls("first000001"), 2, "There must be exactly one original-URL media fallback after the initial metadata extraction");
}

#[test]
#[cfg(target_os = "linux")]
fn full_album_ui_clone_memory_profile() {
    if let Ok(mode) = std::env::var("BETTERPOD_TEST_RAW_PROFILE") {
        let long = "x".repeat(8192);
        let full = json!({"id":"first000001","title":"ANRI - Timely!! FULL ALBUM","chapters":[{"title":"First","start_time":0,"end_time":240}],
            "formats":(0..256).map(|index| json!({"format_id":index.to_string(),"url":format!("https://example.invalid/{long}"),"headers":{"User-Agent":long}})).collect::<Vec<_>>()});
        let item = if mode == "slim" {
            crate::online::youtube_item(full, crate::online::Provider::YouTube)
        } else {
            let mut item = crate::online::OnlineItem::new(crate::online::Provider::YouTube);
            item.raw = full;
            item
        };
        let rss = || {
            fs::read_to_string("/proc/self/status")
                .unwrap()
                .lines()
                .find_map(|line| {
                    line.strip_prefix("VmRSS:")
                        .and_then(|value| value.split_whitespace().next())
                        .and_then(|value| value.parse::<u64>().ok())
                })
                .unwrap()
                * 1024
        };
        let before = rss();
        let copies = (0..5).map(|_| item.clone()).collect::<Vec<_>>();
        std::hint::black_box((&item, &copies));
        println!("profile-rss-delta={}", rss().saturating_sub(before));
        return;
    }
    let profile = |mode: &str| {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "downloads::youtube_tests::full_album_ui_clone_memory_profile",
                "--nocapture",
            ])
            .env("BETTERPOD_TEST_RAW_PROFILE", mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .split_once("profile-rss-delta=")
            .unwrap()
            .1
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap()
    };
    let full = profile("full");
    let slim = profile("slim");
    eprintln!("Five full-album UI clones: RSS growth full={full} bytes; projected={slim} bytes");
    assert!(
        full > slim + 8 * 1024 * 1024,
        "Dropping resolver-only metadata must materially reduce live UI clone allocations"
    );
}

#[test]
fn cancelled_chapter_album_retry_reuses_completed_tracks_without_overwriting() {
    let fixture = Fixture::new();
    let path = fixture.root.join("tools/first.json");
    let mut info: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    info["chapters"] = json!([{"title":"Same chapter","start_time":0,"end_time":240},{"title":"Same chapter","start_time":240,"end_time":480}]);
    fs::write(path, serde_json::to_vec(&info).unwrap()).unwrap();
    let ffmpeg = fixture.root.join("tools/ffmpeg");
    fs::write(
        &ffmpeg,
        r#"#!/bin/sh
base=${0%/*}
input=
output=
printf '%s\n' "$@" >> "$base/chapter-invocations"
while [ "$#" -gt 0 ]; do
  case "$1" in -i) shift; input=$1 ;; esac
  output=$1
  shift
done
case "$output" in *chapter-1.m4a)
  if [ -f "$base/cancel-chapter" ]; then : > "$base/chapter-started"; exec /bin/sleep 30; fi ;;
esac
/bin/cp "$input" "$output"
"#,
    )
    .unwrap();
    fs::set_permissions(ffmpeg, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(fixture.root.join("tools/cancel-chapter"), b"").unwrap();
    let context = Context::standalone();
    let worker_context = context.clone();
    let root = fixture.root.clone();
    let worker = thread::spawn(move || {
        youtube_download(
            &worker_context,
            &root,
            "https://www.youtube.com/watch?v=first000001",
            false,
            true,
            None,
        )
    });
    let until = Instant::now() + Duration::from_secs(5);
    while !fixture.root.join("tools/chapter-started").exists() && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(fixture.root.join("tools/chapter-started").exists());
    context.cancel();
    assert!(worker.join().unwrap().unwrap_err().contains("Cancelled"));
    let first = fixture.files();
    assert_eq!(first.len(), 1);
    let bytes = fs::read(&first[0]).unwrap();
    assert_eq!(payload(&first[0]), fixture.audio);
    fs::remove_file(fixture.root.join("tools/cancel-chapter")).unwrap();
    let files = youtube_download(
        &Context::standalone(),
        &fixture.root,
        "https://www.youtube.com/watch?v=first000001",
        false,
        true,
        None,
    )
    .unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0], first[0]);
    assert_ne!(
        files[0], files[1],
        "Equal chapter titles at different positions must stay separate"
    );
    assert_eq!(fs::read(&first[0]).unwrap(), bytes);
    for (index, file) in files.iter().enumerate() {
        let tags = metadata::read_tag_fields(file).unwrap();
        assert_eq!(tags.title.as_deref(), Some("Same chapter"));
        assert_eq!(tags.artist.as_deref(), Some("Singer One"));
        assert_eq!(tags.album.as_deref(), Some("Same title"));
        assert_eq!(tags.track, Some(index as u32 + 1));
        assert_eq!(tags.track_total, Some(2));
        assert_eq!(payload(file), fixture.audio);
    }
    let cuts = fs::read_to_string(fixture.root.join("tools/chapter-invocations")).unwrap();
    assert_eq!(
        cuts.lines()
            .filter(|line| line.contains("chapter-0.m4a"))
            .count(),
        1,
        "Retry must not cut the verified first chapter again"
    );
    assert_eq!(
        cuts.lines()
            .filter(|line| line.contains("chapter-1.m4a"))
            .count(),
        2
    );
    let media = fs::read_to_string(fixture.root.join("tools/media-invocations")).unwrap();
    assert_eq!(
        youtube_download(
            &Context::standalone(),
            &fixture.root,
            "https://www.youtube.com/watch?v=first000001",
            false,
            true,
            None
        )
        .unwrap(),
        files
    );
    assert_eq!(fixture.files().len(), 2);
    assert_eq!(
        fs::read_to_string(fixture.root.join("tools/media-invocations")).unwrap(),
        media,
        "A complete chapter album must return before fetching its source again"
    );
}

#[test]
fn watch_reuses_detail_formats_and_refreshes_rejected_urls() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    for stale in [false, true] {
        let fixture = Fixture::new();
        let context = Context::standalone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let media = format!("http://{}/media", listener.local_addr().unwrap());
        let info = json!({"id":"first000001","title":"Video","duration":10,
            "formats":[{"url":media,"protocol":"http","vcodec":"avc1","acodec":"mp4a","height":360}]});
        let url = "https://www.youtube.com/watch?v=first000001";
        youtube_info::store(&context, &fixture.root, url, &info).unwrap();
        fs::write(
            fixture.root.join("tools/first.json"),
            serde_json::to_vec(&info).unwrap(),
        )
        .unwrap();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            for n in 0..if stale { 2 } else { 1 } {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "Missing media probe");
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0; 4096];
                let bytes = stream.read(&mut request).unwrap();
                assert!(String::from_utf8_lossy(&request[..bytes])
                    .to_lowercase()
                    .contains("range: bytes=0-0"));
                let status = if stale && n == 0 {
                    "403 Forbidden"
                } else {
                    "206 Partial Content"
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            }
        });
        let item = crate::online::youtube_item(
            json!({"id":"first000001","webpage_url":url}),
            crate::online::Provider::YouTube,
        );
        let stream = resolve_stream(&context, &fixture.root, &item, true).unwrap();
        server.join().unwrap();
        assert_eq!(stream.video_url, media);
        let calls = fs::read_to_string(fixture.root.join("tools/invocations")).unwrap_or_default();
        assert_eq!(
            calls.lines().filter(|line| *line == "-J").count(),
            usize::from(stale)
        );
    }
}
