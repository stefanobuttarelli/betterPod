//! Local M3U playlists. Imported lists cannot be edited; explicit deletion
//! removes only their M3U file. All filesystem work belongs on an I/O worker.

use std::collections::BTreeSet;
use std::ffi::{CString, OsStr};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use crate::storage::atomic_write_at;

const MAX_PLAYLIST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TRACKS: usize = 200_000;
const MAX_PLAYLISTS: usize = 10_000;

#[derive(Clone, Debug)]
pub struct Playlist {
    pub name: String,
    pub path: PathBuf,
    pub tracks: Vec<PathBuf>,
    pub read_only: bool,
}

pub struct PlaylistStore {
    music: PathBuf,
    data: PathBuf,
    directory: PathBuf,
    directory_handle: File,
    // Serializes read/modify/rename operations from this app's workers.
    operation: Mutex<()>,
    cancelled: Option<Arc<AtomicBool>>,
}

impl PlaylistStore {
    pub fn new(music: PathBuf, data: PathBuf) -> Result<Self, String> {
        let music = fs::canonicalize(music).map_err(|error| error.to_string())?;
        if !music.is_dir() {
            return Err("Music folder is not a directory".into());
        }
        fs::create_dir_all(&data).map_err(|error| error.to_string())?;
        let data = fs::canonicalize(data).map_err(|error| error.to_string())?;
        let directory = data.join("playlists");
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err("Private playlists path must be a real directory".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&directory).map_err(|error| error.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
        let directory_handle = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&directory)
            .map_err(|error| format!("Open private playlists: {error}"))?;
        let store = Self {
            music,
            data,
            directory,
            directory_handle,
            operation: Mutex::new(()),
            cancelled: None,
        };
        store.ensure_directory()?;
        Ok(store)
    }

