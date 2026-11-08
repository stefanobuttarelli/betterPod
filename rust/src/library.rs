//! Music is read-only. Metadata, extracted artwork and caches belong to the Rust app.

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::{Accessor, ItemKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The running scan's progress, for the Library screen: music files found
/// while walking the folders, then songs read (one scan at a time).
pub static SCAN_FOUND: AtomicUsize = AtomicUsize::new(0);
pub static SCAN_READ: AtomicUsize = AtomicUsize::new(0);

/// "1,204 of 3,310 songs" while songs are read, "812 songs found" before.
pub fn scan_progress_text() -> String {
    let found = SCAN_FOUND.load(Ordering::Relaxed);
    let read = SCAN_READ.load(Ordering::Relaxed);
    if read > 0 {
        format!("{} of {} songs", read.min(found), found)
    } else {
        format!("{found} songs found")
    }
}

use crate::storage::{atomic_write, atomic_write_with};

const CACHE_VERSION: u32 = 6;
const CACHE_NAME: &str = "library.json";
const MAX_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ART_BYTES: usize = 16 * 1024 * 1024;
const MAX_TRACKS: usize = 200_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    #[serde(alias = "track_num")]
    pub track: u32,
    #[serde(alias = "disc_num")]
    pub disc: u32,
    pub year: u32,
    pub genre: String,
    pub duration: f64,
    pub sample_rate: u32,
    pub bit_depth: u8,
    pub lossless: bool,
    /// Private logical grouping, independent from physical music tags/folders.
    pub album_group: String,
    /// True only for songs moved into another album, not its original songs.
    pub added_album: bool,
    pub cover: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Album {
    pub name: String,
    pub artist: String,
    pub track_indices: Vec<usize>,
    pub cover: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<String>,
    pub scan_folders: Vec<ScanFolder>,
    pub scan_stats: Option<ScanStatistics>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanFolder {
    pub path: PathBuf,
    pub tracks: usize,
    pub other: usize,
    pub skipped: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanStatistics {
    pub when_unix: u64,
    pub took_seconds: f64,
    pub files: usize,
    pub audio_files: usize,
    pub audio_bytes: u64,
    pub by_format: BTreeMap<String, usize>,
    pub skipped_formats: BTreeMap<String, usize>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct InventoryCounts {
    files: usize,
    audio_bytes: u64,
    skipped_formats: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Fingerprint {
    path: PathBuf,
    size: u64,
    modified_seconds: u64,
    modified_nanos: u32,
}

impl Fingerprint {
    fn from_metadata(path: PathBuf, metadata: &fs::Metadata) -> Self {
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .unwrap_or_default();
        Self {
            path,
            size: metadata.len(),
            modified_seconds: modified.as_secs(),
            modified_nanos: modified.subsec_nanos(),
        }
    }

    fn modified(&self) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::new(self.modified_seconds, self.modified_nanos)
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Inventory {
    music: Vec<Fingerprint>,
    covers: Vec<Fingerprint>,
    #[serde(default)]
    overrides: Option<Fingerprint>,
    #[serde(default)]
    override_digest: String,
    #[serde(default)]
    folders: Vec<ScanFolder>,
    #[serde(default)]
    counts: InventoryCounts,
}

#[derive(Serialize, Deserialize)]
struct Cache {
    version: u32,
    root: PathBuf,
    inventory: Inventory,
    library: Library,
}

#[derive(Serialize)]
struct CacheRef<'a> {
    version: u32,
    root: &'a Path,
    inventory: &'a Inventory,
    library: &'a Library,
}

struct BoundedWriter<W> {
    inner: W,
    remaining: u64,
}
impl<W: Write> Write for BoundedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::other(
                "Library cache exceeds its size limit",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn write_cache(path: &Path, cache: &CacheRef<'_>, limit: u64) -> std::io::Result<()> {
    atomic_write_with(path, |file| {
        let mut writer = BoundedWriter {
            inner: BufWriter::with_capacity(64 * 1024, file),
            remaining: limit,
        };
        serde_json::to_writer(&mut writer, cache).map_err(std::io::Error::other)?;
        writer.flush()
    })
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct GoAlbum {
    name: String,
    artist: String,
    key: String,
    #[serde(rename = "artPath", alias = "art_path")]
    art_path: PathBuf,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct GoLibrary {
    tracks: Vec<Track>,
    albums: Vec<GoAlbum>,
}

struct Imported {
    tracks: HashMap<PathBuf, Track>,
    covers: HashMap<String, PathBuf>,
    modified: SystemTime,
}

impl Library {
    /// Run this on a background thread. Even a warm cache walks file metadata
    /// so added, removed or retagged songs cannot leave the index permanently stale.
    pub fn load_or_scan(
        music: &Path,
        data: &Path,
        import_cache: Option<&Path>,
    ) -> Result<Self, String> {
        let started = Instant::now();
        SCAN_FOUND.store(0, Ordering::Relaxed);
        SCAN_READ.store(0, Ordering::Relaxed);
        let music = fs::canonicalize(music)
            .map_err(|e| format!("Open music folder {}: {e}", music.display()))?;
        if !music.is_dir() {
            return Err(format!(
                "Music folder is not a directory: {}",
                music.display()
            ));
        }
        let data = absolute_data_dir(data)?;
        let (inventory, folder_covers) = inventory(&music, &data)?;
        let cache_path = data.join(CACHE_NAME);
        if let Some(cache) = read_json::<Cache>(&cache_path) {
            if cache.version == CACHE_VERSION
                && cache.root == music
                && cache.inventory == inventory
                && valid_cached_library(&cache.library, &inventory)
            {
                // Rebuild derived indices rather than trusting cached integer offsets.
                let mut library = Self::from_tracks(cache.library.tracks);
                library.scan_folders = inventory.folders;
                library.scan_stats = cache.library.scan_stats;
                return Ok(library);
            }
        }

        let imported = import_cache.and_then(|path| import_go(path, &music));
        let artwork_dir = data.join("artwork");
        let mut tracks = Vec::with_capacity(inventory.music.len());
        for file in &inventory.music {
            SCAN_READ.fetch_add(1, Ordering::Relaxed);
            let mut track = imported
                .as_ref()
                .filter(|cache| file.modified() <= cache.modified)
                .and_then(|cache| cache.tracks.get(&file.path))
                .cloned()
                .unwrap_or_else(|| read_track(&file.path, &music, &artwork_dir));
            if track.bit_depth == 0
                && imported
                    .as_ref()
                    .is_some_and(|cache| cache.tracks.contains_key(&file.path))
            {
                if let Ok(parsed) = lofty::read_from_path(&file.path) {
                    track.sample_rate = parsed
                        .properties()
                        .sample_rate()
                        .unwrap_or(track.sample_rate);
                    track.bit_depth = parsed.properties().bit_depth().unwrap_or(0);
                    track.lossless = confirmed_lossless(&file.path, parsed.file_type());
                }
            }
            normalize_track(&mut track, &music);
            // A chosen cover in the existing library takes precedence over folder art.
            let imported_cover = imported
                .as_ref()
                .and_then(|cache| cache.covers.get(&album_key(&track)))
                .cloned();
            let folder_cover = track
                .path
                .parent()
                .and_then(|parent| folder_covers.get(parent))
                .or_else(|| folder_covers.get(&album_dir(&track.path)))
                .cloned();
            track.cover = imported_cover.or(folder_cover).or(track.cover);
            tracks.push(track);
        }
        // Imported metadata is no longer needed. Do not keep another library
        // alive while deriving indices and writing the new private cache.
        drop(imported);
        drop(folder_covers);
        crate::curation::apply(&data, &mut tracks);
        let mut library = Self::from_tracks(tracks);
        library.scan_folders = inventory.folders.clone();
        let mut by_format = BTreeMap::new();
        for track in &library.tracks {
            let extension = track
                .path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let name = match extension.as_str() {
                "m4a" | "mp4" => {
                    if track.lossless {
                        "ALAC".to_owned()
                    } else {
                        "AAC".to_owned()
                    }
                }
                "aif" | "aiff" => "AIFF".to_owned(),
                _ => extension.to_ascii_uppercase(),
            };
            *by_format.entry(name).or_default() += 1;
        }
        library.scan_stats = Some(ScanStatistics {
            when_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            took_seconds: started.elapsed().as_secs_f64(),
            files: inventory.counts.files,
            audio_files: library.tracks.len(),
            audio_bytes: inventory.counts.audio_bytes,
            by_format,
            skipped_formats: inventory.counts.skipped_formats.clone(),
        });
        let cache = CacheRef {
            version: CACHE_VERSION,
            root: &music,
            inventory: &inventory,
            library: &library,
        };
        // A read-only/full card must not make an otherwise playable library fail.
        if let Err(error) = write_cache(&cache_path, &cache, MAX_CACHE_BYTES) {
            eprintln!("Library cache was not saved: {error}");
        }
        Ok(library)
    }

    pub fn from_tracks(mut tracks: Vec<Track>) -> Self {
        tracks.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then_with(|| a.artist.to_lowercase().cmp(&b.artist.to_lowercase()))
                .then_with(|| a.path.cmp(&b.path))
        });
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, track) in tracks.iter().enumerate() {
            groups.entry(album_key(track)).or_default().push(index);
        }
        let mut albums = Vec::with_capacity(groups.len());
        for indices in groups.values_mut() {
            indices.sort_by(|&a, &b| {
                let a = &tracks[a];
                let b = &tracks[b];
                a.disc
                    .cmp(&b.disc)
                    .then_with(|| a.track.cmp(&b.track))
                    .then_with(|| a.path.cmp(&b.path))
            });
            let first = &tracks[indices[0]];
            let name = first.album.clone();
            let artist = album_artist(indices, &tracks);
            let cover = indices
                .iter()
                .find_map(|&index| tracks[index].cover.clone());
            for &index in indices.iter() {
                if tracks[index].cover.is_none() {
                    tracks[index].cover = cover.clone();
                }
            }
            albums.push(Album {
                name,
                artist,
                track_indices: indices.clone(),
                cover,
            });
        }
        albums.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.artist.to_lowercase().cmp(&b.artist.to_lowercase()))
                .then_with(|| a.track_indices.cmp(&b.track_indices))
        });
        let mut artists = BTreeMap::new();
        for album in &albums {
            artists
                .entry(album.artist.to_lowercase())
                .or_insert_with(|| album.artist.clone());
        }
        Self {
            scan_folders: {
                let mut folders = BTreeMap::<PathBuf, usize>::new();
                for track in &tracks {
                    if let Some(parent) = track.path.parent() {
                        *folders.entry(parent.to_path_buf()).or_default() += 1;
                    }
                }
                folders
                    .into_iter()
                    .map(|(path, tracks)| ScanFolder {
                        path,
                        tracks,
                        other: 0,
                        skipped: None,
                    })
                    .collect()
            },
            tracks,
            albums,
            artists: artists.into_values().collect(),
            scan_stats: None,
        }
    }
}

fn absolute_data_dir(data: &Path) -> Result<PathBuf, String> {
    // Creating only our private data directory is intentional. Music is never written.
    fs::create_dir_all(data).map_err(|e| format!("Create Rust data folder: {e}"))?;
    fs::canonicalize(data).map_err(|e| format!("Resolve Rust data folder: {e}"))
}

fn inventory(root: &Path, data: &Path) -> Result<(Inventory, HashMap<PathBuf, PathBuf>), String> {
    let mut music = Vec::new();
    let mut covers = Vec::new();
    let mut folder_covers: HashMap<PathBuf, (u8, PathBuf)> = HashMap::new();
    let mut stack = vec![root.to_path_buf()];
    let downloads = data.join("downloads");
    if fs::symlink_metadata(&downloads).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
        // Completed downloads and received shares belong to this app. Staging
        // files remain hidden or under data/tmp and never enter the library.
        stack.push(downloads);
    }
    let mut visited = HashSet::new();
    let mut folders = Vec::new();
    let mut counts = InventoryCounts::default();
    while let Some(dir) = stack.pop() {
        if !visited.insert(dir.clone()) {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if dir != root => {
                folders.push(ScanFolder {
                    path: dir.clone(),
                    tracks: 0,
                    other: 0,
                    skipped: Some(format!("Unreadable: {error}")),
                });
                eprintln!("Skip unreadable music folder {}: {error}", dir.display());
                continue;
            }
            Err(error) => return Err(format!("Read music folder: {error}")),
        };
        let folder_index = folders.len();
        folders.push(ScanFolder {
            path: dir.clone(),
            tracks: 0,
            other: 0,
            skipped: None,
        });
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    eprintln!("Skip unreadable music entry: {error}");
                    continue;
                }
            };
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(_) => continue,
            };
            if hidden_or_system(&name) || path == data {
                if kind.is_dir() || kind.is_symlink() {
                    let reason = if path == data {
                        "Application data"
                    } else {
                        "Hidden/system folder"
                    };
                    folders.push(ScanFolder {
                        path,
                        tracks: 0,
                        other: 0,
                        skipped: Some(reason.into()),
                    });
                }
                continue;
            }
            // Avoid loops and following a link outside the selected music root.
            if kind.is_symlink() {
                folders.push(ScanFolder {
                    path,
                    tracks: 0,
                    other: 0,
                    skipped: Some("Symlink not followed".into()),
                });
                continue;
            }
            if kind.is_dir() {
                stack.push(path);
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            counts.files += 1;
            if audio_path(&path) {
                folders[folder_index].tracks += 1;
                counts.audio_bytes = counts.audio_bytes.saturating_add(metadata.len());
                music.push(Fingerprint::from_metadata(path, &metadata));
                SCAN_FOUND.fetch_add(1, Ordering::Relaxed);
                if music.len() > MAX_TRACKS {
                    return Err(format!("Library exceeds {MAX_TRACKS} tracks"));
                }
            } else {
                folders[folder_index].other += 1;
                let extension = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if let Some(format) = unsupported_format(&extension) {
                    *counts.skipped_formats.entry(format.into()).or_default() += 1;
                }
                if let Some(priority) = cover_priority(&name) {
                    covers.push(Fingerprint::from_metadata(path.clone(), &metadata));
                    let cover = folder_covers
                        .entry(dir.clone())
                        .or_insert_with(|| (priority, path.clone()));
                    if priority < cover.0 || (priority == cover.0 && path < cover.1) {
                        *cover = (priority, path);
                    }
                }
            }
        }
    }
    music.sort_by(|a, b| a.path.cmp(&b.path));
    covers.sort_by(|a, b| a.path.cmp(&b.path));
    folders.sort_by(|a, b| a.path.cmp(&b.path));
    let folder_covers = folder_covers
        .into_iter()
        .map(|(dir, (_, path))| (dir, path))
        .collect();
    let overrides = fs::symlink_metadata(data.join("curation.json"))
        .ok()
        .filter(|m| m.is_file() && !m.file_type().is_symlink())
        .map(|m| Fingerprint::from_metadata(data.join("curation.json"), &m));
    let override_digest = overrides
        .as_ref()
        .and_then(|fingerprint| {
            if fingerprint.size > MAX_CACHE_BYTES {
                return None;
            }
            fs::read(&fingerprint.path)
                .ok()
                .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        })
        .unwrap_or_default();
    Ok((
        Inventory {
            music,
            covers,
            overrides,
            override_digest,
            folders,
            counts,
        },
        folder_covers,
    ))
}

