use super::{Config, Shared};
use crate::{
    curation,
    library::{Library, Track},
    metadata::{self, TagEdit},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use lofty::file::TaggedFileExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::UNIX_EPOCH,
};

const MAX_ART: usize = 8 * 1024 * 1024;

pub(super) struct Roots {
    music: PathBuf,
    downloads: Option<PathBuf>,
    data: PathBuf,
}
impl Roots {
    pub(super) fn new(config: &Config) -> Result<Self, String> {
        let data = fs::canonicalize(&config.data).map_err(|e| e.to_string())?;
        let downloads = fs::symlink_metadata(data.join("downloads"))
            .ok()
            .filter(|meta| meta.is_dir() && !meta.file_type().is_symlink())
            .and_then(|_| fs::canonicalize(data.join("downloads")).ok())
            .filter(|path| path.starts_with(&data));
        Ok(Self {
            music: fs::canonicalize(&config.music)
                .map_err(|e| format!("Cannot open the music library: {e}"))?,
            downloads,
            data,
        })
    }
    fn audio(&self, path: &Path) -> Result<PathBuf, String> {
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("Symbolic links and non-regular audio files are read-only".into());
        }
        let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
        if !canonical.starts_with(&self.music)
            && !self
                .downloads
                .as_ref()
                .is_some_and(|root| canonical.starts_with(root))
        {
            return Err("This file is outside the approved music library".into());
        }
        Ok(canonical)
    }
    fn art(&self, path: &Path) -> Result<PathBuf, String> {
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_ART as u64 {
            return Err("This cover cannot be read safely".into());
        }
        let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
        if canonical.starts_with(self.data.join("artwork"))
            || canonical.starts_with(self.data.join("chosen-covers"))
            || self
                .downloads
                .as_ref()
                .is_some_and(|root| canonical.starts_with(root))
            || canonical.starts_with(&self.music)
        {
            Ok(canonical)
        } else {
            Err("This cover is outside the approved library".into())
        }
    }
}

pub(super) fn safe_name(name: &str) -> String {
    let name = name
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect::<String>();
    let name = name.trim();
    if name.is_empty() {
        "betterPod".into()
    } else {
        name.into()
    }
}
fn modified(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|t| i64::try_from(t.as_nanos()).ok())
        .unwrap_or(0)
}
fn writable(path: &Path) -> bool {
    path.extension().and_then(|x| x.to_str()).is_some_and(|x| {
        matches!(
            x.to_lowercase().as_str(),
            "mp3" | "flac" | "ogg" | "oga" | "opus" | "m4a" | "m4b"
        )
    })
}

/// A placeholder still occupies its page position if its file disappeared.
#[cfg(test)]
pub(super) fn record(
    track: &Track,
    roots: &Roots,
    tags_changed: bool,
    protected: &HashSet<PathBuf>,
) -> Value {
    let display = tags_changed.then(|| curation::RemoteAlbumDisplay::load(&roots.data));
    record_with_display(track, roots, tags_changed, protected, display.as_ref())
}

pub(super) fn record_with_display(
    track: &Track,
    roots: &Roots,
    tags_changed: bool,
    protected: &HashSet<PathBuf>,
    display: Option<&curation::RemoteAlbumDisplay>,
) -> Value {
    let meta = fs::metadata(&track.path).ok();
    let path_result = roots.audio(&track.path);
    let mut warning = path_result.as_ref().err().cloned();
    let tags = if path_result.is_ok() {
        match metadata::read_tag_fields(&track.path) {
            Ok(tags) => Some(tags),
            Err(error) => {
                warning = Some(format!("This metadata layout is read-only: {error}"));
                None
            }
        }
    } else {
        None
    };
    if protected.contains(&track.path)
        || path_result
            .as_ref()
            .is_ok_and(|path| protected.contains(path))
    {
        warning = Some(
            "Stop playback or choose another song on betterPod before editing this track.".into(),
        );
    }
    let can_write = writable(&track.path)
        && warning.is_none()
        && meta.as_ref().is_some_and(|m| !m.permissions().readonly());
    if !writable(&track.path) && warning.is_none() {
        warning = Some("This audio format can be viewed but not edited.".into());
    }
    let tags = tags.unwrap_or_else(|| TagEdit {
        title: Some(track.title.clone()),
        artist: Some(track.artist.clone()),
        album: Some(track.album.clone()),
        album_artist: Some(track.album_artist.clone()),
        genre: Some(track.genre.clone()),
        year: Some(track.year),
        track: Some(track.track),
        disc: Some(track.disc),
        ..TagEdit::default()
    });
    let mut displayed = track.clone();
    if tags_changed {
        displayed.album = tags.album.clone().unwrap_or_default();
        displayed.album_artist = tags.album_artist.clone().unwrap_or_default();
        displayed.disc = tags.disc.unwrap_or_default();
        displayed.album_group.clear();
        displayed.added_album = false;
        if let Some(display) = display {
            display.apply(&mut displayed);
        }
    }
    let album_id = crate::library::album_key(&displayed);
    let use_display = !tags_changed || !displayed.album_group.is_empty();
    json!({
        "path":track.path,"fileName":track.path.file_name().unwrap_or_default().to_string_lossy(),
        "format":track.path.extension().unwrap_or_default().to_string_lossy().to_uppercase(),
        "writable":can_write,"title":tags.title.unwrap_or_default(),"artist":tags.artist.unwrap_or_default(),
        "album":tags.album.unwrap_or_default(),"albumArtist":tags.album_artist.unwrap_or_default(),
        "genre":tags.genre.unwrap_or_default(),"year":tags.year.unwrap_or(0),"track":tags.track.unwrap_or(0),
        "trackTotal":tags.track_total.unwrap_or(0),"disc":tags.disc.unwrap_or(0),
        "size":meta.as_ref().map_or(0,|m|m.len()),"modified":meta.as_ref().map_or(0,modified),"warning":warning,
        "displayAlbum":if use_display {Some(&displayed.album)} else {None},
        "displayAlbumArtist":if use_display {Some(&displayed.album_artist)} else {None},
        "albumGroup":&displayed.album_group,
        "displayAlbumID":album_id,
    })
}

