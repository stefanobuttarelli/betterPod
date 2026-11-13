//! Splitting one long YouTube video of an album into its songs: find the
//! release in Find Info Online, check its track list, then download and cut.
use super::visual::fact;
use super::*;
use betterpod_rust::downloads::album_split::SplitAlbum;

impl Extras {
    /// Find Info Online for the release a full-album video holds.
    pub(super) fn open_track_list_search(&mut self, video: OnlineItem) {
        let query = track_list_query(&video.title);
        self.cover_search = false;
        self.combined = Some(CombinedState {
            query: query.clone(),
            automatic_query: query,
            cover: false,
            cover_album: String::new(),
            cover_artist: String::new(),
            cover_paths: Arc::new(Vec::new()),
            cover_context: None,
            manual: false,
            generation: 0,
            hits: Vec::new(),
            pending: 0,
            errors: Vec::new(),
            split_video: Some(video),
        });
        self.push("Find Track List", Vec::new());
        self.pages.last_mut().unwrap().role = PageRole::Combined;
        self.run_combined(None);
    }
    /// The video a chosen release would split, while its search is open.
    pub(super) fn track_list_video(&self) -> Option<OnlineItem> {
        self.pages
            .last()
            .filter(|page| page.role == PageRole::Combined)?;
        self.combined.as_ref()?.split_video.clone()
    }
    /// The songs of the chosen release, and the download that cuts the video into them.
    pub(super) fn track_list_rows(video: &OnlineItem, release: &OnlineItem) -> Vec<Item> {
        let Some(mut album) = SplitAlbum::from_item(release) else {
            let mut row = fact("No track list", "");
            row.detail = "This release lists no songs. Choose another one".into();
            return vec![row];
        };
        if album.cover.is_empty() {
            album.cover = video.cover.clone();
        }
        let listed = album.listed_seconds();
        let lengths = match listed {
            Some(listed) if video.seconds > 0.0 => format!(
                "Video {} · songs {}",
                super::super::time_label(video.seconds),
                super::super::time_label(listed)
            ),
            _ => String::new(),
        };
        // The page is titled with the album; the lengths come first so they
        // stay visible next to a long artist name.
        let identity = album.artist.clone();
        let count = album.tracks.len();
        let mut rows = Vec::with_capacity(count + 1);
        if video.seconds > 0.0 && !album.fits(video.seconds) {
            let mut row = fact("Lengths don't match", "");
            row.detail = if lengths.is_empty() {
                "This release doesn't fit the video. Choose another one".into()
            } else {
                format!("{lengths} · Choose another release")
            };
            rows.push(row);
        } else {
            let tracks = album.tracks.clone();
            let mut download = Item::new(
                format!("Download and Split · {count} songs"),
                Choice::Download(DownloadRequest::YouTube {
                    url: video.url.clone(),
                    video: false,
                    split_chapters: false,
                    track_list: Some(Arc::new(album)),
                }),
            );
            download.detail = [lengths, identity]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            rows.push(download);
            let discs = tracks.iter().map(|t| t.disc).max().unwrap_or(1);
            for (index, track) in tracks.iter().enumerate() {
                let number = if track.number > 0 {
                    track.number as usize
                } else {
                    index + 1
                };
                let label = if discs > 1 {
                    format!("{}-{number}. {}", track.disc, track.title)
                } else {
                    format!("{number}. {}", track.title)
                };
                let mut row = fact(label, "");
                if track.seconds > 0.0 {
                    row.visual.value = super::super::time_label(track.seconds);
                }
                rows.push(row);
            }
        }
        rows
    }
}

/// The album name in a video title, without "Full Album", years and rip notes.
pub(super) fn track_list_query(title: &str) -> String {
    let mut text = String::new();
    let mut depth = 0usize;
    for c in title.chars() {
        match c {
            '(' | '[' | '{' | '【' => depth += 1,
            ')' | ']' | '}' | '】' => depth = depth.saturating_sub(1),
            _ if depth == 0 => text.push(c),
            _ => {}
        }
    }
    // The phrases are ASCII, so a match starts and ends on character boundaries.
    let letter = |b: u8| b.is_ascii_alphanumeric() || b >= 0x80;
    for noise in [
        "full album",
        "full lp",
        "album completo",
        "disco completo",
        "complete album",
        "whole album",
        "official audio",
        "hq",
        "hd",
        "cd rip",
    ] {
        let mut at = 0;
        while at + noise.len() <= text.len() {
            let bytes = text.as_bytes();
            let end = at + noise.len();
            if bytes[at..end].eq_ignore_ascii_case(noise.as_bytes())
                && (at == 0 || !letter(bytes[at - 1]))
                && (end == bytes.len() || !letter(bytes[end]))
            {
                text.replace_range(at..end, " ");
            }
            at += 1;
        }
    }
    text.split(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | '|' | '·' | ':'))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_titles_become_album_searches() {
        assert_eq!(
            track_list_query("Basley Click (2001) [Full Album - HQ CD RIP]"),
            "Basley Click"
        );
        assert_eq!(
            track_list_query("DJ Gruff - Rapadopa | FULL ALBUM"),
            "DJ Gruff Rapadopa"
        );
        assert_eq!(
            track_list_query("Neffa – Arrivi e partenze (album completo)"),
            "Neffa Arrivi e partenze"
        );
        assert_eq!(track_list_query("Hdmi Blues"), "Hdmi Blues");
    }

    #[test]
    fn results_naming_every_searched_word_rank_first() {
        let result = |artist: &str, title: &str| {
            let mut item = OnlineItem::new(Provider::Discogs);
            item.artist = artist.into();
            item.title = title.into();
            item
        };
        let query = "Basley Click";
        assert_eq!(
            visual::matched_words(query, &result("Basley Click", "The Album")),
            2
        );
        assert_eq!(
            visual::matched_words(query, &result("Stilettos", "Click Click Click")),
            1
        );
        assert_eq!(visual::matched_words(query, &result("Neffa", "Arrivi")), 0);
    }

    #[test]
    fn a_release_that_fits_offers_its_songs() {
        let mut video = OnlineItem::new(Provider::YouTube);
        video.url = "https://www.youtube.com/watch?v=byWrIsWCqR4".into();
        video.seconds = 3417.0;
        video.cover = "https://i.ytimg.com/vi/byWrIsWCqR4/hq.jpg".into();
        let mut release = OnlineItem::new(Provider::Discogs);
        release.id = "2168207".into();
        release.title = "The Album".into();
        release.artist = "Basley Click".into();
        release.raw = serde_json::json!({"tracks": [
            {"disc":1,"number":1,"title":"Sei Il Nostro Idolo","seconds":293.0},
            {"disc":1,"number":2,"title":"E Dire Che Tu","seconds":3096.0}]});
        let rows = Extras::track_list_rows(&video, &release);
        assert_eq!(rows[0].title, "Download and Split · 2 songs");
        assert_eq!(rows[0].detail, "Video 56:57 · songs 56:29 · Basley Click");
        let Choice::Download(DownloadRequest::YouTube {
            track_list: Some(list),
            ..
        }) = &rows[0].choice
        else {
            panic!("no split download");
        };
        assert_eq!(list.cover, video.cover);
        assert_eq!(rows[1].title, "1. Sei Il Nostro Idolo");
        assert_eq!(rows[1].visual.value, "4:53");

        video.seconds = 1800.0;
        let rows = Extras::track_list_rows(&video, &release);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Lengths don't match");
    }
}
