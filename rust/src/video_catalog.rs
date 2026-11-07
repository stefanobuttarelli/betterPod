//! Saved videos and watch history. All filesystem/probe/download work belongs
//! to the Services worker; callers receive complete immutable menu snapshots.
use crate::{
    downloads,
    online::{Context, OnlineItem, Provider},
    storage::{atomic_write, sync_directory},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Cursor, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Component, Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
static HISTORY: Mutex<()> = Mutex::new(());
const FFMPEG_URL: &str =
    "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-linux-arm64.gz";
const FFMPEG_SHA: &str = "754a678672298bc68156adff58aa7385a592c2b30b1d0ae8750c45c915c4bac0";
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub saved: Vec<OnlineItem>,
    pub recent: Vec<OnlineItem>,
    pub ffmpeg_ready: bool,
    pub ffmpeg_error: String,
    pub can_install_ffmpeg: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    application: String,
    id: String,
    url: String,
    title: String,
    channel: String,
    duration: f64,
    width: u32,
    height: u32,
    fps: f64,
    size: u64,
    at: u64,
    path: PathBuf,
    cover: Option<PathBuf>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Seen {
    id: String,
    url: String,
    title: String,
    channel: String,
    duration: f64,
    position: f64,
    at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_path: Option<PathBuf>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|t| t.as_secs())
        .unwrap_or(0)
}
fn string(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_owned()
}
fn finite(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.)
    } else {
        0.
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn video_file(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "mp4" | "mkv" | "webm" | "mov" | "avi"
        )
    })
}
fn relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        Err("Unsafe private video path".into())
    } else {
        Ok(())
    }
}
fn within_video_root(path: &Path) -> bool {
    path.starts_with("videos") || path.starts_with(Path::new("downloads").join("Videos"))
}
fn private_file(data: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let rel = if path.is_absolute() {
        path.strip_prefix(&root)
            .map_err(|_| "Video is outside private data")?
    } else {
        path
    };
    relative(rel)?;
    if !within_video_root(rel) {
        return Err("File is outside private video folders".into());
    }
    let mut current = root.clone();
    for component in rel.components() {
        current.push(component.as_os_str());
        let meta = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            return Err("Private video paths cannot contain links".into());
        }
    }
    if !fs::symlink_metadata(&current)
        .map_err(|e| e.to_string())?
        .is_file()
    {
        return Err("Video is not a regular file".into());
    }
    Ok(current)
}
fn read_small(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > limit {
        return Err("Video record exceeds its size limit".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Video record exceeds its size limit".into());
    }
    Ok(bytes)
}
fn key(item: &OnlineItem) -> String {
    if valid_id(&item.id) {
        return item.id.clone();
    }
    if let Some(id) = item
        .url
        .split("v=")
        .nth(1)
        .and_then(|s| s.split(['&', '#']).next())
        .filter(|s| valid_id(s))
    {
        return id.into();
    }
    if item.url.contains("youtu.be/") {
        if let Some(id) = item
            .url
            .split("youtu.be/")
            .nth(1)
            .and_then(|s| s.split(['?', '#', '/']).next())
            .filter(|s| valid_id(s))
        {
            return id.into();
        }
    }
    format!("{:x}", Sha256::digest(item.url.as_bytes()))
}
fn history(data: &Path) -> Vec<Seen> {
    read_small(&data.join("video-history.json"), 1024 * 1024)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Seen>>(&b).ok())
        .unwrap_or_default()
        .into_iter()
        .take(50)
        .map(|mut s| {
            if s.id.is_empty() {
                let mut item = OnlineItem::new(Provider::YouTube);
                item.url = s.url.clone();
                s.id = key(&item);
            }
            s
        })
        .collect()
}
fn save_history(data: &Path, seen: &[Seen]) -> Result<(), String> {
    atomic_write(
        &data.join("video-history.json"),
        &serde_json::to_vec(seen).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn remember(data: &Path, item: &OnlineItem, position: f64) -> Result<(), String> {
    let canonical = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let data = canonical.as_path();
    let _guard = HISTORY.lock().map_err(|e| e.to_string())?;
    let duration = finite(item.seconds);
    let mut position = finite(position);
    if position < 10. || duration > 0. && position > duration - 15. {
        position = 0.;
    }
    position = position.round();
    let id = key(item);
    let local = item.raw["local_path"]
        .as_str()
        .map(PathBuf::from)
        .map(|path| private_file(data, &path))
        .transpose()?
        .map(|path| path.strip_prefix(data).unwrap_or(&path).to_path_buf());
    if local.is_none() {
        crate::online::validate_http(&item.url)?;
    }
    let mut seen = history(data);
    seen.retain(|old| old.id != id);
    seen.insert(
        0,
        Seen {
            id,
            url: item.url.clone(),
            title: item.title.clone(),
            channel: item.artist.clone(),
            duration,
            position,
            at: now(),
            local_path: local,
        },
    );
    seen.truncate(50);
    save_history(data, &seen)
}
pub fn forget(data: &Path, id: &str) -> Result<(), String> {
    let canonical = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let data = canonical.as_path();
    let _guard = HISTORY.lock().map_err(|e| e.to_string())?;
    let mut seen = history(data);
    seen.retain(|s| s.id != id && s.url != id);
    save_history(data, &seen)
}
fn read_record(data: &Path, path: &Path) -> Option<Saved> {
    let mut saved: Saved = serde_json::from_slice(&read_small(path, 64 * 1024).ok()?).ok()?;
    if !valid_id(&saved.id) {
        return None;
    }
    if !saved.application.is_empty() && saved.application != "betterpod-rust" {
        return None;
    }
    if saved.path.as_os_str().is_empty() {
        saved.path = PathBuf::from("videos").join(format!("{}.mp4", saved.id));
    }
    let video = private_file(data, &saved.path).ok()?;
    if !video_file(&video) {
        return None;
    }
    if saved.url.is_empty() {
        saved.url = format!("https://www.youtube.com/watch?v={}", saved.id);
    }
    saved.size = fs::metadata(&video).ok()?.len();
    saved.path = video;
    saved.cover = private_file(
        data,
        &PathBuf::from("videos").join(format!("{}.jpg", saved.id)),
    )
    .ok();
    Some(saved)
}
fn saved_item(saved: &Saved, seen: &[Seen]) -> OnlineItem {
    let mut item = OnlineItem::new(Provider::YouTube);
    item.id = saved.id.clone();
    item.url = saved.url.clone();
    item.title = saved.title.clone();
    item.artist = saved.channel.clone();
    item.seconds = saved.duration;
    item.kind = "video".into();
    let position = seen
        .iter()
        .find(|s| s.id == saved.id || s.url == saved.url)
        .map_or(0., |s| s.position);
    item.raw = json!({"local_path":saved.path,"cover_path":saved.cover,"position":position,"size":saved.size,"at":saved.at,"width":saved.width,"height":saved.height,"fps":saved.fps});
    item
}
fn saved(data: &Path, ctx: &Context) -> Result<Vec<Saved>, String> {
    let mut out = Vec::new();
    let mut known = HashSet::new();
    let directory = data.join("videos");
    if let Ok(entries) = fs::read_dir(&directory) {
        if fs::symlink_metadata(&directory).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Private video folder is a link".into());
        }
        for entry in entries {
            ctx.check()?;
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.path().extension().is_some_and(|s| s == "json") {
                if let Some(record) = read_record(data, &entry.path()) {
                    known.insert(record.path.clone());
                    out.push(record);
                }
            }
        }
    }
    for root in [data.join("videos"), data.join("downloads/Videos")] {
        let mut pending = vec![root];
        while let Some(path) = pending.pop() {
            ctx.check()?;
            let meta = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.to_string()),
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                for child in fs::read_dir(&path).map_err(|e| e.to_string())? {
                    pending.push(child.map_err(|e| e.to_string())?.path());
                }
            } else if meta.is_file() && video_file(&path) {
                let path = private_file(data, &path)?;
                if known.insert(path.clone()) {
                    let rel = path.strip_prefix(data).map_err(|e| e.to_string())?;
                    out.push(Saved {
                        application: "betterpod-rust".into(),
                        id: format!("{:x}", Sha256::digest(rel.as_os_str().as_bytes())),
                        url: format!("local:{}", rel.to_string_lossy()),
                        title: path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        size: meta.len(),
                        at: meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map_or(0, |d| d.as_secs()),
                        path,
                        ..Default::default()
                    });
                }
            }
            if out.len() > 10000 || pending.len() > 100000 {
                return Err("Too many private video files".into());
            }
        }
    }
    out.sort_by(|a, b| {
        b.at.cmp(&a.at)
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    Ok(out)
}
pub fn catalog(ctx: &Context, data: &Path) -> Result<Snapshot, String> {
    let canonical = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let data = canonical.as_path();
    let _guard = HISTORY.lock().map_err(|e| e.to_string())?;
    let seen = history(data);
    let saved = saved(data, ctx)?;
    let saved_items = saved
        .iter()
        .map(|s| saved_item(s, &seen))
        .collect::<Vec<_>>();
    let recent = seen
        .into_iter()
        .map(|s| {
            if let Some(saved) = saved
                .iter()
                .find(|saved| saved.id == s.id || saved.url == s.url)
            {
                let mut item = saved_item(saved, &[]);
                item.raw["position"] = json!(s.position);
                return item;
            }
            let mut item = OnlineItem::new(Provider::YouTube);
            item.id = s.id;
            item.url = s.url;
            item.title = s.title;
            item.artist = s.channel;
            item.seconds = s.duration;
            item.kind = "video".into();
            if valid_id(&item.id)
                && (item.url.contains("youtube.com/") || item.url.contains("youtu.be/"))
            {
                item.cover = format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", item.id);
            }
            item.raw = json!({"position":s.position,"at":s.at});
            if let Some(path) = s.local_path.and_then(|path| private_file(data, &path).ok()) {
                item.raw["local_path"] = json!(path);
            }
            item
        })
        .collect();
    let (ffmpeg_ready, ffmpeg_error) = runtime(ctx, data);
    Ok(Snapshot {
        saved: saved_items,
        recent,
        ffmpeg_ready,
        ffmpeg_error,
        can_install_ffmpeg: cfg!(all(target_os = "linux", target_arch = "aarch64")),
    })
}
fn runtime(ctx: &Context, data: &Path) -> (bool, String) {
    let path = downloads::ffmpeg_path(data);
    let mut command = Command::new(path);
    command.args(["-hide_banner", "-decoders"]);
    match downloads::run_command(ctx, &mut command, Duration::from_secs(5)) {
        Ok(output) => {
            let missing = ["h264", "aac", "pcm_s16le"].into_iter().find(|codec| {
                !output
                    .lines()
                    .any(|line| line.split_whitespace().nth(1) == Some(codec))
            });
            missing
                .map(|codec| (false, format!("This system's has no {codec}")))
                .unwrap_or((true, String::new()))
        }
        Err(error) => (false, error),
    }
}
pub fn install_ffmpeg(ctx: &Context, data: &Path) -> Result<(), String> {
    if !cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        return Err("Automatic ffmpeg installation is for the Anbernic ARM64 build".into());
    }
    let _guard = downloads::TOOLS.lock().map_err(|e| e.to_string())?;
    ctx.check()?;
    ctx.progress(0, None, "Downloading ffmpeg…");
    let gzip = ctx.get_bytes(FFMPEG_URL, 64 * 1024 * 1024)?;
    if format!("{:x}", Sha256::digest(&gzip)) != FFMPEG_SHA {
        return Err("The ffmpeg file doesn't match its published checksum".into());
    }
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(Cursor::new(gzip))
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 128 * 1024 * 1024
        || bytes.len() < 20
        || &bytes[..4] != b"\x7fELF"
        || bytes[4] != 2
        || bytes[5] != 1
        || u16::from_le_bytes([bytes[18], bytes[19]]) != 183
    {
        return Err("ffmpeg is not an ARM64 executable".into());
    }
    let staging = downloads::private_dir(data, &["tmp", "ffmpeg", &ctx.id.to_string()])?;
    let candidate = staging.join("ffmpeg");
    atomic_write(&candidate, &bytes).map_err(|e| e.to_string())?;
    fs::set_permissions(&candidate, fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let mut command = Command::new(&candidate);
        command.args(["-hide_banner", "-version"]);
        downloads::run_command(ctx, &mut command, Duration::from_secs(10))?;
        let tools = downloads::private_dir(data, &["tools"])?;
        ctx.check()?;
        atomic_write(&tools.join("ffmpeg"), &bytes).map_err(|e| e.to_string())?;
        fs::set_permissions(tools.join("ffmpeg"), fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
        File::open(tools.join("ffmpeg"))
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        sync_directory(&tools).map_err(|e| e.to_string())
    })();
    let _ = fs::remove_dir_all(staging);
    result
}
pub fn existing_saved(data: &Path, id: &str, url: &str) -> Option<PathBuf> {
    if !valid_id(id) {
        return None;
    }
    let root = fs::canonicalize(data).ok()?;
    let record = read_record(&root, &root.join("videos").join(format!("{id}.json")))?;
    (record.url == url).then_some(record.path)
}
pub fn publish_saved(
    ctx: &Context,
    data: &Path,
    info: &Value,
    url: &str,
    path: &Path,
) -> Result<(), String> {
    let canonical = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let data = canonical.as_path();
    let path = private_file(data, path)?;
    let dir = downloads::private_dir(data, &["videos"])?;
    let id = string(info, "id");
    let id = if valid_id(&id) {
        id
    } else {
        format!("{:x}", Sha256::digest(url.as_bytes()))
    };
    let mut cover = None;
    let thumbnail = string(info, "thumbnail");
    if !thumbnail.is_empty() {
        if let Ok(bytes) = ctx.get_bytes(&thumbnail, 4 * 1024 * 1024) {
            let mut reader = image::ImageReader::new(Cursor::new(bytes))
                .with_guessed_format()
                .map_err(|e| e.to_string())?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(4096);
            limits.max_image_height = Some(4096);
            limits.max_alloc = Some(64 * 1024 * 1024);
            reader.limits(limits);
            if let Ok(image) = reader.decode() {
                let mut jpeg = Cursor::new(Vec::new());
                image
                    .thumbnail(640, 640)
                    .write_to(&mut jpeg, image::ImageFormat::Jpeg)
                    .map_err(|e| e.to_string())?;
                let target = dir.join(format!("{id}.jpg"));
                atomic_write(&target, &jpeg.into_inner()).map_err(|e| e.to_string())?;
                cover = Some(
                    target
                        .strip_prefix(data)
                        .map_err(|e| e.to_string())?
                        .to_path_buf(),
                );
            }
        }
    }
    let saved = Saved {
        application: "betterpod-rust".into(),
        id: id.clone(),
        url: url.into(),
        title: string(info, "title"),
        channel: info["channel"]
            .as_str()
            .or_else(|| info["uploader"].as_str())
            .unwrap_or("")
            .into(),
        duration: finite(info["duration"].as_f64().unwrap_or(0.)),
        width: info["width"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
        height: info["height"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
        fps: finite(info["fps"].as_f64().unwrap_or(0.)),
        size: fs::metadata(&path).map_err(|e| e.to_string())?.len(),
        at: now(),
        path: path
            .strip_prefix(data)
            .map_err(|e| e.to_string())?
            .to_path_buf(),
        cover,
    };
    ctx.check()?;
    atomic_write(
        &dir.join(format!("{id}.json")),
        &serde_json::to_vec(&saved).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn delete_saved(ctx: &Context, data: &Path, path: &Path) -> Result<(), String> {
    let _guard = HISTORY.lock().map_err(|e| e.to_string())?;
    let root = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let owner: Value =
        serde_json::from_slice(&read_small(&root.join(".betterpod-rust.json"), 4096)?)
            .map_err(|e| e.to_string())?;
    if owner["application"] != "betterpod-rust" {
        return Err("Video deletion needs a private Rust database".into());
    }
    let target = private_file(&root, path)?;
    let records = saved(&root, ctx)?;
    let saved = records
        .into_iter()
        .find(|record| record.path == target)
        .ok_or("Saved video is no longer available")?;
    let mut paths = vec![target];
    let record = root.join("videos").join(format!("{}.json", saved.id));
    if record.exists() {
        paths.push(private_file(&root, &record)?);
    }
    if let Some(cover) = saved.cover {
        paths.push(private_file(&root, &cover)?);
    }
    paths.sort();
    paths.dedup();
    for path in &paths {
        ctx.check()?;
        private_file(&root, path)?;
    }
    for path in paths {
        ctx.check()?;
        unlink_private(&root, path.strip_prefix(&root).map_err(|e| e.to_string())?)?;
    }
    Ok(())
}
fn unlink_private(root: &Path, path: &Path) -> Result<(), String> {
    relative(path)?;
    let mut parts = path
        .components()
        .map(|part| part.as_os_str())
        .collect::<Vec<_>>();
    let name = parts.pop().ok_or("Missing private video filename")?;
    let mut parent = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(|e| e.to_string())?;
    for part in parts {
        let part = std::ffi::CString::new(part.as_bytes()).map_err(|e| e.to_string())?;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                part.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        parent = unsafe { File::from_raw_fd(fd) };
    }
    let name = std::ffi::CString::new(name.as_bytes()).map_err(|e| e.to_string())?;
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    crate::storage::sync_open_directory(&parent).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{Event, Output, Services, Task};
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::Instant,
    };
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rust-video-catalog-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(path.join("videos")).unwrap();
            atomic_write(
                &path.join(".betterpod-rust.json"),
                br#"{"application":"betterpod-rust"}"#,
            )
            .unwrap();
            Self(fs::canonicalize(path).unwrap())
        }
        fn record(&self, id: &str, at: u64) -> PathBuf {
            let path = self.0.join("videos").join(format!("{id}.mp4"));
            fs::write(&path, b"private synthetic video fixture").unwrap();
            let record = Saved {
                application: "betterpod-rust".into(),
                id: id.into(),
                url: format!("https://www.youtube.com/watch?v={id}"),
                title: format!("Film {id}"),
                channel: "Fixture author".into(),
                duration: 245.,
                at,
                path: PathBuf::from("videos").join(format!("{id}.mp4")),
                ..Default::default()
            };
            atomic_write(
                &self.0.join("videos").join(format!("{id}.json")),
                &serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn run(services: &mut Services, task: Task) -> Result<Output, String> {
        let id = services.submit(task).unwrap().id;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(Event::Finished { id: found, result }) = services.poll() {
                if id == found {
                    return result;
                }
            }
            assert!(Instant::now() < deadline, "video catalog worker timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn source_history_limits_positions_and_keeps_private_paths_relative() {
        let fixture = Fixture::new();
        let path = fixture.record("local", 1);
        for n in 0..55 {
            let mut item = OnlineItem::new(Provider::YouTube);
            item.id = format!("id{n}");
            item.url = format!("https://www.youtube.com/watch?v=id{n}");
            item.seconds = 100.;
            remember(&fixture.0, &item, 42.4).unwrap();
        }
        assert_eq!(history(&fixture.0).len(), 50);
        let mut item = OnlineItem::new(Provider::YouTube);
        item.id = "local".into();
        item.url = "https://www.youtube.com/watch?v=local".into();
        item.seconds = 100.;
        item.raw = json!({"local_path":path});
        remember(&fixture.0, &item, 9.9).unwrap();
        assert_eq!(history(&fixture.0)[0].position, 0.);
        remember(&fixture.0, &item, 85.1).unwrap();
        assert_eq!(history(&fixture.0)[0].position, 0.);
        remember(&fixture.0, &item, 42.4).unwrap();
        let seen = history(&fixture.0);
        assert_eq!(seen[0].position, 42.);
        assert_eq!(
            seen[0].local_path.as_ref().unwrap(),
            &PathBuf::from("videos/local.mp4")
        );
        assert_eq!(seen.iter().filter(|s| s.id == "local").count(), 1);
        let moved = Fixture::new();
        fs::copy(
            fixture.0.join("video-history.json"),
            moved.0.join("video-history.json"),
        )
        .unwrap();
        assert_eq!(history(&moved.0)[0].local_path, seen[0].local_path);
    }
    #[test]
    fn catalog_projects_real_saved_metadata_newest_first_and_offline_recent() {
        let fixture = Fixture::new();
        fixture.record("old", 10);
        let path = fixture.record("new", 20);
        let mut item = OnlineItem::new(Provider::YouTube);
        item.id = "new".into();
        item.url = "https://www.youtube.com/watch?v=new".into();
        item.title = "Film new".into();
        item.seconds = 245.;
        remember(&fixture.0, &item, 65.).unwrap();
        let mut services = Services::new(fixture.0.clone()).unwrap();
        let Output::VideoCatalog(snapshot) = run(&mut services, Task::VideoCatalog).unwrap() else {
            panic!("wrong catalog result")
        };
        assert_eq!(snapshot.saved.len(), 2);
        assert_eq!(snapshot.saved[0].id, "new");
        assert_eq!(snapshot.saved[0].artist, "Fixture author");
        assert_eq!(snapshot.saved[0].seconds, 245.);
        assert_eq!(snapshot.recent[0].raw["local_path"], json!(path));
        assert_eq!(snapshot.recent[0].raw["position"], 65.);
        assert_eq!(existing_saved(&fixture.0, "new", &item.url), Some(path));
        services.shutdown();
    }
    #[test]
    fn deletion_removes_only_selected_private_media_and_rejects_symlink_escape() {
        let fixture = Fixture::new();
        let target = fixture.record("chosen", 1);
        let keep = fixture.record("keep", 2);
        fs::write(fixture.0.join("videos/chosen.jpg"), b"private art").unwrap();
        let outside = Fixture::new();
        let foreign = outside.record("foreign", 1);
        std::os::unix::fs::symlink(&foreign, fixture.0.join("videos/escape.mp4")).unwrap();
        let mut services = Services::new(fixture.0.clone()).unwrap();
        assert!(run(
            &mut services,
            Task::DeleteSavedVideo {
                path: fixture.0.join("videos/escape.mp4")
            }
        )
        .is_err());
        assert!(foreign.exists());
        run(
            &mut services,
            Task::DeleteSavedVideo {
                path: target.clone(),
            },
        )
        .unwrap();
        assert!(!target.exists());
        assert!(!fixture.0.join("videos/chosen.json").exists());
        assert!(!fixture.0.join("videos/chosen.jpg").exists());
        assert!(keep.exists());
        assert!(foreign.exists());
        services.shutdown();
    }
}
