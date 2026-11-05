//! Podcast directory, RSS/Atom feeds and isolated subscriptions.
use crate::online::{base, encode, number, text, Context};
use crate::storage::atomic_write;
use quick_xml::{events::Event, Reader};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
};
static PODCASTS: Mutex<()> = Mutex::new(());
static ACTIVE: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
pub(crate) struct PodcastOperation(String);
impl Drop for PodcastOperation {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock() {
            active.remove(&self.0);
        }
    }
}
pub(crate) fn begin_download(feed: &str) -> Result<PodcastOperation, String> {
    if !ACTIVE
        .lock()
        .map_err(|e| e.to_string())?
        .insert(feed.into())
    {
        return Err("Wait for this podcast's current download or deletion to end".into());
    }
    Ok(PodcastOperation(feed.into()))
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Podcast {
    pub name: String,
    pub author: String,
    pub feed: String,
    pub art: String,
    pub art600: String,
    pub genre: String,
    pub count: usize,
    pub files: HashMap<String, String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Episode {
    pub guid: String,
    pub title: String,
    pub date: String,
    pub seconds: f64,
    pub url: String,
    pub size: u64,
    pub mime: String,
    pub number: u32,
    pub description: String,
}
impl Episode {
    pub fn key(&self) -> &str {
        if self.guid.is_empty() {
            &self.url
        } else {
            &self.guid
        }
    }
    pub fn extension(&self) -> &str {
        let mime = self.mime.to_lowercase();
        if mime.contains("mpeg") {
            "mp3"
        } else if mime.contains("mp4") || mime.contains("m4a") {
            "m4a"
        } else if mime.contains("flac") {
            "flac"
        } else if mime.contains("opus") {
            "opus"
        } else if mime.contains("ogg") {
            "ogg"
        } else {
            self.url
                .split('?')
                .next()
                .unwrap_or("")
                .rsplit('.')
                .next()
                .filter(|ext| {
                    matches!(
                        *ext,
                        "mp3" | "m4a" | "m4b" | "aac" | "flac" | "ogg" | "opus" | "wav"
                    )
                })
                .unwrap_or("mp3")
        }
    }
}
pub fn search(context: &Context, query: &str) -> Result<Vec<Podcast>, String> {
    let url = format!(
        "{}/search?media=podcast&entity=podcast&limit=25&term={}",
        base("BETTERPOD_RUST_ITUNES_BASE", "https://itunes.apple.com"),
        encode(query)
    );
    let value = context.get_json(&url)?;
    Ok(value["results"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter(|p| !text(p, "feedUrl").is_empty())
        .map(|p| Podcast {
            name: text(p, "collectionName"),
            author: text(p, "artistName"),
            feed: text(p, "feedUrl"),
            art: text(p, "artworkUrl100"),
            art600: text(p, "artworkUrl600"),
            genre: text(p, "primaryGenreName"),
            count: number(p, "trackCount") as usize,
            ..Default::default()
        })
        .collect())
}
pub fn fetch(context: &Context, mut podcast: Podcast) -> Result<(Podcast, Vec<Episode>), String> {
    context.progress(0, None, "Reading podcast feed");
    let bytes = if let Some(rest) = podcast.feed.strip_prefix("http://") {
        context
            .get_bytes(&format!("https://{rest}"), 16 * 1024 * 1024)
            .or_else(|_| context.get_bytes(&podcast.feed, 16 * 1024 * 1024))?
    } else {
        context.get_bytes(&podcast.feed, 16 * 1024 * 1024)?
    };
    let (parsed, episodes) = parse_feed(&bytes)?;
    if !parsed.name.is_empty() {
        podcast.name = parsed.name;
    }
    if !parsed.author.is_empty() {
        podcast.author = parsed.author;
    }
    if !parsed.art600.is_empty() {
        podcast.art600 = parsed.art600;
        if podcast.art.is_empty() {
            podcast.art = podcast.art600.clone();
        }
    }
    if !parsed.genre.is_empty() {
        podcast.genre = parsed.genre;
    }
    podcast.count = episodes.len();
    Ok((podcast, episodes))
}
pub fn saved(data: &Path) -> Vec<Podcast> {
    let path = data.join("podcasts.json");
    if std::fs::metadata(&path).map_or(true, |m| m.len() > 8 * 1024 * 1024) {
        return Vec::new();
    }
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
pub fn remember(data: &Path, podcast: Podcast) -> Result<(), String> {
    let _lock = PODCASTS.lock().map_err(|e| e.to_string())?;
    let mut podcasts = saved(data);
    if let Some(old) = podcasts.iter_mut().find(|p| p.feed == podcast.feed) {
        let mut files = old.files.clone();
        files.extend(podcast.files.clone());
        *old = podcast;
        old.files = files;
    } else {
        podcasts.push(podcast);
    }
    let bytes = serde_json::to_vec(&podcasts).map_err(|e| e.to_string())?;
    atomic_write(&data.join("podcasts.json"), &bytes).map_err(|e| e.to_string())
}
pub fn forget(data: &Path, feed: &str) -> Result<(), String> {
    let _lock = PODCASTS.lock().map_err(|e| e.to_string())?;
    let podcasts: Vec<_> = saved(data).into_iter().filter(|p| p.feed != feed).collect();
    atomic_write(
        &data.join("podcasts.json"),
        &serde_json::to_vec(&podcasts).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn delete_episodes(
    data: &Path,
    podcast: &Podcast,
    episodes: &[Episode],
    whole: bool,
    context: &Context,
) -> Result<(), String> {
    context.check()?;
    let _operation = begin_download(&podcast.feed)?;
    let _lock = PODCASTS.lock().map_err(|e| e.to_string())?;
    context.check()?;
    let mut subscriptions = saved(data);
    let Some(index) = subscriptions.iter().position(|p| p.feed == podcast.feed) else {
        return Ok(());
    };
    let keys: HashSet<String> = if whole {
        subscriptions[index].files.keys().cloned().collect()
    } else {
        episodes.iter().map(|e| e.key().to_string()).collect()
    };
    let private = std::fs::canonicalize(data).map_err(|e| e.to_string())?;
    let root = private.join("downloads/Podcasts");
    let root = if root.exists() {
        let metadata = std::fs::symlink_metadata(&root).map_err(|e| e.to_string())?;
        let canonical = std::fs::canonicalize(&root).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || !canonical.starts_with(&private)
        {
            return Err("Podcast download folder is not private".into());
        }
        Some(canonical)
    } else {
        None
    };
    let mut files = Vec::new();
    for key in &keys {
        let Some(file) = subscriptions[index].files.get(key) else {
            continue;
        };
        let path = PathBuf::from(file);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => {
                if meta.file_type().is_symlink() || !meta.is_file() {
                    return Err("Podcast download is not an owned regular file".into());
                }
                let canonical = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
                if !root.as_ref().is_some_and(|r| canonical.starts_with(r)) {
                    return Err("Cannot delete a podcast file outside private downloads".into());
                }
                files.push(canonical);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    for (n, file) in files.iter().enumerate() {
        context.check()?;
        context.progress(
            n as u64,
            Some(files.len() as u64),
            "Deleting downloaded podcast episodes",
        );
        std::fs::remove_file(file).map_err(|e| e.to_string())?;
    }
    for key in keys {
        subscriptions[index].files.remove(&key);
    }
    if whole {
        subscriptions[index].files.clear();
    }
    atomic_write(
        &data.join("podcasts.json"),
        &serde_json::to_vec(&subscriptions).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn parse_feed(bytes: &[u8]) -> Result<(Podcast, Vec<Episode>), String> {
    let declaration = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
    let encoding = declaration
        .split("encoding=")
        .nth(1)
        .and_then(|s| {
            let q = s.chars().next()?;
            s[1..].split(q).next()
        })
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (decoded, _, _) = encoding.decode(bytes);
    let mut reader = Reader::from_str(&decoded);
    reader.config_mut().trim_text(false);
    let mut podcast = Podcast::default();
    let mut episodes = Vec::new();
    let mut current: Option<Episode> = None;
    let mut stack: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut value = String::new();
    let mut channel = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                let name = local(start.name().as_ref());
                if name == "channel" || name == "feed" {
                    channel = true;
                }
                if name == "item" || name == "entry" {
                    current = Some(Episode::default());
                }
                attributes(&start, &name, &mut podcast, &mut current);
                stack.push(name.clone());
                if matches!(
                    name.as_str(),
                    "title"
                        | "author"
                        | "creator"
                        | "name"
                        | "duration"
                        | "pubDate"
                        | "published"
                        | "updated"
                        | "guid"
                        | "id"
                        | "episode"
                        | "description"
                        | "summary"
                        | "url"
                        | "category"
                ) {
                    field = name;
                    value.clear();
                }
            }
            Ok(Event::Empty(start)) => {
                let name = local(start.name().as_ref());
                attributes(&start, &name, &mut podcast, &mut current);
            }
            Ok(Event::Text(text)) => {
                if !field.is_empty() {
                    let decoded = text.decode().map_err(|e| e.to_string())?;
                    match quick_xml::escape::unescape(&decoded) {
                        Ok(unescaped) => value.push_str(&unescaped),
                        Err(_) => value.push_str(&decoded),
                    }
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if !field.is_empty() {
                    if let Ok(Some(character)) = reference.resolve_char_ref() {
                        value.push(character);
                    } else if let Ok(name) = std::str::from_utf8(reference.as_ref()) {
                        if let Some(entity) = quick_xml::escape::resolve_predefined_entity(name) {
                            value.push_str(entity);
                        } else {
                            value.push_str(&format!("&{name};"));
                        }
                    }
                }
            }
            Ok(Event::CData(text)) => {
                if !field.is_empty() {
                    value.push_str(&String::from_utf8_lossy(&text));
                }
            }
            Ok(Event::End(end)) => {
                let name = local(end.name().as_ref());
                if name == field {
                    let text = value.trim();
                    if let Some(episode) = &mut current {
                        match field.as_str() {
                            "title" => {
                                if episode.title.is_empty() {
                                    episode.title = text.into()
                                }
                            }
                            "guid" | "id" => episode.guid = text.into(),
                            "duration" => episode.seconds = duration(text),
                            "pubDate" | "published" | "updated" => {
                                if episode.date.is_empty() {
                                    episode.date = normalize_date(text)
                                }
                            }
                            "episode" => episode.number = text.parse().unwrap_or(0),
                            "description" | "summary" => episode.description = text.into(),
                            _ => {}
                        }
                    } else if channel {
                        match field.as_str() {
                            "title" => {
                                if podcast.name.is_empty() {
                                    podcast.name = text.into()
                                }
                            }
                            "author" | "creator" | "name" => {
                                if podcast.author.is_empty() {
                                    podcast.author = text.into()
                                }
                            }
                            "url" => {
                                if podcast.art600.is_empty() && stack.iter().any(|s| s == "image") {
                                    podcast.art600 = text.into()
                                }
                            }
                            "category" => {
                                if podcast.genre.is_empty() {
                                    podcast.genre = text.into()
                                }
                            }
                            _ => {}
                        }
                    }
                    field.clear();
                    value.clear();
                }
                if name == "item" || name == "entry" {
                    if let Some(mut episode) = current.take() {
                        if !episode.url.is_empty() && !episode.mime.starts_with("video/") {
                            if episode.title.is_empty() {
                                episode.title = "Episode".into();
                            }
                            episodes.push(episode);
                            if episodes.len() >= 5000 {
                                break;
                            }
                        }
                    }
                }
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                if episodes.is_empty() {
                    return Err(format!("Invalid podcast feed: {error}"));
                }
                break;
            }
            _ => {}
        }
    }
    if podcast.name.is_empty() && episodes.is_empty() {
        return Err("This is not a podcast feed".into());
    }
    episodes.sort_by(|a, b| match (a.date.is_empty(), b.date.is_empty()) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => b.date.cmp(&a.date),
    });
    episodes.truncate(200);
    Ok((podcast, episodes))
}
fn local(name: &[u8]) -> String {
    String::from_utf8_lossy(name.rsplit(|b| *b == b':').next().unwrap_or(name)).into_owned()
}
fn attributes(
    start: &quick_xml::events::BytesStart<'_>,
    name: &str,
    podcast: &mut Podcast,
    current: &mut Option<Episode>,
) {
    let attrs: HashMap<String, String> = start
        .attributes()
        .filter_map(Result::ok)
        .map(|a| {
            let key = local(a.key.as_ref());
            let raw = String::from_utf8_lossy(&a.value);
            let value = quick_xml::escape::unescape(&raw)
                .map(|value| value.into_owned())
                .unwrap_or_else(|_| raw.into_owned());
            (key, value)
        })
        .collect();
    if let Some(episode) = current {
        if name == "enclosure"
            || (name == "link" && attrs.get("rel").is_some_and(|r| r == "enclosure"))
        {
            episode.url = attrs
                .get("url")
                .or_else(|| attrs.get("href"))
                .cloned()
                .unwrap_or_default();
            episode.mime = attrs.get("type").cloned().unwrap_or_default();
            episode.size = attrs
                .get("length")
                .and_then(|s| s.parse().ok())
                .filter(|n: &u64| *n <= 4 * 1024 * 1024 * 1024)
                .unwrap_or(0);
        }
    } else {
        if name == "image" {
            if let Some(url) = attrs.get("href") {
                podcast.art600 = url.clone();
            }
        }
        if name == "category" && podcast.genre.is_empty() {
            podcast.genre = attrs.get("text").cloned().unwrap_or_default();
        }
    }
}
fn normalize_date(value: &str) -> String {
    chrono::DateTime::parse_from_rfc2822(value)
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(value))
        .map(|t| t.with_timezone(&chrono::Utc).to_rfc3339())
        .unwrap_or_else(|_| {
            value
                .get(..10)
                .filter(|s| s.chars().nth(4) == Some('-'))
                .unwrap_or("")
                .into()
        })
}
pub fn duration(value: &str) -> f64 {
    let mut seconds = 0.0;
    for part in value.trim().split(':') {
        let Ok(number) = part.trim().parse::<f64>() else {
            return 0.0;
        };
        if !number.is_finite() || number < 0.0 {
            return 0.0;
        }
        seconds = seconds * 60.0 + number;
    }
    seconds
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rss_namespaces_enclosures_and_dates() {
        let rss=br#"<rss xmlns:itunes="x"><channel><title>Show &amp; Music</title><itunes:author>Host</itunes:author><itunes:image href="https://example.invalid/c.jpg"/><item><title><![CDATA[One & two]]></title><guid>one</guid><pubDate>Wed, 04 Nov 2026 12:00:00 +0000</pubDate><itunes:duration>1:02:03</itunes:duration><enclosure url="https://example.invalid/audio?x=1&amp;y=2" type="audio/mpeg" length="100"/></item></channel></rss>"#;
        let (p, e) = parse_feed(rss).unwrap();
        assert_eq!(p.name, "Show & Music");
        assert_eq!(p.author, "Host");
        assert_eq!(e[0].seconds, 3723.0);
        assert_eq!(e[0].url, "https://example.invalid/audio?x=1&y=2");
        assert!(e[0].date.starts_with("2026-11-04"));
    }
    #[test]
    fn atom_and_bad_duration() {
        let xml=br#"<feed><title>Atom</title><entry><title>A</title><id>1</id><link rel="enclosure" href="https://example.invalid/a.opus" type="audio/opus"/></entry></feed>"#;
        let (_, episodes) = parse_feed(xml).unwrap();
        assert_eq!(episodes[0].extension(), "opus");
        assert_eq!(duration("NaN"), 0.0);
        assert_eq!(duration("1:bad"), 0.0);
    }
    #[test]
    fn deletion_uses_saved_private_ownership_and_preserves_source_music() {
        let root =
            std::env::temp_dir().join(format!("betterpod-podcast-delete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = root.join("rust");
        let episode = data.join("downloads/Podcasts/Show/episode.mp3");
        std::fs::create_dir_all(episode.parent().unwrap()).unwrap();
        std::fs::write(&episode, b"owned download").unwrap();
        let source = root.join("source.mp3");
        std::fs::write(&source, b"original music").unwrap();
        let mut podcast = Podcast {
            name: "Show".into(),
            feed: "https://example.invalid/rss".into(),
            ..Default::default()
        };
        podcast
            .files
            .insert("one".into(), episode.to_string_lossy().into_owned());
        remember(&data, podcast.clone()).unwrap();
        let selected = Episode {
            guid: "one".into(),
            ..Default::default()
        };
        let context = Context::standalone();
        delete_episodes(&data, &podcast, &[selected], false, &context).unwrap();
        assert!(!episode.exists());
        assert!(saved(&data)[0].files.is_empty());
        podcast.files.clear();
        podcast
            .files
            .insert("outside".into(), source.to_string_lossy().into_owned());
        remember(&data, podcast.clone()).unwrap();
        assert!(delete_episodes(&data, &podcast, &[], true, &context).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"original music");
        std::fs::remove_dir_all(root).unwrap();
    }
}
