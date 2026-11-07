//! Relocate only owned path fields, before a database/music pointer switches.
//! Titles, searches, URLs, shared music and imported playlists are never edited.
use super::*;
const MAX_METADATA_BYTES: u64 = 64 * 1024 * 1024;
pub(super) struct Rewrite {
    pub path: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
}
fn mapped(value: &str, source: &Path, destination: &Path) -> Option<String> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return None;
    }
    let relative = path.strip_prefix(source).ok()?;
    if relative
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return None;
    }
    destination
        .join(relative)
        .to_str()
        .map(str::to_owned)
        .filter(|new| new != value)
}
fn path_value(value: &mut serde_json::Value, source: &Path, destination: &Path) {
    if let Some(next) = value.as_str().and_then(|s| mapped(s, source, destination)) {
        *value = next.into();
    }
}
fn cover_fields(value: &mut serde_json::Value, source: &Path, destination: &Path) {
    if let Some(object) = value.as_object_mut() {
        if let Some(cover) = object.get_mut("cover") {
            path_value(cover, source, destination);
        }
        if let Some(prior) = object.get_mut("prior") {
            cover_fields(prior, source, destination);
        }
    }
}
fn json_rewrite(
    bytes: &[u8],
    name: &str,
    source: &Path,
    destination: &Path,
    clear_playback: bool,
) -> Result<Vec<u8>, String> {
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("Cannot relocate {name}: {e}"))?;
    match name {
        "library.json" => {
            if let Some(object) = value.as_object_mut() {
                object.insert("version".into(), 0.into());
            }
        } // derived index: rescan safely
        "playback.json" => {
            if clear_playback {
                value = serde_json::json!({"queue":[],"index":0,"position":0,"paused":true});
            } else {
                for field in ["queue", "original_queue"] {
                    if let Some(queue) = value.get_mut(field).and_then(|v| v.as_array_mut()) {
                        for path in queue {
                            path_value(path, source, destination);
                        }
                    }
                }
            }
        }
        "curation.json" => {
            if let Some(tracks) = value.get_mut("tracks").and_then(|v| v.as_object_mut()) {
                let old = std::mem::take(tracks);
                for (key, mut entry) in old {
                    cover_fields(&mut entry, source, destination);
                    let key = mapped(&key, source, destination).unwrap_or(key);
                    if tracks.insert(key, entry).is_some() {
                        return Err(
                            "Relocating album customizations would merge distinct paths".into()
                        );
                    }
                }
            }
        }
        "podcasts.json" => {
            if let Some(podcasts) = value.as_array_mut() {
                for podcast in podcasts {
                    if let Some(files) = podcast.get_mut("files").and_then(|v| v.as_object_mut()) {
                        for file in files.values_mut() {
                            path_value(file, source, destination);
                        }
                    }
                }
            }
        }
        _ => return Ok(bytes.to_vec()),
    }
    // Keep byte-identical files that need no transformation.
    let original: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if value == original {
        Ok(bytes.to_vec())
    } else {
        serde_json::to_vec(&value).map_err(|e| e.to_string())
    }
}
pub(super) fn playlist_rewrite(
    bytes: &[u8],
    source: &Path,
    destination: &Path,
) -> Result<Vec<u8>, String> {
    let from = source
        .to_str()
        .ok_or("Music root is not UTF-8")?
        .trim_end_matches('/')
        .as_bytes();
    let to = destination
        .to_str()
        .ok_or("Music destination is not UTF-8")?
        .trim_end_matches('/')
        .as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        let mut end = line.len();
        while end > 0 && matches!(line[end - 1], b'\r' | b'\n') {
            end -= 1
        }
        let body = &line[..end];
        if body.starts_with(from)
            && body.get(from.len()) == Some(&b'/')
            && !body
                .split(|b| *b == b'/')
                .any(|part| part == b".." || part == b".")
        {
            output.extend_from_slice(to);
            output.extend_from_slice(&body[from.len()..]);
            output.extend_from_slice(&line[end..]);
        } else {
            output.extend_from_slice(line);
        }
    }
    Ok(output)
}
fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_METADATA_BYTES
    {
        return Err(format!(
            "Unsafe or oversized relocation metadata: {}",
            path.display()
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err("Relocation metadata grew while reading".into());
    }
    Ok(bytes)
}
pub(super) fn plan(
    data: &Path,
    source: &Path,
    destination: &Path,
    clear_playback: bool,
    ctx: &Context<'_>,
) -> Result<Vec<Rewrite>, String> {
    let mut rewrites = Vec::new();
    for name in [
        "library.json",
        "playback.json",
        "curation.json",
        "podcasts.json",
    ] {
        ctx.check()?;
        let path = data.join(name);
        if !path.try_exists().map_err(|e| e.to_string())? {
            continue;
        }
        let before = read_file(&path)?;
        let after = json_rewrite(&before, name, source, destination, clear_playback)?;
        if before != after {
            rewrites.push(Rewrite {
                path,
                before,
                after,
            });
        }
    }
    let directory = data.join("playlists");
    if directory.exists() {
        let metadata = fs::symlink_metadata(&directory).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Private playlists directory is unsafe".into());
        }
        for (index, entry) in fs::read_dir(&directory)
            .map_err(|e| e.to_string())?
            .enumerate()
        {
            ctx.check()?;
            if index >= 10_000 {
                return Err("Too many private playlists to relocate".into());
            }
            let path = entry.map_err(|e| e.to_string())?.path();
            if !path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("m3u") || s.eq_ignore_ascii_case("m3u8"))
            {
                continue;
            }
            let before = read_file(&path)?;
            let after = playlist_rewrite(&before, source, destination)?;
            if before != after {
                rewrites.push(Rewrite {
                    path,
                    before,
                    after,
                });
            }
        }
    }
    Ok(rewrites)
}
pub(super) fn apply(rewrites: &[Rewrite], ctx: &Context<'_>) -> Result<(), String> {
    apply_with(rewrites, ctx, |path, bytes| {
        atomic_write(path, bytes).map_err(|e| e.to_string())
    })
}
fn apply_with(
    rewrites: &[Rewrite],
    ctx: &Context<'_>,
    mut write: impl FnMut(&Path, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    for (index, rewrite) in rewrites.iter().enumerate() {
        let mut attempted = false;
        let result = (|| {
            ctx.check()?;
            if read_file(&rewrite.path)? != rewrite.before {
                return Err("Metadata changed during relocation".into());
            }
            attempted = true;
            write(&rewrite.path, &rewrite.after)
        })();
        if let Err(error) = result {
            // fsync(parent) may fail after atomic_write already renamed the
            // new file. Revert that committed write too; a failed pre-write
            // comparison must never overwrite somebody else's changed file.
            let committed =
                attempted && read_file(&rewrite.path).is_ok_and(|bytes| bytes == rewrite.after);
            rollback(&rewrites[..index + usize::from(committed)])?;
            return Err(error);
        }
    }
    Ok(())
}
pub(super) fn rollback(rewrites: &[Rewrite]) -> Result<(), String> {
    for rewrite in rewrites.iter().rev() {
        atomic_write(&rewrite.path, &rewrite.before)
            .map_err(|e| format!("Relocation rollback failed: {e}"))?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn context(f: impl FnOnce(&Context<'_>)) {
        let (tx, _rx) = mpsc::sync_channel(16);
        let cancel = AtomicBool::new(false);
        let ack = AtomicBool::new(true);
        let stop = AtomicBool::new(false);
        f(&Context {
            id: 1,
            events: &tx,
            cancel: &cancel,
            ack: &ack,
            stop: &stop,
        });
    }
    #[test]
    fn write_error_after_rename_reverts_current_and_previous_metadata() {
        let root =
            std::env::temp_dir().join(format!("betterpod-relocation-fsync-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let rewrites = (0..2)
            .map(|i| {
                let path = root.join(format!("{i}.json"));
                let before = format!("before{i}").into_bytes();
                let after = format!("after{i}").into_bytes();
                fs::write(&path, &before).unwrap();
                Rewrite {
                    path,
                    before,
                    after,
                }
            })
            .collect::<Vec<_>>();
        context(|ctx| {
            let mut calls = 0;
            let error = apply_with(&rewrites, ctx, |path, bytes| {
                calls += 1;
                atomic_write(path, bytes).map_err(|e| e.to_string())?;
                if calls == 2 {
                    Err("injected directory sync failure after rename".into())
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
            assert!(error.contains("after rename"));
            assert_eq!(calls, 2);
        });
        for rewrite in &rewrites {
            assert_eq!(fs::read(&rewrite.path).unwrap(), rewrite.before);
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn prewrite_external_change_is_kept_while_prior_rewrites_roll_back() {
        let root = std::env::temp_dir().join(format!(
            "betterpod-relocation-external-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let rewrites = (0..2)
            .map(|i| {
                let path = root.join(format!("{i}.json"));
                fs::write(&path, b"before").unwrap();
                Rewrite {
                    path,
                    before: b"before".to_vec(),
                    after: b"after".to_vec(),
                }
            })
            .collect::<Vec<_>>();
        fs::write(&rewrites[1].path, b"external edit").unwrap();
        context(|ctx| assert!(apply(&rewrites, ctx).unwrap_err().contains("changed")));
        assert_eq!(fs::read(&rewrites[0].path).unwrap(), b"before");
        assert_eq!(fs::read(&rewrites[1].path).unwrap(), b"external edit");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn both_playback_orders_follow_private_database_relocation() {
        let bytes=br#"{"queue":["/old/db/downloads/b.flac","/music/a.flac"],"original_queue":["/music/a.flac","/old/db/downloads/b.flac"],"index":1,"position":37.25,"paused":true}"#;
        let value: serde_json::Value = serde_json::from_slice(
            &json_rewrite(
                bytes,
                "playback.json",
                Path::new("/old/db"),
                Path::new("/new/db"),
                false,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(value["queue"][0], "/new/db/downloads/b.flac");
        assert_eq!(value["original_queue"][1], "/new/db/downloads/b.flac");
        assert_eq!(value["original_queue"][0], "/music/a.flac");
        assert_eq!(value["index"], 1);
    }
    #[test]
    fn remaps_paths_and_curation_keys_without_rewriting_titles_or_prefix_neighbors() {
        let source = Path::new("/old/db");
        let destination = Path::new("/new/db");
        let bytes=br#"{"tracks":{"/old/db/downloads/song.flac":{"album":"/old/db/album title","cover":"/old/db/chosen-covers/a.png","prior":{"cover":"/old/db/chosen-covers/b.png"}},"/old/db-other/x.flac":{}}}"#;
        let value: serde_json::Value = serde_json::from_slice(
            &json_rewrite(bytes, "curation.json", source, destination, false).unwrap(),
        )
        .unwrap();
        let entry = &value["tracks"]["/new/db/downloads/song.flac"];
        assert_eq!(entry["album"], "/old/db/album title");
        assert_eq!(entry["cover"], "/new/db/chosen-covers/a.png");
        assert_eq!(entry["prior"]["cover"], "/new/db/chosen-covers/b.png");
        assert!(value["tracks"].get("/old/db-other/x.flac").is_some());
    }
    #[test]
    fn playlist_remap_preserves_comments_relative_entries_and_line_endings() {
        let input=b"#EXTM3U\r\n#EXTINF:-1,/old/music/title\r\n/old/music/Album/song.flac\r\nrelative.flac\n/old/music-other/song.flac";
        let result =
            playlist_rewrite(input, Path::new("/old/music"), Path::new("/new/music")).unwrap();
        assert_eq!(result,b"#EXTM3U\r\n#EXTINF:-1,/old/music/title\r\n/new/music/Album/song.flac\r\nrelative.flac\n/old/music-other/song.flac");
        assert_eq!(
            playlist_rewrite(
                b"/old/music/caf\xe9.flac\n",
                Path::new("/old/music"),
                Path::new("/new/music")
            )
            .unwrap(),
            b"/new/music/caf\xe9.flac\n"
        );
    }
}
