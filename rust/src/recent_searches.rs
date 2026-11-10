//! Recent Searches (Go `ij_recent.go`): the last week's searches, listed under
//! the search field of Internet Archive, YouTube and Podcasts until a new
//! search runs. Choosing one runs it again; its options remove it or clear
//! the screen's list.
//!
//! This is a plain model: the app's state writer persists its JSON in
//! `recent-searches.json`, so saving never blocks drawing.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A search older than this is no longer offered.
pub const KEEP_SECONDS: u64 = 7 * 24 * 60 * 60;
/// Each search screen keeps at most this many, newest first.
pub const PER_SCREEN: usize = 10;
/// Longer text is not a search anybody retypes; it is not kept.
const MAX_TERM_BYTES: usize = 256;
/// A corrupt or hostile file is ignored rather than parsed.
pub const MAX_FILE_BYTES: usize = 64 * 1024;

/// The search screens that keep their searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchScreen {
    Archive,
    /// Videos and playlists share one list, as in Go.
    YouTube,
    Podcasts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentSearch {
    pub term: String,
    /// Seconds since the Unix epoch.
    pub at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecentSearches(BTreeMap<SearchScreen, Vec<RecentSearch>>);

impl RecentSearches {
    /// Reads a saved list. Anything malformed starts empty; old, empty,
    /// duplicate and excess entries are dropped.
    pub fn from_json(bytes: &[u8], now: u64) -> Self {
        if bytes.len() > MAX_FILE_BYTES {
            return Self::default();
        }
        let mut searches: Self = serde_json::from_slice(bytes).unwrap_or_default();
        for list in searches.0.values_mut() {
            let saved = std::mem::take(list);
            for search in saved {
                if let Some(term) = clean(&search.term) {
                    if !list.iter().any(|kept| same(&kept.term, &term)) {
                        list.push(RecentSearch {
                            term,
                            at: search.at,
                        });
                    }
                }
            }
            list.sort_by(|a, b| b.at.cmp(&a.at));
        }
        searches.prune(now);
        searches
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| b"{}".to_vec())
    }

    /// The screen's searches still offered at `now`, newest first.
    pub fn list(&self, screen: SearchScreen, now: u64) -> Vec<RecentSearch> {
        self.0
            .get(&screen)
            .map(|list| {
                list.iter()
                    .filter(|search| fresh(search.at, now))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Puts a search first. Running it again moves it up instead of adding a
    /// copy; letter case and surrounding spaces do not make a new search.
    /// Returns whether anything changed.
    pub fn record(&mut self, screen: SearchScreen, term: &str, now: u64) -> bool {
        let Some(term) = clean(term) else {
            return false;
        };
        let list = self.0.entry(screen).or_default();
        if list
            .first()
            .is_some_and(|first| first.term == term && first.at == now)
        {
            return false;
        }
        list.retain(|search| !same(&search.term, &term));
        list.insert(0, RecentSearch { term, at: now });
        self.prune(now);
        true
    }

    pub fn remove(&mut self, screen: SearchScreen, term: &str) -> bool {
        let Some(list) = self.0.get_mut(&screen) else {
            return false;
        };
        let before = list.len();
        list.retain(|search| !same(&search.term, term));
        let changed = list.len() != before;
        if list.is_empty() {
            self.0.remove(&screen);
        }
        changed
    }

    pub fn clear(&mut self, screen: SearchScreen) -> bool {
        self.0.remove(&screen).is_some()
    }

    fn prune(&mut self, now: u64) {
        for list in self.0.values_mut() {
            list.retain(|search| fresh(search.at, now));
            list.truncate(PER_SCREEN);
        }
        self.0.retain(|_, list| !list.is_empty());
    }
}

fn clean(term: &str) -> Option<String> {
    let term = term.split_whitespace().collect::<Vec<_>>().join(" ");
    (!term.is_empty() && term.len() <= MAX_TERM_BYTES).then_some(term)
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// A search from the future (a clock that was behind, then corrected) is kept
/// until the week after it, rather than lost.
fn fresh(at: u64, now: u64) -> bool {
    now.saturating_sub(at) < KEEP_SECONDS
}

/// "Today", "Yesterday" or "3 days ago", by calendar day in local time.
pub fn when(at: u64, now: u64) -> String {
    when_with_offset(at, now, local_offset(now))
}

fn when_with_offset(at: u64, now: u64, offset: i64) -> String {
    let day = |secs: u64| (secs as i64 + offset).div_euclid(86_400);
    match day(now) - day(at) {
        i64::MIN..=0 => "Today".into(),
        1 => "Yesterday".into(),
        days => format!("{days} days ago"),
    }
}

/// Seconds east of UTC for the console's local time at `now`.
fn local_offset(now: u64) -> i64 {
    #[cfg(unix)]
    {
        let time = now as libc::time_t;
        // SAFETY: `localtime_r` writes only into the `tm` passed to it; both
        // pointers are valid for the duration of the call.
        let mut parts: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&time, &mut parts) }.is_null() {
            return parts.tm_gmtoff as i64;
        }
    }
    let _ = now;
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;
    const NOW: u64 = 1_800_000_000;

    #[test]
    fn newest_first_without_case_or_space_duplicates() {
        let mut recent = RecentSearches::default();
        assert!(recent.record(SearchScreen::YouTube, "  lofi   beats ", NOW - 30));
        assert!(recent.record(SearchScreen::YouTube, "jazz", NOW - 20));
        assert!(recent.record(SearchScreen::YouTube, "LOFI beats", NOW - 10));
        let terms: Vec<_> = recent
            .list(SearchScreen::YouTube, NOW)
            .into_iter()
            .map(|s| s.term)
            .collect();
        assert_eq!(terms, ["LOFI beats", "jazz"]);
        assert!(recent.list(SearchScreen::Archive, NOW).is_empty());
        assert!(!recent.record(SearchScreen::Archive, "   ", NOW));
    }

    #[test]
    fn keeps_ten_per_screen_for_a_week() {
        let mut recent = RecentSearches::default();
        for i in 0..12 {
            recent.record(SearchScreen::Podcasts, &format!("term {i}"), NOW - 12 + i);
        }
        let list = recent.list(SearchScreen::Podcasts, NOW);
        assert_eq!(list.len(), PER_SCREEN);
        assert_eq!(list[0].term, "term 11");
        assert!(recent
            .list(SearchScreen::Podcasts, NOW + KEEP_SECONDS)
            .is_empty());
    }

    #[test]
    fn remove_and_clear_touch_one_screen() {
        let mut recent = RecentSearches::default();
        recent.record(SearchScreen::Archive, "Grateful Dead", NOW);
        recent.record(SearchScreen::Archive, "Bach", NOW);
        recent.record(SearchScreen::YouTube, "Bach", NOW);
        assert!(recent.remove(SearchScreen::Archive, "bach"));
        assert!(!recent.remove(SearchScreen::Archive, "bach"));
        assert_eq!(recent.list(SearchScreen::Archive, NOW).len(), 1);
        assert!(recent.clear(SearchScreen::Archive));
        assert!(recent.list(SearchScreen::Archive, NOW).is_empty());
        assert_eq!(recent.list(SearchScreen::YouTube, NOW).len(), 1);
    }

    #[test]
    fn saved_json_round_trips_and_bad_files_start_empty() {
        let mut recent = RecentSearches::default();
        recent.record(SearchScreen::YouTube, "jazz", NOW);
        let bytes = recent.to_json();
        assert_eq!(
            String::from_utf8(bytes.clone()).unwrap(),
            format!(r#"{{"youtube":[{{"term":"jazz","at":{NOW}}}]}}"#)
        );
        assert_eq!(RecentSearches::from_json(&bytes, NOW), recent);
        assert_eq!(
            RecentSearches::from_json(b"not json", NOW),
            Default::default()
        );
        let old = format!(
            r#"{{"archive":[{{"term":"x","at":{}}}],"other":[]}}"#,
            NOW - KEEP_SECONDS
        );
        assert_eq!(
            RecentSearches::from_json(old.as_bytes(), NOW),
            Default::default()
        );
        let doubled = format!(
            r#"{{"podcasts":[{{"term":"A","at":{NOW}}},{{"term":"a","at":{}}},{{"term":" ","at":{NOW}}}]}}"#,
            NOW - 1
        );
        let loaded = RecentSearches::from_json(doubled.as_bytes(), NOW);
        assert_eq!(loaded.list(SearchScreen::Podcasts, NOW).len(), 1);
    }

    #[test]
    fn days_follow_the_local_calendar() {
        // 23:30 and 00:30 local time are different days at UTC+1.
        let offset = 3_600;
        let midnight = 1_799_971_200 - offset as u64; // a local midnight
        assert_eq!(
            when_with_offset(midnight - 1_800, midnight + 1_800, offset),
            "Yesterday"
        );
        assert_eq!(
            when_with_offset(midnight + 60, midnight + 1_800, offset),
            "Today"
        );
        assert_eq!(
            when_with_offset(midnight - 3 * DAY, midnight + 60, offset),
            "3 days ago"
        );
        assert_eq!(when_with_offset(NOW + 60, NOW, 0), "Today");
    }
}