    pub fn list(&self) -> Result<Vec<Playlist>, String> {
        self.ensure_directory()?;
        let mut candidates = BTreeSet::new();
        for entry in fs::read_dir(&self.directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if playlist_path(&entry.path()) && !entry.file_name().to_string_lossy().starts_with('.')
            {
                candidates.insert(entry.path());
            }
        }
        let mut directories = vec![self.music.clone()];
        while let Some(directory) = directories.pop() {
            self.check_cancelled()?;
            if directory.starts_with(&self.directory) {
                continue;
            }
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    eprintln!("Skip playlist folder {}: {error}", directory.display());
                    continue;
                }
            };
            for entry in entries.flatten() {
                self.check_cancelled()?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.')
                    || matches!(
                        name.to_ascii_lowercase().as_str(),
                        "__macosx" | "system volume information" | "$recycle.bin"
                    )
                {
                    continue;
                }
                let kind = match entry.file_type() {
                    Ok(kind) => kind,
                    Err(_) => continue,
                };
                if kind.is_dir() {
                    directories.push(entry.path());
                } else if (kind.is_file() || kind.is_symlink()) && playlist_path(&entry.path()) {
                    candidates.insert(entry.path());
                }
                if candidates.len() > MAX_PLAYLISTS {
                    return Err(format!("More than {MAX_PLAYLISTS} playlists found"));
                }
            }
        }
        let mut playlists = Vec::with_capacity(candidates.len());
        let mut seen = BTreeSet::new();
        for path in candidates {
            match self.load(&path) {
                Ok(playlist) if seen.insert(playlist.path.clone()) => playlists.push(playlist),
                Ok(_) => {}
                Err(error) => eprintln!("Skip playlist {}: {error}", path.display()),
            }
        }
        playlists.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.read_only.cmp(&b.read_only))
                .then_with(|| a.path.cmp(&b.path))
        });
        Ok(playlists)
    }

    pub fn create(&self, name: &str) -> Result<Playlist, String> {
        let _operation = self.operation.lock().map_err(|error| error.to_string())?;
        self.ensure_directory()?;
        let name = clean_name(name)?;
        let filename = format!("{name}.m3u8");
        let filename_c = c_filename(OsStr::new(&filename))?;
        // Exclusive creation must not replace another playlist with the same name.
        let fd = unsafe {
            libc::openat(
                self.directory_handle.as_raw_fd(),
                filename_c.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if fd < 0 {
            return Err(format!(
                "Create playlist: {}",
                std::io::Error::last_os_error()
            ));
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        if let Err(error) = file.write_all(b"#EXTM3U\n").and_then(|_| file.sync_all()) {
            unsafe { libc::unlinkat(self.directory_handle.as_raw_fd(), filename_c.as_ptr(), 0) };
            return Err(format!("Save new playlist: {error}"));
        }
        Ok(Playlist {
            name,
            path: self.directory.join(filename),
            tracks: Vec::new(),
            read_only: false,
        })
    }

    /// Adds the songs not yet in the playlist, once each, as the Go app does.
    /// Returns the playlist's name and how many songs were added.
    pub fn append(&self, path: &Path, tracks: &[PathBuf]) -> Result<(String, usize), String> {
        let _operation = self.operation.lock().map_err(|error| error.to_string())?;
        let filename = self.private_filename(path)?;
        let current = self.read_private(&filename)?;
        let mut playlist = self.parse(&self.directory.join(&filename), &current, false)?;
        // Validate the entire append before changing anything on disk.
        let mut have: std::collections::HashSet<PathBuf> =
            playlist.tracks.iter().cloned().collect();
        let mut append = Vec::new();
        for path in tracks {
            let track = self.music_track(path)?;
            if have.insert(track.clone()) {
                append.push(track);
            }
        }
        if append.is_empty() {
            return Ok((playlist.name, 0));
        }
        if playlist.tracks.len().saturating_add(append.len()) > MAX_TRACKS {
            return Err(format!(
                "A playlist may contain at most {MAX_TRACKS} tracks"
            ));
        }
        let added = append.len();
        playlist.tracks.extend(append);
        self.write_private(&filename, &playlist.tracks)?;
        Ok((playlist.name, added))
    }

    pub fn remove_track(&self, path: &Path, index: usize) -> Result<(), String> {
        let _operation = self.operation.lock().map_err(|error| error.to_string())?;
        let filename = self.private_filename(path)?;
        let current = self.read_private(&filename)?;
        let mut playlist = self.parse(&self.directory.join(&filename), &current, false)?;
        if index >= playlist.tracks.len() {
            return Err("Playlist track index is out of range".into());
        }
        playlist.tracks.remove(index);
        self.write_private(&filename, &playlist.tracks)
    }

    pub fn delete(&self, path: &Path) -> Result<(), String> {
        let _operation = self.operation.lock().map_err(|error| error.to_string())?;
        if !self.lexically_private(path) {
            return self.delete_imported(path);
        }
        let filename = self.private_filename(path)?;
        // Opening with O_NOFOLLOW rejects directories and substituted symlinks.
        let _file = self.open_private(&filename)?;
        let filename = c_filename(&filename)?;
        if unsafe { libc::unlinkat(self.directory_handle.as_raw_fd(), filename.as_ptr(), 0) } != 0 {
            return Err(format!(
                "Delete playlist: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    fn delete_imported(&self, path: &Path) -> Result<(), String> {
        self.check_cancelled()?;
        let relative = path
            .strip_prefix(&self.music)
            .map_err(|_| "Playlist is outside the music folder")?;
        if !playlist_path(path)
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("Expected a music-folder M3U file".into());
        }
        let mut directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&self.music)
            .map_err(|error| error.to_string())?;
        // Walk through pinned directory handles: a substituted symlink cannot
        // redirect deletion to a playlist outside the music root.
        if let Some(parent) = relative.parent() {
            for part in parent.components() {
                let name = c_filename(part.as_os_str())?;
                let fd = unsafe {
                    libc::openat(
                        directory.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if fd < 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                directory = unsafe { File::from_raw_fd(fd) };
            }
        }
        let name = c_filename(relative.file_name().ok_or("Missing playlist filename")?)?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !file
            .metadata()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            return Err("Playlist must be a regular file".into());
        }
        self.check_cancelled()?;
        if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(format!(
                "Delete playlist: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    pub fn load(&self, path: &Path) -> Result<Playlist, String> {
        if self.lexically_private(path) {
            let filename = self.private_filename(path)?;
            let bytes = self.read_private(&filename)?;
            return self.parse(&self.directory.join(filename), &bytes, false);
        }
        if !path.is_absolute() {
            return Err("Imported playlist path must be absolute".into());
        }
        let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
        if !path.starts_with(&self.music) || !playlist_path(&path) || !path.is_file() {
            return Err("Imported playlist must be a music-folder M3U file".into());
        }
        // Do not turn a canonical alias of private data into an imported writable file.
        if path.starts_with(&self.directory) {
            return Err("Use the direct private playlist path".into());
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|error| error.to_string())?;
        let bytes = read_limited(file)?;
        self.parse(&path, &bytes, true)
    }

    fn lexically_private(&self, path: &Path) -> bool {
        !path.is_absolute() || path.starts_with(&self.directory)
    }

    fn private_filename(&self, path: &Path) -> Result<std::ffi::OsString, String> {
        self.ensure_directory()?;
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        {
            return Err("Playlist paths may not contain traversal components".into());
        }
        let direct = if path.is_absolute() {
            path.parent() == Some(self.directory.as_path())
        } else {
            path.components().count() == 1
        };
        if !direct || !playlist_path(path) {
            return Err("Imported music playlists are read-only".into());
        }
        let filename = path.file_name().ok_or("Missing playlist filename")?;
        c_filename(filename)?;
        Ok(filename.to_owned())
    }

    fn ensure_directory(&self) -> Result<(), String> {
        self.check_cancelled()?;
        let current = fs::symlink_metadata(&self.directory).map_err(|error| error.to_string())?;
        let pinned = self
            .directory_handle
            .metadata()
            .map_err(|error| error.to_string())?;
        if current.file_type().is_symlink()
            || !current.is_dir()
            || current.dev() != pinned.dev()
            || current.ino() != pinned.ino()
        {
            return Err("Private playlists directory changed; reopen the app".into());
        }
        Ok(())
    }

    fn open_private(&self, filename: &OsStr) -> Result<File, String> {
        self.ensure_directory()?;
        let filename = c_filename(filename)?;
        let fd = unsafe {
            libc::openat(
                self.directory_handle.as_raw_fd(),
                filename.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(format!(
                "Open private playlist: {}",
                std::io::Error::last_os_error()
            ));
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !file
            .metadata()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            return Err("Playlist must be a regular file".into());
        }
        Ok(file)
    }

    fn read_private(&self, filename: &OsStr) -> Result<Vec<u8>, String> {
        read_limited(self.open_private(filename)?)
    }

    fn write_private(&self, filename: &OsStr, tracks: &[PathBuf]) -> Result<(), String> {
        self.ensure_directory()?;
        let mut bytes = b"#EXTM3U\n".to_vec();
        for track in tracks {
            let text = track.to_str().ok_or("Playlist paths must be UTF-8")?;
            if text.contains(['\r', '\n']) {
                return Err("A playlist path may not contain a line break".into());
            }
            bytes.extend_from_slice(text.as_bytes());
            bytes.push(b'\n');
        }
        if bytes.len() as u64 > MAX_PLAYLIST_BYTES {
            return Err("Playlist is too large".into());
        }
        atomic_write_at(&self.directory_handle, filename, &bytes).map_err(|error| error.to_string())
    }

    fn music_track(&self, path: &Path) -> Result<PathBuf, String> {
        self.check_cancelled()?;
        let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
        let downloads = self.data.join("downloads");
        let private = fs::symlink_metadata(&downloads)
            .ok()
            .filter(|m| m.is_dir() && !m.file_type().is_symlink())
            .and_then(|_| fs::canonicalize(downloads).ok())
            .is_some_and(|root| root.starts_with(&self.data) && path.starts_with(root));
        if (!path.starts_with(&self.music) && !private) || !path.is_file() || !audio_path(&path) {
            return Err(
                "Playlist tracks must be audio files inside the music or private downloads folder"
                    .into(),
            );
        }
        let text = path.to_str().ok_or("Playlist paths must be UTF-8")?;
        if text.contains(['\r', '\n']) {
            return Err("A playlist path may not contain a line break".into());
        }
        Ok(path)
    }
    fn check_cancelled(&self) -> Result<(), String> {
        if self
            .cancelled
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Acquire))
        {
            Err("Cancelled".into())
        } else {
            Ok(())
        }
    }

    fn parse(&self, path: &Path, bytes: &[u8], read_only: bool) -> Result<Playlist, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "Playlist is not valid UTF-8")?;
        let base = path.parent().ok_or("Playlist has no parent folder")?;
        let mut tracks = Vec::new();
        let mut skipped = 0;
        for line in text.trim_start_matches('\u{feff}').lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.contains("://") || line.starts_with("\\\\") {
                skipped += 1;
                continue;
            }
            let entry = Path::new(line);
            let candidate = if entry.is_absolute() {
                entry.to_path_buf()
            } else {
                base.join(entry)
            };
            match self.music_track(&candidate) {
                Ok(track) => tracks.push(track),
                Err(_) => skipped += 1,
            }
            if tracks.len() > MAX_TRACKS {
                return Err(format!("Playlist exceeds {MAX_TRACKS} tracks"));
            }
        }
        if skipped > 0 {
            eprintln!(
                "Playlist {}: skipped {skipped} unavailable or non-local entries",
                path.display()
            );
        }
        Ok(Playlist {
            name: path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path: path.to_path_buf(),
            tracks,
            read_only,
        })
    }
}

fn playlist_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "m3u" | "m3u8"))
}

