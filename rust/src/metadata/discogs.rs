//! Discogs releases as an album information source. Searches prefer CD
//! editions, whose numbering and lengths match ripped files, keep one
//! edition per album and fall back to other formats. A pasted Discogs link
//! opens that release. The Mac companion uses the same rules.
use super::wikipedia::duration;
use crate::online::{base, encode, text, Context, OnlineItem, Provider};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

/// Discogs answers 25 requests a minute without an account. Keep a margin
/// and fail this source promptly, so the other sources stay quick.
const BUDGET: usize = 22;
static REQUESTS: Mutex<VecDeque<Instant>> = Mutex::new(VecDeque::new());
static LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)discogs\.com/(?:[a-z]{2}(?:-[a-z]{2})?/)?(?:[^/\s]+/)?(release|master)/(\d+)")
        .unwrap()
});
static DISC_POSITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:cd|disc|disk)?\s*(\d{1,2})\s*[-.]\s*(\d{1,3})$").unwrap()
});
static VIDEO_POSITION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:dvd|bd|blu-?ray|vhs)").unwrap());
static ARTIST_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\(\d+\)$").unwrap());

fn reserve() -> Result<(), String> {
    let mut requests = REQUESTS.lock().map_err(|e| e.to_string())?;
    let now = Instant::now();
    while requests
        .front()
        .is_some_and(|at| now.duration_since(*at) >= Duration::from_secs(60))
    {
        requests.pop_front();
    }
    if requests.len() >= BUDGET {
        return Err(
            "Discogs allows a few searches a minute. Wait a minute or use another source".into(),
        );
    }
    requests.push_back(now);
    Ok(())
}

fn get(context: &Context, path: &str) -> Result<Value, String> {
    reserve()?;
    let url = format!(
        "{}{path}",
        base("BETTERPOD_RUST_DISCOGS_BASE", "https://api.discogs.com")
    );
    let value = context.get_json(&url).map_err(|error| {
        if error.contains("429") {
            "Discogs is busy. Try again in a minute".into()
        } else if error.contains("404") {
            "Discogs has no such release".into()
        } else {
            error
        }
    })?;
    match value["message"].as_str() {
        Some(message) if value["results"].is_null() && value["title"].is_null() => Err(format!(
            "Discogs: {}",
            message.chars().take(256).collect::<String>()
        )),
        _ => Ok(value),
    }
}

pub(super) fn search(context: &Context, query: &str) -> Result<Vec<OnlineItem>, String> {
    if let Some((kind, id)) = link(query) {
        let mut item = OnlineItem::new(Provider::Discogs);
        item.id = format!("{kind}:{id}");
        item.url = format!("https://www.discogs.com/{kind}/{id}");
        return Ok(vec![details(context, item)?]);
    }
    let words = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.is_empty() {
        return Ok(Vec::new());
    }
    for format in ["&format=CD", ""] {
        let found = parse_search(&get(
            context,
            &format!(
                "/database/search?type=release&per_page=25&q={}{format}",
                encode(&words)
            ),
        )?);
        if !found.is_empty() {
            return Ok(found);
        }
    }
    Ok(Vec::new())
}

