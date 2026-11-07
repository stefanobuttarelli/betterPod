//! Pure query/matching rules from Go1.1 ij_covers.go. No network or file access.
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_normalization::UnicodeNormalization;

// Go regexp's Perl classes are ASCII, including its word boundaries. Spell
// them explicitly: Rust regex defaults would otherwise broaden \s/\d/\b.
static BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\t\n\f\r ]*[\(\[\{][^\)\]\}]*[\)\]\}]").unwrap());
static EDITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[\t\n\f\r ]*[-–:][\t\n\f\r ]*([0-9]{4}[\t\n\f\r ]+)?(remaster(ed)?|deluxe|expanded|anniversary|edition|bonus|special|collector'?s?|mono|stereo|live)(?-u:\b).*$").unwrap()
});
static DISC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)[\t\n\f\r ]*[-–,]?[\t\n\f\r ]*(?-u:\b)(cd|disc|disk)[\t\n\f\r ]*[0-9]+(?-u:\b).*$",
    )
    .unwrap()
});
static FEAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[\t\n\f\r ]+(feat\.?|ft\.?|featuring|with)[\t\n\f\r ]+.*$").unwrap()
});
static VARIOUS_ARTISTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?i)^various artists$").unwrap());

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackContext {
    pub artist: String,
    pub album_artist: String,
    pub genre: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AlbumQuery {
    pub title: String,
    pub artists: Vec<String>,
    pub various: bool,
    pub soundtrack: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchStage {
    /// Original Lucene query for /ws/2/release-group (not /release).
    pub query: String,
    pub need_artist: bool,
}
impl AlbumQuery {
    pub fn new(title: &str, album_artist: &str, tracks: &[TrackContext]) -> Self {
        let mut count = HashMap::<String, usize>::new();
        let mut artists = Vec::<String>::new();
        let mut add = |name: &str, weight: usize| {
            let name = name.trim();
            if !usable_artist(name) {
                return;
            }
            if !count.contains_key(name) {
                artists.push(name.to_owned());
            }
            *count.entry(name.to_owned()).or_default() += weight;
        };
        add(album_artist, 100);
        for track in tracks {
            add(&track.album_artist, 10);
            add(&track.artist, 1);
        }
        artists.sort_by_key(|name| std::cmp::Reverse(count[name])); // stable ties keep encounter order
        let track_artists = tracks
            .iter()
            .filter(|t| usable_artist(&t.artist))
            .map(|t| clean_artist(&t.artist).to_lowercase())
            .collect::<HashSet<_>>();
        let lower = title.to_lowercase();
        let mut soundtrack =
            lower.contains("soundtrack") || lower.contains("o.s.t") || lower.contains(" ost");
        let genres = tracks
            .iter()
            .filter(|track| soundtrack_genre(&track.genre))
            .count();
        if genres > 0 && genres * 2 >= tracks.len() {
            soundtrack = true;
        }
        Self {
            title: title.into(),
            artists,
            various: track_artists.len() > 2 || !usable_artist(album_artist),
            soundtrack,
        }
    }
    pub fn artist_ok(&self, artist: &str) -> bool {
        self.artists.iter().any(|ours| same_artist(ours, artist))
            || self.various && (VARIOUS_ARTISTS.is_match(artist.trim()) || artist.is_empty())
    }
    pub fn title_ok(&self, title: &str, artist: &str, candidate_soundtrack: bool) -> bool {
        if same_title(&self.title, title) {
            return true;
        }
        let main_same = same_title(main_title(&self.title), main_title(title))
            || same_title(main_title(&self.title), title);
        main_same && (self.artist_ok(artist) || self.soundtrack && candidate_soundtrack)
    }
    pub fn matches(&self, title: &str, artist: &str, candidate_soundtrack: bool) -> bool {
        self.title_ok(title, artist, candidate_soundtrack)
            && (self.artist_ok(artist) || self.soundtrack || self.various)
    }
    pub fn source_terms(&self) -> Vec<String> {
        let title = clean_title(&self.title);
        let mut terms = Vec::new();
        if let Some(artist) = self.artists.first() {
            terms.push(format!("{title} {}", clean_artist(artist)));
        }
        terms.push(title);
        terms
    }
    pub fn musicbrainz_stages(&self) -> Vec<SearchStage> {
        let title = clean_title(&self.title);
        let mut stages = Vec::new();
        for artist in self.artists.iter().take(2) {
            let artist = clean_artist(artist);
            stages.push(SearchStage {
                query: format!(
                    "releasegroup:{} AND artist:{}",
                    mb_escape(&title),
                    mb_escape(&artist)
                ),
                need_artist: true,
            });
            stages.push(SearchStage {
                query: format!(
                    "releasegroup:({}) AND artist:({})",
                    lucene(&title),
                    lucene(&artist)
                ),
                need_artist: true,
            });
        }
        stages.push(SearchStage {
            query: format!("releasegroup:{}", mb_escape(&title)),
            need_artist: !(self.soundtrack || self.various),
        });
        let short = clean_title(main_title(&self.title));
        if short != title {
            stages.push(SearchStage {
                query: format!("releasegroup:{}", mb_escape(&short)),
                need_artist: !(self.soundtrack || self.various),
            });
        }
        stages
    }
}
pub fn soundtrack_genre(genre: &str) -> bool {
    let genre = genre.trim().to_lowercase();
    matches!(genre.as_str(), "ost" | "vgm")
        || genre.contains("soundtrack")
        || genre.contains("game")
        || genre.contains("score")
}
fn mb_escape(text: &str) -> String {
    format!(
        "\"{}\"",
        text.trim().replace('\\', "\\\\").replace('"', "\\\"")
    )
}
pub fn lucene(text: &str) -> String {
    text.chars()
        .map(|c| {
            if "+-&|!(){}[]^\"~*?:\\/".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn query_title(title: &str) -> String {
    clean_title(title)
}
pub fn clean_title(title: &str) -> String {
    let cleaned = BRACKETS.replace_all(title, "");
    let cleaned = EDITION.replace_all(&cleaned, "");
    let cleaned = DISC.replace_all(&cleaned, "");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        title.trim().to_owned()
    } else {
        cleaned.to_owned()
    }
}
pub fn clean_artist(artist: &str) -> String {
    let cleaned = FEAT.replace_all(artist, "");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        artist.trim().to_owned()
    } else {
        cleaned.to_owned()
    }
}
fn words(text: &str) -> Vec<String> {
    let mut out = String::with_capacity(text.len());
    for character in text.nfd().flat_map(char::to_lowercase) {
        match character {
            '&' => out.push_str(" and "),
            '\'' | '’' | '‘' | '`' | '.' => {}
            _ => match get_general_category(character) {
                GeneralCategory::NonspacingMark => {}
                GeneralCategory::UppercaseLetter
                | GeneralCategory::LowercaseLetter
                | GeneralCategory::TitlecaseLetter
                | GeneralCategory::ModifierLetter
                | GeneralCategory::OtherLetter
                | GeneralCategory::DecimalNumber => out.push(character),
                _ => out.push(' '),
            },
        }
    }
    out.split_whitespace()
        .filter(|word| *word != "the")
        .map(str::to_owned)
        .collect()
}
pub fn title_words(title: &str) -> Vec<String> {
    words(&clean_title(title))
        .into_iter()
        .filter_map(|mut word| {
            if matches!(
                word.as_str(),
                "remaster"
                    | "remastered"
                    | "deluxe"
                    | "edition"
                    | "expanded"
                    | "anniversary"
                    | "bonus"
                    | "special"
                    | "version"
                    | "mono"
                    | "stereo"
                    | "explicit"
                    | "digital"
                    | "collector"
                    | "collectors"
            ) || word.len() == 4 && word.as_str() >= "1900" && word.as_str() <= "2099"
            {
                return None;
            }
            if word.len() > 3 && word.ends_with('s') && !word.ends_with("ss") {
                word.pop();
            }
            Some(word)
        })
        .collect()
}
pub fn same_title(ours: &str, theirs: &str) -> bool {
    let ours = title_words(ours);
    let theirs = title_words(theirs);
    !ours.is_empty() && !theirs.is_empty() && ours == theirs
}
pub fn same_artist(ours: &str, theirs: &str) -> bool {
    let ours = words(&clean_artist(ours));
    let theirs = words(&clean_artist(theirs));
    if ours.is_empty() || theirs.is_empty() {
        return false;
    }
    if ours == theirs {
        return true;
    }
    let ours = format!(" {} ", ours.join(" "));
    let theirs = format!(" {} ", theirs.join(" "));
    ours.contains(&theirs) || theirs.contains(&ours)
}
pub fn usable_artist(artist: &str) -> bool {
    !matches!(
        artist.trim().to_lowercase().as_str(),
        "" | "unknown artist" | "various artists" | "various" | "va"
    )
}
pub fn main_title(title: &str) -> &str {
    for separator in [": ", " - ", " – ", " — "] {
        if let Some(index) = title.find(separator).filter(|index| *index > 0) {
            return &title[..index];
        }
    }
    title
}
/// Same normalized title plus the named artist. An untagged/compilation
/// query may use a title-only match, as in the source automatic cover search.
pub fn cover_matches(
    our_album: &str,
    our_artist: &str,
    their_album: &str,
    their_artist: &str,
) -> bool {
    AlbumQuery::new(our_album, our_artist, &[]).matches(their_album, their_artist, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires executed original-Go matching fixtures; no network"]
    fn matches_executed_original_go_reference() -> Result<(), String> {
        let path = std::env::var("BETTERPOD_COVER_MATCH_GO_RESULTS").map_err(|e| e.to_string())?;
        let cases: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        assert!(!cases.is_empty());
        for case in &cases {
            let get = |key: &str| case[key].as_str().unwrap_or("");
            let context = || {
                let tracks: Vec<TrackContext> = serde_json::from_value(
                    case.get("tracks")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!([])),
                )
                .unwrap();
                AlbumQuery::new(get("a"), get("b"), &tracks)
            };
            let actual = match get("kind") {
                "album_query" => {
                    let query = context();
                    serde_json::json!({"title":query.title,"artists":query.artists,"various":query.various,"soundtrack":query.soundtrack,"source_terms":query.source_terms(),"musicbrainz_stages":query.musicbrainz_stages()})
                }
                "album_candidate" => {
                    let query = context();
                    let soundtrack = case["their_soundtrack"].as_bool().unwrap_or(false);
                    serde_json::json!({"matches":query.matches(get("c"),get("d"),soundtrack),"artist_ok":query.artist_ok(get("d")),"title_ok":query.title_ok(get("c"),get("d"),soundtrack)})
                }
                "clean_title" => serde_json::json!(clean_title(get("a"))),
                "title_words" => serde_json::json!(title_words(get("a"))),
                "same_title" => serde_json::json!(same_title(get("a"), get("b"))),
                "same_artist" => serde_json::json!(same_artist(get("a"), get("b"))),
                "usable_artist" => serde_json::json!(usable_artist(get("a"))),
                "cover_matches" => {
                    serde_json::json!(cover_matches(get("a"), get("b"), get("c"), get("d")))
                }
                other => return Err(format!("Unknown original fixture {other}")),
            };
            assert_eq!(actual, case["result"], "original Go fixture {case}");
        }
        Ok(())
    }
    #[test]
    fn album_query_weights_artists_stably_and_uses_compilation_genre_context() {
        let tracks = vec![
            TrackContext {
                artist: "A".into(),
                album_artist: "Compiler".into(),
                genre: "Soundtrack".into(),
            },
            TrackContext {
                artist: "B".into(),
                album_artist: "Compiler".into(),
                genre: "OST".into(),
            },
            TrackContext {
                artist: "C".into(),
                album_artist: "Compiler".into(),
                genre: "Rock".into(),
            },
        ];
        let query = AlbumQuery::new("Suite [Deluxe]", "Primary", &tracks);
        assert_eq!(query.artists, vec!["Primary", "Compiler", "A", "B", "C"]);
        assert!(query.various);
        assert!(query.soundtrack);
        assert_eq!(query.source_terms(), vec!["Suite Primary", "Suite"]);
        assert!(query.artist_ok("Various Artists"));
        assert!(query.artist_ok("Various Artiſts"));
        assert!(query.matches("Suite", "Unrelated", false));
        let stages = query.musicbrainz_stages();
        assert_eq!(stages.len(), 5);
        assert!(stages[..4].iter().all(|stage| stage.need_artist));
        assert!(!stages[4].need_artist);
    }
    #[test]
    fn soundtrack_main_title_requires_candidate_context_when_artist_differs() {
        let tracks = [TrackContext {
            artist: "Composer".into(),
            album_artist: String::new(),
            genre: "Video Game Music".into(),
        }];
        let query = AlbumQuery::new("Hercules: Original Score", "Composer", &tracks);
        assert!(query.soundtrack);
        assert!(!query.various);
        assert!(query.matches("Hercules", "Unrelated", true));
        assert!(!query.matches("Hercules", "Unrelated", false));
        assert!(query.matches("Hercules: Original Score", "Unrelated", false));
        let stages = query.musicbrainz_stages();
        assert_eq!(stages.last().unwrap().query, "releasegroup:\"Hercules\"");
        assert!(!stages.last().unwrap().need_artist);
        let normal = AlbumQuery::new("Ordinary Album", "Artist", &[]);
        assert!(normal.musicbrainz_stages().last().unwrap().need_artist);
    }
    #[test]
    fn source_edition_disc_and_empty_cleaning_rules() {
        for (input, expected) in [
            ("Rumours [Deluxe Edition]", "Rumours"),
            ("Rumours (Remastered 2011)", "Rumours"),
            ("Rumours - 2004 Remaster", "Rumours"),
            ("Album, Disc 2 Bonus", "Album"),
            ("Album CD1", "Album"),
            (" [Deluxe Edition] ", "[Deluxe Edition]"),
            ("Moon – Live", "Moon"),
        ] {
            assert_eq!(query_title(input), expected, "{input}");
        }
        assert_eq!(query_title("Moon Live"), "Moon Live");
    }
    #[test]
    fn source_words_normalize_accents_apostrophes_punctuation_and_editions() {
        for (a, b) in [
            ("Rumours 2004 Remaster", "Rumours"),
            (
                "Sgt. Pepper’s Lonely Hearts Club Band",
                "Sgt Peppers Lonely Hearts Club Band",
            ),
            ("Café del Mar", "Cafe\u{301} del Mar"),
            ("The Records & Songs", "Record and Song"),
            ("Glass", "Glass"),
        ] {
            assert!(same_title(a, b), "{a} / {b}");
        }
        for (a, b) in [
            ("Rumours Live", "Rumours"),
            ("Blue Train", "Blue"),
            ("1989", ""),
            ("", ""),
            ("AC/DC", "ACDC"),
        ] {
            assert!(!same_title(a, b), "{a} / {b}");
        }
    }
    #[test]
    fn source_artist_words_use_token_boundaries_and_drop_featured_names() {
        for (a, b) in [
            ("The Beatles", "Beatles & Friends"),
            ("Björk feat. Arca", "Bjork"),
            ("Miles Davis with John Coltrane", "Miles Davis"),
            ("Sigur Rós", "Sigur Ros"),
        ] {
            assert!(same_artist(a, b), "{a} / {b}");
        }
        assert!(!same_artist("Abba", "Abbacadabra"));
        assert!(!same_artist("", ""));
        assert!(!same_artist("Blue Train", "Train Blue"));
    }
    #[test]
    fn automatic_cover_matches_keep_artist_checks_and_compilation_title_fallback() {
        assert!(cover_matches(
            "Rumours [Deluxe]",
            "Fleetwood Mac",
            "Rumours",
            "Fleetwood Mac"
        ));
        assert!(!cover_matches(
            "Rumours",
            "Fleetwood Mac",
            "Rumours",
            "Other Artist"
        ));
        assert!(!cover_matches(
            "Rumours Live",
            "Fleetwood Mac",
            "Rumours",
            "Fleetwood Mac"
        ));
        assert!(cover_matches(
            "Rumours",
            "Unknown Artist",
            "Rumours",
            "Fleetwood Mac"
        ));
        assert!(cover_matches(
            "Compilation",
            "Various Artists",
            "Compilation",
            "Various Artists"
        ));
        assert!(cover_matches(
            "Hercules: An Original Walt Disney Records Soundtrack",
            "Alan Menken",
            "Hercules",
            "Alan Menken"
        ));
        assert!(!cover_matches(
            "Hercules: An Original Walt Disney Records Soundtrack",
            "Alan Menken",
            "Hercules",
            "Unrelated"
        ));
        for artist in ["", " Unknown Artist ", "Various Artists", "VARIOUS", "va"] {
            assert!(!usable_artist(artist));
        }
        assert!(usable_artist("Unknown"));
        assert!(usable_artist("Björk"));
    }
}
