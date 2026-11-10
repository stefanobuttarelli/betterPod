//! Resumable downloads and yt-dlp orchestration. All destinations are app-private.
use crate::{
    metadata::{self, TagEdit},
    online::{
        http_agent, number, text, validate_http, Context, OnlineItem, StreamSource, USER_AGENT,
    },
    podcasts::{self, Episode, Podcast},
};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::thread;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod archive;
mod process;
mod youtube_info;
mod youtube_plugin;
mod youtube_receipts;
pub use archive::status as archive_status;
pub use process::run_command;
#[derive(Clone, Debug)]
pub enum DownloadRequest {
    Archive {
        item: OnlineItem,
        format: String,
    },
    YouTube {
        url: String,
        video: bool,
        split_chapters: bool,
    },
    Episodes {
        podcast: Podcast,
        episodes: Vec<Episode>,
    },
    File {
        url: String,
        name: String,
        category: String,
        tags: Option<TagEdit>,
    },
}
#[derive(Clone, Copy, Debug)]
pub enum SquareCrop {
    Start,
    Center,
    End,
}
pub fn run_with_cover(
    context: &Context,
    data: &Path,
    request: DownloadRequest,
    cover: &Path,
    crop: SquareCrop,
) -> Result<Vec<PathBuf>, String> {
    let target = prepare_square_cover(context, data, cover, crop)?;
    match request {
        DownloadRequest::YouTube {
            url,
            video,
            split_chapters,
        } => youtube_download(context, data, &url, video, split_chapters, Some(&target)),
        _ => Err("Square cover selection is only available for YouTube audio".into()),
    }
}
pub(crate) fn prepare_square_cover(
    context: &Context,
    data: &Path,
    cover: &Path,
    crop: SquareCrop,
) -> Result<PathBuf, String> {
    let root = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let cover = fs::canonicalize(cover).map_err(|e| e.to_string())?;
    if !cover.starts_with(&root) || !cover.is_file() {
        return Err("Cover is outside private artwork".into());
    }
    if fs::metadata(&cover).map_err(|e| e.to_string())?.len() > 8 * 1024 * 1024 {
        return Err("Cover exceeds 8 MB".into());
    }
    let mut reader = image::ImageReader::open(&cover)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?;
    let edge = image.width().min(image.height());
    let k = match crop {
        SquareCrop::Start => 0,
        SquareCrop::Center => 1,
        SquareCrop::End => 2,
    };
    let square = image
        .crop_imm(
            (image.width() - edge) * k / 2,
            (image.height() - edge) * k / 2,
            edge,
            edge,
        )
        .thumbnail(1024, 1024);
    let mut bytes = std::io::Cursor::new(Vec::new());
    square
        .write_to(&mut bytes, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    let bytes = bytes.into_inner();
    let dir = private_dir(data, &["artwork", "online"])?;
    let target = dir.join(format!("crop-{:x}.jpg", Sha256::digest(&bytes)));
    context.check()?;
    if !target.is_file() {
        crate::storage::atomic_write(&target, &bytes).map_err(|e| e.to_string())?;
    }
    Ok(target)
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PartInfo {
    url: String,
    etag: String,
    modified: String,
    total: u64,
}
pub(crate) static TOOLS: Mutex<()> = Mutex::new(());
fn tools_guard(context: &Context) -> Result<std::sync::MutexGuard<'static, ()>, String> {
    let mut reported = Instant::now() - Duration::from_secs(1);
    loop {
        context.check()?;
        match TOOLS.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(error)) => return Err(error.to_string()),
            Err(std::sync::TryLockError::WouldBlock) => {
                if reported.elapsed() >= Duration::from_millis(500) {
                    context.progress(0, None, "Waiting for the current YouTube task");
                    reported = Instant::now();
                }
                context.sleep(Duration::from_millis(50))?;
            }
        }
    }
}
static COMMIT_LOCK: Mutex<()> = Mutex::new(());
static ACTIVE_DOWNLOADS: LazyLock<Mutex<std::collections::HashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));
struct DestinationGuard(PathBuf);
impl Drop for DestinationGuard {
    fn drop(&mut self) {
        if let Ok(mut paths) = ACTIVE_DOWNLOADS.lock() {
            paths.remove(&self.0);
        }
    }
}
pub fn safe_name(name: &str) -> String {
    let mut value: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    value = value.trim().trim_matches('.').to_string();
    if value.is_empty() {
        value = "Untitled".into();
    }
    while value.len() > 180 {
        value.pop();
    }
    value
}
pub(crate) fn private_dir(data: &Path, parts: &[&str]) -> Result<PathBuf, String> {
    fs::create_dir_all(data).map_err(|e| e.to_string())?;
    let root = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let mut current = root.clone();
    for part in parts {
        current.push(safe_name(part));
        match fs::symlink_metadata(&current) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                return Err("Download folder is not a private directory".into())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|e| e.to_string())?
            }
            Err(e) => return Err(e.to_string()),
        }
        if !fs::canonicalize(&current)
            .map_err(|e| e.to_string())?
            .starts_with(&root)
        {
            return Err("Download path escaped private data".into());
        }
    }
    Ok(current)
}
fn free_file(dir: &Path, name: &str) -> PathBuf {
    let name = safe_name(name);
    let initial = dir.join(&name);
    if !initial.exists() {
        return initial;
    }
    let stem = Path::new(&name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let ext = Path::new(&name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    for n in 2.. {
        let candidate = dir.join(if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        });
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}
fn part_name(target: &Path) -> PathBuf {
    let ext = target
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("data");
    target.with_file_name(format!(
        ".{}.part.{ext}",
        target.file_stem().unwrap_or_default().to_string_lossy()
    ))
}
fn download(
    context: &Context,
    url: &str,
    target: &Path,
    expected_sha: Option<&str>,
) -> Result<(), String> {
    download_tagged(context, url, target, expected_sha, None)
}
fn download_tagged(
    context: &Context,
    url: &str,
    target: &Path,
    expected_sha: Option<&str>,
    tags: Option<&TagEdit>,
) -> Result<(), String> {
    download_tagged_before_commit(context, url, target, expected_sha, tags, None)
}
fn download_tagged_before_commit(
    context: &Context,
    url: &str,
    target: &Path,
    expected_sha: Option<&str>,
    tags: Option<&TagEdit>,
    mut before_commit: Option<&mut dyn FnMut(&Path) -> Result<(), String>>,
) -> Result<(), String> {
    validate_http(url)?;
    context.check()?;
    if !ACTIVE_DOWNLOADS
        .lock()
        .map_err(|e| e.to_string())?
        .insert(target.to_path_buf())
    {
        return Err("This file is already being downloaded".into());
    }
    let _destination = DestinationGuard(target.to_path_buf());
    let _parent = target.parent().ok_or("Missing download directory")?;
    let stage = part_name(target);
    let sidecar = stage.with_extension(format!(
        "{}.json",
        stage.extension().unwrap_or_default().to_string_lossy()
    ));
    let old: PartInfo = fs::read(&sidecar)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut offset = fs::metadata(&stage).map(|m| m.len()).unwrap_or(0);
    if old.url != url
        || offset > old.total && old.total != 0
        || !strong_etag(&old.etag) && old.modified.is_empty()
    {
        offset = 0;
    }
    let agent = http_agent(false);
    let mut request = agent.get(url).header("User-Agent", USER_AGENT);
    if offset > 0 {
        request = request.header("Range", format!("bytes={offset}-"));
        request = request.header(
            "If-Range",
            if strong_etag(&old.etag) {
                old.etag.clone()
            } else {
                old.modified.clone()
            },
        );
    }
    let mut response = match request.call() {
        Ok(r) => r,
        Err(e) => {
            if offset > 0 {
                offset = 0;
                agent
                    .get(url)
                    .header("User-Agent", USER_AGENT)
                    .call()
                    .map_err(|e| e.to_string())?
            } else {
                return Err(e.to_string());
            }
        }
    };
    let status = response.status().as_u16();
    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let modified = response
        .headers()
        .get("last-modified")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let length = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());
    let mut total = length.unwrap_or(0);
    if offset > 0 && status == 206 {
        let range = response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let Some((start, range_total)) = content_range(range) else {
            let _ = fs::remove_file(&sidecar);
            return Err("Invalid resumed download range".into());
        };
        if start != offset
            || (old.total > 0 && range_total != old.total)
            || (strong_etag(&old.etag) && etag != old.etag)
            || (!strong_etag(&old.etag) && !old.modified.is_empty() && modified != old.modified)
        {
            let _ = fs::remove_file(&sidecar);
            return Err("Download changed while resuming; retry starts a new file".into());
        }
        total = range_total;
    } else if status == 200 {
        offset = 0;
    } else if status != 206 {
        return Err(format!("Download returned HTTP {status}"));
    } else {
        return Err("Unexpected partial download".into());
    }
    let info = PartInfo {
        url: url.into(),
        etag,
        modified,
        total,
    };
    crate::storage::atomic_write(
        &sidecar,
        &serde_json::to_vec(&info).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut output = OpenOptions::new()
        .write(true)
        .create(true)
        .append(offset > 0)
        .truncate(offset == 0)
        .open(&stage)
        .map_err(|e| e.to_string())?;
    let mut body = response.body_mut().as_reader();
    let mut buffer = [0u8; 65536];
    let mut done = offset;
    let mut report = Instant::now() - Duration::from_secs(1);
    loop {
        context.check()?;
        let count = body.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
        done += count as u64;
        if total > 0 && done > total {
            let _ = fs::remove_file(&sidecar);
            return Err("Download exceeded its declared size".into());
        }
        if report.elapsed() > Duration::from_millis(200) {
            context.progress(
                done,
                (total > 0).then_some(total),
                format!(
                    "Downloading {}",
                    target.file_name().unwrap_or_default().to_string_lossy()
                ),
            );
            report = Instant::now();
        }
    }
    output.sync_all().map_err(|e| e.to_string())?;
    drop(output);
    if total > 0 && done != total {
        return Err(format!("Incomplete download ({done}/{total} bytes)"));
    }
    let extension = target
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if expected_sha.is_none()
        && matches!(
            extension.as_str(),
            "mp3"
                | "m4a"
                | "m4b"
                | "flac"
                | "ogg"
                | "oga"
                | "opus"
                | "aac"
                | "wav"
                | "aif"
                | "aiff"
        )
    {
        if (|| -> Result<(), String> {
            lofty::probe::Probe::open(&stage)
                .map_err(|e| e.to_string())?
                .guess_file_type()
                .map_err(|e| e.to_string())?
                .read()
                .map_err(|e| e.to_string())?;
            Ok(())
        })()
        .is_err()
        {
            let _ = fs::remove_file(&stage);
            let _ = fs::remove_file(&sidecar);
            return Err("Downloaded response is not a supported audio file".into());
        }
    }
    if let Some(expected) = expected_sha {
        let actual = sha256_file(&stage)?;
        if actual != expected.to_ascii_lowercase() {
            let _ = fs::remove_file(&stage);
            let _ = fs::remove_file(&sidecar);
            return Err("Download checksum does not match".into());
        }
    }
    if let Some(tags) = tags {
        // Tag the complete hidden file before exposing it to library scans.
        // A failed/cancelled edit restarts the source download on the next try.
        let _ = fs::remove_file(&sidecar);
        tag_download(context, &stage, tags)?;
    }
    context.check()?;
    let _commit = COMMIT_LOCK.lock().map_err(|e| e.to_string())?;
    if target.exists() {
        return Err("Download destination already exists".into());
    }
    if let Some(callback) = before_commit.as_mut() {
        callback(&stage)?;
    }
    context.check()?;
    fs::rename(&stage, target).map_err(|e| e.to_string())?;
    crate::storage::sync_directory(target.parent().ok_or("Missing download parent")?)
        .map_err(|e| e.to_string())?;
    let _ = fs::remove_file(sidecar);
    Ok(())
}
fn strong_etag(value: &str) -> bool {
    value.starts_with('"') && value.ends_with('"') && value.len() >= 2
}
fn content_range(value: &str) -> Option<(u64, u64)> {
    let value = value.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let (start, end, total) = (
        start.parse::<u64>().ok()?,
        end.parse::<u64>().ok()?,
        total.parse::<u64>().ok()?,
    );
    (start <= end && end < total).then_some((start, total))
}
fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn tag_download(context: &Context, path: &Path, edit: &TagEdit) -> Result<(), String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if matches!(
        ext.as_str(),
        "mp3" | "m4a" | "m4b" | "flac" | "ogg" | "oga" | "opus"
    ) {
        metadata::write_tags_cancellable(path, edit, context)?;
    }
    Ok(())
}
fn download_episode(
    context: &Context,
    url: &str,
    requested: &Path,
    tags: &TagEdit,
) -> Result<PathBuf, String> {
    use lofty::file::{FileType, TaggedFileExt};
    if !ACTIVE_DOWNLOADS
        .lock()
        .map_err(|e| e.to_string())?
        .insert(requested.to_path_buf())
    {
        return Err("This podcast episode is already being downloaded".into());
    }
    let _destination = DestinationGuard(requested.to_path_buf());
    let parent = requested.parent().ok_or("Missing podcast folder")?;
    let identity = format!("{:x}", Sha256::digest(url.as_bytes()));
    let extension = requested
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp3");
    let prepared = parent.join(format!(".episode-{}.{}", &identity[..16], extension));
    download(context, url, &prepared, None)?;
    let mut ready = prepared.clone();
    let result = (|| {
        context.check()?;
        let parsed = lofty::probe::Probe::open(&prepared)
            .map_err(|e| e.to_string())?
            .guess_file_type()
            .map_err(|e| e.to_string())?
            .read()
            .map_err(|e| e.to_string())?;
        let actual = match parsed.file_type() {
            FileType::Mpeg => "mp3",
            FileType::Mp4 if extension == "m4b" => "m4b",
            FileType::Mp4 => "m4a",
            FileType::Flac => "flac",
            FileType::Vorbis => "ogg",
            FileType::Opus => "opus",
            FileType::Aac => "aac",
            FileType::Wav => "wav",
            FileType::Aiff => "aiff",
            FileType::Ape => "ape",
            FileType::WavPack => "wv",
            _ => return Err("Podcast returned an unsupported audio format".into()),
        };
        if actual != extension {
            ready = prepared.with_extension(actual);
            if ready.exists() {
                return Err("This podcast file is already being prepared".into());
            }
            fs::rename(&prepared, &ready).map_err(|e| e.to_string())?;
        }
        let target = free_file(
            parent,
            &format!(
                "{}.{}",
                requested.file_stem().unwrap_or_default().to_string_lossy(),
                actual
            ),
        );
        tag_download(context, &ready, tags)?;
        commit_media(context, &ready, &target)?;
        Ok(target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&prepared);
        let _ = fs::remove_file(&ready);
    }
    result
}
pub fn run(
    context: &Context,
    data: &Path,
    request: DownloadRequest,
) -> Result<Vec<PathBuf>, String> {
    match request {
        DownloadRequest::File {
            url,
            name,
            category,
            tags,
        } => {
            let dir = private_dir(data, &["downloads", &category])?;
            let target = free_file(&dir, &name);
            download_tagged(context, &url, &target, None, tags.as_ref())?;
            Ok(vec![target])
        }
        DownloadRequest::Archive { item, format } => archive::run(context, data, &item, &format),
        DownloadRequest::Episodes {
            mut podcast,
            episodes,
        } => {
            let _operation = podcasts::begin_download(&podcast.feed)?;
            let dir = private_dir(data, &["downloads", "Podcasts", &podcast.name])?;
            let art = if podcast.art600.is_empty() {
                &podcast.art
            } else {
                &podcast.art600
            };
            let cover = dir.join("cover.jpg");
            if !art.is_empty() && !cover.exists() {
                let _ = download(context, art, &cover, None);
            }
            let mut out = Vec::new();
            for episode in episodes {
                context.check()?;
                if podcast
                    .files
                    .get(episode.key())
                    .is_some_and(|p| Path::new(p).is_file())
                {
                    continue;
                }
                let name = format!(
                    "{} {}.{}",
                    episode.date.get(..10).unwrap_or(""),
                    episode.title,
                    episode.extension()
                );
                let target = free_file(&dir, &name);
                let tags = TagEdit {
                    title: Some(episode.title.clone()),
                    artist: Some(podcast.author.clone()),
                    album: Some(podcast.name.clone()),
                    genre: Some(if podcast.genre.is_empty() {
                        "Podcast".into()
                    } else {
                        podcast.genre.clone()
                    }),
                    track: (episode.number > 0).then_some(episode.number),
                    cover: cover.is_file().then_some(cover.clone()),
                    ..Default::default()
                };
                let target = download_episode(context, &episode.url, &target, &tags)?;
                podcast
                    .files
                    .insert(episode.key().into(), target.to_string_lossy().into_owned());
                podcasts::remember(data, podcast.clone())?;
                out.push(target);
            }
            Ok(out)
        }
        DownloadRequest::YouTube {
            url,
            video,
            split_chapters,
        } => youtube_download(context, data, &url, video, split_chapters, None),
    }
}
pub fn downloaded(data: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![data.join("downloads")];
    let mut out = Vec::new();
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                dirs.push(entry.path());
            } else if kind.is_file() {
                let ext = entry
                    .path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if matches!(
                    ext.as_str(),
                    "mp3"
                        | "m4a"
                        | "m4b"
                        | "flac"
                        | "ogg"
                        | "oga"
                        | "opus"
                        | "aac"
                        | "wav"
                        | "aiff"
                        | "aif"
                        | "mp4"
                        | "mkv"
                        | "webm"
                ) {
                    out.push(entry.path());
                }
            }
        }
    }
    out.sort();
    out
}
pub fn ytdlp_path(data: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("BETTERPOD_RUST_YTDLP") {
        return path.into();
    }
    for dir in [
        data.join("tools"),
        std::env::var_os("BETTERPOD_RUST_IMPORT_TOOLS")
            .map(PathBuf::from)
            .unwrap_or_default(),
    ] {
        for suffix in [
            "ytdlp/yt-dlp_linux_aarch64",
            "ytdlp/yt-dlp",
            "yt-dlp",
            "yt-dlp_macos",
        ] {
            let path = dir.join(suffix);
            if path.is_file() {
                return path;
            }
        }
    }
    PathBuf::from("yt-dlp")
}
fn yt_command(data: &Path) -> Command {
    let mut command = Command::new(ytdlp_path(data));
    let tools = data.join("tools");
    let _ = fs::create_dir_all(tools.join("tmp"));
    command
        .args(["--no-config", "--no-warnings", "--cache-dir"])
        .arg(tools.join("cache"));
    let configured = std::env::var("BETTERPOD_RUST_YTJS").ok();
    let runtime = configured.or_else(|| {
        for dir in [
            tools.clone(),
            std::env::var_os("BETTERPOD_RUST_IMPORT_TOOLS")
                .map(PathBuf::from)
                .unwrap_or_default(),
        ] {
            for (name, kind) in [("deno", "deno"), ("qjs", "quickjs")] {
                let path = dir.join(name);
                if path.is_file() {
                    return Some(format!("{kind}:{}", path.display()));
                }
            }
        }
        None
    });
    if let Some(runtime) = runtime {
        command.args(["--js-runtimes", &runtime]);
    }
    command
        .env("HOME", &tools)
        .env("XDG_CACHE_HOME", tools.join("cache"))
        .env("TMPDIR", tools.join("tmp"));
    command
}
pub fn yt_json(context: &Context, data: &Path, args: &[&str]) -> Result<Value, String> {
    let _tools = tools_guard(context)?;
    let mut command = yt_command(data);
    if let Err(error) = youtube_plugin::configure(&mut command, data) {
        eprintln!("[YouTube] media metadata optimization unavailable: {error}");
    }
    command.arg("-J").args(args);
    let output = process::run_ytdlp(context, &mut command, Duration::from_secs(120))?;
    output
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .ok_or_else(|| "yt-dlp returned no metadata".to_string())
        .and_then(|line| serde_json::from_str(line).map_err(|e| e.to_string()))
}

