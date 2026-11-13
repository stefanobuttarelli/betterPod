//! Public album-information providers and atomic tag edits. Selected fields only.
mod discogs;
mod wikipedia;
mod wikitext;
use crate::online::{base, encode, number, text, Context, OnlineItem, Provider};
use lofty::{
    file::TaggedFileExt,
    tag::{Accessor, ItemKey, Tag, TagExt},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TagEdit {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track: Option<u32>,
    pub track_total: Option<u32>,
    pub disc: Option<u32>,
    pub cover: Option<PathBuf>,
}
static MB_NEXT: Mutex<Option<Instant>> = Mutex::new(None);
pub(crate) fn mb_wait(context: &Context) -> Result<(), String> {
    let now = Instant::now();
    let at = {
        let mut next = MB_NEXT.lock().map_err(|e| e.to_string())?;
        let at = next.unwrap_or(now).max(now);
        *next = Some(at + Duration::from_millis(1100));
        at
    };
    context.sleep(at.saturating_duration_since(now))
}
fn credits(value: &Value) -> String {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .map(|v| {
                    let name = if text(v, "name").is_empty() {
                        text(&v["artist"], "name")
                    } else {
                        text(v, "name")
                    };
                    name + &text(v, "joinphrase")
                })
                .collect::<String>()
        })
        .unwrap_or_default()
        .trim()
        .into()
}
pub fn search(
    context: &Context,
    provider: Provider,
    query: &str,
) -> Result<(Vec<OnlineItem>, usize), String> {
    let items = match provider {
        Provider::Discogs => Some(discogs::search(context, query)?),
        Provider::Wikipedia => Some(wikipedia::search(context, query)?),
        _ => None,
    };
    if let Some(items) = items {
        let total = items.len();
        return Ok((items, total));
    }
    let value = match provider {
        Provider::MusicBrainz => {
            mb_wait(context)?;
            context.get_json(&format!(
                "{}/ws/2/release/?fmt=json&limit=10&dismax=true&query={}",
                base("BETTERPOD_RUST_MB_BASE", "https://musicbrainz.org"),
                encode(query)
            ))?
        }
        Provider::Itunes => context.get_json(&format!(
            "{}/search?entity=album&limit=12&term={}",
            base("BETTERPOD_RUST_ITUNES_BASE", "https://itunes.apple.com"),
            encode(query)
        ))?,
        Provider::Deezer => context.get_json(&format!(
            "{}/search/album?limit=12&q={}",
            base("BETTERPOD_RUST_DEEZER_BASE", "https://api.deezer.com"),
            encode(query)
        ))?,
        _ => return Err("Unsupported metadata provider".into()),
    };
    let rows: Vec<&Value> = match provider {
        Provider::MusicBrainz => value["releases"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default(),
        Provider::Itunes => value["results"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default(),
        Provider::Deezer => value["data"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default(),
        _ => vec![],
    };
    let mut items = Vec::new();
    for row in rows {
        let mut item = OnlineItem::new(provider);
        item.raw = row.clone();
        match provider {
            Provider::MusicBrainz => {
                item.id = text(row, "id");
                item.title = text(row, "title");
                item.artist = credits(&row["artist-credit"]);
                item.year = text(row, "date").chars().take(4).collect();
                item.count = number(row, "track-count") as usize;
                item.kind = text(&row["release-group"], "primary-type");
                item.cover = format!(
                    "https://coverartarchive.org/release/{}/front-500",
                    encode(&item.id)
                );
                item.url = format!("https://musicbrainz.org/release/{}", item.id);
            }
            Provider::Itunes => {
                item.id = text(row, "collectionId");
                item.title = text(row, "collectionName");
                item.artist = text(row, "artistName");
                item.year = text(row, "releaseDate").chars().take(4).collect();
                item.count = number(row, "trackCount") as usize;
                item.cover = text(row, "artworkUrl100").replace("100x100bb", "600x600bb");
                item.url = text(row, "collectionViewUrl");
            }
            Provider::Deezer => {
                item.id = text(row, "id");
                item.title = text(row, "title");
                item.artist = text(&row["artist"], "name");
                item.cover = text(row, "cover_xl");
                if item.cover.is_empty() {
                    item.cover = text(row, "cover_big");
                }
                item.url = text(row, "link");
                item.count = number(row, "nb_tracks") as usize;
            }
            _ => {}
        }
        if !item.id.is_empty() {
            items.push(item);
        }
    }
    let total = items.len();
    Ok((items, total))
}
pub fn details(context: &Context, mut item: OnlineItem) -> Result<OnlineItem, String> {
    match item.provider {
        Provider::Discogs => return discogs::details(context, item),
        // The search already read each article's fields and tracks.
        Provider::Wikipedia => return Ok(item),
        _ => {}
    }
    let mut songs = Vec::new();
    let value = match item.provider {
        Provider::MusicBrainz => {
            mb_wait(context)?;
            let value = context.get_json(&format!(
                "{}/ws/2/release/{}?fmt=json&inc=recordings%2Bartist-credits%2Bgenres",
                base("BETTERPOD_RUST_MB_BASE", "https://musicbrainz.org"),
                encode(&item.id)
            ))?;
            for medium in value["media"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
                for track in medium["tracks"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    let artist = credits(&track["artist-credit"]);
                    songs.push(json!({"disc":number(medium,"position")as u32,"number":number(track,"position")as u32,"title":if text(track,"title").is_empty(){text(&track["recording"],"title")}else{text(track,"title")},"artist":if artist.is_empty(){item.artist.clone()}else{artist},"seconds":number(track,"length")/1000.0}));
                }
            }
            value
        }
        Provider::Itunes => {
            let value = context.get_json(&format!(
                "{}/lookup?entity=song&limit=200&id={}",
                base("BETTERPOD_RUST_ITUNES_BASE", "https://itunes.apple.com"),
                encode(&item.id)
            ))?;
            for track in value["results"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[])
            {
                if text(track, "wrapperType") == "track" {
                    songs.push(json!({"disc":number(track,"discNumber")as u32,"number":number(track,"trackNumber")as u32,"title":text(track,"trackName"),"artist":text(track,"artistName"),"seconds":number(track,"trackTimeMillis")/1000.0}));
                }
            }
            value
        }
        Provider::Deezer => {
            let server = base("BETTERPOD_RUST_DEEZER_BASE", "https://api.deezer.com");
            let value = context.get_json(&format!("{server}/album/{}", encode(&item.id)))?;
            item.year = text(&value, "release_date").chars().take(4).collect();
            let mut tracks = value["tracks"].clone();
            for _ in 0..10 {
                for track in tracks["data"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
                    songs.push(json!({"disc":number(track,"disk_number").max(1.0)as u32,"number":if number(track,"track_position")>0.0{number(track,"track_position")as usize}else{songs.len()+1},"title":text(track,"title"),"artist":text(&track["artist"],"name"),"seconds":number(track,"duration")}));
                }
                let next = text(&tracks, "next");
                if next.is_empty() || !next.starts_with(&format!("{server}/")) {
                    break;
                }
                tracks = context.get_json(&next)?;
            }
            value
        }
        _ => return Ok(item),
    };
    item.count = songs.len().max(item.count);
    item.raw = value;
    let genre = normalized_genre(&item.raw);
    item.raw["genre"] = json!(genre);
    item.raw["tracks"] = Value::Array(songs);
    Ok(item)
}
fn normalized_genre(raw: &Value) -> String {
    let direct = text(raw, "genre");
    if !direct.is_empty() {
        return direct;
    }
    if let Some(genres) = raw["genres"].as_array() {
        return genres
            .iter()
            .max_by(|a, b| number(a, "count").total_cmp(&number(b, "count")))
            .map(|genre| text(genre, "name"))
            .unwrap_or_default();
    }
    if let Some(genre) = raw["genres"]["data"].as_array().and_then(|a| a.first()) {
        return text(genre, "name");
    }
    raw["results"]
        .as_array()
        .and_then(|a| {
            a.iter().find_map(|row| {
                let genre = text(row, "primaryGenreName");
                (!genre.is_empty()).then_some(genre)
            })
        })
        .unwrap_or_default()
}
pub fn write_tags(path: &Path, edit: &TagEdit) -> Result<(), String> {
    write_tags_impl(path, edit, None, false)
}
/// The LAN transaction holds the ordinary tag-writer lock from its conflict
/// check through publication, so an on-console edit cannot be overwritten.
pub(crate) struct RemoteTagWriter {
    _guard: std::sync::MutexGuard<'static, ()>,
}
pub(crate) fn lock_remote_tags() -> Result<RemoteTagWriter, String> {
    Ok(RemoteTagWriter {
        _guard: TAG_WRITE_LOCK.lock().map_err(|e| e.to_string())?,
    })
}
impl RemoteTagWriter {
    pub(crate) fn write(
        &self,
        path: &Path,
        edit: &TagEdit,
        remove_cover: bool,
    ) -> Result<(), String> {
        write_tags_unlocked(path, edit, None, remove_cover)
    }
}
pub fn write_tags_cancellable(
    path: &Path,
    edit: &TagEdit,
    context: &Context,
) -> Result<(), String> {
    write_tags_impl(path, edit, Some(context), false)
}
fn check(context: Option<&Context>) -> Result<(), String> {
    context.map_or(Ok(()), Context::check)
}
static TAG_WRITE_LOCK: Mutex<()> = Mutex::new(());
fn write_tags_impl(
    path: &Path,
    edit: &TagEdit,
    context: Option<&Context>,
    remove_cover: bool,
) -> Result<(), String> {
    let _write_lock = TAG_WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    write_tags_unlocked(path, edit, context, remove_cover)
}
fn write_tags_unlocked(
    path: &Path,
    edit: &TagEdit,
    context: Option<&Context>,
    remove_cover: bool,
) -> Result<(), String> {
    check(context)?;
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if matches!(extension.as_str(), "m4a" | "m4b") {
        return write_m4a(path, edit, context, remove_cover);
    }
    if !matches!(extension.as_str(), "mp3" | "flac" | "ogg" | "oga" | "opus") {
        return Err(format!("Tag writes are not supported for .{extension}"));
    }
    let temp = temporary(path)?;
    let result = (|| {
        copy_cancellable(path, &temp, context)?;
        let mut file = lofty::probe::Probe::open(&temp)
            .map_err(|e| e.to_string())?
            .guess_file_type()
            .map_err(|e| e.to_string())?
            .read()
            .map_err(|e| e.to_string())?;
        let kind = file.primary_tag_type();
        if file.primary_tag().is_none() {
            file.insert_tag(Tag::new(kind));
        }
        let tag = file.primary_tag_mut().ok_or("No editable tag")?;
        if remove_cover {
            while !tag.pictures().is_empty() {
                tag.remove_picture(0);
            }
        }
        for (key, value) in [
            (ItemKey::TrackTitle, &edit.title),
            (ItemKey::TrackArtist, &edit.artist),
            (ItemKey::AlbumTitle, &edit.album),
            (ItemKey::AlbumArtist, &edit.album_artist),
            (ItemKey::Genre, &edit.genre),
        ] {
            if let Some(value) = value {
                tag.remove_key(key);
                if !value.is_empty() {
                    tag.insert_text(key, value.clone());
                }
            }
        }
        if let Some(value) = edit.year {
            tag.remove_key(ItemKey::RecordingDate);
            tag.remove_key(ItemKey::Year);
            if value != 0 {
                tag.insert_text(ItemKey::RecordingDate, value.to_string());
            }
        }
        if let Some(value) = edit.track {
            if value == 0 {
                tag.remove_track()
            } else {
                tag.set_track(value)
            }
        }
        if let Some(value) = edit.track_total {
            if value == 0 {
                tag.remove_track_total()
            } else {
                tag.set_track_total(value)
            }
        }
        if let Some(value) = edit.disc {
            if value == 0 {
                tag.remove_disk()
            } else {
                tag.set_disk(value)
            }
        }
        if let Some(cover) = &edit.cover {
            let bytes = fs::read(cover).map_err(|e| e.to_string())?;
            if bytes.len() > 8 * 1024 * 1024 {
                return Err("Cover is too large".into());
            }
            let mut reader = std::io::Cursor::new(bytes);
            let mut picture =
                lofty::picture::Picture::from_reader(&mut reader).map_err(|e| e.to_string())?;
            picture.set_pic_type(lofty::picture::PictureType::CoverFront);
            tag.remove_picture_type(lofty::picture::PictureType::CoverFront);
            tag.push_picture(picture);
        }
        tag.save_to_path(&temp, lofty::config::WriteOptions::default())
            .map_err(|e| e.to_string())?;
        File::open(&temp)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        check(context)?;
        fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
fn temporary(path: &Path) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static ID: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or("Missing media folder")?;
    let filename = path
        .file_name()
        .ok_or("Missing media filename")?
        .to_string_lossy();
    for _ in 0..32 {
        let temp = parent.join(format!(
            ".{filename}.rust-tag-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(_) => return Ok(temp),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Cannot create tag staging file".into())
}
fn copy_cancellable(from: &Path, to: &Path, context: Option<&Context>) -> Result<(), String> {
    let mut input = File::open(from).map_err(|e| e.to_string())?;
    let mut output = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(to)
        .map_err(|e| e.to_string())?;
    let permissions = input.metadata().map_err(|e| e.to_string())?.permissions();
    let mut buffer = [0u8; 65536];
    loop {
        check(context)?;
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
    }
    let _ = output.set_permissions(permissions);
    Ok(())
}
#[derive(Clone, Debug)]
struct BoxInfo {
    start: u64,
    body: u64,
    end: u64,
    kind: [u8; 4],
    zero: bool,
}
fn boxes(reader: &mut impl ReadSeek, start: u64, end: u64) -> Result<Vec<BoxInfo>, String> {
    let mut at = start;
    let mut out = Vec::new();
    while at < end {
        if end - at < 8 {
            return Err("Incomplete M4A box list".into());
        }
        reader
            .seek(SeekFrom::Start(at))
            .map_err(|e| e.to_string())?;
        let mut h = [0u8; 8];
        reader.read_exact(&mut h).map_err(|e| e.to_string())?;
        let size = u32::from_be_bytes(h[..4].try_into().unwrap());
        let mut header = 8;
        let length = if size == 1 {
            let mut ext = [0u8; 8];
            reader.read_exact(&mut ext).map_err(|e| e.to_string())?;
            header = 16;
            u64::from_be_bytes(ext)
        } else if size == 0 {
            end - at
        } else {
            u64::from(size)
        };
        if length < header || length > end - at {
            return Err("Invalid M4A box size".into());
        }
        out.push(BoxInfo {
            start: at,
            body: at + header,
            end: at + length,
            kind: h[4..].try_into().unwrap(),
            zero: size == 0,
        });
        at += length;
    }
    Ok(out)
}
trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}
fn make_box(kind: [u8; 4], body: &[u8]) -> Result<Vec<u8>, String> {
    let length = body
        .len()
        .checked_add(8)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("M4A tag is too large")?;
    let mut out = length.to_be_bytes().to_vec();
    out.extend_from_slice(&kind);
    out.extend_from_slice(body);
    Ok(out)
}
fn edit_child(
    body: &[u8],
    kind: [u8; 4],
    mut edit: impl FnMut(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    let list = boxes(&mut std::io::Cursor::new(body), 0, body.len() as u64)?;
    let mut found = false;
    let mut out = Vec::new();
    for b in list {
        if b.zero {
            return Err("Nested size-zero M4A box cannot be edited".into());
        }
        if b.kind == kind {
            if found {
                return Err("Duplicate M4A metadata container".into());
            }
            found = true;
            out.extend(make_box(
                kind,
                &edit(&body[b.body as usize..b.end as usize])?,
            )?);
        } else {
            out.extend_from_slice(&body[b.start as usize..b.end as usize]);
        }
    }
    if !found {
        out.extend(make_box(kind, &edit(&[])?)?);
    }
    Ok(out)
}
fn edit_meta(body: &[u8], edit: &TagEdit, remove_cover: bool) -> Result<Vec<u8>, String> {
    let mut body = body.to_vec();
    if body.is_empty() {
        body = vec![0; 4];
        let mut h = vec![0; 8];
        h.extend_from_slice(b"mdirappl");
        h.extend_from_slice(&[0; 9]);
        body.extend(make_box(*b"hdlr", &h)?);
    }
    if body.len() < 4 || body[..4] != [0; 4] {
        return Err("Unsupported M4A metadata version".into());
    }
    let list = boxes(&mut std::io::Cursor::new(&body), 4, body.len() as u64)?;
    let mut handler = false;
    for b in &list {
        if b.kind == *b"keys" {
            return Err("Keyed M4A metadata is not editable".into());
        }
        if b.kind == *b"hdlr" {
            if handler
                || b.end - b.body < 12
                || body[(b.body + 8) as usize..(b.body + 12) as usize] != *b"mdir"
            {
                return Err("Unsupported M4A metadata handler".into());
            }
            handler = true;
        }
    }
    let mut out = vec![0; 4];
    if !handler {
        let mut h = vec![0; 8];
        h.extend_from_slice(b"mdirappl");
        h.extend_from_slice(&[0; 9]);
        out.extend(make_box(*b"hdlr", &h)?);
    }
    out.extend(edit_child(&body[4..], *b"ilst", |body| {
        edit_items(body, edit, remove_cover)
    })?);
    Ok(out)
}
fn edit_items(body: &[u8], edit: &TagEdit, remove_cover: bool) -> Result<Vec<u8>, String> {
    let list = boxes(&mut std::io::Cursor::new(body), 0, body.len() as u64)?;
    let mut drop = std::collections::HashSet::new();
    let mut added = Vec::new();
    if remove_cover {
        drop.insert(*b"covr");
    }
    for (key, value) in [
        ([0xa9, b'n', b'a', b'm'], &edit.title),
        ([0xa9, b'A', b'R', b'T'], &edit.artist),
        (*b"aART", &edit.album_artist),
        ([0xa9, b'a', b'l', b'b'], &edit.album),
        ([0xa9, b'g', b'e', b'n'], &edit.genre),
    ] {
        if let Some(value) = value {
            drop.insert(key);
            if key == [0xa9, b'A', b'R', b'T'] {
                drop.insert([0xa9, b'a', b'r', b't']);
            }
            if key == [0xa9, b'g', b'e', b'n'] {
                drop.insert(*b"gnre");
            }
            if !value.is_empty() {
                let mut data = vec![0, 0, 0, 1, 0, 0, 0, 0];
                data.extend_from_slice(value.as_bytes());
                added.extend(make_box(key, &make_box(*b"data", &data)?)?);
            }
        }
    }
    if let Some(year) = edit.year {
        let key = [0xa9, b'd', b'a', b'y'];
        drop.insert(key);
        if year > 0 {
            let mut data = vec![0, 0, 0, 1, 0, 0, 0, 0];
            data.extend_from_slice(year.to_string().as_bytes());
            added.extend(make_box(key, &make_box(*b"data", &data)?)?);
        }
    }
    for (key, number, total) in [
        (*b"trkn", edit.track, edit.track_total),
        (*b"disk", edit.disc, None),
    ] {
        if number.is_none() && total.is_none() {
            continue;
        }
        drop.insert(key);
        let mut data = vec![0; 16];
        if let Some(old) = list.iter().find(|b| b.kind == key) {
            let children = boxes(&mut std::io::Cursor::new(body), old.body, old.end)?;
            if let Some(old) = children
                .iter()
                .find(|b| b.kind == *b"data" && b.end - b.body >= 14)
            {
                let n = (old.end - old.body).min(16) as usize;
                data[..n].copy_from_slice(&body[old.body as usize..old.body as usize + n]);
            }
        }
        for (offset, value) in [(10, number), (12, total)] {
            if let Some(value) = value {
                let number = u16::try_from(value).map_err(|_| "Track/disc number is too large")?;
                data[offset..offset + 2].copy_from_slice(&number.to_be_bytes());
            }
        }
        if data[10..14] != [0; 4] {
            added.extend(make_box(key, &make_box(*b"data", &data)?)?);
        }
    }
    if let Some(cover) = &edit.cover {
        let bytes = fs::read(cover).map_err(|e| e.to_string())?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("Cover is too large".into());
        }
        let format = image::guess_format(&bytes).map_err(|e| e.to_string())?;
        let kind = match format {
            image::ImageFormat::Jpeg => 13,
            image::ImageFormat::Png => 14,
            _ => return Err("M4A covers must be JPEG or PNG".into()),
        };
        let mut data = vec![0, 0, 0, kind, 0, 0, 0, 0];
        data.extend(bytes);
        drop.insert(*b"covr");
        added.extend(make_box(*b"covr", &make_box(*b"data", &data)?)?);
    }
    let mut out = Vec::new();
    for b in list {
        if b.zero {
            return Err("Nested size-zero tag cannot be edited".into());
        }
        if !drop.contains(&b.kind) {
            out.extend_from_slice(&body[b.start as usize..b.end as usize]);
        }
    }
    out.extend(added);
    Ok(out)
}
fn write_m4a(
    path: &Path,
    edit: &TagEdit,
    context: Option<&Context>,
    remove_cover: bool,
) -> Result<(), String> {
    let mut source = File::open(path).map_err(|e| e.to_string())?;
    let length = source.metadata().map_err(|e| e.to_string())?.len();
    let top = boxes(&mut source, 0, length)?;
    let moovs: Vec<_> = top.iter().filter(|b| b.kind == *b"moov").collect();
    if moovs.len() != 1
        || !top.iter().any(|b| b.kind == *b"mdat")
        || top
            .first()
            .is_none_or(|b| b.kind != *b"ftyp" || b.body != 8)
    {
        return Err("M4A movie/audio header is missing or ambiguous".into());
    }
    let moov = moovs[0];
    if moov.end - moov.body > 16 * 1024 * 1024 || top.iter().any(|b| b.kind == *b"moof") {
        return Err("Fragmented or oversized M4A metadata is not editable".into());
    }
    let mut body = vec![0; (moov.end - moov.body) as usize];
    source
        .seek(SeekFrom::Start(moov.body))
        .and_then(|_| source.read_exact(&mut body))
        .map_err(|e| e.to_string())?;
    let children = boxes(&mut std::io::Cursor::new(&body), 0, body.len() as u64)?;
    if children
        .iter()
        .any(|b| matches!(&b.kind, b"mvex" | b"cmov" | b"meta"))
    {
        return Err("Unsupported M4A movie layout".into());
    }
    let body = edit_child(&body, *b"udta", |udta| {
        edit_child(udta, *b"meta", |meta| edit_meta(meta, edit, remove_cover))
    })?;
    if body.len() > 16 * 1024 * 1024 - 8 {
        return Err("Edited M4A metadata is too large".into());
    }
    let updated = make_box(*b"moov", &body)?;
    let copy_end = if moov.end == length {
        moov.start
    } else {
        length
    };
    let temporary = temporary(path)?;
    let result = (|| {
        let mut output = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        let _ = output.set_permissions(source.metadata().map_err(|e| e.to_string())?.permissions());
        source.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut remaining = copy_end;
        let mut buffer = [0; 65536];
        while remaining > 0 {
            check(context)?;
            let want = (remaining as usize).min(buffer.len());
            source
                .read_exact(&mut buffer[..want])
                .map_err(|e| e.to_string())?;
            output
                .write_all(&buffer[..want])
                .map_err(|e| e.to_string())?;
            remaining -= want as u64;
        }
        if copy_end == length {
            output
                .seek(SeekFrom::Start(moov.start + 4))
                .and_then(|_| output.write_all(b"free"))
                .map_err(|e| e.to_string())?;
        }
        for b in &top {
            if b.body - b.start == 16 && b.end <= copy_end {
                let size = u32::try_from(b.end - b.start - 8)
                    .map_err(|_| "M4A extended box is too large")?;
                output
                    .seek(SeekFrom::Start(b.start))
                    .map_err(|e| e.to_string())?;
                output
                    .write_all(&[0, 0, 0, 8, b'f', b'r', b'e', b'e'])
                    .map_err(|e| e.to_string())?;
                output
                    .write_all(&size.to_be_bytes())
                    .map_err(|e| e.to_string())?;
                output
                    .write_all(if b.start == moov.start {
                        b"free"
                    } else {
                        &b.kind
                    })
                    .map_err(|e| e.to_string())?;
            }
        }
        if let Some(last) = top.last().filter(|b| b.zero && copy_end == length) {
            let size = u32::try_from(last.end - last.start)
                .map_err(|_| "Size-zero M4A box is too large")?;
            output
                .seek(SeekFrom::Start(last.start))
                .and_then(|_| output.write_all(&size.to_be_bytes()))
                .map_err(|e| e.to_string())?;
        }
        output
            .seek(SeekFrom::Start(copy_end))
            .and_then(|_| output.write_all(&updated))
            .and_then(|_| output.sync_all())
            .map_err(|e| e.to_string())?;
        check(context)?;
        drop(output);
        drop(source);
        fs::rename(&temporary, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

/// Match a complete numbered tracklist by order before considering old titles.
/// For partial lists, only unambiguous title/duration matches replace song fields.
#[derive(Clone, Debug)]
pub struct AlbumInfoPlan {
    pub edits: Vec<(PathBuf, TagEdit)>,
    pub matched: usize,
    pub total: usize,
}

/// Load editable fields only. Artwork remains in its existing display cache;
/// reading tags never extracts or writes embedded pictures.
pub fn read_album_tags(
    paths: &[PathBuf],
    context: &Context,
) -> Result<Vec<(PathBuf, TagEdit)>, String> {
    let mut tags = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        context.check()?;
        context.progress(
            index as u64,
            Some(paths.len() as u64),
            format!("Reading tags · {} of {}", index + 1, paths.len()),
        );
        tags.push((path.clone(), read_tag_fields(path)?));
    }
    Ok(tags)
}
pub(crate) fn read_tag_fields(path: &Path) -> Result<TagEdit, String> {
    let file = lofty::probe::Probe::open(path)
        .map_err(|e| e.to_string())?
        .guess_file_type()
        .map_err(|e| e.to_string())?
        .options(
            lofty::config::ParseOptions::new()
                .read_cover_art(false)
                .read_properties(false),
        )
        .read()
        .map_err(|e| e.to_string())?;
    let tag = file.primary_tag().or_else(|| file.first_tag());
    Ok(TagEdit {
        title: Some(
            tag.and_then(|t| t.title())
                .map(|s| s.into_owned())
                .unwrap_or_default(),
        ),
        artist: Some(
            tag.and_then(|t| t.artist())
                .map(|s| s.into_owned())
                .unwrap_or_default(),
        ),
        album: Some(
            tag.and_then(|t| t.album())
                .map(|s| s.into_owned())
                .unwrap_or_default(),
        ),
        album_artist: Some(
            tag.and_then(|t| t.get_string(ItemKey::AlbumArtist))
                .unwrap_or_default()
                .to_owned(),
        ),
        genre: Some(
            tag.and_then(|t| t.genre())
                .map(|s| s.into_owned())
                .unwrap_or_default(),
        ),
        year: Some(
            tag.and_then(|t| {
                t.get_string(ItemKey::RecordingDate)
                    .or_else(|| t.get_string(ItemKey::Year))
            })
            .and_then(|v| v.get(..4).unwrap_or(v).parse::<u32>().ok())
            .unwrap_or(0),
        ),
        track: Some(tag.and_then(|t| t.track()).unwrap_or(0)),
        track_total: Some(tag.and_then(|t| t.track_total()).unwrap_or(0)),
        disc: Some(tag.and_then(|t| t.disk()).unwrap_or(0)),
        cover: None,
    })
}
pub fn write_album_tags(edits: &[(PathBuf, TagEdit)], context: &Context) -> Result<(), String> {
    for (index, (path, edit)) in edits.iter().enumerate() {
        context.check()?;
        context.progress(
            index as u64,
            Some(edits.len() as u64),
            format!("Saving tags · {} of {}", index + 1, edits.len()),
        );
        write_tags_cancellable(path, edit, context)?;
    }
    Ok(())
}
/// Compute source-field replacements with the same complete/partial matching
/// as Save, without changing any audio file or private curation state.
pub fn preview_album_info(
    paths: &[PathBuf],
    item: &OnlineItem,
    context: &Context,
) -> Result<AlbumInfoPlan, String> {
    album_info_plan(paths, item, Some(context))
}
pub fn apply_album_info(
    paths: &[PathBuf],
    item: &OnlineItem,
    context: &Context,
) -> Result<usize, String> {
    let plan = preview_album_info(paths, item, context)?;
    write_album_tags(&plan.edits, context)?;
    Ok(plan.matched)
}

fn album_info_plan(
    paths: &[PathBuf],
    item: &OnlineItem,
    context: Option<&Context>,
) -> Result<AlbumInfoPlan, String> {
    let theirs = item.raw["tracks"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut numbered: Vec<_> = theirs
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let n = number(t, "number") as u32;
            let d = (number(t, "disc") as u32).max(1);
            (n > 0).then_some(((d, n), i))
        })
        .collect();
    let unique = numbered
        .iter()
        .map(|(key, _)| *key)
        .collect::<std::collections::HashSet<_>>()
        .len()
        == theirs.len();
    let complete = theirs.len() == paths.len()
        && !paths.is_empty()
        && numbered.len() == theirs.len()
        && unique;
    let mut ours = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        check(context)?;
        let file = lofty::probe::Probe::open(path)
            .map_err(|e| e.to_string())?
            .guess_file_type()
            .map_err(|e| e.to_string())?
            .options(lofty::config::ParseOptions::new().read_cover_art(false))
            .read()
            .map_err(|e| e.to_string())?;
        let tag = file.primary_tag().or_else(|| file.first_tag());
        use lofty::file::AudioFile;
        let duration = file.properties().duration().as_secs_f64();
        let title = tag
            .and_then(|t| t.title())
            .map(|s| s.into_owned())
            .unwrap_or_default();
        let disc = tag.and_then(|t| t.disk()).unwrap_or(1).max(1);
        let track = tag.and_then(|t| t.track()).unwrap_or(index as u32 + 1);
        let prefix = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .take_while(char::is_ascii_digit)
            .take(3)
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0);
        ours.push((index, disc, track, prefix, title, duration));
    }
    let mut mapping = vec![None; paths.len()];
    if complete {
        numbered.sort_by_key(|(key, _)| *key);
        let youtube = paths.iter().all(|path| {
            path.components().any(|component| {
                component
                    .as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("YouTube")
            })
        });
        let unique_prefix = ours
            .iter()
            .map(|t| t.3)
            .collect::<std::collections::HashSet<_>>()
            .len()
            == ours.len()
            && ours.iter().all(|t| t.3 > 0);
        ours.sort_by_key(|t| {
            if youtube && unique_prefix {
                (1, t.3, t.0)
            } else {
                (t.1, t.2, t.0)
            }
        });
        for (ours, (_, theirs)) in ours.iter().zip(numbered.iter()) {
            mapping[ours.0] = Some(*theirs);
        }
    } else {
        let mut used = std::collections::HashSet::new();
        for (index, _, _, _, title, duration) in &ours {
            let norm = normalize_title(title);
            let exact: Vec<_> = theirs
                .iter()
                .enumerate()
                .filter(|(i, t)| {
                    !used.contains(i)
                        && !norm.is_empty()
                        && normalize_title(&text(t, "title")) == norm
                })
                .map(|(i, _)| i)
                .collect();
            let matched = if exact.len() == 1 {
                Some(exact[0])
            } else if *duration > 0.0 {
                let mut candidates: Vec<_> = theirs
                    .iter()
                    .enumerate()
                    .filter(|(i, t)| !used.contains(i) && number(t, "seconds") > 0.0)
                    .map(|(i, t)| (i, (number(t, "seconds") - duration).abs()))
                    .filter(|(_, difference)| *difference <= 2.0)
                    .collect();
                candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
                if candidates.first().is_some_and(|first| {
                    candidates
                        .get(1)
                        .is_none_or(|second| second.1 - first.1 > 0.25)
                }) {
                    Some(candidates[0].0)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(matched) = matched {
                mapping[*index] = Some(matched);
                used.insert(matched);
            }
        }
    }
    let genre = normalized_genre(&item.raw);
    let matched = mapping.iter().flatten().count();
    let mut edits = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        check(context)?;
        let mut edit = TagEdit {
            album: (!item.title.is_empty()).then_some(item.title.clone()),
            album_artist: (!item.artist.is_empty()).then_some(item.artist.clone()),
            year: item.year.parse::<u32>().ok().filter(|year| *year > 0),
            genre: (!genre.is_empty()).then_some(genre.clone()),
            ..Default::default()
        };
        if let Some(song) = mapping[index] {
            let song = &theirs[song];
            let title = text(song, "title");
            edit.title = (!title.is_empty()).then_some(title);
            let artist = text(song, "artist");
            edit.artist = Some(if artist.is_empty() {
                item.artist.clone()
            } else {
                artist
            })
            .filter(|s| !s.is_empty());
            edit.track = (number(song, "number") > 0.0).then_some(number(song, "number") as u32);
            edit.disc = (number(song, "disc") > 0.0).then_some(number(song, "disc") as u32);
            if complete {
                edit.track_total = Some(
                    theirs
                        .iter()
                        .filter(|t| number(t, "disc").max(1.0) == number(song, "disc").max(1.0))
                        .count() as u32,
                );
            }
        }
        if let Some(context) = context {
            context.progress(
                index as u64,
                Some(paths.len() as u64),
                format!(
                    "Preview metadata · {} of {} matched",
                    mapping.iter().flatten().count(),
                    paths.len()
                ),
            );
        }
        edits.push((path.clone(), edit));
    }
    Ok(AlbumInfoPlan {
        edits,
        matched,
        total: paths.len(),
    })
}
fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A real YouTube download (format 251, Opus copied into Ogg) takes the
    /// tags and cover a download writes. The edited copy is left beside the
    /// fixture, so an independent decoder can compare its audio.
    #[test]
    #[ignore = "requires BETTERPOD_OPUS_FIXTURE, a real .opus download"]
    fn real_opus_download_takes_tags_and_cover() {
        use lofty::file::AudioFile;
        let input = PathBuf::from(std::env::var_os("BETTERPOD_OPUS_FIXTURE").expect("opus file"));
        let output = input.with_file_name("edited.opus");
        let cover = input.with_file_name("cover.png");
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(64, 64)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        fs::write(&cover, png.into_inner()).unwrap();
        fs::copy(&input, &output).unwrap();
        write_tags(
            &output,
            &TagEdit {
                title: Some("Never Gonna Give You Up".into()),
                artist: Some("Rick Astley".into()),
                album: Some("Whenever You Need Somebody".into()),
                track: Some(1),
                track_total: Some(10),
                cover: Some(cover),
                ..Default::default()
            },
        )
        .unwrap();
        let before = lofty::read_from_path(&input).unwrap();
        let after = lofty::read_from_path(&output).unwrap();
        let tag = after.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("Never Gonna Give You Up"));
        assert_eq!(tag.artist().as_deref(), Some("Rick Astley"));
        assert_eq!(tag.album().as_deref(), Some("Whenever You Need Somebody"));
        assert_eq!((tag.track(), tag.track_total()), (Some(1), Some(10)));
        assert_eq!(tag.pictures().len(), 1);
        let (a, b) = (
            before.properties().duration().as_secs_f64(),
            after.properties().duration().as_secs_f64(),
        );
        assert!((a - b).abs() < 0.01, "duration {a} became {b}");
        assert_eq!(after.properties().sample_rate(), Some(48000));
    }
    /// Rapadopa exists only on Discogs and the Italian Wikipedia.
    #[test]
    #[ignore = "live network: Discogs and Wikipedia"]
    fn live_rapadopa_on_discogs_and_wikipedia() {
        let context = Context::standalone();
        let (found, _) = search(&context, Provider::Discogs, "Rapadopa DJ Gruff").unwrap();
        let first = found.first().expect("a Discogs result");
        assert_eq!(
            (first.title.as_str(), first.artist.as_str()),
            ("Rapadopa", "DJ Gruff")
        );
        assert!(first.raw["_source"].as_str().unwrap().contains("CD"));
        let album = details(&context, first.clone()).unwrap();
        let tracks = album.raw["tracks"].as_array().unwrap();
        assert_eq!((album.year.as_str(), tracks.len()), ("1993", 25));
        assert_eq!(tracks[0]["title"], "La Rapadopa");
        assert_eq!(tracks[0]["seconds"], 293.0);
        assert_eq!(album.raw["genre"], "Hip Hop");

        let link = "https://www.discogs.com/it/release/2621910-DJ-Gruff-Rapadopa?srsltid=x";
        let (linked, _) = search(&context, Provider::Discogs, link).unwrap();
        assert_eq!((linked.len(), linked[0].count), (1, 25));

        let (found, _) = search(&context, Provider::Wikipedia, "Rapadopa DJ Gruff").unwrap();
        let article = found
            .iter()
            .find(|item| item.title == "Rapadopa")
            .expect("the Italian article");
        assert_eq!(article.raw["_source"], "Wikipedia (it)");
        assert_eq!(
            (article.artist.as_str(), article.year.as_str()),
            ("DJ Gruff", "1993")
        );
        assert_eq!(article.count, 26);
        let (found, _) = search(&context, Provider::Wikipedia, "OK Computer Radiohead").unwrap();
        assert_eq!(found[0].raw["_source"], "Wikipedia");
        assert_eq!(found[0].count, 12);
    }
    #[test]
    #[ignore = "Requires local BETTERPOD_METADATA_FIXTURES; no network or user files"]
    fn album_preview_reads_all_fields_and_changes_nothing_until_save() {
        let fixtures = PathBuf::from(
            std::env::var_os("BETTERPOD_METADATA_FIXTURES").expect("local codec fixtures"),
        );
        let root = temporary_dir("preview-editor");
        let mut paths = Vec::new();
        let mut bytes = Vec::new();
        for (name, extension) in [
            ("aac", "m4a"),
            ("alac", "m4a"),
            ("mp3", "mp3"),
            ("flac", "flac"),
            ("ogg", "ogg"),
        ] {
            let path = root.join(format!("{name}.{extension}"));
            fs::copy(fixtures.join(format!("{name}-original.{extension}")), &path).unwrap();
            let tag = read_tag_fields(&path).unwrap();
            assert_eq!(tag.track, Some(2));
            assert_eq!(tag.track_total, Some(11));
            assert_eq!(tag.year, Some(1998));
            assert!(tag.title.as_ref().is_some_and(|v| !v.is_empty()));
            bytes.push(fs::read(&path).unwrap());
            paths.push(path);
        }
        let mut item = OnlineItem::new(Provider::MusicBrainz);
        item.title = "Preview Album".into();
        item.artist = "Preview Album Artist".into();
        item.year = "2026".into();
        item.raw = json!({"tracks":(0..5).map(|i|json!({"number":i+1,"disc":1,"title":format!("New Song {}",i+1),"artist":format!("Per-Song Artist {}",i+1)})).collect::<Vec<_>>()});
        let plan = album_info_plan(&paths, &item, None).unwrap();
        assert_eq!((plan.matched, plan.total), (5, 5));
        for (index, (path, patch)) in plan.edits.iter().enumerate() {
            assert_eq!(
                fs::read(path).unwrap(),
                bytes[index],
                "preview wrote a file"
            );
            assert_eq!(patch.title, Some(format!("New Song {}", index + 1)));
            assert_eq!(patch.track_total, Some(5));
            assert_eq!(patch.genre, None);
        }
        for (index, (path, patch)) in plan.edits.iter().enumerate() {
            write_tags(path, patch).unwrap();
            let tag = read_tag_fields(path).unwrap();
            assert_eq!(tag.title, Some(format!("New Song {}", index + 1)));
            assert_eq!(tag.artist, Some(format!("Per-Song Artist {}", index + 1)));
            assert_eq!(tag.album_artist, Some("Preview Album Artist".into()));
            assert_eq!(tag.track_total, Some(5));
            assert_eq!(tag.year, Some(2026));
        }
        fs::remove_dir_all(root).unwrap();
    }
    fn temporary_dir(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("betterpod-rust-tags-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }
    fn data(key: [u8; 4], value: &[u8]) -> Vec<u8> {
        let mut body = vec![0, 0, 0, 1, 0, 0, 0, 0];
        body.extend_from_slice(value);
        make_box(key, &make_box(*b"data", &body).unwrap()).unwrap()
    }
    #[test]
    fn m4a_edit_keeps_media_offsets_unknown_tags_and_does_not_grow_per_save() {
        let root = temporary_dir("m4a");
        let path = root.join("song.m4a");
        let mut ftyp = b"M4A \0\0\0\0isom".to_vec();
        ftyp = make_box(*b"ftyp", &ftyp).unwrap();
        let payload: Vec<_> = (0..4096).map(|i| (i % 251) as u8).collect();
        let media = make_box(*b"mdat", &payload).unwrap();
        let mut tags = data([0xa9, b'c', b'm', b't'], b"keep this comment");
        tags.extend(data(*b"aART", b"Album Artist"));
        tags.extend(
            make_box(
                *b"covr",
                &make_box(*b"data", b"untouched artwork bytes").unwrap(),
            )
            .unwrap(),
        );
        let ilst = make_box(*b"ilst", &tags).unwrap();
        let mut meta = vec![0; 4];
        let mut handler = vec![0; 8];
        handler.extend_from_slice(b"mdirappl");
        handler.extend_from_slice(&[0; 9]);
        meta.extend(make_box(*b"hdlr", &handler).unwrap());
        meta.extend(ilst);
        let moov = make_box(
            *b"moov",
            &make_box(*b"udta", &make_box(*b"meta", &meta).unwrap()).unwrap(),
        )
        .unwrap();
        let mut original = ftyp;
        original.extend(moov);
        let offset = original.len() + 8;
        original.extend(media);
        fs::write(&path, &original).unwrap();
        write_tags(
            &path,
            &TagEdit {
                title: Some("Changed".into()),
                track: Some(2),
                disc: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        let first = fs::read(&path).unwrap();
        assert_eq!(&first[offset..offset + payload.len()], payload);
        assert!(first
            .windows(b"keep this comment".len())
            .any(|w| w == b"keep this comment"));
        assert!(first
            .windows(b"untouched artwork bytes".len())
            .any(|w| w == b"untouched artwork bytes"));
        write_tags(
            &path,
            &TagEdit {
                title: Some("Another".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let second = fs::read(&path).unwrap();
        assert_eq!(first.len(), second.len());
        assert_eq!(&second[offset..offset + payload.len()], payload);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_m4a_is_not_modified() {
        let root = temporary_dir("invalid");
        let path = root.join("song.m4a");
        let bytes = b"not a movie";
        fs::write(&path, bytes).unwrap();
        assert!(write_tags(
            &path,
            &TagEdit {
                title: Some("Changed".into()),
                ..Default::default()
            }
        )
        .is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "Requires locally generated BETTERPOD_METADATA_FIXTURES; no network or user files"]
    fn real_codec_roundtrip_fixtures() {
        let root = PathBuf::from(
            std::env::var_os("BETTERPOD_METADATA_FIXTURES").expect("fixture directory"),
        );
        for (name, ext) in [
            ("aac", "m4a"),
            ("alac", "m4a"),
            ("mp3", "mp3"),
            ("flac", "flac"),
            ("ogg", "ogg"),
        ] {
            let input = root.join(format!("{name}-original.{ext}"));
            let output = root.join(format!("{name}-edited.{ext}"));
            let original = lofty::read_from_path(&input).unwrap();
            let original_tag = original
                .primary_tag()
                .or_else(|| original.first_tag())
                .unwrap();
            fs::copy(&input, &output).unwrap();
            write_tags(
                &output,
                &TagEdit {
                    title: Some("Rust Changed Title".into()),
                    artist: Some("Rust Changed Artist".into()),
                    album: Some("Rust Album".into()),
                    year: Some(2026),
                    track: Some(3),
                    disc: Some(2),
                    ..Default::default()
                },
            )
            .unwrap();
            let info = lofty::read_from_path(&output).unwrap();
            let tag = info.primary_tag().or_else(|| info.first_tag()).unwrap();
            assert_eq!(tag.title().as_deref(), Some("Rust Changed Title"));
            assert_eq!(tag.artist().as_deref(), Some("Rust Changed Artist"));
            assert_eq!(tag.track(), Some(3));
            assert_eq!(tag.disk(), Some(2));
            for key in [
                ItemKey::Comment,
                ItemKey::Composer,
                ItemKey::CopyrightMessage,
                ItemKey::AlbumArtist,
            ] {
                assert_eq!(
                    tag.get_string(key),
                    original_tag.get_string(key),
                    "unrelated tag changed in {name}"
                );
            }
            assert_eq!(tag.track_total(), original_tag.track_total());
            assert_eq!(tag.disk_total(), original_tag.disk_total());
        }
    }
}
