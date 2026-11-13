//! The cover-only provider adapters from ij_covers.go/ij_coversources.go.
//! Album-information searches intentionally keep their separate release API.
use super::*;
use crate::{
    cover_match::{self, AlbumQuery},
    online::{base, encode, text, OnlineItem},
};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
static ITUNES_NEXT: Mutex<Option<Instant>> = Mutex::new(None);
static WIKI_DISAMB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\([^)]*\)$").unwrap());
static WIKI_YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(1[89]|20)\d\d\b").unwrap());
static WIKI_SONG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bsong\b").unwrap());
fn itunes_wait(context: &Context) -> Result<(), String> {
    let now = Instant::now();
    let at = {
        let mut next = ITUNES_NEXT.lock().map_err(|e| e.to_string())?;
        let at = next.unwrap_or(now).max(now);
        *next = Some(at + Duration::from_secs(3));
        at
    };
    context.sleep(at.saturating_duration_since(now))
}
fn mb_groups(
    context: &Context,
    query: &str,
    dismax: bool,
    limit: usize,
) -> Result<Vec<OnlineItem>, String> {
    let mut attempt = 0;
    loop {
        metadata::mb_wait(context)?;
        let url = format!(
            "{}/ws/2/release-group/?fmt=json&limit={limit}&query={}{}",
            base("BETTERPOD_RUST_MB_BASE", "https://musicbrainz.org"),
            encode(query),
            if dismax { "&dismax=true" } else { "" }
        );
        match context.get_json(&url) {
            Ok(value) => {
                return Ok(value["release-groups"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|row| {
                        let mut item = OnlineItem::new(Provider::MusicBrainz);
                        item.id = text(row, "id");
                        item.title = text(row, "title");
                        item.artist = row["artist-credit"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|credit| text(credit, "name"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        item.year = text(row, "first-release-date").chars().take(4).collect();
                        item.kind = text(row, "primary-type");
                        if item.kind == "Album" {
                            item.kind.clear();
                        }
                        let soundtrack = row["secondary-types"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .any(|s| s.eq_ignore_ascii_case("Soundtrack"));
                        if soundtrack {
                            item.kind = "Soundtrack".into();
                        }
                        item.cover = format!(
                            "{}/release-group/{}/front-1200",
                            base("BETTERPOD_RUST_CAA_BASE", "https://coverartarchive.org"),
                            encode(&item.id)
                        );
                        item.url =
                            format!("https://musicbrainz.org/release-group/{}", encode(&item.id));
                        item.raw = row.clone();
                        item.raw["_cover_soundtrack"] = json!(soundtrack);
                        item
                    })
                    .filter(|item| !item.id.is_empty())
                    .collect());
            }
            Err(error) if error.contains("503") && attempt < 2 => {
                context.sleep(Duration::from_secs(2 + attempt * 3))?;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}
fn wiki_kind(description: &str) -> &'static str {
    let d = description.to_lowercase();
    if d.contains("video game series") || WIKI_SONG.is_match(&d) {
        ""
    } else if d.contains("video game") {
        "Video game"
    } else if d.contains("soundtrack") || d.contains("film score") {
        "Soundtrack"
    } else if d.contains("extended play")
        || d.starts_with("ep ")
        || d.contains(" ep ")
        || d.contains(" ep by")
    {
        "EP"
    } else if d.contains("compilation album") {
        "Compilation"
    } else if d.contains("album") || d.contains("mixtape") {
        "Album"
    } else if d.contains("single by") {
        "Single"
    } else {
        ""
    }
}
fn provider_items(
    context: &Context,
    provider: Provider,
    term: &str,
    limit: usize,
) -> Result<Vec<OnlineItem>, String> {
    let url=match provider{
        Provider::Itunes=>{itunes_wait(context)?;format!("{}/search?entity=album&limit={limit}&term={}",base("BETTERPOD_RUST_ITUNES_BASE","https://itunes.apple.com"),encode(term))},
        Provider::Deezer=>format!("{}/search/album?limit={limit}&q={}",base("BETTERPOD_RUST_DEEZER_BASE","https://api.deezer.com"),encode(term)),
        Provider::Wikipedia=>format!("{}/w/api.php?action=query&format=json&formatversion=2&generator=search&gsrnamespace=0&gsrlimit={limit}&gsrsearch={}&prop=pageimages%7Cdescription&piprop=thumbnail%7Coriginal&pithumbsize=250&pilicense=any",base("BETTERPOD_RUST_WIKI_BASE","https://en.wikipedia.org"),encode(term)),
        _=>return Err("Choose a cover source".into()),
    };
    let value = context.get_json(&url)?;
    if provider == Provider::Deezer && !value["error"].is_null() {
        return Err(format!("Deezer: {}", text(&value["error"], "message")));
    }
    let mut rows: Vec<&Value> = match provider {
        Provider::Itunes => value["results"].as_array().into_iter().flatten().collect(),
        Provider::Deezer => value["data"].as_array().into_iter().flatten().collect(),
        Provider::Wikipedia => value["query"]["pages"]
            .as_array()
            .into_iter()
            .flatten()
            .collect(),
        _ => Vec::new(),
    };
    if provider == Provider::Wikipedia {
        rows.sort_by_key(|row| row["index"].as_i64().unwrap_or(i64::MAX));
    }
    let mut items = Vec::new();
    for row in rows {
        let mut item = OnlineItem::new(provider);
        item.raw = row.clone();
        match provider {
            Provider::Itunes => {
                let art = text(row, "artworkUrl100");
                if art.is_empty() || !art.contains("100x100") {
                    continue;
                }
                item.title = text(row, "collectionName");
                item.artist = text(row, "artistName");
                item.year = text(row, "releaseDate").chars().take(4).collect();
                item.cover = art.replacen("100x100", "1200x1200", 1);
                item.raw["_cover_thumb"] = json!(art.replacen("100x100", "250x250", 1));
                for suffix in ["Single", "EP"] {
                    if let Some(title) = item.title.strip_suffix(&format!(" - {suffix}")) {
                        item.title = title.into();
                        item.kind = suffix.into();
                    }
                }
                if text(row, "primaryGenreName")
                    .to_lowercase()
                    .contains("soundtrack")
                {
                    item.kind = "Soundtrack".into();
                }
                item.id = text(row, "collectionId");
            }
            Provider::Deezer => {
                item.cover = text(row, "cover_xl");
                if item.cover.is_empty() || text(row, "md5_image").is_empty() {
                    continue;
                }
                item.title = text(row, "title");
                item.artist = text(&row["artist"], "name");
                item.raw["_cover_thumb"] = json!(text(row, "cover_medium"));
                item.kind = match text(row, "record_type").as_str() {
                    "ep" => "EP",
                    "single" => "Single",
                    "compile" => "Compilation",
                    _ => "",
                }
                .into();
                item.id = text(row, "id");
            }
            Provider::Wikipedia => {
                let description = text(row, "description");
                let kind = wiki_kind(&description);
                let mut full = text(&row["original"], "source");
                if kind.is_empty() || full.is_empty() {
                    continue;
                }
                if full.to_lowercase().contains(".svg") {
                    full = text(&row["thumbnail"], "source");
                    if full.is_empty() {
                        continue;
                    }
                }
                item.id = text(row, "title");
                item.title = WIKI_DISAMB.replace(&item.id, "").into_owned();
                if let Some((_, artist)) = description.rsplit_once(" by ") {
                    item.artist = artist.trim().into();
                }
                item.year = WIKI_YEAR
                    .find(&description)
                    .map_or(String::new(), |m| m.as_str().into());
                item.kind = if kind == "Album" {
                    String::new()
                } else {
                    kind.into()
                };
                item.cover = full;
                item.raw["_cover_thumb"] = json!(text(&row["thumbnail"], "source"));
            }
            _ => {}
        }
        if item.id.is_empty() {
            item.id = format!("{}|{}", item.title, item.artist);
        }
        item.raw["_cover_soundtrack"] =
            json!(item.kind == "Soundtrack" || item.kind == "Video game");
        items.push(item);
    }
    Ok(items)
}
fn source_items(
    context: &Context,
    provider: Provider,
    q: &AlbumQuery,
    limit: usize,
) -> Result<Vec<OnlineItem>, String> {
    let mut error = None;
    for term in q.source_terms() {
        match provider_items(context, provider, &term, limit) {
            Ok(items) if !items.is_empty() => return Ok(items),
            Ok(_) => {}
            Err(e) => {
                context.check()?;
                error = Some(e);
            }
        }
    }
    if let Some(error) = error {
        Err(error)
    } else {
        Ok(Vec::new())
    }
}
pub fn soundtrack(item: &OnlineItem) -> bool {
    item.raw["_cover_soundtrack"].as_bool().unwrap_or(false)
}
fn mark(mut item: OnlineItem, q: &AlbumQuery) -> OnlineItem {
    let best = q.matches(&item.title, &item.artist, soundtrack(&item));
    item.raw["_cover_best"] = json!(best);
    item.raw["_cover_exact"] = json!(cover_match::same_title(&q.title, &item.title));
    item
}
/// The source picker collects automatic candidates and keeps non-matches for
/// manual choice. Its query preparation and best-match classification stay
/// on this service worker, including the cached per-track album context.
pub fn search_items(
    context: &Context,
    provider: Provider,
    q: &AlbumQuery,
    manual: Option<&str>,
) -> Result<Vec<OnlineItem>, String> {
    let mut items = if provider == Provider::MusicBrainz {
        if let Some(term) = manual {
            mb_groups(context, term, true, 25)?
        } else {
            let title = cover_match::clean_title(&q.title);
            let mut queries = Vec::new();
            if let Some(artist) = q.artists.first() {
                queries.push(format!(
                    "releasegroup:({}) AND artist:({})",
                    cover_match::lucene(&title),
                    cover_match::lucene(&cover_match::clean_artist(artist))
                ));
            }
            queries.push(format!("releasegroup:({})", cover_match::lucene(&title)));
            let short = cover_match::clean_title(cover_match::main_title(&q.title));
            if short != title && !short.is_empty() {
                queries.push(format!("releasegroup:({})", cover_match::lucene(&short)));
            }
            let mut items = Vec::new();
            for query in queries {
                for item in mb_groups(context, &query, false, 15)? {
                    if !items.iter().any(|old: &OnlineItem| old.id == item.id) {
                        items.push(item);
                    }
                }
                if items.len() >= 10 {
                    break;
                }
            }
            items
        }
    } else if let Some(term) = manual {
        provider_items(context, provider, term, 8)?
    } else {
        source_items(context, provider, q, 8)?
    };
    for item in &mut items {
        *item = mark(item.clone(), q);
    }
    items.sort_by_key(|item| {
        (
            !item.raw["_cover_best"].as_bool().unwrap_or(false),
            !item.raw["_cover_exact"].as_bool().unwrap_or(false),
        )
    });
    Ok(items)
}
fn image_bytes(context: &Context, url: &str) -> Result<Option<Vec<u8>>, String> {
    match context.get_bytes(url, MAX_COVER) {
        Ok(bytes) => {
            if image::ImageReader::new(Cursor::new(&bytes))
                .with_guessed_format()
                .map_err(|e| e.to_string())?
                .into_dimensions()
                .is_err()
            {
                return Ok(None);
            }
            Ok(Some(bytes))
        }
        Err(error) if error.contains("404") => Ok(None),
        Err(error) => Err(error),
    }
}
/// Automatic MusicBrainz-first search and the original exact-title pass
/// across all other providers before accepting a main-title-only match.
pub(super) fn find_cover(context: &Context, q: &AlbumQuery) -> Result<Option<Vec<u8>>, String> {
    let mut groups = Vec::new();
    for stage in q.musicbrainz_stages() {
        for item in mb_groups(context, &stage.query, false, 8)? {
            if q.title_ok(&item.title, &item.artist, soundtrack(&item))
                && (!stage.need_artist || q.artist_ok(&item.artist))
                && !groups.iter().any(|old: &OnlineItem| old.id == item.id)
                && groups.len() < 3
            {
                groups.push(item);
            }
        }
        if !groups.is_empty() {
            break;
        }
    }
    let mut group_error = None;
    for item in groups {
        match image_bytes(context, &item.cover) {
            Ok(Some(bytes)) => return Ok(Some(bytes)),
            Ok(None) => {}
            Err(error) => group_error = Some(error),
        }
    }
    if let Some(error) = group_error {
        return Err(error);
    }
    let providers = [Provider::Itunes, Provider::Deezer, Provider::Wikipedia];
    let mut found: [Option<Vec<OnlineItem>>; 3] = [None, None, None];
    let mut last_error = None;
    for pass in 0..2 {
        for (i, provider) in providers.into_iter().enumerate() {
            if found[i].is_none() {
                found[i] = Some(match source_items(context, provider, q, 8) {
                    Ok(items) => items,
                    Err(error) => {
                        context.check()?;
                        last_error = Some(error);
                        Vec::new()
                    }
                });
            }
            for item in found[i].as_ref().unwrap() {
                if !q.matches(&item.title, &item.artist, soundtrack(item))
                    || cover_match::same_title(&q.title, &item.title) != (pass == 0)
                {
                    continue;
                }
                match image_bytes(context, &item.cover) {
                    Ok(Some(bytes)) => return Ok(Some(bytes)),
                    Ok(None) => {}
                    Err(error) => {
                        context.check()?;
                        last_error = Some(error);
                    }
                }
            }
        }
    }
    if let Some(error) = last_error {
        Err(error)
    } else {
        Ok(None)
    }
}