pub(crate) fn youtube_video_info(
    context: &Context,
    data: &Path,
    url: &str,
) -> Result<Value, String> {
    if let Some((info, _)) = youtube_info::load(context, data, url)? {
        return Ok(info);
    }
    youtube_video_info_fresh(context, data, url)
}
fn youtube_video_info_fresh(context: &Context, data: &Path, url: &str) -> Result<Value, String> {
    let info = yt_json(
        context,
        data,
        &[
            "--no-playlist",
            "--no-check-formats",
            "--extractor-args",
            "youtube:skip=hls",
            url,
        ],
    )?;
    if let Err(error) = youtube_info::store(context, data, url, &info) {
        context.check()?;
        eprintln!("[YouTube] metadata cache unavailable: {error}");
    }
    Ok(info)
}
fn youtube_download(
    context: &Context,
    data: &Path,
    url: &str,
    video: bool,
    split: bool,
    cover: Option<&Path>,
) -> Result<Vec<PathBuf>, String> {
    validate_http(url)?;
    if crate::online::youtube_playlist_url(url) {
        if video {
            return Err("Open the playlist and choose an individual video to download".into());
        }
        let parent = yt_json(
            context,
            data,
            &["--flat-playlist", "--playlist-end", "501", url],
        )?;
        let entries = crate::online::youtube_playlist_entries(&parent)?;
        let parent = crate::online::youtube_item(parent, crate::online::Provider::YouTubePlaylists);
        let album = if parent.title.trim().is_empty() {
            "YouTube album"
        } else {
            parent.title.trim().trim_start_matches("Album - ")
        };
        let artist = if parent.artist.trim().is_empty() {
            "Various Artists"
        } else {
            parent.artist.trim()
        };
        let directory = private_dir(data, &["downloads", "YouTube", artist, album])?;
        let mut receipts = youtube_receipts::Album::open(data, &directory)?;
        let playlist = if parent.id.is_empty() {
            url
        } else {
            &parent.id
        };
        let mut completed = Vec::new();
        let total = entries.len();
        for (index, entry) in entries.into_iter().enumerate() {
            context.check()?;
            let item = crate::online::youtube_item(entry, crate::online::Provider::YouTube);
            let receipt = youtube_receipts::key(playlist, index + 1, &item.id);
            if let Some(path) = receipts.completed(&receipt, total, context)? {
                completed.push(path);
                context.progress(0, None, format!("{} of {total} tracks ready", index + 1));
                continue;
            }
            let scope = context.progress_file(format!("Track {} of {total}", index + 1));
            scope.progress(0, None, "Downloading album");
            let identity = YoutubeAlbum {
                directory: &directory,
                album,
                artist,
                track: index + 1,
                total,
            };
            let mut remember = |stage: &Path, target: &Path| {
                receipts.remember(&receipt, total, stage, target, &scope)
            };
            match youtube_download_single(
                &scope,
                data,
                &item.url,
                false,
                false,
                cover,
                Some(&identity),
                Some(&mut remember),
            ) {
                Ok(paths) => completed.extend(paths),
                Err(error) => {
                    return Err(format!(
                        "Stopped after {} of {total} album tracks: {error}",
                        completed.len()
                    ))
                }
            }
        }
        let bytes = completed
            .iter()
            .filter_map(|path| fs::metadata(path).ok())
            .map(|meta| meta.len())
            .sum();
        context.progress(
            bytes,
            Some(bytes),
            format!("Downloaded {total} album tracks"),
        );
        return Ok(completed);
    }
    youtube_download_single(context, data, url, video, split, cover, None, None)
}