/// One edition per album: the first result of each master release.
fn parse_search(value: &Value) -> Vec<OnlineItem> {
    let mut albums = HashSet::new();
    let mut items = Vec::new();
    for row in value["results"].as_array().into_iter().flatten() {
        let id = row["id"].as_i64().unwrap_or(0);
        let name = text(row, "title");
        if id <= 0 || text(row, "type") != "release" || name.is_empty() {
            continue;
        }
        let master = row["master_id"].as_i64().unwrap_or(0);
        if !albums.insert(if master > 0 { master } else { -id }) {
            continue;
        }
        let mut item = OnlineItem::new(Provider::Discogs);
        item.id = format!("release:{id}");
        match name.split_once(" - ") {
            Some((artist, title)) => {
                item.artist = clean_artist(artist);
                item.title = title.trim().into();
            }
            None => item.title = name,
        }
        item.year = text(row, "year");
        if item.year.parse::<u32>().unwrap_or(0) == 0 {
            item.year.clear();
        }
        let formats: Vec<String> = row["format"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        item.kind = ["EP", "Single", "Compilation"]
            .into_iter()
            .find(|kind| formats.iter().any(|format| format == kind))
            .unwrap_or_default()
            .into();
        item.url = match text(row, "uri") {
            uri if uri.starts_with('/') => format!("https://www.discogs.com{uri}"),
            _ => format!("https://www.discogs.com/release/{id}"),
        };
        item.cover = image(&text(row, "cover_image"));
        let source = formats
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        item.raw = json!({"_source": if source.is_empty() {
            "Discogs".to_string()
        } else {
            format!("Discogs · {source}")
        }});
        items.push(item);
        if items.len() == 10 {
            break;
        }
    }
    items
}

pub(super) fn details(context: &Context, mut item: OnlineItem) -> Result<OnlineItem, String> {
    let (kind, id) = item
        .id
        .split_once(':')
        .filter(|(kind, id)| matches!(*kind, "release" | "master") && id.parse::<u64>().is_ok())
        .ok_or("Discogs: invalid result")?;
    let value = get(context, &format!("/{kind}s/{id}"))?;
    let source = item.raw["_source"]
        .as_str()
        .unwrap_or("Discogs")
        .to_string();
    apply_details(&mut item, &value)?;
    item.raw["_source"] = json!(source);
    Ok(item)
}

fn apply_details(item: &mut OnlineItem, value: &Value) -> Result<(), String> {
    let title = text(value, "title").trim().to_string();
    if title.is_empty() {
        return Err("Discogs: invalid response".into());
    }
    item.title = title;
    item.artist = credit(&value["artists"]);
    let year = value["year"].as_u64().filter(|year| *year > 0);
    item.year = year.map_or_else(
        || {
            let released = text(value, "released");
            released
                .get(..4)
                .filter(|year| year.parse::<u32>().is_ok_and(|year| year > 0))
                .unwrap_or("")
                .into()
        },
        |year| year.to_string(),
    );
    let uri = text(value, "uri");
    if uri.starts_with("https://www.discogs.com/") {
        item.url = uri;
    }
    let images = value["images"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let picture = images
        .iter()
        .find(|image| text(image, "type") == "primary")
        .or(images.first());
    if let Some(picture) = picture {
        item.cover = image(&text(picture, "uri"));
    }
    let tracks = tracks(&value["tracklist"], &item.artist);
    item.count = tracks.len();
    item.raw = json!({"genre": genre(value), "tracks": tracks});
    Ok(())
}

/// Several broad genres say little ("Electronic, Rock"); the first style is
/// then more precise.
fn genre(value: &Value) -> String {
    let list = |key: &str| -> Vec<String> {
        value[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    };
    let (genres, styles) = (list("genres"), list("styles"));
    if genres.len() > 1 {
        styles.first().or(genres.first())
    } else {
        genres.first().or(styles.first())
    }
    .cloned()
    .unwrap_or_default()
}

/// Positions such as "1-03" or "CD2-1" give discs; plain numbers keep their
/// order; vinyl sides ("A1", "B2") and missing positions count from one.
/// Headings and video parts are not songs; an index lists its parts.
fn tracks(tracklist: &Value, artist: &str) -> Vec<Value> {
    let mut entries = Vec::new();
    for entry in tracklist.as_array().into_iter().flatten() {
        match entry["type_"].as_str().unwrap_or("track") {
            "track" => entries.push(entry),
            "index" => entries.extend(
                entry["sub_tracks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|sub| sub["type_"].as_str().unwrap_or("track") == "track"),
            ),
            _ => {}
        }
    }
    entries.retain(|entry| !VIDEO_POSITION.is_match(text(entry, "position").trim()));
    let titles: Vec<String> = entries
        .iter()
        .map(|entry| text(entry, "title").trim().to_string())
        .collect();
    if entries.is_empty() || titles.iter().any(String::is_empty) {
        return Vec::new();
    }
    let positions: Vec<String> = entries
        .iter()
        .map(|entry| text(entry, "position").trim().to_string())
        .collect();
    let mut numbers: Vec<(u32, u32)> = positions
        .iter()
        .map(|position| {
            DISC_POSITION
                .captures(position)
                .and_then(|found| Some((found[1].parse().ok()?, found[2].parse().ok()?)))
                .unwrap_or((0, 0))
        })
        .collect();
    if numbers
        .iter()
        .any(|(disc, track)| *disc == 0 || *track == 0)
    {
        let plain: Vec<u32> = positions.iter().filter_map(|p| p.parse().ok()).collect();
        let unique = plain.iter().collect::<HashSet<_>>().len() == plain.len();
        numbers = if plain.len() == positions.len() && unique && plain.iter().all(|n| *n > 0) {
            plain.into_iter().map(|track| (1, track)).collect()
        } else {
            (1..=entries.len() as u32).map(|track| (1, track)).collect()
        };
    }
    if numbers.iter().collect::<HashSet<_>>().len() != numbers.len() {
        return Vec::new();
    }
    let mut totals: HashMap<u32, usize> = HashMap::new();
    for (disc, _) in &numbers {
        *totals.entry(*disc).or_default() += 1;
    }
    let mut songs: Vec<((u32, u32), Value)> = entries
        .iter()
        .zip(titles)
        .zip(numbers)
        .map(|((entry, title), (disc, number))| {
            let credited = credit(&entry["artists"]);
            (
                (disc, number),
                json!({
                    "disc": disc,
                    "number": number,
                    "title": title,
                    "artist": if credited.is_empty() { artist.to_string() } else { credited },
                    "seconds": duration(&text(entry, "duration")).unwrap_or(0.0),
                }),
            )
        })
        .collect();
    songs.sort_by_key(|(position, _)| *position);
    songs.into_iter().map(|(_, song)| song).collect()
}

/// The names as credited on the release, joined as Discogs joins them.
fn credit(artists: &Value) -> String {
    let artists = artists.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut credit = String::new();
    for (index, artist) in artists.iter().enumerate() {
        let variation = text(artist, "anv");
        let name = if variation.trim().is_empty() {
            text(artist, "name")
        } else {
            variation
        };
        credit.push_str(&clean_artist(&name));
        if index + 1 < artists.len() {
            match text(artist, "join").trim() {
                "" | "," => credit.push_str(", "),
                join => {
                    credit.push(' ');
                    credit.push_str(join);
                    credit.push(' ');
                }
            }
        }
    }
    credit.trim().into()
}

/// Discogs numbers artists who share a name ("Nirvana (2)") and marks name
/// variations with "*".
fn clean_artist(name: &str) -> String {
    ARTIST_NUMBER
        .replace(name.trim(), "")
        .trim_matches(|c: char| c == '*' || c.is_whitespace())
        .into()
}

fn image(url: &str) -> String {
    let host = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("")
        .to_lowercase();
    if (host == "i.discogs.com" || host.ends_with(".discogs.com")) && !url.ends_with(".gif") {
        url.into()
    } else {
        String::new()
    }
}

/// A Discogs release or master link, for example
/// discogs.com/it/release/2621910-DJ-Gruff-Rapadopa.
fn link(text: &str) -> Option<(&'static str, u64)> {
    let found = LINK.captures(text)?;
    let kind = if found[1].eq_ignore_ascii_case("master") {
        "master"
    } else {
        "release"
    };
    Some((kind, found[2].parse().ok().filter(|id| *id > 0)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_keeps_one_edition_per_album() {
        let value = json!({"results": [
            {"type": "release", "id": 2621910, "master_id": 406159, "title": "DJ Gruff - Rapadopa",
             "year": "1993", "format": ["CD", "Album"], "uri": "/release/2621910-DJ-Gruff-Rapadopa", "cover_image": ""},
            {"type": "release", "id": 34049056, "master_id": 406159, "title": "DJ Gruff - Rapadopa", "year": "1993"},
            {"type": "release", "id": 4085868, "master_id": 1975159, "title": "D.J. Gruff* - La Musica",
             "year": "1994", "format": ["Vinyl", "12\"", "EP"]},
            {"type": "release", "id": 7315089, "master_id": 0, "title": "Sangue Misto (2) - Live Padova", "year": "0"},
            {"type": "master", "id": 406159, "title": "DJ Gruff - Rapadopa"}
        ]});
        let items = parse_search(&value);
        let rows: Vec<(&str, &str, &str, &str)> = items
            .iter()
            .map(|i| {
                (
                    i.id.as_str(),
                    i.artist.as_str(),
                    i.title.as_str(),
                    i.year.as_str(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("release:2621910", "DJ Gruff", "Rapadopa", "1993"),
                ("release:4085868", "D.J. Gruff", "La Musica", "1994"),
                ("release:7315089", "Sangue Misto", "Live Padova", "")
            ]
        );
        assert_eq!(items[0].raw["_source"], "Discogs · CD, Album");
        assert_eq!(
            items[0].url,
            "https://www.discogs.com/release/2621910-DJ-Gruff-Rapadopa"
        );
        assert_eq!(items[1].kind, "EP");
    }

    #[test]
    fn release_details_number_discs_sides_and_indexes() {
        let value = json!({
            "title": "Rapadopa", "year": 1993, "genres": ["Hip Hop"], "styles": ["Jazzy Hip-Hop"],
            "artists": [{"name": "DJ Gruff", "anv": "", "join": ""}],
            "images": [{"type": "secondary", "uri": "https://i.discogs.com/b.jpg"},
                       {"type": "primary", "uri": "https://i.discogs.com/a.jpg"}],
            "tracklist": [
                {"position": "", "type_": "heading", "title": "Part One"},
                {"position": "1", "type_": "track", "title": "La Rapadopa", "duration": "4:53"},
                {"position": "2", "type_": "track", "title": "Faustona", "duration": "2:35",
                 "artists": [{"name": "Neffa", "join": "&"}, {"name": "Deda (2)", "anv": "Deda*", "join": ""}]}
            ]
        });
        let mut item = OnlineItem::new(Provider::Discogs);
        apply_details(&mut item, &value).unwrap();
        assert_eq!(
            (item.artist.as_str(), item.year.as_str()),
            ("DJ Gruff", "1993")
        );
        assert_eq!(item.cover, "https://i.discogs.com/a.jpg");
        assert_eq!(item.raw["genre"], "Hip Hop");
        assert_eq!(item.raw["tracks"][0]["seconds"], 293.0);
        assert_eq!(item.raw["tracks"][1]["artist"], "Neffa & Deda");
        assert_eq!(item.count, 2);

        let discs = json!([
            {"position": "1-1", "title": "Mellon Collie", "duration": "2:52"},
            {"position": "1-2", "title": "Tonight, Tonight"},
            {"position": "CD2-1", "title": "Where Boys Fear to Tread"},
            {"position": "DVD-1", "title": "A Video"}
        ]);
        let numbers: Vec<(u64, u64)> = tracks(&discs, "SP")
            .iter()
            .map(|t| (t["disc"].as_u64().unwrap(), t["number"].as_u64().unwrap()))
            .collect();
        assert_eq!(numbers, [(1, 1), (1, 2), (2, 1)]);

        let sides = json!([
            {"position": "A1", "title": "Daftendirekt"},
            {"position": "A2", "type_": "index", "title": "Medley",
             "sub_tracks": [{"position": "A2a", "type_": "track", "title": "Part A"},
                            {"position": "A2b", "type_": "track", "title": "Part B"}]},
            {"position": "B1", "title": "Da Funk"}
        ]);
        let titles: Vec<(u64, String)> = tracks(&sides, "Daft Punk")
            .iter()
            .map(|t| {
                (
                    t["number"].as_u64().unwrap(),
                    t["title"].as_str().unwrap().into(),
                )
            })
            .collect();
        assert_eq!(
            titles,
            [
                (1, "Daftendirekt".into()),
                (2, "Part A".into()),
                (3, "Part B".into()),
                (4, "Da Funk".into())
            ]
        );

        let several =
            json!({"title": "X", "genres": ["Electronic", "Rock"], "styles": ["Trip Hop"]});
        assert_eq!(genre(&several), "Trip Hop");
    }

    #[test]
    fn pasted_links_name_a_release_or_master() {
        assert_eq!(
            link("https://www.discogs.com/it/release/2621910-DJ-Gruff-Rapadopa?srsltid=AYC"),
            Some(("release", 2621910))
        );
        assert_eq!(
            link("discogs.com/DJ-Gruff-Rapadopa/master/406159"),
            Some(("master", 406159))
        );
        assert_eq!(link("Rapadopa DJ Gruff"), None);
        assert_eq!(clean_artist("Nirvana (2)"), "Nirvana");
        assert_eq!(image("https://example.com/a.jpg"), "");
    }
}
