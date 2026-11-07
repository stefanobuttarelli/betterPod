//! Playlist refreshes target their own page. Artwork slots exist before IO.
use super::*;

impl Extras {
    pub(super) fn apply_playlist_listing(&mut self, lists: Vec<Playlist>) {
        self.lists = Arc::new(lists);
        let Some(page) = self.pages.last() else {
            return;
        };
        match page.role {
            PageRole::Playlists(_) => self.show_playlists(),
            PageRole::PlaylistSongs => {
                let Some(playlist) = page
                    .playlist_path
                    .as_ref()
                    .and_then(|path| self.lists.iter().find(|p| &p.path == path))
                    .cloned()
                else {
                    return;
                };
                let rows = self.playlist_rows(&playlist);
                let page = self.pages.last_mut().unwrap();
                page.title = playlist.name;
                page.rows = rows;
                page.selected = page.selected.min(page.rows.len().saturating_sub(1));
                page.visual_rows = None;
                Self::reveal(page, self.viewport_height);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture {
        extras: Extras,
        root: PathBuf,
    }
    impl Fixture {
        fn new(name: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "betterpod-list-layout-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(root.join("music")).unwrap();
            std::fs::create_dir_all(root.join("data")).unwrap();
            let extras = Extras::new(root.join("music"), root.join("data")).unwrap();
            Self { extras, root }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.extras.shutdown();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn playlist_listing_refresh_keeps_song_page_and_ignores_unrelated_pages() {
        let mut fixture = Fixture::new("playlist");
        let extras = &mut fixture.extras;
        let mut playlist = Playlist {
            name: "Playlists".into(), // A title cannot identify the page kind.
            path: fixture.root.join("data/playlists/owned.m3u8"),
            tracks: vec![fixture.root.join("music/First.m4a")],
            read_only: false,
        };
        extras.open(ExtraPage::Playlists);
        extras.apply_playlist_listing(vec![playlist.clone()]);
        extras.choose(Choice::Playlist(playlist.clone()));
        playlist.tracks.push(fixture.root.join("music/Second.m4a"));
        extras.apply_playlist_listing(vec![playlist.clone()]);
        let page = extras.visual_page().unwrap();
        assert_eq!(page.kind, crate::app::ViewKind::Full);
        assert!(page.mini);
        assert_eq!(page.rows.len(), 3);
        assert_eq!(page.rows[1].label, "First");
        assert_eq!(page.rows[2].label, "Second");
        assert_eq!(
            extras.pages.last().unwrap().playlist_path,
            Some(playlist.path)
        );
        extras.push(
            "Unrelated page",
            vec![Item::new("Keep me", Choice::Nothing)],
        );
        extras.apply_playlist_listing(Vec::new());
        assert_eq!(extras.title(), "Unrelated page");
        assert_eq!(extras.pages.last().unwrap().rows[0].title, "Keep me");
    }

    #[test]
    fn empty_playlist_submit_keeps_an_editable_keyboard_and_creates_no_playlist() {
        let mut fixture = Fixture::new("empty-name");
        let extras = &mut fixture.extras;
        extras.open(ExtraPage::Playlists);
        extras.choose(Choice::NewPlaylist);
        extras.keyboard.as_mut().unwrap().0.query = "   ".into();
        extras.type_key(Action::Start);
        assert!(extras.is_typing());
        assert!(!extras.keyboard.as_ref().unwrap().0.is_exiting());
        assert!(extras.effects.iter().any(|effect| {
            matches!(effect, Effect::Notice(text) if text == "Enter a playlist name")
        }));
        extras.type_key(Action::Select);
        assert!(!extras.keyboard.as_ref().unwrap().0.query.trim().is_empty());
        assert!(extras.lists.is_empty());
    }

    #[test]
    fn podcast_artwork_arrival_does_not_change_projected_row_geometry() {
        let mut fixture = Fixture::new("podcast-cover");
        let extras = &mut fixture.extras;
        let podcast = Podcast {
            name: "Podcast".into(),
            author: "Host".into(),
            feed: "https://example.invalid/feed.xml".into(),
            art600: "https://example.invalid/cover.jpg".into(),
            ..Podcast::default()
        };
        let episode = Episode {
            title: "Episode".into(),
            url: "https://example.invalid/episode.mp3".into(),
            ..Episode::default()
        };
        let mut online = OnlineItem::new(Provider::Podcast);
        online.title = podcast.name.clone();
        online.cover = podcast.art600.clone();
        extras.searches.push(SearchState {
            generation: 0,
            provider: Provider::Podcast,
            metadata: false,
            query: "Podcast".into(),
            items: vec![online],
            total: 1,
            page: 1,
            error: String::new(),
            busy: false,
        });
        extras.subscriptions.push(podcast.clone());
        let variants = [
            extras.podcast_home_rows(),
            extras.search_rows(Provider::Podcast, false),
            extras.podcast_feed_rows(&podcast, &[episode]),
        ];
        for rows in variants {
            extras.pages.clear();
            extras.push("Podcast rows", rows);
            let before = extras.visual_page().unwrap();
            let row = before.rows.iter().find(|r| r.visual.thumbnail).unwrap();
            assert!(row.cover.is_none());
            let height = crate::app::app_visual_menu::row_height(row);
            extras.previews.insert(
                podcast.art600.clone(),
                Some(fixture.root.join("downloaded-cover.jpg")),
            );
            extras.invalidate_visual_rows();
            let after = extras.visual_page().unwrap();
            let loaded = after.rows.iter().find(|r| r.visual.thumbnail).unwrap();
            assert_eq!(loaded.label, row.label);
            assert!(loaded.cover.is_some());
            assert_eq!(crate::app::app_visual_menu::row_height(loaded), height);
            assert_eq!(height, 96.0);
            extras.previews.clear();
        }
    }
}