fn unsupported_format(extension: &str) -> Option<&'static str> {
    match extension {
        "wma" => Some("WMA"),
        "dsf" | "dff" => Some("DSD"),
        "mpc" => Some("Musepack"),
        "ac3" => Some("AC3"),
        "mka" => Some("MKA"),
        "amr" => Some("AMR"),
        _ => None,
    }
}

fn hidden_or_system(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name.to_ascii_lowercase().as_str(),
            "__macosx" | "system volume information" | "$recycle.bin" | "lost+found"
        )
}

fn audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp3"
                    | "flac"
                    | "m4a"
                    | "m4b"
                    | "aac"
                    | "wav"
                    | "aiff"
                    | "aif"
                    | "ogg"
                    | "oga"
                    | "opus"
                    | "ape"
                    | "wv"
            )
        })
}

fn cover_priority(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "cover.jpg" | "cover.jpeg" => Some(0),
        "folder.jpg" | "folder.jpeg" => Some(1),
        "cover.png" => Some(2),
        "folder.png" => Some(3),
        "cover.webp" => Some(4),
        "folder.webp" => Some(5),
        "front.jpg" | "front.jpeg" | "front.png" => Some(6),
        _ => None,
    }
}

fn valid_cached_library(library: &Library, inventory: &Inventory) -> bool {
    if library.tracks.len() != inventory.music.len() {
        return false;
    }
    let expected: HashSet<_> = inventory
        .music
        .iter()
        .map(|file| file.path.as_path())
        .collect();
    let mut seen = HashSet::with_capacity(library.tracks.len());
    library.tracks.iter().all(|track| {
        expected.contains(track.path.as_path())
            && seen.insert(track.path.as_path())
            && track.duration.is_finite()
            && track.duration >= 0.0
            && track.cover.as_ref().is_none_or(|path| path.is_file())
    })
}

