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
    collections::{BTreeMap, HashSet},
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Mutex,
};
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
#[path = "curation_search.rs"]
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
#[path = "curation_cover_test.rs"]
mod cover_integration_tests;
#[cfg(test)]
mod tests {
    use super::*;
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