fn audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp3"
                    | "flac"
                    | "m4a"
                    | "m4b"
                    | "aac"
                    | "wav"
                    | "aif"
                    | "aiff"
                    | "ogg"
                    | "oga"
                    | "opus"
                    | "ape"
                    | "wv"
            )
        })
}

fn clean_name(name: &str) -> Result<String, String> {
    let mut name = name.trim();
    if name.to_ascii_lowercase().ends_with(".m3u8") {
        name = &name[..name.len() - 5];
    } else if name.to_ascii_lowercase().ends_with(".m3u") {
        name = &name[..name.len() - 4];
    }
    if name.is_empty() {
        return Err("Enter a playlist name".into());
    }
    if name.len() > 120
        || name.starts_with('.')
        || name.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
        })
    {
        return Err("Choose a playlist name without path separators or control characters".into());
    }
    Ok(name.into())
}

fn c_filename(name: &OsStr) -> Result<CString, String> {
    if name.as_bytes().is_empty() || name.as_bytes().contains(&b'/') || name == "." || name == ".."
    {
        return Err("Expected a single playlist filename".into());
    }
    CString::new(name.as_bytes()).map_err(|_| "Filename contains a NUL".into())
}

fn read_limited(file: File) -> Result<Vec<u8>, String> {
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_PLAYLIST_BYTES {
        return Err("Playlist is too large".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_PLAYLIST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_PLAYLIST_BYTES {
        return Err("Playlist is too large".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fixture(name: &str) -> (PathBuf, PlaylistStore, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "betterpod-rust-playlist-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let music = root.join("music");
        let song = music.join("Artista/È un album/01 Caffè.flac");
        fs::create_dir_all(song.parent().unwrap()).unwrap();
        fs::write(&song, b"music stays read-only").unwrap();
        let store = PlaylistStore::new(music, root.join("rust")).unwrap();
        (root, store, fs::canonicalize(song).unwrap())
    }

    #[test]
    fn private_unicode_playlist_roundtrips_and_mutations_preserve_music() {
        let (root, store, song) = fixture("unicode");
        let playlist = store.create("Caffè e musica").unwrap();
        // a song goes in once, however often it is added
        assert_eq!(
            store
                .append(&playlist.path, &[song.clone(), song.clone()])
                .unwrap(),
            ("Caffè e musica".to_string(), 1)
        );
        assert_eq!(
            store.append(&playlist.path, std::slice::from_ref(&song)).unwrap(),
            ("Caffè e musica".to_string(), 0)
        );
        let loaded = store.load(&playlist.path).unwrap();
        assert_eq!(loaded.name, "Caffè e musica");
        assert!(!loaded.read_only);
        assert_eq!(loaded.tracks, vec![song.clone()]);
        store.remove_track(&playlist.path, 0).unwrap();
        assert!(store.load(&playlist.path).unwrap().tracks.is_empty());
        assert!(store.create("Caffè e musica").is_err());
        assert!(store.remove_track(&playlist.path, 9).is_err());
        assert_eq!(fs::read(&song).unwrap(), b"music stays read-only");
        store.delete(&playlist.path).unwrap();
        assert!(!playlist.path.exists());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_relative_and_absolute_entries_are_read_only_and_root_bound() {
        let (root, store, song) = fixture("import");
        let outside = root.join("outside.flac");
        fs::write(&outside, b"outside").unwrap();
        let imported = store.music.join("Imported.M3U8");
        let contents = format!("\u{feff}#EXTM3U\r\n#EXTINF:1,Title\r\nArtista/È un album/01 Caffè.flac\r\n{}\r\n{}\nhttps://example.invalid/audio.mp3\nmissing.mp3\n", song.display(), outside.display());
        fs::write(&imported, &contents).unwrap();
        let loaded = store.load(&imported).unwrap();
        assert!(loaded.read_only);
        assert_eq!(loaded.tracks, vec![song.clone(), song.clone()]);
        assert!(store
            .append(&imported, std::slice::from_ref(&song))
            .is_err());
        assert!(store.remove_track(&imported, 0).is_err());
        assert_eq!(fs::read_to_string(&imported).unwrap(), contents);
        assert_eq!(store.list().unwrap().len(), 1);
        store.delete(&imported).unwrap();
        assert!(!imported.exists());
        assert!(song.exists());
        assert!(outside.exists());
        assert!(store.list().unwrap().is_empty());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_playlist_deletion_rejects_symlink_paths_and_preserves_targets() {
        use std::os::unix::fs::symlink;
        let (root, store, song) = fixture("delete-import-links");
        let outside = root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let list = outside.join("list.m3u");
        fs::write(&list, b"#EXTM3U\n").unwrap();
        symlink(&outside, store.music.join("linked")).unwrap();
        symlink(&list, store.music.join("alias.m3u")).unwrap();
        for path in [
            list.clone(),
            store.music.join("linked/list.m3u"),
            store.music.join("alias.m3u"),
            store.music.join("../outside/list.m3u"),
            song.clone(),
        ] {
            assert!(store.delete(&path).is_err(), "{}", path.display());
        }
        assert_eq!(fs::read(&list).unwrap(), b"#EXTM3U\n");
        assert!(song.exists());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn append_is_transactional_and_rejects_escaping_track_symlinks() {
        use std::os::unix::fs::symlink;
        let (root, store, song) = fixture("append");
        let playlist = store.create("Safe").unwrap();
        let before = fs::read(&playlist.path).unwrap();
        let outside = root.join("outside.flac");
        fs::write(&outside, b"outside").unwrap();
        let link = store.music.join("escape.flac");
        symlink(&outside, &link).unwrap();
        assert!(store.append(&playlist.path, &[song, link]).is_err());
        assert_eq!(fs::read(&playlist.path).unwrap(), before);
        for name in ["../Other", "a/b", "a\\b", ".", "..", "bad\nname", ""] {
            assert!(store.create(name).is_err(), "accepted {name:?}");
        }
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn own_completed_downloads_can_join_private_playlists() {
        let (root, store, song) = fixture("download-playlist");
        let downloaded = store.data.join("downloads/Shared/Album/new.flac");
        fs::create_dir_all(downloaded.parent().unwrap()).unwrap();
        fs::write(&downloaded, b"downloaded audio").unwrap();
        let playlist = store.create("Mixed sources").unwrap();
        store
            .append(&playlist.path, &[song, downloaded.clone()])
            .unwrap();
        let loaded = store.load(&playlist.path).unwrap();
        assert_eq!(loaded.tracks.len(), 2);
        assert!(loaded
            .tracks
            .contains(&fs::canonicalize(downloaded).unwrap()));
        let outside = root.join("outside.flac");
        fs::write(&outside, b"external").unwrap();
        let link = store.data.join("downloads/escape.flac");
        std::os::unix::fs::symlink(outside, &link).unwrap();
        assert!(store.append(&playlist.path, &[link]).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn private_playlist_symlinks_and_replaced_directory_cannot_redirect_writes() {
        use std::os::unix::fs::symlink;
        let (root, store, song) = fixture("symlink");
        let original = store.music.join("Original.m3u8");
        fs::write(&original, "#EXTM3U\n").unwrap();
        let link = store.directory.join("Alias.m3u8");
        symlink(&original, &link).unwrap();
        assert!(store.load(&link).is_err());
        assert!(store.append(&link, std::slice::from_ref(&song)).is_err());
        assert!(store.delete(&link).is_err());
        let moved = root.join("old-playlists");
        fs::rename(&store.directory, &moved).unwrap();
        symlink(&store.music, &store.directory).unwrap();
        assert!(store.create("Unexpected").is_err());
        assert!(!store.music.join("Unexpected.m3u8").exists());
        assert_eq!(fs::read_to_string(&original).unwrap(), "#EXTM3U\n");
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}

/// UI-facing facade. Construction and commands do all disk work on one worker.
pub enum Command {
    List,
    Create { name: String },
    Append { path: PathBuf, tracks: Vec<PathBuf> },
    Remove { path: PathBuf, index: usize },
    Delete { path: PathBuf },
}
pub enum Event {
    Listed(Vec<Playlist>),
    Saved,
    /// Songs added to a playlist; 0 when all of them were already in it.
    Appended { name: String, added: usize },
    Created(Playlist),
    Error(String),
}
pub struct PlaylistClient {
    tx: Option<std::sync::mpsc::SyncSender<Command>>,
    rx: Option<std::sync::mpsc::Receiver<Event>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl PlaylistClient {
    pub fn new(music: PathBuf, data: PathBuf) -> Self {
        let (tx, commands) = std::sync::mpsc::sync_channel::<Command>(16);
        let (events, rx) = std::sync::mpsc::sync_channel(16);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let spawn = std::thread::Builder::new()
            .name("playlists".into())
            .spawn(move || {
                let mut store = match PlaylistStore::new(music, data) {
                    Ok(store) => store,
                    Err(error) => {
                        let _ = events.send(Event::Error(error));
                        return;
                    }
                };
                store.cancelled = Some(worker_cancelled.clone());
                while let Ok(command) = commands.recv() {
                    if worker_cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    let result = match command {
                        Command::List => store.list().map(Event::Listed),
                        Command::Create { name } => store.create(&name).map(Event::Created),
                        Command::Append { path, tracks } => {
                            store
                                .append(&path, &tracks)
                                .map(|(name, added)| Event::Appended { name, added })
                        }
                        Command::Remove { path, index } => {
                            store.remove_track(&path, index).map(|_| Event::Saved)
                        }
                        Command::Delete { path } => store.delete(&path).map(|_| Event::Saved),
                    };
                    if events.send(result.unwrap_or_else(Event::Error)).is_err() {
                        break;
                    }
                }
            });
        let worker = spawn
            .map_err(|error| eprintln!("Could not start playlist worker: {error}"))
            .ok();
        Self {
            tx: Some(tx),
            rx: Some(rx),
            cancelled,
            worker,
        }
    }
    pub fn submit(&self, command: Command) -> Result<(), String> {
        self.tx
            .as_ref()
            .ok_or("Playlist worker is shut down")?
            .try_send(command)
            .map_err(|error| format!("Playlist worker queue: {error}"))
    }
    pub fn poll(&self) -> Option<Event> {
        self.rx.as_ref()?.try_recv().ok()
    }
    pub fn shutdown(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.tx.take();
        self.rx.take();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                eprintln!("Playlist worker stopped unexpectedly");
            }
        }
    }
}
impl Drop for PlaylistClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn shutdown_discards_queued_mutations_and_joins_before_root_removal() {
        let root =
            std::env::temp_dir().join(format!("betterpod-playlist-stop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let music = root.join("music");
        fs::create_dir_all(&music).unwrap();
        let mut client = PlaylistClient::new(music, root.join("rust"));
        for i in 0..64 {
            let _ = client.submit(Command::Create {
                name: format!("queued-{i}"),
            });
        }
        client.shutdown();
        assert!(client.submit(Command::List).is_err());
        fs::remove_dir_all(&root).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!root.exists());
        client.shutdown();
    }
}