fn import_go(path: &Path, root: &Path) -> Option<Imported> {
    let go = read_json::<GoLibrary>(path)?;
    let source_root = fs::canonicalize(path.parent()?).ok()?;
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let mut tracks = HashMap::new();
    for mut track in go.tracks {
        let canonical = match fs::canonicalize(&track.path) {
            Ok(path) if path.starts_with(root) && path.is_file() && audio_path(&path) => path,
            _ => continue,
        };
        track.path = canonical.clone();
        track.cover = None; // Covers are resolved through the separate album records.
        normalize_track(&mut track, root);
        tracks.insert(canonical, track);
    }
    if tracks.is_empty() {
        return None;
    }
    let mut covers = HashMap::new();
    for album in go.albums {
        if album.art_path.as_os_str().is_empty() {
            continue;
        }
        let Some(cover) = imported_cover(&album.art_path, &source_root, root) else {
            continue;
        };
        if let Some((folder, name)) = album.key.rsplit_once('|') {
            if let Ok(folder) = fs::canonicalize(folder) {
                if folder.starts_with(root) {
                    covers.insert(
                        format!(
                            "{}|{}",
                            folder.to_string_lossy(),
                            name.trim().to_lowercase()
                        ),
                        cover,
                    );
                    continue;
                }
            }
        }
        // Old caches without a folder key can be matched only unambiguously.
        let matching: BTreeSet<_> = tracks
            .values()
            .filter(|track| {
                track.album == album.name
                    && (track.album_artist == album.artist || track.artist == album.artist)
            })
            .map(album_key)
            .collect();
        if matching.len() == 1 {
            covers.insert(matching.into_iter().next().unwrap(), cover);
        }
    }
    Some(Imported {
        tracks,
        covers,
        modified,
    })
}