fn valid_art(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_ART {
        return Err("Choose a JPEG or PNG cover smaller than 8 MB".into());
    }
    let format = image::guess_format(bytes).map_err(|_| "This cover is not a readable image")?;
    if !matches!(format, image::ImageFormat::Jpeg | image::ImageFormat::Png) {
        return Err("Choose a JPEG or PNG cover".into());
    }
    let reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| "This cover has invalid dimensions")?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 40_000_000 {
        return Err("Choose a cover up to 40 megapixels".into());
    }
    Ok(())
}
fn read_art_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_ART as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    valid_art(&bytes)?;
    Ok(bytes)
}
pub(super) fn cover(track: &Track, roots: &Roots, artwork_changed: bool) -> Result<Value, String> {
    roots.audio(&track.path)?;
    if !artwork_changed {
        if let Some(path) = track.cover.as_ref().and_then(|path| roots.art(path).ok()) {
            if let Ok(bytes) = read_art_file(&path) {
                return Ok(json!({"cover":STANDARD.encode(bytes)}));
            }
        }
    }
    let file = lofty::probe::Probe::open(&track.path)
        .map_err(|e| e.to_string())?
        .guess_file_type()
        .map_err(|e| e.to_string())?
        .options(lofty::config::ParseOptions::new().read_properties(false))
        .read()
        .map_err(|e| e.to_string())?;
    let tag = file.primary_tag().or_else(|| file.first_tag());
    let picture = tag.and_then(|tag| {
        tag.pictures()
            .iter()
            .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
            .or_else(|| tag.pictures().first())
    });
    match picture {
        Some(picture) => {
            valid_art(picture.data())?;
            Ok(json!({"cover":STANDARD.encode(picture.data())}))
        }
        None => Ok(json!({"cover":null})),
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Fields {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    album_artist: Option<String>,
    genre: Option<String>,
    year: Option<u32>,
    track: Option<u32>,
    track_total: Option<u32>,
    disc: Option<u32>,
}
impl Fields {
    fn tags(&self) -> Result<TagEdit, String> {
        for text in [
            &self.title,
            &self.artist,
            &self.album,
            &self.album_artist,
            &self.genre,
        ]
        .into_iter()
        .flatten()
        {
            if text.len() > 65_536 || text.contains('\0') {
                return Err("Metadata text is too long or contains a null character".into());
            }
        }
        if self.year.is_some_and(|n| n > 9999)
            || [self.track, self.track_total, self.disc]
                .into_iter()
                .flatten()
                .any(|n| n > 65535)
        {
            return Err(
                "Track and disc numbers must be between 0 and 65535; years between 0 and 9999"
                    .into(),
            );
        }
        Ok(TagEdit {
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            album_artist: self.album_artist.clone(),
            genre: self.genre.clone(),
            year: self.year,
            track: self.track,
            track_total: self.track_total,
            disc: self.disc,
            cover: None,
        })
    }
    fn empty(&self) -> bool {
        self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.album_artist.is_none()
            && self.genre.is_none()
            && self.year.is_none()
            && self.track.is_none()
            && self.track_total.is_none()
            && self.disc.is_none()
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Change {
    pub(super) path: PathBuf,
    #[serde(default)]
    fields: Fields,
    pub(super) cover: Option<String>,
    #[serde(default)]
    pub(super) remove_cover: bool,
    #[serde(default)]
    pub(super) join_album: bool,
    size: u64,
    modified: i64,
}
impl Change {
    pub(super) fn changes_grouping(&self) -> bool {
        self.join_album || self.fields.album.is_some() || self.fields.album_artist.is_some()
    }
}

fn identity(meta: &fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        (meta.len(), modified(meta) as u64)
    }
}
fn protected(shared: &Shared, path: &Path) -> Result<(), String> {
    let paths = shared
        .snapshot
        .lock()
        .map_err(|e| e.to_string())?
        .protected
        .clone();
    if paths
        .iter()
        .any(|p| p == path || fs::canonicalize(p).ok().is_some_and(|p| p == path))
    {
        Err(
            "Stop playback or choose another song on betterPod before editing the current track."
                .into(),
        )
    } else {
        Ok(())
    }
}
static NEXT: AtomicU64 = AtomicU64::new(1);
fn private_stage(path: &Path) -> Result<PathBuf, String> {
    let directory = path.parent().ok_or("Missing audio folder")?;
    let extension = path.extension().unwrap_or_default().to_string_lossy();
    for _ in 0..32 {
        let stage = directory.join(format!(
            ".betterpod-wifi-{}-{}.{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            extension
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&stage) {
            Ok(_) => return Ok(stage),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Cannot create a private metadata staging file".into())
}
struct Cleanup(Vec<PathBuf>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

pub(super) fn save(
    config: &Config,
    roots: &Roots,
    shared: &Shared,
    library: &Library,
    change: &Change,
) -> Result<Option<String>, String> {
    if !library.tracks.iter().any(|track| track.path == change.path) {
        return Err("This track is not in the paired library".into());
    }
    if !writable(&change.path) {
        return Err("This audio format is read-only".into());
    }
    let path = roots.audio(&change.path)?;
    protected(shared, &path)?;
    if shared.cancelled.load(Ordering::Acquire) {
        return Err("The Wi-Fi library session was stopped before saving this track.".into());
    }
    let writer = metadata::lock_remote_tags()?;
    let original = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if original.permissions().readonly() {
        return Err("This audio file is read-only".into());
    }
    if original.len() != change.size || modified(&original) != change.modified {
        return Err("This file changed since it was opened. Reload it before saving.".into());
    }
    if change.cover.is_some() && change.remove_cover {
        return Err("A cover cannot be replaced and removed in the same edit".into());
    }
    let mut tags = change.fields.tags()?;
    if change.join_album {
        curation::remote_album_group(
            tags.album
                .as_deref()
                .ok_or("Joining tracks requires an album field")?,
            tags.album_artist
                .as_deref()
                .ok_or("Joining tracks requires an album artist field")?,
        )?;
    }
    if change.fields.empty() && change.cover.is_none() && !change.remove_cover {
        return Ok(None);
    }
    let mut cleanup = Cleanup(Vec::new());
    if let Some(cover) = &change.cover {
        if cover.len() > ((MAX_ART + 2) / 3) * 4 {
            return Err("The cover is too large".into());
        }
        let bytes = STANDARD
            .decode(cover)
            .map_err(|_| "The cover data is invalid")?;
        valid_art(&bytes)?;
        let art = private_stage(&path)?;
        cleanup.0.push(art.clone());
        fs::write(&art, bytes).map_err(|e| e.to_string())?;
        tags.cover = Some(art);
    }
    let stage = private_stage(&path)?;
    cleanup.0.push(stage.clone());
    let mut input = File::open(&path).map_err(|e| e.to_string())?;
    let mut output = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&stage)
        .map_err(|e| e.to_string())?;
    let mut buffer = [0; 65536];
    loop {
        if shared.cancelled.load(Ordering::Acquire) {
            return Err(
                "The Wi-Fi session was stopped. No changes were written to this track.".into(),
            );
        }
        let length = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if length == 0 {
            break;
        }
        output
            .write_all(&buffer[..length])
            .map_err(|e| e.to_string())?;
    }
    drop(output);
    drop(input);
    writer.write(&stage, &tags, change.remove_cover)?;
    metadata::read_tag_fields(&stage)?;
    fs::set_permissions(&stage, original.permissions()).map_err(|e| e.to_string())?;
    File::open(&stage)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    protected(shared, &path)?;
    if shared.cancelled.load(Ordering::Acquire) {
        return Err("The Wi-Fi library session was stopped before saving this track.".into());
    }
    let latest = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if latest.file_type().is_symlink()
        || identity(&latest) != identity(&original)
        || latest.len() != original.len()
        || modified(&latest) != modified(&original)
    {
        return Err("The original file changed during saving. This track was not replaced.".into());
    }
    // The original remains untouched until the complete, checked file exists.
    {
        // Only the final rename is serialized with the UI's short protection
        // update. Reads, tag parsing, copies and syncing hold no UI mutex.
        let state = shared.snapshot.lock().map_err(|e| e.to_string())?;
        if state
            .protected
            .iter()
            .any(|p| p == &path || p == &change.path)
            || shared.cancelled.load(Ordering::Acquire)
        {
            return Err("This track became active before saving. It was not replaced.".into());
        }
        fs::rename(&stage, &path).map_err(|e| e.to_string())?;
    }
    let artwork_changed = change.cover.is_some() || change.remove_cover;
    let reconciliation = if change.join_album {
        curation::reconcile_remote_album(&config.data, &change.path, &tags, artwork_changed, true)
    } else {
        curation::reconcile_remote_tags(&config.data, &change.path, &tags, artwork_changed)
    };
    let warning = reconciliation.err().map(|error| {
        format!(
            "{} was saved, but its private album/cover override could not be refreshed: {error}",
            change.path.display()
        )
    });
    Ok(warning)
}
