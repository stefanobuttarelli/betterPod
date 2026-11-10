//! Album articles on the English and Italian Wikipedia. The infobox gives
//! the album fields and the track listing gives the songs; only the first
//! edition of an album is used. One search request per language returns the
//! articles' wikitext, so opening a result needs no further request. The Mac
//! companion reads articles by the same rules.
use super::wikitext::{self, Span, Template};
use crate::online::{base, encode, text, Context, OnlineItem, Provider};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

struct Edition {
    code: &'static str,
    base: &'static str,
    /// The album infobox template, used to keep searches to album articles.
    infobox: &'static str,
    label: &'static str,
}
const EDITIONS: [Edition; 2] = [
    Edition {
        code: "en",
        base: "BETTERPOD_RUST_WIKI_BASE",
        infobox: "Infobox album",
        label: "Wikipedia",
    },
    Edition {
        code: "it",
        base: "BETTERPOD_RUST_WIKI_IT_BASE",
        infobox: "Album",
        label: "Wikipedia (it)",
    },
];
const INFOBOXES: [&str; 2] = ["infobox album", "album"];
const TRACK_LISTS: [&str; 4] = ["track listing", "tracklist", "track list", "tracce"];

static SEARCH_SYNTAX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"["*?~:\\]"#).unwrap());
static ALBUM_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*\([^()]*\balbum\b[^()]*\)$").unwrap());
static FILE_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*(?:file|image|immagine)\s*:").unwrap());
static NUMBERED_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(title|titolo|length|durata|extra)(\d+)$").unwrap());
static ARTIST_COLUMN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)artist|performer|interprete").unwrap());
static SECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(track ?listing|track ?list|tracks|tracce|lista tracce|tracce del disco|brani)$",
    )
    .unwrap()
});
static TIME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{1,2}:\d{2}(?::\d{2})?").unwrap());
static QUOTED: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [r#""([^"]+)""#, r"“([^”]+)”", r"''(.+?)''"].map(|pattern| Regex::new(pattern).unwrap())
});
static DISC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:disc|disk|cd|disco)\s*(\d{1,2}|one|two|three|four|five|six|uno|due|tre|quattro|cinque)\b").unwrap()
});
static SIDE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:side|lato|facciata)\b").unwrap());
static BONUS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)bonus|\bdvd\b|\bvideo\b|\bextra\b").unwrap());
static ALTERNATIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)edition|deluxe|reissue|re-issue|expanded|anniversary|remaster|japan|limited|digital|itunes|vinyl|cassette|\brelease\b|pressing|version|edizione|ristampa|vinile|musicassetta|versione").unwrap()
});
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());

