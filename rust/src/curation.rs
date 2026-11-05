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
#[derive(Clone, Debug)]
pub struct CoverQuery {
    pub paths: Vec<PathBuf>,
    pub album: String,
    pub artist: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Override {
    album: Option<String>,
    artist: Option<String>,
    group: String,
    cover: Option<PathBuf>,
    prior: Option<BeforeMerge>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct BeforeMerge {
    album: Option<String>,
    artist: Option<String>,
    group: String,
    cover: Option<PathBuf>,
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
    let _lock = WRITER.lock().map_err(|e| e.to_string())?;
    context.check()?;
    let mut manifest = read(data);
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
        let entry = manifest.tracks.entry(path).or_default();
        if entry.prior.is_none() {
            entry.prior = Some(BeforeMerge {
                album: entry.album.clone(),
                artist: entry.artist.clone(),
                group: entry.group.clone(),
                cover: entry.cover.clone(),
            });
        }
        entry.group = group.clone();
        entry.album = Some(album.into());
        entry.artist = Some(artist.into());
        if cover.is_some() {
            entry.cover = cover.clone();
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
        } else {
            entry.album = None;
            entry.artist = None;
            entry.group.clear();
        }
    }
    save(data, &manifest)
}
fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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
    let mut found = 0;
    for (index, album) in albums.iter().enumerate() {
        context.check()?;
        let paths = validate_paths(data, music, &album.paths)?;
        context.progress(
            index as u64,
            Some(albums.len() as u64),
            format!("Finding cover · {}", album.album),
        );
        let query = format!("{} {}", album.artist, album.album);
        for provider in [
            Provider::MusicBrainz,
            Provider::Itunes,
            Provider::Deezer,
            Provider::Wikipedia,
        ] {
            context.check()?;
            let Ok((items, _)) = metadata::search(context, provider, &query) else {
                context.check()?;
                continue;
            };
            let item = items.into_iter().find(|i| {
                !i.cover.is_empty()
                    && normalized(&i.title) == normalized(&album.album)
                    && (album.artist.is_empty()
                        || normalized(&i.artist) == normalized(&album.artist))
            });
            let Some(item) = item else {
                continue;
            };
            let Ok(bytes) = context.get_bytes(&item.cover, MAX_COVER) else {
                context.check()?;
                continue;
            };
            if set_cover(data, &paths, &bytes, context).is_ok() {
                found += 1;
                break;
            }
            context.check()?;
        }
    }
    context.progress(
        albums.len() as u64,
        Some(albums.len() as u64),
        format!("Found {found} covers"),
    );
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
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