fn imported_cover(path: &Path, source: &Path, music: &Path) -> Option<PathBuf> {
    let mut candidates = vec![path.to_path_buf()];
    if let (Some(folder), Some(name)) = (path.parent().and_then(Path::file_name), path.file_name())
    {
        if matches!(folder.to_str(), Some(".miyoopod_artwork" | "chosen_covers")) {
            candidates.push(source.join(folder).join(name));
        }
    }
    candidates.into_iter().find_map(|candidate| {
        let resolved = fs::canonicalize(candidate).ok()?;
        if resolved.is_file() && (resolved.starts_with(source) || resolved.starts_with(music)) {
            Some(resolved)
        } else {
            None
        }
    })
}

fn read_track(path: &Path, root: &Path, artwork: &Path) -> Track {
    let mut track = Track {
        path: path.to_path_buf(),
        ..Default::default()
    };
    match lofty::read_from_path(path) {
        Ok(file) => {
            let properties = file.properties();
            track.duration = properties.duration().as_secs_f64();
            track.sample_rate = properties.sample_rate().unwrap_or(0);
            track.bit_depth = properties.bit_depth().unwrap_or(0);
            track.lossless = confirmed_lossless(path, file.file_type());
            let tag = file.primary_tag().or_else(|| file.first_tag());
            if let Some(tag) = tag {
                track.title = tag
                    .title()
                    .map(|value| value.into_owned())
                    .unwrap_or_default();
                track.artist = tag
                    .artist()
                    .map(|value| value.into_owned())
                    .unwrap_or_default();
                track.album = tag
                    .album()
                    .map(|value| value.into_owned())
                    .unwrap_or_default();
                track.album_artist = tag
                    .get_string(ItemKey::AlbumArtist)
                    .unwrap_or_default()
                    .into();
                track.genre = tag
                    .genre()
                    .map(|value| value.into_owned())
                    .unwrap_or_default();
                track.track = tag.track().unwrap_or(0);
                track.disc = tag.disk().unwrap_or(0);
                track.year = [ItemKey::RecordingDate, ItemKey::Year, ItemKey::ReleaseDate]
                    .into_iter()
                    .find_map(|key| {
                        tag.get_string(key)
                            .and_then(|value| value.get(..4))
                            .and_then(|value| value.parse().ok())
                    })
                    .unwrap_or(0);
                if let Some(picture) = tag.pictures().first() {
                    track.cover = extract_art(picture.data(), artwork);
                }
            }
        }
        Err(error) => eprintln!("Metadata fallback for {}: {error}", path.display()),
    }
    normalize_track(&mut track, root);
    track
}