/// Album articles for `query`, English first. A failed language does not
/// hide the other one's results.
pub(super) fn search(context: &Context, query: &str) -> Result<Vec<OnlineItem>, String> {
    // Quotes, colons and wildcards are search syntax; album names only need words.
    let words = SEARCH_SYNTAX
        .replace_all(query, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let mut error = None;
    for edition in &EDITIONS {
        let url = format!(
            "{}/w/api.php?action=query&format=json&formatversion=2&generator=search\
             &gsrnamespace=0&gsrlimit=5&gsrsearch={}&prop=revisions%7Cpageimages%7Cinfo\
             &rvprop=content&rvslots=main&piprop=thumbnail%7Cname&pithumbsize=1200\
             &pilicense=any&inprop=url",
            base(
                edition.base,
                &format!("https://{}.wikipedia.org", edition.code)
            ),
            encode(&format!("{words} hastemplate:\"{}\"", edition.infobox))
        );
        match context
            .get_json(&url)
            .and_then(|value| parse(&value, edition))
        {
            Ok(found) => items.extend(found),
            Err(e) => {
                context.check()?;
                error = Some(e);
            }
        }
    }
    match error {
        Some(error) if items.is_empty() => Err(error),
        _ => Ok(items),
    }
}

fn parse(value: &Value, edition: &Edition) -> Result<Vec<OnlineItem>, String> {
    if value["error"].is_object() {
        let info = text(&value["error"], "info");
        return Err(format!(
            "Wikipedia: {}",
            if info.is_empty() {
                "the service rejected the request"
            } else {
                &info
            }
        ));
    }
    let mut pages: Vec<&Value> = value["query"]["pages"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    pages.sort_by_key(|page| page["index"].as_i64().unwrap_or(i64::MAX));
    Ok(pages
        .into_iter()
        .filter_map(|page| album(page, edition))
        .collect())
}

/// One album article, or None for pages without an album infobox.
fn album(page: &Value, edition: &Edition) -> Option<OnlineItem> {
    let id = page["pageid"].as_u64().filter(|id| *id > 0)?;
    let revision = &page["revisions"][0];
    let content = revision["slots"]["main"]["content"]
        .as_str()
        .or_else(|| revision["content"].as_str())?;
    let article = wikitext::cleaned(content);
    let infobox = wikitext::templates(&article).find(|t| INFOBOXES.contains(&t.name.as_str()))?;
    let mut title = wikitext::plain(infobox.value(&["name", "titolo"]));
    if title.is_empty() {
        title = ALBUM_SUFFIX.replace(&text(page, "title"), "").into_owned();
    }
    if title.is_empty() {
        return None;
    }
    let artist = wikitext::plain(infobox.value(&["artist", "artista"]));
    let tracks = tracks(&article, &artist);
    let mut item = OnlineItem::new(Provider::Wikipedia);
    item.id = format!("{}:{id}", edition.code);
    item.year = first_year(infobox.value(&["released", "anno"])).unwrap_or_default();
    item.url = text(page, "fullurl");
    if !item.url.starts_with("https://") {
        item.url = format!("https://{}.wikipedia.org/?curid={id}", edition.code);
    }
    // Only the infobox picture is the cover; other page images are photos.
    let cover = file_name(infobox.value(&["cover", "immagine"]));
    let thumbnail = text(&page["thumbnail"], "source");
    if !cover.is_empty()
        && file_name(&text(page, "pageimage")) == cover
        && thumbnail.starts_with("https://")
    {
        item.cover = thumbnail;
    }
    item.count = tracks.len();
    item.raw = json!({
        "genre": first_genre(infobox.value(&["genre", "genere"])),
        "tracks": tracks,
        "_source": edition.label,
    });
    item.title = title;
    item.artist = artist;
    Some(item)
}

fn first_year(raw: &str) -> Option<String> {
    DIGITS
        .find_iter(&wikitext::plain(raw))
        .map(|m| m.as_str())
        .find(|digits| {
            digits.len() == 4
                && (digits.starts_with("18")
                    || digits.starts_with("19")
                    || digits.starts_with("20"))
        })
        .map(str::to_string)
}

fn first_genre(raw: &str) -> String {
    let plain = wikitext::plain(raw);
    let Some(first) = plain
        .split([',', '/', '·', '•', ';'])
        .map(str::trim)
        .find(|genre| !genre.is_empty())
    else {
        return String::new();
    };
    let mut chars = first.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// A file name as the infobox and the page image name it: without the
/// namespace, link markup or underscores, case-insensitive.
fn file_name(raw: &str) -> String {
    let name = raw.replace("[[", "").replace("]]", "");
    let name = name.split('|').next().unwrap_or("");
    FILE_PREFIX
        .replace(name, "")
        .replace('_', " ")
        .trim()
        .to_lowercase()
}

struct Row {
    number: Option<u32>,
    title: String,
    artist: String,
    seconds: Option<f64>,
}
/// One track list and the labels above it, such as "Disc two", "Side B"
/// or "Vinyl edition".
struct Block {
    label: String,
    rows: Vec<Row>,
}
impl Block {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            rows: Vec::new(),
        }
    }
}
struct Listed {
    disc: u32,
    track: u32,
    title: String,
    artist: String,
    seconds: Option<f64>,
}

/// Track listing templates first, then numbered lists under a track listing
/// heading. An unusable list gives no tracks, so the album fields can still
/// be applied.
fn tracks(article: &str, artist: &str) -> Vec<Value> {
    let mut listed = assemble(template_blocks(article, artist));
    if listed.is_empty() {
        listed = assemble(list_blocks(article, artist));
    }
    let mut positions = HashSet::new();
    if !listed.iter().all(|track| {
        !track.title.is_empty()
            && track.disc > 0
            && track.track > 0
            && positions.insert((track.disc, track.track))
    }) {
        return Vec::new();
    }
    listed.sort_by_key(|track| (track.disc, track.track));
    listed
        .into_iter()
        .map(|track| {
            json!({
                "disc": track.disc,
                "number": track.track,
                "title": track.title,
                "artist": track.artist,
                "seconds": track.seconds.unwrap_or(0.0),
            })
        })
        .collect()
}

/// The main edition's tracks. A disc label starts that disc, a side
/// continues the current one, and an unlabeled list that restarts at one is
/// the next disc. Once tracks are found, a bonus or alternative edition ends
/// the list.
fn assemble(blocks: Vec<Block>) -> Vec<Listed> {
    let mut result: Vec<Listed> = Vec::new();
    let mut disc = 1;
    let mut last: HashMap<u32, u32> = HashMap::new();
    for block in blocks {
        if result.is_empty() {
            if BONUS.is_match(&block.label) {
                continue;
            }
        } else if BONUS.is_match(&block.label) || ALTERNATIVE.is_match(&block.label) {
            break;
        }
        let Some(first) = block.rows.first().map(|row| row.number.unwrap_or(1)) else {
            continue;
        };
        let previous = |disc| last.get(&disc).copied().unwrap_or(0);
        if let Some(number) = disc_number(&block.label) {
            disc = number;
        } else if !result.is_empty()
            && !SIDE.is_match(&block.label)
            && first == 1
            && previous(disc) > 0
        {
            disc += 1;
        }
        // Vinyl sides restart at one in some articles; they continue the disc.
        let offset = if first == 1 { previous(disc) } else { 0 };
        for (index, row) in block.rows.into_iter().enumerate() {
            let track = row.number.unwrap_or(first + index as u32) + offset;
            let highest = last.entry(disc).or_default();
            *highest = (*highest).max(track);
            result.push(Listed {
                disc,
                track,
                title: row.title,
                artist: row.artist,
                seconds: row.seconds,
            });
        }
    }
    result
}

fn template_blocks(article: &str, artist: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut previous_end = 0;
    for Span {
        template,
        start,
        end,
    } in wikitext::spans(article)
    {
        if !TRACK_LISTS.contains(&template.name.as_str()) {
            continue;
        }
        let mut labels = label_lines(&article[previous_end.min(start)..start]);
        let headline = wikitext::plain(template.value(&["headline"]));
        if !headline.is_empty() {
            labels.push(headline);
        }
        previous_end = end;
        blocks.push(Block {
            label: labels.join(" · "),
            rows: template_rows(&template, artist),
        });
    }
    blocks
}

fn template_rows(list: &Template, artist: &str) -> Vec<Row> {
    let mut titles = HashMap::new();
    let mut lengths = HashMap::new();
    let mut extras = HashMap::new();
    for (key, value) in &list.named {
        let Some(captures) = NUMBERED_KEY.captures(key) else {
            continue;
        };
        let Ok(number) = captures[2].parse::<u32>() else {
            continue;
        };
        if !(1..1000).contains(&number) {
            continue;
        }
        match &captures[1] {
            "title" | "titolo" => titles.insert(number, wikitext::plain(value)),
            "extra" => extras.insert(number, wikitext::plain(value)),
            _ => lengths.insert(number, value.clone()),
        };
    }
    let extra_is_artist = ARTIST_COLUMN.is_match(&wikitext::plain(list.value(&["extra_column"])));
    let mut numbers: Vec<u32> = titles.keys().copied().collect();
    numbers.sort_unstable();
    numbers
        .into_iter()
        .map(|number| {
            let track_artist = extras
                .get(&number)
                .filter(|_| extra_is_artist)
                .filter(|artist| !artist.is_empty());
            Row {
                number: Some(number),
                title: titles.remove(&number).unwrap_or_default(),
                artist: track_artist.cloned().unwrap_or_else(|| artist.into()),
                seconds: lengths.get(&number).and_then(|length| duration(length)),
            }
        })
        .collect()
}

/// Labels written between two lists: sub-headings, ";term" lines and bold
/// lines, from the current section only.
fn label_lines(gap: &str) -> Vec<String> {
    let mut labels = Vec::new();
    for line in gap.lines().map(str::trim) {
        if let Some((level, name)) = heading(line) {
            if level <= 2 {
                labels.clear();
            }
            labels.push(name);
        } else if let Some(label) = label(line) {
            labels.push(label);
        }
    }
    labels
}

fn label(line: &str) -> Option<String> {
    if let Some(term) = line.strip_prefix(';') {
        return Some(wikitext::plain(term));
    }
    (line.starts_with("'''") && line.ends_with("'''") && line.len() > 6)
        .then(|| wikitext::plain(line))
}

fn list_blocks(article: &str, artist: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut section = None;
    for line in article.lines().map(str::trim) {
        if let Some((level, name)) = heading(line) {
            if let Some(section) = section {
                if level <= section {
                    break;
                }
                blocks.push(Block::new(name));
            } else if SECTION.is_match(&name) {
                section = Some(level);
                blocks.push(Block::new(""));
            }
            continue;
        }
        if section.is_none() {
            continue;
        }
        if let Some(label) = label(line) {
            // A label under a sub-heading belongs to it, as "Vinyl edition · Side A".
            let parent = match blocks.last() {
                Some(block) if block.rows.is_empty() => blocks.pop().unwrap().label,
                _ => String::new(),
            };
            let joined = [parent, label]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            blocks.push(Block::new(joined));
        } else if let Some(entry) = line
            .strip_prefix('#')
            .filter(|entry| !entry.starts_with(['#', ':', '*']))
        {
            let title = listed_title(entry);
            if title.is_empty() {
                continue;
            }
            let seconds = TIME
                .find_iter(entry)
                .last()
                .and_then(|m| duration(m.as_str()));
            if blocks.is_empty() {
                blocks.push(Block::new(""));
            }
            blocks.last_mut().unwrap().rows.push(Row {
                number: None,
                title,
                artist: artist.into(),
                seconds,
            });
        }
    }
    blocks
}

/// "== Name ==" with the same number of signs on both sides.
fn heading(line: &str) -> Option<(usize, String)> {
    let level = line.bytes().take_while(|b| *b == b'=').count();
    let closing = line.bytes().rev().take_while(|b| *b == b'=').count();
    if level < 2 || closing != level || line.len() <= level * 2 {
        return None;
    }
    Some((level, wikitext::plain(&line[level..line.len() - level])))
}

fn listed_title(entry: &str) -> String {
    for pattern in QUOTED.iter() {
        if let Some(found) = pattern.find(entry) {
            let title = wikitext::plain(found.as_str().trim_matches(['"', '“', '”', '\'']));
            if !title.is_empty() {
                return title;
            }
        }
    }
    let first = entry.split(" – ").next().unwrap_or(entry);
    let first = first.split(" — ").next().unwrap_or(first);
    wikitext::plain(first.split(" - ").next().unwrap_or(first))
}

fn disc_number(label: &str) -> Option<u32> {
    let found = DISC.captures(label)?;
    let word = found[1].to_lowercase();
    word.parse().ok().or(match word.as_str() {
        "one" | "uno" => Some(1),
        "two" | "due" => Some(2),
        "three" | "tre" => Some(3),
        "four" | "quattro" => Some(4),
        "five" | "cinque" => Some(5),
        "six" => Some(6),
        _ => None,
    })
}

/// "4:53" or "1:02:03" in seconds.
pub(super) fn duration(text: &str) -> Option<f64> {
    let plain = wikitext::plain(text);
    let parts: Vec<u32> = plain
        .trim()
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let seconds = parts.iter().fold(0, |total, part| total * 60 + part);
    (seconds > 0).then_some(f64::from(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(content: &str) -> Value {
        json!({"pageid": 7, "index": 1, "title": "Rapadopa",
               "fullurl": "https://it.wikipedia.org/wiki/Rapadopa",
               "revisions": [{"slots": {"main": {"content": content}}}]})
    }

    #[test]
    fn italian_article_gives_the_cd_edition_only() {
        let content = "{{Album\n|titolo = Rapadopa\n|artista = DJ Gruff\n|anno = 1993\n\
            |genere = Hip hop\n|genere2 = Jazz Rap\n}}\n\n== Tracce ==\n\
            === Edizione CD/Musicassetta ===\n\
            # ''La Rapadopa'' (feat. [[Neffa]] e [[Deda]])\n# ''Faustona''\n\
            # ''\"E Di Certo...\"'' (feat. [[Esa aka El Presidente|Esa]])\n\
            # ''XP'' [presente nell'edizione su cassetta e non nella versione CD]\n\
            === Edizione Vinile ===\n;Lato A\n# ''Stammi Lontano''\n;Lato B\n# ''I.P.A.S.''\n\
            == Crediti ==\n# ''Not a track''\n";
        let item = album(&page(content), &EDITIONS[1]).unwrap();
        assert_eq!(
            (
                item.title.as_str(),
                item.artist.as_str(),
                item.year.as_str()
            ),
            ("Rapadopa", "DJ Gruff", "1993")
        );
        assert_eq!(item.raw["genre"], "Hip hop");
        assert_eq!(item.raw["_source"], "Wikipedia (it)");
        let titles: Vec<&str> = item.raw["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|track| track["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, ["La Rapadopa", "Faustona", "E Di Certo...", "XP"]);
        assert_eq!(item.raw["tracks"][3]["number"], 4);
        assert_eq!(item.raw["tracks"][0]["artist"], "DJ Gruff");
        assert_eq!(item.count, 4);
    }

    #[test]
    fn english_templates_split_discs_and_join_sides() {
        let discs = "{{Infobox album | name = Mellon Collie | artist = [[The Smashing Pumpkins]] \
            | released = {{Start date|1995|10|23}} | genre = [[Alternative rock]] | cover = Mellon.jpg}}\n\
            ==Track listing==\n{{Track listing\n| headline = Disc one – ''Dawn to Dusk''\n\
            | title1 = Mellon Collie\n| length1 = 2:52\n| title2 = [[Tonight, Tonight (song)|Tonight, Tonight]]\n| length2 = 4:14\n}}\n\
            {{Track listing\n| headline = Disc two – ''Twilight''\n| title1 = Where Boys Fear to Tread\n| length1 = 4:22\n}}\n\
            {{Track listing\n| headline = 2012 deluxe edition bonus\n| title1 = Extra\n}}";
        let mut value = page(discs);
        value["pageimage"] = json!("Mellon.jpg");
        value["thumbnail"] = json!({"source": "https://upload.wikimedia.org/m.jpg"});
        let item = album(&value, &EDITIONS[0]).unwrap();
        assert_eq!(item.year, "1995");
        assert_eq!(item.raw["genre"], "Alternative rock");
        assert_eq!(item.cover, "https://upload.wikimedia.org/m.jpg");
        let tracks = item.raw["tracks"].as_array().unwrap();
        let positions: Vec<(u64, u64, &str, f64)> = tracks
            .iter()
            .map(|t| {
                (
                    t["disc"].as_u64().unwrap(),
                    t["number"].as_u64().unwrap(),
                    t["title"].as_str().unwrap(),
                    t["seconds"].as_f64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            positions,
            [
                (1, 1, "Mellon Collie", 172.0),
                (1, 2, "Tonight, Tonight", 254.0),
                (2, 1, "Where Boys Fear to Tread", 262.0)
            ]
        );

        let sides = "{{Infobox album | name = Rumours | artist = Fleetwood Mac}}\n\
            ==Track listing==\n{{Track listing | headline = Side one | title1 = Second Hand News | length1 = 2:43}}\n\
            {{Track listing | headline = Side two | title1 = Never Going Back Again | length1 = 2:02}}";
        let item = album(&page(sides), &EDITIONS[0]).unwrap();
        assert_eq!(item.raw["tracks"][1]["disc"], 1);
        assert_eq!(item.raw["tracks"][1]["number"], 2);
    }

    #[test]
    fn english_numbered_list_reads_quoted_titles_and_times() {
        let content = "{{Infobox album | name = OK Computer | artist = [[Radiohead]] | released = 21 May 1997}}\n\
            ==Track listing==\nAll tracks are written by Radiohead.\n\
            # \"[[Airbag / How Am I Driving?|Airbag]]\" – 4:44\n# \"[[Paranoid Android]]\" – 6:23\n\
            ==Personnel==\n* [[Nigel Godrich]]\n";
        let item = album(&page(content), &EDITIONS[0]).unwrap();
        assert_eq!(item.raw["tracks"][0]["title"], "Airbag");
        assert_eq!(item.raw["tracks"][1]["seconds"], 383.0);
        assert_eq!(item.year, "1997");
    }

    #[test]
    fn pages_without_an_album_infobox_are_left_out() {
        let value = json!({"query": {"pages": [
            page("{{Infobox musical artist | name = DJ Gruff}}"),
            page("{{Album | titolo = SxM | artista = Sangue Misto}}")
        ]}});
        let items = parse(&value, &EDITIONS[1]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "SxM");
        assert!(parse(&json!({"error": {"info": "Bad"}}), &EDITIONS[0]).is_err());
    }
}