struct YoutubeAlbum<'a> {
    directory: &'a Path,
    album: &'a str,
    artist: &'a str,
    track: usize,
    total: usize,
}

struct YoutubeStage(PathBuf);
impl Drop for YoutubeStage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn youtube_stage(context: &Context, data: &Path) -> Result<YoutubeStage, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let parent = private_dir(data, &["tmp", "youtube", &context.id.to_string()])?;
    for _ in 0..32 {
        let path = parent.join(format!(
            "part-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(YoutubeStage(path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Cannot create a private YouTube staging folder".into())
}

fn youtube_download_single(
    context: &Context,
    data: &Path,
    url: &str,
    video: bool,
    split: bool,
    cover: Option<&Path>,
    album: Option<&YoutubeAlbum<'_>>,
    mut before_commit: Option<&mut dyn FnMut(&Path, &Path) -> Result<(), String>>,
) -> Result<Vec<PathBuf>, String> {
    validate_http(url)?;
    let info = youtube_video_info(context, data, url)?;
    if info["entries"].is_array() || text(&info, "_type") == "playlist" {
        return Err(
            "Open the playlist and choose its album download or an individual video".into(),
        );
    }
    let title = text(&info, "title");
    let artist = text(&info, "artist");
    let artist = if artist.is_empty() {
        text(&info, "uploader")
    } else {
        artist
    };
    if video {
        if let Some(saved) = crate::video_catalog::existing_saved(data, &text(&info, "id"), url) {
            return Ok(vec![saved]);
        }
    }
    let dir = if let Some(album) = album {
        album.directory.to_path_buf()
    } else if video {
        private_dir(data, &["videos"])?
    } else {
        private_dir(data, &["downloads", "YouTube", &artist, &title])?
    };
    let chapter_values = info["chapters"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let chapter_total = chapter_values.len();
    let chapter_source = format!(
        "chapters:{}",
        if text(&info, "id").is_empty() {
            url.to_string()
        } else {
            text(&info, "id")
        }
    );
    let chapters = if split && !video {
        chapter_values
            .iter()
            .enumerate()
            .filter_map(|(index, chapter)| {
                let start = number(chapter, "start_time");
                let end = number(chapter, "end_time");
                if end <= start {
                    return None;
                }
                let title = text(chapter, "title");
                let key = youtube_receipts::key(
                    &chapter_source,
                    index + 1,
                    &format!("{}:{}:{title}", start.to_bits(), end.to_bits()),
                );
                Some((index, start, end, title, key))
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if split && !video && chapter_total > 0 && chapters.is_empty() {
        return Err("This video has no valid chapters to split".into());
    }
    let mut chapter_receipts = if !chapters.is_empty() {
        Some(youtube_receipts::Album::open(data, &dir)?)
    } else {
        None
    };
    let mut completed_chapters = std::collections::BTreeMap::new();
    if let Some(receipts) = chapter_receipts.as_ref() {
        for (index, _, _, _, key) in &chapters {
            if let Some(path) = receipts.completed(key, chapter_total, context)? {
                completed_chapters.insert(*index, path);
            }
        }
        if completed_chapters.len() == chapters.len() {
            return Ok(completed_chapters.into_values().collect());
        }
    }
    let staging = youtube_stage(context, data)?;
    let stage = &staging.0;
    let info_path = stage.join("info.json");
    crate::storage::atomic_write_with(&info_path, |file| {
        serde_json::to_writer(file, &info).map_err(std::io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    let info = crate::online::youtube_ui_projection(info);
    let _tools = tools_guard(context)?;
    let mut command = yt_command(data);
    command
        .args([
            "--no-playlist",
            "--newline",
            "--progress",
            "--progress-template",
            "download:BP_PROGRESS|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s",
            "--retries",
            "3",
            "--fragment-retries",
            "3",
            "--socket-timeout",
            "15",
            "--extractor-args",
            "youtube:skip=hls",
            "-o",
        ])
        .arg(stage.join("source.%(ext)s"));
    if video {
        command.args([
            "-f",
            "bv*[height<=360][vcodec^=avc]+ba[ext=m4a]/b[height<=360]/b",
            "--merge-output-format",
            "mp4",
        ]);
    } else {
        command.args([
            "-f",
            "ba[ext=m4a]/ba",
            "--extract-audio",
            "--audio-format",
            "best",
        ]);
    }
    command.arg("--load-info-json").arg(&info_path);
    if let Err(error) = process::run_ytdlp(context, &mut command, Duration::from_secs(7200)) {
        context.check()?;
        if !youtube_info::stale_error(&error) {
            return Err(error);
        }
        youtube_info::invalidate(data, url)?;
        eprintln!("[YouTube] cached media URLs expired; retrying the original video once");
        let args = command
            .get_args()
            .map(|v| v.to_os_string())
            .collect::<Vec<_>>();
        let mut retry = yt_command(data);
        let mut skip = false;
        for arg in args {
            if skip {
                skip = false;
                continue;
            }
            if arg == "--load-info-json" {
                skip = true;
                continue;
            }
            retry.arg(arg);
        }
        retry.arg(url);
        process::run_ytdlp(context, &mut retry, Duration::from_secs(7200))?;
    }
    let source = fs::read_dir(&stage)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|e| matches!(e, "m4a" | "mp4" | "opus" | "webm" | "mkv"))
        })
        .ok_or("YouTube produced no media file")?;
    let mut out = Vec::new();
    if !chapters.is_empty() {
        for (index, start, end, title, key) in &chapters {
            context.check()?;
            if let Some(path) = completed_chapters.remove(index) {
                out.push(path);
                context.progress(
                    0,
                    None,
                    format!("{} of {chapter_total} chapters ready", index + 1),
                );
                continue;
            }
            let extension = source.extension().unwrap_or_default().to_string_lossy();
            let target = youtube_receipts::free_file(
                &dir,
                &format!("{:02} {}.{extension}", index + 1, title),
            )?;
            let prepared = stage.join(format!("chapter-{index}.{extension}"));
            let mut ffmpeg = Command::new(ffmpeg_path(data));
            ffmpeg
                .args(["-nostdin", "-v", "error", "-ss", &start.to_string(), "-i"])
                .arg(&source)
                .args([
                    "-t",
                    &(end - start).to_string(),
                    "-vn",
                    "-c:a",
                    "copy",
                    "-y",
                ])
                .arg(&prepared);
            run_command(context, &mut ffmpeg, Duration::from_secs(300))?;
            tag_download(
                context,
                &prepared,
                &TagEdit {
                    title: Some(title.clone()),
                    artist: Some(artist.clone()),
                    album: Some(text(&info, "title")),
                    track: Some((index + 1) as u32),
                    track_total: Some(chapter_total as u32),
                    cover: cover.map(Path::to_path_buf),
                    ..Default::default()
                },
            )?;
            if let Some(receipts) = chapter_receipts.as_mut() {
                receipts.remember(key, chapter_total, &prepared, &target, context)?;
            }
            youtube_receipts::commit(context, &prepared, &target)?;
            out.push(target);
        }
    } else {
        let extension = source.extension().unwrap_or_default().to_string_lossy();
        let name = album.map_or_else(
            || format!("{title}.{extension}"),
            |album| format!("{:02} {title}.{extension}", album.track),
        );
        let target = youtube_receipts::free_file(&dir, &name)?;
        if !video {
            tag_download(
                context,
                &source,
                &TagEdit {
                    title: Some(title),
                    artist: Some(if artist.is_empty() {
                        album.map_or_else(String::new, |album| album.artist.to_string())
                    } else {
                        artist
                    }),
                    album: Some(
                        album.map_or_else(|| text(&info, "album"), |album| album.album.to_string()),
                    ),
                    album_artist: album.map(|album| album.artist.to_string()),
                    track: album.map(|album| album.track as u32),
                    track_total: album.map(|album| album.total as u32),
                    cover: cover.map(Path::to_path_buf),
                    ..Default::default()
                },
            )?;
        }
        if let Some(remember) = before_commit.as_mut() {
            remember(&source, &target)?;
        }
        youtube_receipts::commit(context, &source, &target)?;
        out.push(target);
    }
    if video {
        for path in &out {
            crate::video_catalog::publish_saved(context, data, &info, url, path)?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod youtube_tests;
fn commit_media(context: &Context, prepared: &Path, target: &Path) -> Result<(), String> {
    context.check()?;
    File::open(prepared)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    let _commit = COMMIT_LOCK.lock().map_err(|e| e.to_string())?;
    if target.exists() {
        return Err("Download destination already exists".into());
    }
    fs::rename(prepared, target).map_err(|e| e.to_string())
}
pub fn ffmpeg_path(data: &Path) -> PathBuf {
    std::env::var_os("BETTERPOD_RUST_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let private = data.join("tools/ffmpeg");
            if private.is_file() {
                private
            } else {
                PathBuf::from("ffmpeg")
            }
        })
}
/// Listen follows the original AAC/M4A preference so SIDX fragments retain
/// whole compressed frames. Full-screen video keeps its independent formats.
pub fn resolve_listen_stream(
    context: &Context,
    data: &Path,
    item: &OnlineItem,
) -> Result<StreamSource, String> {
    let info = yt_json(
        context,
        data,
        &["--no-playlist", "-f", "140/bestaudio[ext=m4a]", &item.url],
    )?;
    let url = text(&info, "url");
    validate_http(&url)?;
    let at = number(&info, "available_at");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs_f64();
    let wait = (at - now + 0.2).max(0.0);
    if wait > 20.0 {
        return Err("YouTube stream is not available yet; try again shortly".into());
    }
    if wait > 0.0 {
        context.sleep(Duration::from_secs_f64(wait))?;
    }
    let headers = info["http_headers"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| {
                    v.as_str()
                        .filter(|v| !v.contains(['\r', '\n']))
                        .map(|v| (k.clone(), v.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(StreamSource {
        video_url: url,
        audio_url: None,
        headers,
        duration: number(&info, "duration"),
        title: text(&info, "title"),
        available_at: (at > 0.0).then_some(at),
        chapters: 0,
    })
}
pub fn resolve_stream(
    context: &Context,
    data: &Path,
    item: &OnlineItem,
    video: bool,
) -> Result<StreamSource, String> {
    let opened = Instant::now();
    let mut last = "No playable streams".to_string();
    for attempt in 0..3 {
        context.check()?;
        context.progress(attempt, None, "Opening YouTube stream");
        // Opening details already resolved the formats. Probe those URLs first;
        // only rejected/expired formats require another expensive extraction.
        let info = if attempt == 0 {
            youtube_video_info(context, data, &item.url)?
        } else {
            youtube_video_info_fresh(context, data, &item.url)?
        };
        eprintln!(
            "[Video startup] metadata ready: {} ms",
            opened.elapsed().as_millis()
        );
        let (formats, audio) = match select_formats(&info, video) {
            Ok(selected) => selected,
            Err(error) => {
                last = error;
                continue;
            }
        };
        let at = [
            number(&formats, "available_at"),
            audio
                .as_ref()
                .map(|f| number(f, "available_at"))
                .unwrap_or(0.0),
        ]
        .into_iter()
        .fold(0.0, f64::max);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs_f64();
        let wait = (at - now + 0.2).max(0.0);
        if wait > 20.0 {
            return Err("YouTube stream is not available yet; try again shortly".into());
        }
        if wait > 0.0 {
            context.progress(
                0,
                None,
                format!("Waiting for YouTube · {:.0} s", wait.ceil()),
            );
            context.sleep(Duration::from_secs_f64(wait))?;
        }
        let video_url = text(&formats, "url");
        let audio_url = audio.as_ref().map(|f| text(f, "url"));
        let headers = info["http_headers"]
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| {
                        v.as_str()
                            .filter(|v| !v.contains(['\r', '\n']))
                            .map(|v| (k.clone(), v.into()))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut valid = true;
        for (index, url) in std::iter::once(video_url.as_str())
            .chain(audio_url.as_deref())
            .enumerate()
        {
            let probe_started = Instant::now();
            validate_http(url)?;
            let mut request = http_agent(true).get(url).header("Range", "bytes=0-0");
            for (k, v) in &headers {
                request = request.header(k, v);
            }
            if let Err(error) = request.call() {
                last = error.to_string();
                valid = false;
                break;
            }
            eprintln!(
                "[Video startup] probe {index}: {} ms",
                probe_started.elapsed().as_millis()
            );
        }
        eprintln!(
            "[Video startup] probes ready: {} ms",
            opened.elapsed().as_millis()
        );
        if valid {
            return Ok(StreamSource {
                video_url,
                audio_url,
                headers,
                duration: number(&info, "duration"),
                title: text(&info, "title"),
                available_at: (at > 0.0).then_some(at),
                chapters: info["chapters"].as_array().map_or(0, Vec::len),
            });
        }
        context.sleep(Duration::from_millis(300))?;
    }
    Err(format!("YouTube stream could not be opened: {last}"))
}
fn select_formats(info: &Value, video: bool) -> Result<(Value, Option<Value>), String> {
    let formats = info["formats"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let http = |v: &&Value| {
        let url = text(v, "url");
        let protocol = text(v, "protocol");
        url.starts_with("http") && !protocol.contains("m3u8") && !protocol.contains("dash")
    };
    let audio = formats
        .iter()
        .filter(http)
        .filter(|f| text(f, "vcodec") == "none" && text(f, "acodec") != "none")
        .max_by(|a, b| number(a, "abr").total_cmp(&number(b, "abr")))
        .cloned();
    if !video {
        return audio
            .map(|a| (a, None))
            .ok_or("No direct audio format available".into());
    }
    let selected = formats
        .iter()
        .filter(http)
        .filter(|f| text(f, "vcodec") != "none" && number(f, "height") <= 480.0)
        .max_by_key(|f| {
            let codec = text(f, "vcodec");
            (
                (codec.starts_with("avc") || codec.starts_with("h264")) as u8,
                (number(f, "height") <= 360.0) as u8,
                number(f, "height") as u32,
                number(f, "tbr") as u64,
            )
        })
        .cloned()
        .ok_or("No compatible video format available")?;
    if text(&selected, "acodec") == "none" && audio.is_none() {
        return Err("Video has no matching audio stream".into());
    }
    let separate = (text(&selected, "acodec") == "none")
        .then_some(audio)
        .flatten();
    Ok((selected, separate))
}
#[derive(Clone, Debug, Default)]
pub struct ToolStatus {
    pub ready: bool,
    pub version: String,
    pub runtime: String,
}
pub fn tool_status(context: &Context, data: &Path) -> Result<ToolStatus, String> {
    let _lock = tools_guard(context)?;
    context.check()?;
    let path = ytdlp_path(data);
    if !path.is_file() {
        return Ok(ToolStatus::default());
    }
    let version = run_command(
        context,
        Command::new(&path).arg("--version"),
        Duration::from_secs(15),
    )?
    .lines()
    .next()
    .unwrap_or("")
    .trim()
    .to_owned();
    let mut runtime = String::new();
    if let Ok(config) = std::env::var("BETTERPOD_RUST_YTJS") {
        if !config.trim().is_empty() {
            runtime = config.split(':').next().unwrap_or("JavaScript").into();
        }
    }
    if runtime.is_empty() {
        for dir in [
            Some(data.join("tools")),
            std::env::var_os("BETTERPOD_RUST_IMPORT_TOOLS").map(PathBuf::from),
        ]
        .into_iter()
        .flatten()
        {
            for (name, label) in [("deno", "Deno"), ("qjs", "QuickJS")] {
                if dir.join(name).is_file() {
                    runtime = label.into();
                    break;
                }
            }
            if !runtime.is_empty() {
                break;
            }
        }
    }
    Ok(ToolStatus {
        ready: !version.is_empty() && !runtime.is_empty(),
        version,
        runtime,
    })
}
pub fn install_tools(context: &Context, data: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let _lock = tools_guard(context)?;
    let tools = private_dir(data, &["tools"])?;
    let staging = private_dir(
        data,
        &[
            "tmp",
            &format!("tools-install-{}-{}", std::process::id(), context.id),
        ],
    )?;
    let result = (|| {
        let linux = cfg!(target_os = "linux");
        let asset_name = if linux {
            "yt-dlp_linux_aarch64.zip"
        } else {
            "yt-dlp_macos"
        };
        let yt = verified_asset(context, "yt-dlp/yt-dlp", asset_name, &staging)?;
        let yt_dir = staging.join("ytdlp");
        fs::create_dir(&yt_dir).map_err(|e| e.to_string())?;
        let binary = if linux {
            extract_zip(&yt, &yt_dir)?;
            yt_dir.join("yt-dlp_linux_aarch64")
        } else {
            let binary = yt_dir.join("yt-dlp");
            fs::rename(&yt, &binary).map_err(|e| e.to_string())?;
            binary
        };
        let _ = fs::set_permissions(&binary, fs::Permissions::from_mode(0o755));
        run_command(
            context,
            Command::new(&binary).arg("--version"),
            Duration::from_secs(30),
        )?;
        let deno_name = if linux {
            "deno-aarch64-unknown-linux-gnu.zip"
        } else {
            "deno-aarch64-apple-darwin.zip"
        };
        let deno = (|| {
            let archive = verified_asset(context, "denoland/deno", deno_name, &staging)?;
            let runtime_dir = staging.join("runtime");
            fs::create_dir(&runtime_dir).map_err(|e| e.to_string())?;
            extract_zip(&archive, &runtime_dir)?;
            let binary = runtime_dir.join("deno");
            let _ = fs::set_permissions(&binary, fs::Permissions::from_mode(0o755));
            run_command(
                context,
                Command::new(&binary).arg("--version"),
                Duration::from_secs(30),
            )?;
            Ok::<_, String>(binary)
        })();
        let (runtime, name) = match deno {
            Ok(binary) => (binary, "deno"),
            Err(error) if linux => {
                context.check()?;
                context.progress(
                    0,
                    None,
                    format!("Deno unavailable ({error}); getting QuickJS"),
                );
                let binary =
                    verified_asset(context, "quickjs-ng/quickjs", "qjs-linux-aarch64", &staging)?;
                let _ = fs::set_permissions(&binary, fs::Permissions::from_mode(0o755));
                run_command(
                    context,
                    Command::new(&binary).arg("--version"),
                    Duration::from_secs(30),
                )?;
                if tools.join("deno").exists()
                    && run_command(
                        context,
                        Command::new(tools.join("deno")).arg("--version"),
                        Duration::from_secs(10),
                    )
                    .is_err()
                {
                    let _ = fs::rename(tools.join("deno"), tools.join("deno.disabled"));
                }
                (binary, "qjs")
            }
            Err(error) => return Err(error),
        };
        context.check()?;
        let previous = tools.join(format!(".ytdlp-previous-{}", std::process::id()));
        if previous.exists() {
            fs::remove_dir_all(&previous).map_err(|e| e.to_string())?;
        }
        let target = tools.join("ytdlp");
        if target.exists() {
            fs::rename(&target, &previous).map_err(|e| e.to_string())?;
        }
        if let Err(error) = fs::rename(&yt_dir, &target) {
            if previous.exists() {
                let _ = fs::rename(&previous, &target);
            }
            return Err(error.to_string());
        }
        fs::rename(runtime, tools.join(name)).map_err(|e| e.to_string())?;
        if previous.exists() {
            let _ = fs::remove_dir_all(previous);
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(staging);
    result
}
/// Remove only recognized tools from this Rust application's owned data.
/// Imported stable tools and environment overrides are never removal targets.
pub fn remove_tools(context: &Context, data: &Path) -> Result<(), String> {
    let _lock = tools_guard(context)?;
    context.check()?;
    let marker = data.join(".betterpod-rust.json");
    let metadata = fs::symlink_metadata(&marker).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4096 {
        return Err("Tools removal needs a private Rust database".into());
    }
    let owner: Value = serde_json::from_slice(&fs::read(marker).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if owner["application"] != "betterpod-rust" {
        return Err("Tools belong to another application".into());
    }
    let root = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let tools = root.join("tools");
    match fs::symlink_metadata(&tools) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
            return Err("Tools folder is not private".into())
        }
        _ => {}
    }
    // Validate the entire allowlisted tree before any deletion; no symlink is followed.
    let names = [
        "ytdlp",
        "yt-dlp",
        "yt-dlp_macos",
        "deno",
        "deno.disabled",
        "qjs",
        "ffmpeg",
        "cache",
        "tmp",
    ];
    let mut targets = Vec::new();
    for name in names {
        let path = tools.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => Some(m),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        if let Some(m) = metadata {
            if m.file_type().is_symlink() {
                return Err("Tools contain a symlink".into());
            }
            let mut pending = vec![path.clone()];
            while let Some(p) = pending.pop() {
                let m = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
                if m.file_type().is_symlink() {
                    return Err("Tools contain a symlink".into());
                }
                if m.is_dir() {
                    for entry in fs::read_dir(p).map_err(|e| e.to_string())? {
                        pending.push(entry.map_err(|e| e.to_string())?.path());
                    }
                } else if !m.is_file() {
                    return Err("Tools contain a special file".into());
                }
            }
            targets.push(path);
        }
    }
    context.check()?;
    for path in targets {
        if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        }
        .map_err(|e| e.to_string())?;
        crate::storage::sync_directory(&tools).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn verified_asset(
    context: &Context,
    repository: &str,
    name: &str,
    staging: &Path,
) -> Result<PathBuf, String> {
    let release = context.get_json(&format!(
        "https://api.github.com/repos/{repository}/releases/latest"
    ))?;
    let asset = release["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|a| text(a, "name") == name))
        .ok_or_else(|| format!("{repository} has no compatible {name} asset"))?;
    let digest = text(asset, "digest");
    let checksum = digest
        .strip_prefix("sha256:")
        .ok_or("Release asset has no SHA-256 digest")?;
    let target = staging.join(name);
    download(
        context,
        &text(asset, "browser_download_url"),
        &target,
        Some(checksum),
    )?;
    Ok(target)
}

fn extract_zip(archive: &Path, destination: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(File::open(archive).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let name = entry.enclosed_name().ok_or("Unsafe archive path")?;
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err("Tool archive contains a symlink".into());
        }
        total = total.saturating_add(entry.size());
        if total > 1024 * 1024 * 1024 {
            return Err("Tool archive is too large".into());
        }
        let target = destination.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waiting_for_youtube_tools_can_be_cancelled() {
        let guard = TOOLS.lock().unwrap();
        let context = Context::standalone();
        let worker_context = context.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            tx.send(tools_guard(&worker_context).map(|_| ())).unwrap();
        });
        thread::sleep(Duration::from_millis(30));
        context.cancel();
        let result = rx.recv_timeout(Duration::from_millis(500));
        drop(guard);
        worker.join().unwrap();
        assert_eq!(result.unwrap(), Err("Cancelled".into()));
    }
    #[test]
    fn removing_tools_preserves_music_unknown_files_and_symlink_targets() {
        let root =
            std::env::temp_dir().join(format!("betterpod-tools-remove-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("tools/ytdlp")).unwrap();
        fs::write(
            root.join(".betterpod-rust.json"),
            br#"{"application":"betterpod-rust"}"#,
        )
        .unwrap();
        fs::write(root.join("tools/ytdlp/bin"), b"fixture").unwrap();
        fs::write(root.join("tools/keep-user-file"), b"keep").unwrap();
        fs::write(root.join("song.flac"), b"music").unwrap();
        remove_tools(&Context::standalone(), &root).unwrap();
        assert!(!root.join("tools/ytdlp").exists());
        assert_eq!(
            fs::read(root.join("tools/keep-user-file")).unwrap(),
            b"keep"
        );
        assert_eq!(fs::read(root.join("song.flac")).unwrap(), b"music");
        #[cfg(unix)]
        {
            fs::create_dir_all(root.join("tools/ytdlp")).unwrap();
            std::os::unix::fs::symlink(root.join("song.flac"), root.join("tools/ytdlp/link"))
                .unwrap();
            assert!(remove_tools(&Context::standalone(), &root).is_err());
            assert!(root.join("tools/ytdlp").exists());
            assert!(root.join("song.flac").exists());
        }
        fs::write(
            root.join(".betterpod-rust.json"),
            br#"{"application":"betterpod-stable"}"#,
        )
        .unwrap();
        assert!(remove_tools(&Context::standalone(), &root).is_err());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn range_validation() {
        assert_eq!(content_range("bytes 42-99/100"), Some((42, 100)));
        assert_eq!(content_range("bytes 100-90/100"), None);
        assert_eq!(content_range("bytes 1-2/*"), None);
    }
    #[test]
    fn filenames_and_formats() {
        assert!(!safe_name("../a/b\\c").contains('/'));
        let info = json!({"formats":[{"url":"https://example.invalid/a","protocol":"https","vcodec":"none","acodec":"mp4a","abr":128},{"url":"https://example.invalid/v","protocol":"https","vcodec":"avc1","acodec":"none","height":360}]});
        let (v, a) = select_formats(&info, true).unwrap();
        assert_eq!(text(&v, "url"), "https://example.invalid/v");
        assert!(a.is_some());
    }

    #[test]
    fn resumed_download_verifies_identity_and_commits_only_complete_file() {
        use std::net::TcpListener;
        let root =
            std::env::temp_dir().join(format!("betterpod-download-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let target = root.join("song.bin");
        let stage = part_name(&target);
        fs::write(&stage, b"abc").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/song", listener.local_addr().unwrap());
        fs::write(
            stage.with_extension("bin.json"),
            serde_json::to_vec(&PartInfo {
                url: url.clone(),
                etag: "\"version1\"".into(),
                modified: String::new(),
                total: 6,
            })
            .unwrap(),
        )
        .unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.contains("range: bytes=3-"));
            assert!(request.contains("if-range: \"version1\""));
            socket.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/6\r\nETag: \"version1\"\r\nConnection: close\r\n\r\ndef").unwrap();
        });
        download(&Context::standalone(), &url, &target, None).unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"abcdef");
        assert!(!stage.exists());
        assert!(!stage.with_extension("bin.json").exists());
        assert!(!strong_etag("W/\"version1\""));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn html_response_cannot_become_a_completed_audio_download() {
        use std::net::TcpListener;
        let root =
            std::env::temp_dir().join(format!("betterpod-download-html-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("song.mp3");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/song", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0; 4096];
            let _ = socket.read(&mut bytes);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n<html></html>").unwrap();
        });
        assert!(download(&Context::standalone(), &url, &target, None).is_err());
        server.join().unwrap();
        assert!(!target.exists());
        assert!(!part_name(&target).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn podcast_uses_real_audio_container_when_feed_mime_is_wrong() {
        use std::net::TcpListener;
        let root =
            std::env::temp_dir().join(format!("betterpod-podcast-sniff-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let requested = root.join("Episode.m4a");
        let mut wave = Vec::new();
        let pcm = vec![0u8; 1600];
        wave.extend(b"RIFF");
        wave.extend((36u32 + pcm.len() as u32).to_le_bytes());
        wave.extend(b"WAVEfmt ");
        wave.extend(16u32.to_le_bytes());
        wave.extend(1u16.to_le_bytes());
        wave.extend(1u16.to_le_bytes());
        wave.extend(8000u32.to_le_bytes());
        wave.extend(16000u32.to_le_bytes());
        wave.extend(2u16.to_le_bytes());
        wave.extend(16u16.to_le_bytes());
        wave.extend(b"data");
        wave.extend((pcm.len() as u32).to_le_bytes());
        wave.extend(pcm);
        let body = wave.clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/mislabelled", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request);
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: audio/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).unwrap();
            socket.write_all(&body).unwrap();
        });
        let actual = download_episode(
            &Context::standalone(),
            &url,
            &requested,
            &TagEdit::default(),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(actual.extension().unwrap(), "wav");
        assert_eq!(fs::read(actual).unwrap(), wave);
        assert!(!requested.exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn square_crop_anchors_preserve_the_chosen_side_and_source_image() {
        let data = std::env::temp_dir().join(format!("betterpod-crop-{}", std::process::id()));
        fs::create_dir_all(&data).unwrap();
        let source = data.join("wide.png");
        let image = image::RgbImage::from_fn(80, 40, |x, _| {
            if x < 40 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        image.save(&source).unwrap();
        let original = fs::read(&source).unwrap();
        let context = Context::standalone();
        let left = prepare_square_cover(&context, &data, &source, SquareCrop::Start).unwrap();
        let right = prepare_square_cover(&context, &data, &source, SquareCrop::End).unwrap();
        let left = image::open(left).unwrap().to_rgb8();
        let right = image::open(right).unwrap().to_rgb8();
        assert_eq!(left.width(), left.height());
        assert!(left.get_pixel(20, 20)[0] > 230);
        assert!(right.get_pixel(20, 20)[2] > 230);
        assert_eq!(fs::read(source).unwrap(), original);
        fs::remove_dir_all(data).unwrap();
    }
}