fn confirmed_lossless(path: &Path, kind: lofty::file::FileType) -> bool {
    use lofty::{config::ParseOptions, file::FileType};
    let options = ParseOptions::new().read_tags(false);
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    match kind {
        FileType::Flac | FileType::Ape => true,
        FileType::Mp4 => lofty::mp4::Mp4File::read_from(&mut file, options).is_ok_and(|f| {
            matches!(
                f.properties().codec(),
                Some(lofty::mp4::Mp4Codec::ALAC | lofty::mp4::Mp4Codec::FLAC)
            )
        }),
        FileType::Wav => lofty::iff::wav::WavFile::read_from(&mut file, options).is_ok_and(|f| {
            matches!(
                f.properties().format(),
                lofty::iff::wav::WavFormat::PCM | lofty::iff::wav::WavFormat::IEEE_FLOAT
            )
        }),
        FileType::Aiff => {
            lofty::iff::aiff::AiffFile::read_from(&mut file, options).is_ok_and(|f| {
                matches!(
                    f.properties().compression_type(),
                    None | Some(
                        lofty::iff::aiff::AiffCompressionType::None
                            | lofty::iff::aiff::AiffCompressionType::sowt
                            | lofty::iff::aiff::AiffCompressionType::fl32
                            | lofty::iff::aiff::AiffCompressionType::fl64
                            | lofty::iff::aiff::AiffCompressionType::FL32
                    )
                )
            })
        }
        FileType::WavPack => lofty::wavpack::WavPackFile::read_from(&mut file, options)
            .is_ok_and(|f| f.properties().is_lossless()),
        _ => false,
    }
}

fn normalize_track(track: &mut Track, root: &Path) {
    let parent = track.path.parent().unwrap_or(root);
    if track.title.trim().is_empty() {
        track.title = track
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    }
    if track.album.trim().is_empty() {
        track.album = if parent != root {
            album_dir(&track.path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            "Unknown Album".into()
        };
    }
    if track.artist.trim().is_empty() {
        let album_folder = album_dir(&track.path);
        track.artist = album_folder
            .parent()
            .filter(|parent| *parent != root && album_folder != root)
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Unknown Artist".into());
    }
    if track.track == 0 {
        let prefix: String = track
            .title
            .chars()
            .take_while(char::is_ascii_digit)
            .take(3)
            .collect();
        if let Ok(number) = prefix.parse::<u32>() {
            track.track = number;
            let remainder = track.title[prefix.len()..].trim_start_matches([' ', '-', '_', '.']);
            if !remainder.is_empty() {
                track.title = remainder.into();
            }
        }
    }
    if !track.duration.is_finite() || track.duration < 0.0 {
        track.duration = 0.0;
    }
}

fn album_dir(path: &Path) -> PathBuf {
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    let name = folder
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let disc = ["disco", "disc", "disk", "cd"].iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|tail| {
            let tail = tail.trim_start_matches([' ', '_', '.', '-']);
            !tail.is_empty() && tail.chars().all(|character| character.is_ascii_digit())
        })
    });
    if disc {
        folder.parent().unwrap_or(folder).to_path_buf()
    } else {
        folder.to_path_buf()
    }
}

/// The original album identity: private logical override, or physical folder
/// (coalescing disc folders) and album title. Playback shuffle uses the same
/// identity as the library instead of merging equal titles across folders.
pub fn album_key(track: &Track) -> String {
    if !track.album_group.is_empty() {
        return track.album_group.clone();
    }
    format!(
        "{}|{}",
        album_dir(&track.path).to_string_lossy(),
        track.album.trim().to_lowercase()
    )
}

fn album_artist(indices: &[usize], tracks: &[Track]) -> String {
    if let Some(artist) = indices
        .iter()
        .map(|&index| &tracks[index].album_artist)
        .find(|artist| !artist.trim().is_empty())
    {
        return artist.clone();
    }
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for &index in indices {
        *counts.entry(&tracks[index].artist).or_default() += 1;
    }
    let Some((artist, count)) = counts
        .iter()
        .max_by(|(a_name, a_count), (b_name, b_count)| {
            a_count.cmp(b_count).then_with(|| b_name.cmp(a_name))
        })
    else {
        return "Unknown Artist".into();
    };
    if counts.len() > 1 && count * 2 < indices.len() {
        "Various Artists".into()
    } else {
        (*artist).into()
    }
}

