//! Private presentation overrides. Choosing covers or merging albums never
//! rewrites the music files or neighbouring artwork.
use crate::{
    library::Track,
    metadata,
    online::{Context, Provider},
    storage::atomic_write,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Mutex,
};
use unicode_normalization::UnicodeNormalization;
const MAX_MANIFEST: u64 = 64 * 1024 * 1024;
const MAX_COVER: usize = 8 * 1024 * 1024;
static WRITER: Mutex<()> = Mutex::new(());
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CoverQuery {
    pub paths: Vec<PathBuf>,
    pub album: String,
    pub artist: String,
    pub tracks: Vec<crate::cover_match::TrackContext>,
}
impl CoverQuery {
    /// Projection only; callers run this on their library/service worker.
    pub fn from_album(album: &crate::library::Album, library: &crate::library::Library) -> Self {
        let tracks = album
            .track_indices
            .iter()
            .filter_map(|&i| library.tracks.get(i))
            .collect::<Vec<_>>();
        Self {
            paths: tracks.iter().map(|t| t.path.clone()).collect(),
            album: album.name.clone(),
            artist: album.artist.clone(),
            tracks: tracks
                .into_iter()
                .map(|t| crate::cover_match::TrackContext {
                    artist: t.artist.clone(),
                    album_artist: t.album_artist.clone(),
                    genre: t.genre.clone(),
                })
                .collect(),
        }
    }
    pub fn from_paths(
        paths: Vec<PathBuf>,
        album: String,
        artist: String,
        library: &crate::library::Library,
    ) -> Self {
        let selected = paths.iter().collect::<HashSet<_>>();
        let tracks = library
            .tracks
            .iter()
            .filter(|t| selected.contains(&t.path))
            .map(|t| crate::cover_match::TrackContext {
                artist: t.artist.clone(),
                album_artist: t.album_artist.clone(),
                genre: t.genre.clone(),
            })
            .collect();
        Self {
            paths,
            album,
            artist,
            tracks,
        }
    }
    pub fn album_query(&self) -> crate::cover_match::AlbumQuery {
        crate::cover_match::AlbumQuery::new(&self.album, &self.artist, &self.tracks)
    }
}
mod search;
pub use search::search_items as search_cover_items;
/// Detailed state for the original Find Missing Covers view. It travels in
/// this job's existing progress channel, keeping other service events stable.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CoverSearchProgress {
    pub total: usize,
    pub index: usize,
    pub found: usize,
    pub failed: usize,
    pub current: String,
    pub artist: String,
    pub finished: bool,
    pub cancelled: bool,
    pub errors: usize,
    pub error: String,
    pub missed: Vec<String>,
}
const COVER_PROGRESS: &str = "betterpod-cover-search:";
impl CoverSearchProgress {
    fn publish(&self, context: &Context) {
        if let Ok(json) = serde_json::to_string(self) {
            let status = format!("{COVER_PROGRESS}{json}");
            if self.finished {
                let _ =
                    context.progress_important(self.index as u64, Some(self.total as u64), status);
            } else {
                context.progress(self.index as u64, Some(self.total as u64), status);
            }
        }
    }
    pub fn from_status(status: &str) -> Option<Self> {
        serde_json::from_str(status.strip_prefix(COVER_PROGRESS)?).ok()
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Override {
    album: Option<String>,
    artist: Option<String>,
    group: String,
    cover: Option<PathBuf>,
    // A physical cover edit from the Mac supersedes older folder/import art,
    // including an explicit removal. Later console cover choices still win.
    embedded_cover: bool,
    prior: Option<BeforeMerge>,
    disc: Option<u32>,
    added: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct BeforeMerge {
    album: Option<String>,
    artist: Option<String>,
    group: String,
    cover: Option<PathBuf>,
    disc: Option<u32>,
    added: bool,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Manifest {
    tracks: BTreeMap<PathBuf, Override>,
}
fn read(data: &Path) -> Manifest {
    let path = data.join("curation.json");
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return Manifest::default();
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_MANIFEST {
        return Manifest::default();
    }
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
fn save(data: &Path, manifest: &Manifest) -> Result<(), String> {
    let bytes = serde_json::to_vec(manifest).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err("Album customizations are too large".into());
    }
    atomic_write(&data.join("curation.json"), &bytes).map_err(|e| e.to_string())
}
pub(crate) fn apply(data: &Path, tracks: &mut [Track]) {
    let manifest = read(data);
    let data_root = fs::canonicalize(data).ok();
    let cover_root = fs::symlink_metadata(data.join("chosen-covers"))
        .ok()
        .filter(|m| m.is_dir() && !m.file_type().is_symlink())
        .and_then(|_| fs::canonicalize(data.join("chosen-covers")).ok())
        .filter(|p| data_root.as_ref().is_some_and(|root| p.starts_with(root)));
    for track in tracks {
        let Some(over) = manifest.tracks.get(&track.path) else {
            continue;
        };
        if let Some(album) = &over.album {
            track.album.clone_from(album);
        }
        if let Some(artist) = &over.artist {
            track.album_artist.clone_from(artist);
        }
        if !over.group.is_empty() {
            track.album_group.clone_from(&over.group);
        }
        if let Some(disc) = over.disc {
            track.disc = disc;
        }
        track.added_album = over.added;
        if let Some(cover) = &over.cover {
            if let (Some(root), Ok(path)) = (&cover_root, fs::canonicalize(cover)) {
                if path.starts_with(root) && path.is_file() {
                    track.cover = Some(path);
                }
            }
        }
    }
}

pub(crate) fn embedded_cover_paths(data: &Path) -> HashSet<PathBuf> {
    read(data)
        .tracks
        .into_iter()
        .filter_map(|(path, entry)| entry.embedded_cover.then_some(path))
        .collect()
}
fn validate_paths(
    data: &Path,
    music: Option<&Path>,
    paths: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    if paths.is_empty() || paths.len() > 200_000 {
        return Err("Choose valid library songs".into());
    }
    let music = music
        .map(fs::canonicalize)
        .transpose()
        .map_err(|e| e.to_string())?;
    let private = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let downloads = fs::symlink_metadata(private.join("downloads"))
        .ok()
        .filter(|m| m.is_dir() && !m.file_type().is_symlink())
        .and_then(|_| fs::canonicalize(private.join("downloads")).ok())
        .filter(|p| p.starts_with(&private));
    let mut seen = HashSet::new();
    paths
        .iter()
        .map(|path| {
            let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
            let allowed = music.as_ref().is_some_and(|r| path.starts_with(r))
                || downloads.as_ref().is_some_and(|r| path.starts_with(r));
            let audio = path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                matches!(
                    s.to_ascii_lowercase().as_str(),
                    "mp3"
                        | "m4a"
                        | "m4b"
                        | "flac"
                        | "ogg"
                        | "oga"
                        | "opus"
                        | "wav"
                        | "aac"
                        | "aiff"
                        | "aif"
                        | "ape"
                        | "wv"
                )
            });
            if !allowed || !path.is_file() || !audio {
                return Err("Song is outside this application's music roots".into());
            }
            if !seen.insert(path.clone()) {
                return Ok(None);
            }
            Ok(Some(path))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|paths| paths.into_iter().flatten().collect())
}
fn store_cover(data: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    if bytes.len() > MAX_COVER {
        return Err("Cover exceeds 8 MB".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|e| format!("Invalid cover: {e}"))?
        .thumbnail(1024, 1024);
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    let bytes = encoded.into_inner();
    let name = format!("{:x}.jpg", Sha256::digest(&bytes));
    let dir = crate::downloads::private_dir(data, &["chosen-covers"])?;
    let path = dir.join(name);
    if !path.exists() {
        atomic_write(&path, &bytes).map_err(|e| e.to_string())?;
    }
    Ok(path)
}
fn set_cover(
    data: &Path,
    paths: &[PathBuf],
    bytes: &[u8],
    context: &Context,
) -> Result<(), String> {
    let cover = store_cover(data, bytes)?;
    context.check()?;
    let _lock = WRITER.lock().map_err(|e| e.to_string())?;
    context.check()?;
    let mut manifest = read(data);
    for path in paths {
        manifest.tracks.entry(path.clone()).or_default().cover = Some(cover.clone());
    }
    save(data, &manifest)
}
pub fn set_cover_url(
    data: &Path,
    music: Option<&Path>,
    paths: &[PathBuf],
    url: &str,
    context: &Context,
) -> Result<(), String> {
    let paths = validate_paths(data, music, paths)?;
    let bytes = context.get_bytes(url, MAX_COVER)?;
    set_cover(data, &paths, &bytes, context)
}
pub fn set_cover_file(
    data: &Path,
    music: Option<&Path>,
    paths: &[PathBuf],
    cover: &Path,
    context: &Context,
) -> Result<(), String> {
    let paths = validate_paths(data, music, paths)?;
    // Covers must come from the music roots or this application's private files.
    let root = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let path = fs::canonicalize(cover).map_err(|e| e.to_string())?;
    let permitted = path.starts_with(&root)
        || music
            .and_then(|m| fs::canonicalize(m).ok())
            .is_some_and(|m| path.starts_with(m));
    if !permitted || !path.is_file() {
        return Err("Cover is outside this application's roots".into());
    }
    if fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_COVER as u64 {
        return Err("Cover exceeds 8 MB".into());
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    set_cover(data, &paths, &bytes, context)
}
pub fn merge(
    data: &Path,
    music: Option<&Path>,
    paths: &[PathBuf],
    target_paths: &[PathBuf],
    album: &str,
    artist: &str,
    context: &Context,
) -> Result<(), String> {
    let source = validate_paths(data, music, paths)?;
    let target = validate_paths(data, music, target_paths)?;
    let album = album.trim();
    let artist = artist.trim();
    if album.is_empty() || album.len() > 1024 || artist.len() > 1024 {
        return Err("Invalid target album name".into());
    }
    context.check()?;
    // Read the existing numbers on the service worker, with picture extraction
    // disabled by metadata's reader. No music bytes are changed by a merge.
    let mut positions = BTreeMap::new();
    for path in source.iter().chain(target.iter()) {
        context.check()?;
        if positions.contains_key(path) {
            continue;
        }
        let tags = metadata::read_album_tags(std::slice::from_ref(path), context)
            .ok()
            .and_then(|mut v| v.pop())
            .map(|(_, tags)| (tags.disc.unwrap_or(0), tags.track.unwrap_or(0)))
            .unwrap_or((0, 0));
        positions.insert(path.clone(), tags);
    }
    let _lock = WRITER.lock().map_err(|e| e.to_string())?;
    context.check()?;
    let mut manifest = read(data);
    for (path, position) in &mut positions {
        if let Some(disc) = manifest.tracks.get(path).and_then(|v| v.disc) {
            position.0 = disc;
        }
    }
    let target_positions = target
        .iter()
        .filter_map(|p| positions.get(p))
        .copied()
        .collect::<Vec<_>>();
    let source_positions = source
        .iter()
        .filter_map(|p| positions.get(p))
        .copied()
        .collect::<Vec<_>>();
    let next_disc = merge_disc_positions(&target_positions, &source_positions);
    let source_set = source.iter().cloned().collect::<HashSet<_>>();
    let first = &target[0];
    let group = manifest
        .tracks
        .get(first)
        .map(|v| v.group.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            format!(
                "private:{:x}",
                Sha256::digest(first.to_string_lossy().as_bytes())
            )
        });
    let cover = target
        .iter()
        .find_map(|p| manifest.tracks.get(p).and_then(|v| v.cover.clone()));
    for path in source.into_iter().chain(target) {
        let entry = manifest.tracks.entry(path.clone()).or_default();
        if entry.prior.is_none() {
            entry.prior = Some(BeforeMerge {
                album: entry.album.clone(),
                artist: entry.artist.clone(),
                group: entry.group.clone(),
                cover: entry.cover.clone(),
                disc: entry.disc,
                added: entry.added,
            });
        }
        entry.group = group.clone();
        entry.album = Some(album.into());
        entry.artist = Some(artist.into());
        entry.added |= source_set.contains(&path);
        if next_disc > 0 && source_set.contains(&path) {
            let own = positions.get(&path).map_or(1, |p| p.0.max(1));
            entry.disc = Some(next_disc.saturating_add(own - 1));
        }
        if cover.is_some() {
            entry.cover = cover.clone();
        }
    }
    save(data, &manifest)
}
/// Return to each song's embedded/folder art without altering its private
/// logical album membership, disc assignment or another song's chosen art.
pub fn reset_chosen_cover(
    data: &Path,
    music: Option<&Path>,
    paths: &[PathBuf],
    context: &Context,
) -> Result<(), String> {
    let paths = validate_paths(data, music, paths)?;
    context.check()?;
    let _lock = WRITER.lock().map_err(|e| e.to_string())?;
    context.check()?;
    let mut manifest = read(data);
    for path in paths {
        if let Some(entry) = manifest.tracks.get_mut(&path) {
            entry.cover = None;
        }
    }
    save(data, &manifest)
}

pub fn unmerge(
    data: &Path,
    music: Option<&Path>,
    paths: &[PathBuf],
    context: &Context,
) -> Result<(), String> {
    let paths = validate_paths(data, music, paths)?;
    context.check()?;
    let _lock = WRITER.lock().map_err(|e| e.to_string())?;
    context.check()?;
    let mut manifest = read(data);
    for path in paths {
        let Some(entry) = manifest.tracks.get_mut(&path) else {
            continue;
        };
        if let Some(prior) = entry.prior.take() {
            entry.album = prior.album;
            entry.artist = prior.artist;
            entry.group = prior.group;
            entry.cover = prior.cover;
            entry.disc = prior.disc;
            entry.added = prior.added;
        } else {
            entry.album = None;
            entry.artist = None;
            entry.group.clear();
            entry.disc = None;
            entry.added = false;
        }
    }
    save(data, &manifest)
}

/// Album Info on the console renamed an album or its artist. A merged album
/// keeps its songs together under the new name: every override in an edited
/// song's group follows (Go's retargetAlbumMerges). An edited disc number
/// replaces the private disc a merge assigned. Other choices stay.
pub(crate) fn follow_album_edit(
    data: &Path,
    edits: &[(PathBuf, metadata::TagEdit)],
) -> Result<(), String> {
    let _lock = WRITER.lock().map_err(|e| e.to_string())?;
    let mut manifest = read(data);
    let mut changed = false;
    let mut renamed: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    for (path, edit) in edits {
        let Some(entry) = manifest.tracks.get_mut(path) else {
            continue;
        };
        if edit.disc.is_some() && entry.disc.take().is_some() {
            changed = true;
        }
        if edit.album.is_none() && edit.album_artist.is_none() {
            continue;
        }
        if entry.group.is_empty() {
            // A grouped name with no group cannot be shared; follow it here.
            changed |= rename(entry, &edit.album, &edit.album_artist);
        } else {
            renamed
                .entry(entry.group.clone())
                .or_insert_with(|| (edit.album.clone(), edit.album_artist.clone()));
        }
    }
    for entry in manifest.tracks.values_mut() {
        if let Some((album, artist)) = renamed.get(&entry.group) {
            changed |= rename(entry, album, artist);
        }
    }
    if changed {
        save(data, &manifest)?;
    }
    Ok(())
}
fn rename(entry: &mut Override, album: &Option<String>, artist: &Option<String>) -> bool {
    let mut changed = false;
    if let Some(album) = album.as_ref().filter(|_| entry.album.is_some()) {
        changed |= entry.album.as_ref() != Some(album);
        entry.album = Some(album.clone());
    }
    if let Some(artist) = artist.as_ref().filter(|_| entry.artist.is_some()) {
        changed |= entry.artist.as_ref() != Some(artist);
        entry.artist = Some(artist.clone());
    }
    changed
}

/// Physical edits from the paired Mac must not remain hidden behind older
/// private album grouping or chosen-cover overrides. Unrelated choices stay.
pub(crate) fn reconcile_remote_tags(
    data: &Path,
    path: &Path,
    edit: &metadata::TagEdit,
    artwork_changed: bool,
) -> Result<(), String> {
    reconcile_remote_album(data, path, edit, artwork_changed, false)
}

/// The client never chooses an arbitrary private group. Identical confirmed
/// album names and album artists derive the same identity across source folders.
pub(crate) fn remote_album_group(album: &str, artist: &str) -> Result<String, String> {
    fn normalized(value: &str) -> String {
        value
            .nfkc()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }
    let album = normalized(album);
    let artist = normalized(artist);
    if album.is_empty()
        || album.len() > 1024
        || artist.len() > 1024
        || album.contains('\0')
        || artist.contains('\0')
    {
        return Err(
            "Choose a nonempty album and names shorter than 1025 bytes before joining tracks"
                .into(),
        );
    }
    Ok(format!(
        "remote-album:{:x}",
        Sha256::digest(format!("{album}\0{artist}").as_bytes())
    ))
}

/// One manifest transaction reconciles physical tags and optional joining. A
/// failed manifest write is reported separately from a successful audio edit.
pub(crate) fn reconcile_remote_album(
    data: &Path,
    path: &Path,
    edit: &metadata::TagEdit,
    artwork_changed: bool,
    join_album: bool,
) -> Result<(), String> {
    let group = if join_album {
        Some(remote_album_group(
            edit.album
                .as_deref()
                .ok_or("Joining tracks requires an album field")?,
            edit.album_artist
                .as_deref()
                .ok_or("Joining tracks requires an album artist field")?,
        )?)
    } else {
        None
    };
    let _lock = WRITER.lock().map_err(|e| e.to_string())?;
    let mut manifest = read(data);
    if group.is_none() && !artwork_changed && !manifest.tracks.contains_key(path) {
        return Ok(());
    }
    let entry = manifest.tracks.entry(path.to_path_buf()).or_default();
    if edit.album.is_some() || edit.album_artist.is_some() {
        entry.album = None;
        entry.artist = None;
        entry.group.clear();
        entry.prior = None;
        entry.added = false;
        // Joining changes membership only. An existing private disc assignment
        // remains until the client explicitly edits the physical disc field.
        if !join_album {
            entry.disc = None;
        }
    }
    if edit.disc.is_some() {
        entry.disc = None;
    }
    if artwork_changed {
        entry.cover = None;
        entry.embedded_cover = true;
    }
    if let Some(group) = group {
        entry.group = group;
    }
    save(data, &manifest)
}

/// Load once for a response page, rather than rereading a potentially large
/// manifest for every newly edited track in that page.
pub(crate) struct RemoteAlbumDisplay(Manifest);
impl RemoteAlbumDisplay {
    pub(crate) fn load(data: &Path) -> Self {
        Self(read(data))
    }
    pub(crate) fn apply(&self, track: &mut Track) {
        let Some(entry) = self.0.tracks.get(&track.path) else {
            return;
        };
        if let Some(album) = &entry.album {
            track.album.clone_from(album);
        }
        if let Some(artist) = &entry.artist {
            track.album_artist.clone_from(artist);
        }
        if !entry.group.is_empty() {
            track.album_group.clone_from(&entry.group);
        }
        if let Some(disc) = entry.disc {
            track.disc = disc;
        }
        track.added_album = entry.added;
    }
}
/// Original ij_albummerge.go: only a numbered position collision moves songs
/// to the next disc; unnumbered songs keep their own disc.
pub fn merge_disc_positions(target: &[(u32, u32)], moving: &[(u32, u32)]) -> u32 {
    let have = target
        .iter()
        .map(|(d, n)| ((*d).max(1), *n))
        .collect::<HashSet<_>>();
    let last = target.iter().map(|(d, _)| *d).max().unwrap_or(1).max(1);
    if moving
        .iter()
        .any(|(d, n)| *n > 0 && have.contains(&(1.max(*d), *n)))
    {
        last.saturating_add(1)
    } else {
        0
    }
}
pub fn find_covers(
    data: &Path,
    music: Option<&Path>,
    albums: &[CoverQuery],
    context: &Context,
) -> Result<usize, String> {
    if albums.len() > 2000 {
        return Err("Choose at most 2000 albums for one cover search".into());
    }
    let mut state = CoverSearchProgress {
        total: albums.len(),
        ..Default::default()
    };
    let mut consecutive_errors = 0;
    for (index, album) in albums.iter().enumerate() {
        if context.check().is_err() {
            state.cancelled = true;
            break;
        }
        let paths = validate_paths(data, music, &album.paths)?;
        state.index = index;
        state.current = album.album.clone();
        state.artist = album.artist.clone();
        state.publish(context);
        match search::find_cover(context, &album.album_query()) {
            Ok(Some(bytes)) => match set_cover(data, &paths, &bytes, context) {
                Ok(()) => {
                    state.found += 1;
                    consecutive_errors = 0;
                }
                Err(error) => {
                    if context.check().is_err() {
                        state.cancelled = true;
                        break;
                    }
                    state.failed += 1;
                    state.errors += 1;
                    state.error = error;
                    consecutive_errors += 1;
                }
            },
            Ok(None) => {
                state.failed += 1;
                state.missed.push(album.album.clone());
                consecutive_errors = 0;
            }
            Err(error) => {
                if context.check().is_err() {
                    state.cancelled = true;
                    break;
                }
                state.failed += 1;
                state.errors += 1;
                state.error = error;
                consecutive_errors += 1;
            }
        }
        state.index = index + 1;
        state.publish(context);
        if consecutive_errors >= 3 {
            break;
        }
    }
    state.finished = true;
    state.current.clear();
    state.artist.clear();
    state.index = state.total;
    state.publish(context);
    Ok(state.found)
}

#[cfg(test)]
mod cover_integration_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn album_info_renames_carry_a_merged_album_and_its_added_songs() {
        let data = std::env::temp_dir().join(format!("bp-follow-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        fs::create_dir_all(&data).unwrap();
        let own = PathBuf::from("/music/Rumours/01.flac");
        let added = PathBuf::from("/music/Rumours Bonus/01.flac");
        let alone = PathBuf::from("/music/Other/01.flac");
        let merged = |added: bool, disc: Option<u32>| Override {
            album: Some("Rumours".into()),
            artist: Some("Fleetwood Mac".into()),
            group: "private:rumours".into(),
            disc,
            added,
            ..Default::default()
        };
        let mut manifest = Manifest::default();
        manifest.tracks.insert(own.clone(), merged(false, None));
        manifest.tracks.insert(added.clone(), merged(true, Some(2)));
        manifest.tracks.insert(
            alone.clone(),
            Override {
                cover: Some("/data/chosen-covers/x.jpg".into()),
                ..Default::default()
            },
        );
        save(&data, &manifest).unwrap();

        // Only the album's own song is edited; the added one follows.
        let edit = metadata::TagEdit {
            album: Some("Rumours (Deluxe)".into()),
            ..Default::default()
        };
        follow_album_edit(&data, &[(own.clone(), edit)]).unwrap();
        let after = read(&data);
        for path in [&own, &added] {
            let entry = &after.tracks[path];
            assert_eq!(entry.album.as_deref(), Some("Rumours (Deluxe)"));
            assert_eq!(entry.artist.as_deref(), Some("Fleetwood Mac"));
            assert_eq!(entry.group, "private:rumours");
        }
        assert_eq!(after.tracks[&added].disc, Some(2));
        assert!(after.tracks[&added].added);
        // A song with only a chosen cover gains no album name.
        assert_eq!(after.tracks[&alone].album, None);

        // An edited disc number replaces the merge's private disc.
        let disc = metadata::TagEdit {
            disc: Some(1),
            ..Default::default()
        };
        follow_album_edit(&data, &[(added.clone(), disc)]).unwrap();
        assert_eq!(read(&data).tracks[&added].disc, None);
        fs::remove_dir_all(&data).unwrap();
    }
    #[test]
    fn cancelled_search_finishes_before_validating_or_accessing_any_music_path() {
        let context = Context::standalone();
        context.cancel_for_test();
        let queries = vec![CoverQuery {
            paths: vec!["/not-an-authorized-test-path/song.flac".into()],
            album: "Never requested".into(),
            artist: String::new(),
            ..Default::default()
        }];
        assert_eq!(
            find_covers(Path::new("/nonexistent"), None, &queries, &context).unwrap(),
            0
        );
    }
    #[test]
    fn cumulative_cover_report_keeps_missing_errors_and_cancellation_distinct() {
        let report = CoverSearchProgress {
            total: 4,
            index: 4,
            found: 1,
            failed: 2,
            current: String::new(),
            artist: String::new(),
            finished: true,
            cancelled: true,
            errors: 1,
            error: "Connection unavailable".into(),
            missed: vec!["An unmatched album".into()],
        };
        let status = format!(
            "{COVER_PROGRESS}{}",
            serde_json::to_string(&report).unwrap()
        );
        let restored = CoverSearchProgress::from_status(&status).unwrap();
        assert_eq!(
            (restored.found, restored.failed, restored.errors),
            (1, 2, 1)
        );
        assert_eq!(restored.missed, vec!["An unmatched album"]);
        assert!(restored.finished && restored.cancelled);
        assert!(CoverSearchProgress::from_status("ordinary download progress").is_none());
    }
    #[test]
    fn resetting_cover_preserves_merge_disc_other_choices_and_original_bytes() {
        let root =
            std::env::temp_dir().join(format!("betterpod-reset-cover-{}", std::process::id()));
        let music = root.join("music");
        let data = root.join("rust");
        fs::create_dir_all(&music).unwrap();
        fs::create_dir_all(data.join("chosen-covers")).unwrap();
        let one = music.join("one.mp3");
        let two = music.join("two.mp3");
        fs::write(&one, b"original one").unwrap();
        fs::write(&two, b"original two").unwrap();
        let one = fs::canonicalize(one).unwrap();
        let two = fs::canonicalize(two).unwrap();
        let art = data.join("chosen-covers/retained.png");
        fs::write(&art, b"cache retained").unwrap();
        let mut manifest = Manifest::default();
        manifest.tracks.insert(
            one.clone(),
            Override {
                album: Some("Merged".into()),
                artist: Some("Artist".into()),
                group: "logical-group".into(),
                cover: Some(art.clone()),
                disc: Some(3),
                prior: Some(BeforeMerge {
                    album: Some("Before".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        manifest.tracks.insert(
            two.clone(),
            Override {
                cover: Some(art.clone()),
                ..Default::default()
            },
        );
        save(&data, &manifest).unwrap();
        reset_chosen_cover(
            &data,
            Some(&music),
            std::slice::from_ref(&one),
            &Context::standalone(),
        )
        .unwrap();
        let result = read(&data);
        let changed = &result.tracks[&one];
        assert!(changed.cover.is_none());
        assert_eq!(changed.album.as_deref(), Some("Merged"));
        assert_eq!(changed.artist.as_deref(), Some("Artist"));
        assert_eq!(changed.group, "logical-group");
        assert_eq!(changed.disc, Some(3));
        assert_eq!(
            changed.prior.as_ref().unwrap().album.as_deref(),
            Some("Before")
        );
        assert_eq!(result.tracks[&two].cover.as_ref(), Some(&art));
        assert!(art.is_file());
        assert_eq!(fs::read(&one).unwrap(), b"original one");
        assert_eq!(fs::read(&two).unwrap(), b"original two");
        let outside = root.join("outside.mp3");
        fs::write(&outside, b"outside").unwrap();
        assert!(
            reset_chosen_cover(&data, Some(&music), &[outside], &Context::standalone()).is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn private_cover_and_logical_merge_survive_rescan_and_retagging() {
        let root = std::env::temp_dir().join(format!("betterpod-curation-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let music = root.join("music");
        let data = root.join("rust");
        let one = music.join("One/one.mp3");
        let two = music.join("Two/two.mp3");
        fs::create_dir_all(one.parent().unwrap()).unwrap();
        fs::create_dir_all(two.parent().unwrap()).unwrap();
        fs::create_dir_all(&data).unwrap();
        fs::write(&one, b"source one").unwrap();
        fs::write(&two, b"source two").unwrap();
        let cover = data.join("selected.png");
        image::DynamicImage::new_rgb8(3, 2).save(&cover).unwrap();
        let context = Context::standalone();
        set_cover_file(
            &data,
            Some(&music),
            std::slice::from_ref(&two),
            &cover,
            &context,
        )
        .unwrap();
        merge(
            &data,
            Some(&music),
            std::slice::from_ref(&one),
            std::slice::from_ref(&two),
            "Together",
            "Album Artist",
            &context,
        )
        .unwrap();
        let library = crate::library::Library::load_or_scan(&music, &data, None).unwrap();
        assert_eq!(library.albums.len(), 1);
        assert_eq!(library.albums[0].name, "Together");
        assert_eq!(library.albums[0].artist, "Album Artist");
        assert!(
            library
                .tracks
                .iter()
                .find(|t| t.path == fs::canonicalize(&one).unwrap())
                .unwrap()
                .added_album
        );
        assert!(
            !library
                .tracks
                .iter()
                .find(|t| t.path == fs::canonicalize(&two).unwrap())
                .unwrap()
                .added_album
        );
        assert!(library.albums[0]
            .cover
            .as_ref()
            .unwrap()
            .starts_with(fs::canonicalize(&data).unwrap()));
        assert_eq!(fs::read(&one).unwrap(), b"source one");
        assert_eq!(fs::read(&two).unwrap(), b"source two");
        fs::write(&one, b"user retagged externally").unwrap();
        let rescanned = crate::library::Library::load_or_scan(&music, &data, None).unwrap();
        assert_eq!(rescanned.albums.len(), 1);
        assert_eq!(rescanned.albums[0].name, "Together");
        unmerge(&data, Some(&music), &[one.clone(), two.clone()], &context).unwrap();
        let separated = crate::library::Library::load_or_scan(&music, &data, None).unwrap();
        assert_eq!(separated.albums.len(), 2);
        assert!(separated.tracks.iter().all(|t| t.album_group.is_empty()));
        assert!(separated.tracks.iter().all(|t| !t.added_album));
        assert!(separated
            .tracks
            .iter()
            .find(|t| t.path == std::fs::canonicalize(&two).unwrap())
            .unwrap()
            .cover
            .is_some());
        fs::remove_file(&one).unwrap();
        assert_eq!(
            crate::library::Library::load_or_scan(&music, &data, None)
                .unwrap()
                .tracks
                .len(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn original_number_collision_places_next_disc_and_preserves_unnumbered_tracks() {
        assert_eq!(merge_disc_positions(&[(1, 1), (3, 7)], &[(0, 1)]), 4);
        assert_eq!(merge_disc_positions(&[(1, 1), (3, 7)], &[(2, 1)]), 0);
        assert_eq!(merge_disc_positions(&[(1, 0)], &[(0, 0)]), 0);
    }
    #[test]
    fn malicious_source_paths_and_cover_symlinks_are_rejected() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(format!("betterpod-curation-paths-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let music = root.join("music");
        let data = root.join("rust");
        fs::create_dir_all(&music).unwrap();
        fs::create_dir_all(&data).unwrap();
        let outside = root.join("outside.mp3");
        fs::write(&outside, b"source").unwrap();
        let linked = music.join("linked.mp3");
        symlink(&outside, &linked).unwrap();
        assert!(validate_paths(&data, Some(&music), &[linked]).is_err());
        let song = music.join("song.mp3");
        fs::write(&song, b"song").unwrap();
        let cover = root.join("outside.png");
        image::DynamicImage::new_rgb8(2, 2).save(&cover).unwrap();
        let link = data.join("cover.png");
        symlink(&cover, &link).unwrap();
        assert!(
            set_cover_file(&data, Some(&music), &[song], &link, &Context::standalone()).is_err()
        );
        assert_eq!(fs::read(outside).unwrap(), b"source");
        fs::remove_dir_all(root).unwrap();
    }
}

/// Delete Album, as in the Go app: the album's songs leave the card while
/// the app keeps running. Only regular files inside the music folder go;
/// the album's folder goes too when nothing but pictures, lists, lyrics or
/// cue sheets is left in it, and its artist's folder when left empty.
pub fn delete_album(
    music: Option<&Path>,
    paths: &[PathBuf],
    context: &Context,
) -> Result<usize, String> {
    let music = fs::canonicalize(music.ok_or("No music folder")?).map_err(|e| e.to_string())?;
    let mut checked = Vec::new();
    for path in paths {
        let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("Only music files are deleted, never links or folders".into());
        }
        let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
        if !path.starts_with(&music) {
            return Err("A song is outside the music folder".into());
        }
        if !checked.contains(&path) {
            checked.push(path);
        }
    }
    let mut folders = Vec::new();
    for path in &checked {
        context.check()?;
        fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(parent) = path.parent() {
            if !folders.iter().any(|f: &PathBuf| f == parent) {
                folders.push(parent.to_path_buf());
            }
        }
    }
    for folder in folders {
        let _ = crate::storage::sync_directory(&folder);
        remove_album_folder_if_done(&music, &folder);
    }
    Ok(checked.len())
}

fn remove_album_folder_if_done(music: &Path, folder: &Path) {
    if folder == music || !folder.starts_with(music) {
        return;
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if entry.file_type().map_or(true, |t| t.is_dir()) {
            return;
        }
        let extra = [
            ".jpg", ".jpeg", ".png", ".webp", ".gif", ".bmp", ".m3u", ".m3u8", ".lrc", ".cue",
            ".txt", ".nfo", ".log", ".json",
        ]
        .iter()
        .any(|ext| name.ends_with(ext));
        if !extra && !name.starts_with('.') {
            return; // something else: the folder stays
        }
    }
    if fs::remove_dir_all(folder).is_err() {
        return;
    }
    if let Some(up) = folder.parent() {
        if up != music
            && up.starts_with(music)
            && fs::read_dir(up).is_ok_and(|mut left| left.next().is_none())
        {
            let _ = fs::remove_dir(up);
        }
    }
}

#[cfg(test)]
mod delete_album_tests {
    use super::*;

    #[test]
    fn songs_go_and_a_folder_left_with_only_pictures_goes_too() {
        let root = std::env::temp_dir().join(format!("bp-delete-album-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let music = root.join("music");
        let album = music.join("Artist/Album");
        let other = music.join("Other/Kept");
        fs::create_dir_all(&album).unwrap();
        fs::create_dir_all(&other).unwrap();
        for name in ["01.flac", "02.flac"] {
            fs::write(album.join(name), b"audio").unwrap();
        }
        fs::write(album.join("cover.jpg"), b"jpg").unwrap();
        fs::write(other.join("01.flac"), b"audio").unwrap();
        fs::write(other.join("notes.pdf"), b"pdf").unwrap();
        let outside = root.join("outside.flac");
        fs::write(&outside, b"audio").unwrap();
        let context = Context::standalone();
        // nothing outside the music folder, and nothing deleted on refusal
        assert!(delete_album(Some(&music), &[album.join("01.flac"), outside.clone()], &context).is_err());
        assert!(album.join("01.flac").exists() && outside.exists());
        let deleted = delete_album(
            Some(&music),
            &[album.join("01.flac"), album.join("02.flac"), album.join("01.flac")],
            &context,
        )
        .unwrap();
        assert_eq!(deleted, 2);
        assert!(!album.exists(), "only a cover was left: the folder goes");
        assert!(!music.join("Artist").exists(), "the empty artist folder goes");
        // a folder with something else in it stays
        delete_album(Some(&music), &[other.join("01.flac")], &context).unwrap();
        assert!(other.join("notes.pdf").exists());
        assert!(music.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