fn extract_art(bytes: &[u8], dir: &Path) -> Option<PathBuf> {
    if bytes.is_empty() || bytes.len() > MAX_ART_BYTES {
        return None;
    }
    let format = image::guess_format(bytes).ok()?;
    let extension = match format {
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Png => "png",
        image::ImageFormat::WebP => "webp",
        _ => return None,
    };
    let (width, height) = image::ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .ok()?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 32_000_000 {
        return None;
    }
    // Stable content identity prevents a decoded old cover surviving a replacement.
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    let path = dir.join(format!("{hash:016x}-{}.{extension}", bytes.len()));
    if path.is_file() {
        return Some(path);
    }
    atomic_write(&path, bytes).ok()?;
    Some(path)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CACHE_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity((metadata.len() as usize).min(1024 * 1024));
    File::open(path)
        .ok()?
        .take(MAX_CACHE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temporary(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "betterpod-rust-library-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn dummy(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"not audio; test metadata fallback").unwrap();
    }

    #[test]
    fn streamed_cache_preserves_format_and_failed_write_preserves_previous_file() {
        let root = temporary("streamed-cache");
        let path = root.join(CACHE_NAME);
        let inventory = Inventory {
            music: Vec::new(),
            covers: Vec::new(),
            overrides: None,
            override_digest: String::new(),
            folders: Vec::new(),
            counts: InventoryCounts::default(),
        };
        let library = Library::from_tracks(vec![Track {
            path: root.join("音楽/à song.flac"),
            title: "音楽 à song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            cover: Some(root.join("cover.png")),
            ..Track::default()
        }]);
        let cache = CacheRef {
            version: CACHE_VERSION,
            root: &root,
            inventory: &inventory,
            library: &library,
        };
        write_cache(&path, &cache, MAX_CACHE_BYTES).unwrap();
        let saved = fs::read(&path).unwrap();
        let decoded: Cache = serde_json::from_slice(&saved).unwrap();
        assert_eq!(decoded.version, CACHE_VERSION);
        assert_eq!(decoded.root, root);
        assert_eq!(decoded.inventory, inventory);
        assert_eq!(decoded.library.tracks[0].title, "音楽 à song");
        assert_eq!(decoded.library.albums[0].cover, library.albums[0].cover);
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), saved);
        assert!(write_cache(&path, &cache, 32).is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_statistics_count_other_files_and_survive_warm_cache() {
        let root = temporary("scan-statistics");
        let music = root.join("music");
        let data = root.join("rust");
        fs::create_dir_all(music.join("Only/Child")).unwrap();
        fs::create_dir_all(music.join("Empty")).unwrap();
        fs::create_dir_all(music.join("Other")).unwrap();
        wave(&music.join("song.wav"));
        wave(&music.join("Only/Child/second.wav"));
        fs::write(music.join("Other/notes.txt"), b"notes").unwrap();
        fs::write(music.join("Other/song.wma"), b"unsupported").unwrap();
        fs::write(music.join("Other/cover.png"), b"cover fallback").unwrap();
        fs::write(music.join(".ignored.mp3"), b"hidden").unwrap();
        let library = Library::load_or_scan(&music, &data, None).unwrap();
        let stats = library.scan_stats.as_ref().unwrap();
        assert!(stats.when_unix > 0);
        assert!(stats.took_seconds >= 0.0);
        assert_eq!(stats.files, 5);
        assert_eq!(stats.audio_files, 2);
        assert_eq!(
            stats.audio_bytes,
            fs::metadata(music.join("song.wav")).unwrap().len()
                + fs::metadata(music.join("Only/Child/second.wav"))
                    .unwrap()
                    .len()
        );
        assert_eq!(stats.by_format.get("WAV"), Some(&2));
        assert_eq!(stats.skipped_formats.get("WMA"), Some(&1));
        let other = library
            .scan_folders
            .iter()
            .find(|f| f.path.ends_with("Other"))
            .unwrap();
        assert_eq!((other.tracks, other.other), (0, 3));
        let empty = library
            .scan_folders
            .iter()
            .find(|f| f.path.ends_with("Empty"))
            .unwrap();
        assert_eq!((empty.tracks, empty.other), (0, 0));
        let cached = Library::load_or_scan(&music, &data, None).unwrap();
        let cached = cached.scan_stats.unwrap();
        assert_eq!(cached.when_unix, stats.when_unix);
        assert_eq!(cached.took_seconds, stats.took_seconds);
        assert_eq!(cached.files, stats.files);
        let old: ScanFolder =
            serde_json::from_str(r#"{"path":"old","tracks":2,"skipped":null}"#).unwrap();
        assert_eq!(old.other, 0);
        fs::remove_dir_all(root).unwrap();
    }

    fn wave(path: &Path) {
        let mut body = b"WAVEfmt ".to_vec();
        body.extend_from_slice(&16_u32.to_le_bytes());
        body.extend_from_slice(&1_u16.to_le_bytes()); // PCM
        body.extend_from_slice(&1_u16.to_le_bytes()); // mono
        body.extend_from_slice(&22_050_u32.to_le_bytes());
        body.extend_from_slice(&44_100_u32.to_le_bytes());
        body.extend_from_slice(&2_u16.to_le_bytes());
        body.extend_from_slice(&16_u16.to_le_bytes());
        body.extend_from_slice(b"data");
        body.extend_from_slice(&44_100_u32.to_le_bytes());
        body.resize(body.len() + 44_100, 0);
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&body);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn real_metadata_sample_rate_and_embedded_art_are_read_without_touching_music() {
        use lofty::config::WriteOptions;
        use lofty::picture::{Picture, PictureType};
        use lofty::tag::{Tag, TagExt, TagType};

        let root = temporary("metadata");
        let music = root.join("music");
        let data = root.join("rust");
        let song = music.join("fallback/song.wav");
        wave(&song);
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(3, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let png = png.into_inner();
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_title("Tagged Song".into());
        tag.set_artist("Track Artist".into());
        tag.set_album("Tagged Album".into());
        tag.insert_text(ItemKey::AlbumArtist, "Album Artist".into());
        tag.insert_text(ItemKey::RecordingDate, "2025-02-03".into());
        tag.set_track(7);
        tag.set_disk(2);
        tag.set_genre("Electronic".into());
        tag.push_picture(
            Picture::unchecked(png.clone())
                .pic_type(PictureType::CoverFront)
                .build(),
        );
        tag.save_to_path(&song, WriteOptions::default()).unwrap();
        let original = fs::read(&song).unwrap();
        let library = Library::load_or_scan(&music, &data, None).unwrap();
        let track = &library.tracks[0];
        assert_eq!(track.title, "Tagged Song");
        assert_eq!(track.artist, "Track Artist");
        assert_eq!(track.album, "Tagged Album");
        assert_eq!(track.album_artist, "Album Artist");
        assert_eq!(track.track, 7);
        assert_eq!(track.disc, 2);
        assert_eq!(track.year, 2025);
        assert_eq!(track.sample_rate, 22_050);
        assert_eq!(track.bit_depth, 16);
        assert!(track.lossless);
        assert!((track.duration - 1.0).abs() < 0.001);
        let cover = track.cover.as_ref().unwrap();
        assert!(cover.starts_with(fs::canonicalize(&data).unwrap()));
        assert_eq!(fs::read(cover).unwrap(), png);
        assert_eq!(fs::read(&song).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_cannot_pull_music_from_another_root_or_modify_original_cache() {
        let root = temporary("import");
        let music = root.join("music");
        let original = root.join("stable");
        let data = root.join("rust");
        let inside = music.join("Artist/Album/01 Song.mp3");
        let outside = root.join("outside/Other.mp3");
        dummy(&inside);
        dummy(&outside);
        fs::create_dir_all(&original).unwrap();
        let cache = original.join(".miyoopod_library.json");
        let bytes = serde_json::to_vec(&serde_json::json!({"tracks":[
            {"path":inside,"title":"Imported Title","artist":"Artist","album":"Album","duration":12.0,"track_num":1},
            {"path":outside,"title":"Outside","artist":"Bad","album":"Bad"}
        ]})).unwrap();
        fs::write(&cache, &bytes).unwrap();
        let library = Library::load_or_scan(&music, &data, Some(&cache)).unwrap();
        assert_eq!(library.tracks.len(), 1);
        assert_eq!(library.tracks[0].title, "Imported Title");
        assert_eq!(library.tracks[0].track, 1);
        assert_eq!(fs::read(&cache).unwrap(), bytes);
        assert_eq!(fs::read_dir(&original).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_inventory_detects_additions_removals_and_changed_music() {
        let root = temporary("inventory");
        let music = root.join("music");
        let data = root.join("rust");
        let one = music.join("Artist/Album/01 One.mp3");
        dummy(&one);
        let first = Library::load_or_scan(&music, &data, None).unwrap();
        assert_eq!(first.tracks.len(), 1);
        let two = music.join("Artist/Album/02 Two.FLAC");
        dummy(&two);
        assert_eq!(
            Library::load_or_scan(&music, &data, None)
                .unwrap()
                .tracks
                .len(),
            2
        );
        fs::remove_file(&one).unwrap();
        let after = Library::load_or_scan(&music, &data, None).unwrap();
        assert_eq!(after.tracks.len(), 1);
        assert_eq!(after.tracks[0].title, "Two");
        fs::write(&two, b"changed length").unwrap();
        let saved = read_json::<Cache>(&data.join(CACHE_NAME)).unwrap();
        let (current, _) = inventory(
            &fs::canonicalize(&music).unwrap(),
            &fs::canonicalize(&data).unwrap(),
        )
        .unwrap();
        assert_ne!(saved.inventory, current);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn private_completed_downloads_join_library_and_invalidate_cache() {
        let root = temporary("downloads");
        let music = root.join("music");
        let data = root.join("rust");
        dummy(&music.join("Original/song.mp3"));
        let original = fs::read(music.join("Original/song.mp3")).unwrap();
        assert_eq!(
            Library::load_or_scan(&music, &data, None)
                .unwrap()
                .tracks
                .len(),
            1
        );
        let added = data.join("downloads/Shared/Album/02 New.flac");
        dummy(&added);
        dummy(&data.join("downloads/Shared/Album/.unfinished.part.mp3"));
        dummy(&data.join("tmp/download.mp3"));
        let cover = added.parent().unwrap().join("cover.jpg");
        fs::write(&cover, b"private cover").unwrap();
        let library = Library::load_or_scan(&music, &data, None).unwrap();
        assert_eq!(library.tracks.len(), 2);
        assert_eq!(
            library
                .tracks
                .iter()
                .find(|t| t.path == fs::canonicalize(&added).unwrap())
                .unwrap()
                .cover,
            Some(fs::canonicalize(cover).unwrap())
        );
        fs::remove_file(added).unwrap();
        assert_eq!(
            Library::load_or_scan(&music, &data, None)
                .unwrap()
                .tracks
                .len(),
            1
        );
        assert_eq!(fs::read(music.join("Original/song.mp3")).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn customization_digest_invalidates_cache_when_size_and_timestamp_are_identical() {
        let root = temporary("override-digest");
        let music = root.join("music");
        let data = root.join("rust");
        let song = music.join("Album/song.mp3");
        dummy(&song);
        fs::create_dir_all(&data).unwrap();
        let cover = data.join("selected.png");
        image::DynamicImage::new_rgb8(2, 2).save(&cover).unwrap();
        let context = crate::online::Context::standalone();
        crate::curation::set_cover_file(
            &data,
            Some(&music),
            std::slice::from_ref(&song),
            &cover,
            &context,
        )
        .unwrap();
        let first = Library::load_or_scan(&music, &data, None).unwrap();
        let manifest = data.join("curation.json");
        let previous = fs::metadata(&manifest).unwrap();
        let timestamp = previous.modified().unwrap();
        let mut picture = image::RgbImage::new(2, 2);
        for pixel in picture.pixels_mut() {
            *pixel = image::Rgb([255, 0, 0]);
        }
        picture.save(&cover).unwrap();
        crate::curation::set_cover_file(
            &data,
            Some(&music),
            std::slice::from_ref(&song),
            &cover,
            &context,
        )
        .unwrap();
        File::options()
            .write(true)
            .open(&manifest)
            .unwrap()
            .set_modified(timestamp)
            .unwrap();
        assert_eq!(fs::metadata(&manifest).unwrap().len(), previous.len());
        assert_eq!(
            fs::metadata(&manifest).unwrap().modified().unwrap(),
            timestamp
        );
        let second = Library::load_or_scan(&music, &data, None).unwrap();
        assert_ne!(first.tracks[0].cover, second.tracks[0].cover);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compilation_and_disc_folders_stay_together_in_track_order() {
        let tracks = vec![
            Track {
                path: "/music/Album/CD2/02 B.flac".into(),
                title: "B".into(),
                artist: "Guest".into(),
                album_artist: "Album Artist".into(),
                album: "Album".into(),
                disc: 2,
                track: 2,
                ..Default::default()
            },
            Track {
                path: "/music/Album/CD1/01 A.flac".into(),
                title: "A".into(),
                artist: "Other".into(),
                album: "Album".into(),
                disc: 1,
                track: 1,
                ..Default::default()
            },
            Track {
                path: "/music/Other/01 C.flac".into(),
                title: "C".into(),
                artist: "Other".into(),
                album: "Album".into(),
                disc: 1,
                track: 1,
                ..Default::default()
            },
        ];
        let library = Library::from_tracks(tracks);
        assert_eq!(library.albums.len(), 2);
        let album = library
            .albums
            .iter()
            .find(|album| album.track_indices.len() == 2)
            .unwrap();
        assert_eq!(album.artist, "Album Artist");
        assert_eq!(library.tracks[album.track_indices[0]].disc, 1);
        assert_eq!(library.tracks[album.track_indices[1]].disc, 2);
    }

    #[test]
    fn folder_covers_are_case_insensitive_and_system_entries_are_ignored() {
        let root = temporary("covers");
        let music = root.join("music");
        let data = root.join("rust");
        dummy(&music.join("Artist/Album/song.mp3"));
        dummy(&music.join(".hidden/fake.mp3"));
        dummy(&music.join("__MACOSX/fake.mp3"));
        let cover = music.join("Artist/Album/COVER.JPG");
        fs::write(&cover, b"cover fixture").unwrap();
        let library = Library::load_or_scan(&music, &data, None).unwrap();
        assert_eq!(library.tracks.len(), 1);
        assert_eq!(
            library.albums[0].cover,
            Some(fs::canonicalize(cover).unwrap())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn poisoned_cache_paths_are_rejected_even_with_a_matching_inventory() {
        let root = temporary("poison");
        let music = root.join("music");
        let data = root.join("rust");
        dummy(&music.join("song.mp3"));
        let _ = Library::load_or_scan(&music, &data, None).unwrap();
        let path = data.join(CACHE_NAME);
        let mut cache = read_json::<Cache>(&path).unwrap();
        cache.library.tracks[0].path = root.join("outside.mp3");
        fs::write(&path, serde_json::to_vec(&cache).unwrap()).unwrap();
        let loaded = Library::load_or_scan(&music, &data, None).unwrap();
        assert_eq!(
            loaded.tracks[0].path,
            fs::canonicalize(music.join("song.mp3")).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "Requires locally generated BETTERPOD_METADATA_FIXTURES; no network or user files"]
    fn codec_badges_distinguish_alac_from_aac_in_the_same_m4a_container() {
        let fixtures = PathBuf::from(std::env::var_os("BETTERPOD_METADATA_FIXTURES").unwrap());
        let data = temporary("codec-badges");
        for (name, expected) in [
            ("aac-original.m4a", false),
            ("alac-original.m4a", true),
            ("mp3-original.mp3", false),
            ("flac-original.flac", true),
            ("ogg-original.ogg", false),
        ] {
            let track = read_track(&fixtures.join(name), &fixtures, &data);
            assert_eq!(track.lossless, expected, "{name}");
            if expected {
                assert!(track.bit_depth >= 16, "{name}");
            }
        }
        fs::remove_dir_all(data).unwrap();
    }
}
