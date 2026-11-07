//! Service pages adapt their actual state into the same Go1.1 list renderer.
//! The immutable row projection is built once per page/status/artwork mutation.
use super::*;

pub(super) fn item_height(item: &Item) -> f32 {
    if item.visual.header {
        44.0
    } else if item.visual.search_field {
        92.0
    } else if item.visual.progress_visible || item.visual.progress > 0.0 {
        136.0
    } else {
        let mut height = 16.0 + crate::render::line_box(28.0).0;
        if !item.detail.is_empty() {
            height += crate::render::line_box(24.0).0;
        }
        if item.visual.thumbnail
            || item_art(item).is_some()
            || item.cover.is_some()
            || !item.visual.icon.is_empty()
        {
            height = height.max(96.0);
        }
        height
    }
}
impl Extras {
    #[cfg(test)]
    pub(in crate::app) fn qa_youtube_details(&mut self, item: OnlineItem, cover: Option<PathBuf>) {
        self.cancel_browse_jobs();
        self.keyboard = None;
        self.pages.clear();
        if let Some(path) = cover {
            self.previews.insert(item.cover.clone(), Some(path));
        }
        self.detail_item = Some(item.clone());
        self.media_queue = Self::youtube_playlist_entries(&item);
        self.push("YouTube", self.youtube_details_rows(&item));
        let page = self.pages.last_mut().unwrap();
        page.role = PageRole::YouTubeItem;
        page.selected = 1;
    }
    #[cfg(test)]
    pub(in crate::app) fn qa_youtube_busy(&mut self, item: OnlineItem, cover: Option<PathBuf>) {
        self.cancel_browse_jobs();
        self.keyboard = None;
        self.pages.clear();
        if let Some(path) = cover {
            self.previews.insert(item.cover.clone(), Some(path));
        }
        let mut result = Item::new(item.title.clone(), Choice::Online(item.clone(), false));
        result.detail = item.artist.clone();
        result.visual.thumbnail = true;
        self.push(
            "YouTube",
            vec![
                search_row(Provider::YouTube, "anri timely", ""),
                header("Showing: Videos"),
                result,
            ],
        );
        let page = self.pages.last_mut().unwrap();
        page.selected = 2;
        let owner = page.key;
        self.claim_foreground(
            999_998,
            owner,
            Purpose::Details {
                item,
                metadata: false,
                generation: self.generation,
            },
        );
    }
    pub(in crate::app) fn has_custom_visual(&self) -> bool {
        self.share_view.is_some()
            || self
                .pages
                .last()
                .is_some_and(|page| matches!(page.role, PageRole::Download(_)))
            || self
                .forecast
                .is_some_and(|_| self.pages.len() == self.forecast_depth)
    }
    pub(in crate::app) fn visual_page(&mut self) -> Option<super::super::Page> {
        if self.has_custom_visual() {
            return None;
        }
        let mut index = self.pages.len().checked_sub(1)?;
        while self.pages[index].role == PageRole::Sheet {
            index = index.checked_sub(1)?;
        }
        self.project_page(index, true)
    }
    pub(in crate::app) fn visual_options(&mut self) -> Option<super::super::Page> {
        let index = self.pages.len().checked_sub(1)?;
        if self.pages[index].role != PageRole::Sheet {
            return None;
        }
        self.project_page(index, false)
    }
    pub(super) fn project_page(
        &mut self,
        index: usize,
        include_status: bool,
    ) -> Option<super::super::Page> {
        let foreground_row = self.foreground_busy_row();
        let external_row = self.external_busy_row();
        let page = self.pages.get_mut(index)?;
        let status = if include_status && self.status_page.is_none_or(|owner| owner == page.key) {
            self.status.as_str()
        } else {
            ""
        };
        if page.visual_rows.is_none() || page.visual_status != status {
            let mut rows = Vec::with_capacity(page.rows.len() + 1);
            for (item_index, item) in page.rows.iter().enumerate() {
                let mut row = super::super::Row::new(&item.title, super::super::Intent::Noop);
                row.detail = item.detail.clone();
                row.visual = item.visual.clone();
                row.visual.busy |= foreground_row == Some((page.key, item_index));
                row.visual.busy |= external_row == Some((page.key, item_index));
                row.visual.search_gear = item.gear.is_some();
                row.visual.search_gear_focused =
                    page.gear_on && page.selected == item_index && self.keyboard.is_none();
                if row.visual.search_field {
                    if let Some((keyboard, Typing::Search(provider, _))) = self.keyboard.as_ref() {
                        if matches!(item.choice,Choice::Search(p) if p==*provider) {
                            row.label = keyboard.with_caret();
                            row.visual.typing = true;
                            row.visual.value.clear();
                        }
                    }
                }
                row.arrow = !matches!(
                    item.choice,
                    Choice::Nothing | Choice::BtPower(_) | Choice::WeatherUnits
                ) && !row.visual.header
                    && !row.visual.no_arrow;
                if row.visual.search_field {
                    if let Some((keyboard, Typing::Combined | Typing::Weather)) =
                        self.keyboard.as_ref()
                    {
                        if matches!(item.choice, Choice::CombinedSearch | Choice::WeatherSearch) {
                            row.label = keyboard.with_caret();
                            row.visual.typing = true;
                            row.visual.value.clear();
                        }
                    }
                }
                if let Some((keyboard, Typing::Tag(field))) = self.keyboard.as_ref() {
                    if matches!(item.choice,Choice::TagField(f)if f==*field) {
                        row.detail = keyboard.with_caret();
                        row.visual.typing = true;
                    }
                }
                row.cover = item.cover.clone().or_else(|| {
                    item_art(item)
                        .and_then(|url| self.previews.get(url))
                        .and_then(Clone::clone)
                });
                if let Choice::Preview(entry, _, _) = &item.choice {
                    if self.preview_origin.as_ref() == Some(&entry.url) {
                        if let Some(state) = self.preview_state.as_ref() {
                            row.label = if state.error.is_some() {
                                row.label.clone()
                            } else if state.loading {
                                "Loading…".into()
                            } else {
                                entry.title.clone()
                            };
                            row.detail = if let Some(error) = &state.error {
                                format!("{error} · press [A] to try again")
                            } else if state.loading {
                                let progress = self
                                    .listen_snapshot
                                    .total
                                    .filter(|total| *total > 0)
                                    .map_or(0, |total| {
                                        self.listen_snapshot.done.saturating_mul(100) / total
                                    });
                                format!("{progress}% · {}", entry.title)
                            } else {
                                format!(
                                    "{} · {} of {}",
                                    if state.paused {
                                        "Paused · press [A] to play"
                                    } else {
                                        "Playing"
                                    },
                                    super::super::time_label(state.position),
                                    super::super::time_label(state.duration)
                                )
                            };
                            row.visual.progress = if state.error.is_some() {
                                0.0
                            } else if state.loading {
                                self.listen_snapshot
                                    .total
                                    .filter(|total| *total > 0)
                                    .map_or(0.02, |total| {
                                        (self.listen_snapshot.done as f64 / total as f64).max(0.02)
                                    }) as f32
                            } else if state.duration > 0.0 {
                                (state.position / state.duration).clamp(0.01, 1.0) as f32
                            } else {
                                0.02
                            };
                        }
                    }
                }
                rows.push(row);
            }
            if !status.is_empty() {
                let mut row = super::super::Row::new(status, super::super::Intent::Noop);
                row.arrow = false;
                row.visual.no_arrow = true;
                rows.push(row);
            }
            page.visual_rows = Some(Arc::new(rows));
            page.visual_status = status.into();
        }
        Some(super::super::Page {
            screen: super::super::Screen::Extra,
            kind: if page.role == PageRole::VideoHome {
                super::super::ViewKind::Split
            } else {
                super::super::ViewKind::Full
            },
            mini: page.mini,
            title: page.title.clone(),
            rows: Arc::clone(page.visual_rows.as_ref().unwrap()),
            selected: page.selected,
            scroll: page.scroll,
            target_scroll: page.target,
            enter: 0.0,
        })
    }
    pub(in crate::app) fn take_navigation(&mut self) -> Option<(super::super::Page, bool, f64)> {
        self.pending_navigation.take()
    }
    pub(in crate::app) fn draw_overlay(
        &mut self,
        r: &mut Renderer,
        assets: &VisualAssets,
        dark: bool,
        now: f64,
    ) {
        self.visual_time = now;
        if let Some((keyboard, _)) = self.keyboard.as_mut() {
            keyboard.draw(r, assets, dark, now);
        }
        if let Some(keyboard) = self.closing_keyboard.as_mut() {
            keyboard.draw(r, assets, dark, now);
        }
        if self
            .closing_keyboard
            .as_ref()
            .is_some_and(|keyboard| keyboard.finished(now))
        {
            self.closing_keyboard = None;
        }
    }
    pub(super) fn invalidate_visual_rows(&mut self) {
        for page in &mut self.pages {
            page.visual_rows = None;
        }
    }
}

fn item_art(item: &Item) -> Option<&str> {
    // Go's inline Listen/track rows have text and transport feedback. The
    // source's picture belongs to the poster and Cover rows above them.
    if matches!(item.choice, Choice::Preview(_, _, _)) && !item.visual.thumbnail {
        None
    } else {
        choice_art(&item.choice)
    }
}

pub(super) fn header(title: impl Into<String>) -> Item {
    let mut row = Item::new(title, Choice::Nothing);
    row.visual.header = true;
    row.visual.no_arrow = true;
    row
}
pub(super) fn fact(title: impl Into<String>, value: impl Into<String>) -> Item {
    let mut row = Item::new(title, Choice::Nothing);
    row.visual.value = value.into();
    row.visual.no_arrow = true;
    row
}
pub(super) fn search_row(provider: Provider, query: &str, note: impl Into<String>) -> Item {
    let mut row = Item::new(query, Choice::Search(provider));
    row.visual.value = note.into();
    row.visual.search_field = true;
    row.visual.placeholder = match provider {
        Provider::Archive => "Search archive.org",
        Provider::Podcast => "Search podcasts",
        _ => "Search YouTube",
    }
    .into();
    row.visual.no_arrow = true;
    row
}
pub(super) fn license_label(url: &str) -> String {
    let lower = url.trim().to_lowercase();
    if lower.is_empty() {
        return "No license stated".into();
    }
    if lower.contains("publicdomain/zero") {
        return "CC0 (public domain)".into();
    }
    if lower.contains("publicdomain/mark") || lower.contains("public domain") {
        return "Public domain".into();
    }
    if let Some((_, tail)) = lower.split_once("creativecommons.org/licenses/") {
        let mut p = tail.trim_matches('/').split('/');
        let kind = p.next().unwrap_or("").to_uppercase();
        let version = p.next().unwrap_or("");
        return format!(
            "CC {kind}{}",
            if version.is_empty() {
                String::new()
            } else {
                format!(" {version}")
            }
        );
    }
    lower
        .split_once("://")
        .and_then(|(_, tail)| tail.split('/').next())
        .filter(|s| !s.is_empty())
        .unwrap_or(url)
        .into()
}
pub(super) fn archive_identifier(value: &str) -> bool {
    (3..=100).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
pub(super) fn bytes_label(value: u64) -> String {
    if value >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", value as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if value >= 1024 * 1024 {
        format!("{:.1} MB", value as f64 / (1024.0 * 1024.0))
    } else if value >= 1024 {
        format!("{} KB", value / 1024)
    } else {
        format!("{value} bytes")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paired_and_discovered_bluetooth_rows_keep_their_device_pictures() {
        let root = std::env::temp_dir().join(format!("betterpod-bt-icons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("music")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        let mut extras = Extras::new(root.join("music"), root.join("data")).unwrap();
        extras.bt_state.powered = true;
        for (index, name, icon, paired) in [
            (0, "WH-1000XM6", "audio-headset", true),
            (1, "AirPods Pro", "audio-headphones", true),
            (2, "JBL Flip", "audio-card", false),
            (3, "Controller", "input-gaming", false),
            (4, "Laptop", "computer", false),
        ] {
            extras
                .bt_state
                .devices
                .push(betterpod_rust::bluetooth::Device {
                    address: format!("00:11:22:33:44:{index:02}"),
                    name: name.into(),
                    icon: icon.into(),
                    paired,
                    connected: index == 0,
                    battery: (index == 0).then_some(80),
                });
        }
        let rows = extras.bluetooth_rows();
        for (name, expected) in [
            ("WH-1000XM6", "bt_headphones"),
            ("AirPods Pro", "bt_earbuds"),
            ("JBL Flip", "bt_speaker"),
            ("Controller", "bt_gamepad"),
            ("Laptop", "bt_other"),
        ] {
            let row = rows.iter().find(|row| row.title == name).unwrap();
            assert_eq!(row.visual.icon, expected, "{name}");
        }
        assert_eq!(
            rows.iter()
                .find(|row| row.title == "WH-1000XM6")
                .unwrap()
                .detail,
            "Connected · Battery 80%"
        );
        assert!(rows
            .iter()
            .find(|row| row.title == "JBL Flip")
            .unwrap()
            .detail
            .starts_with("New"));
        drop(extras);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn first_share_requires_explicit_nonempty_name_before_network_or_sender_setup() {
        let root =
            std::env::temp_dir().join(format!("betterpod-share-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let music = root.join("music");
        let data = root.join("data");
        std::fs::create_dir_all(&music).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let mut extras = Extras::new(music, data).unwrap();
        extras.send_share(vec![root.join("missing.flac")], ShareInfo::default(), None);
        assert_eq!(extras.title(), "Name This betterPod");
        assert!(matches!(
            extras.pending_share,
            Some(PendingShare::Send { .. })
        ));
        assert!(extras.share_view.is_none());
        assert!(extras.is_typing());
        assert!(!extras
            .pages
            .last()
            .unwrap()
            .rows
            .iter()
            .any(|row| matches!(row.choice, Choice::FinishShareName)));
        extras.cancel_typing();
        assert!(extras.pending_share.is_some());
        assert!(extras.sharing_name.is_empty());
        extras.open(ExtraPage::Receive);
        assert!(matches!(extras.pending_share, Some(PendingShare::Receive)));
        extras.submit_typing(Typing::SharingName, "  ".into());
        assert!(extras.pending_share.is_some());
        assert!(extras.share_view.is_none());
        extras.submit_typing(Typing::SharingName, "  Emanuele  ".into());
        assert!(extras.pending_share.is_none());
        assert_eq!(extras.sharing_name, "Emanuele");
        assert!(extras.share_view.is_some());
        assert!(extras
            .effects
            .iter()
            .any(|effect| matches!(effect,Effect::SharingName(name)if name=="Emanuele")));
        extras.shutdown();
        drop(extras);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn episode_date_uses_original_readable_date_not_iso_prefix() {
        assert_eq!(podcast_date("2026-11-04T12:00:00+00:00", 2026), "Nov 4");
        assert_eq!(podcast_date("2025-11-04", 2026), "Nov 4, 2025");
        assert_eq!(podcast_date("not a date", 2026), "");
    }
    #[test]
    fn selected_service_row_stays_above_mini_or_keyboard_without_scroll_lag() {
        let mut page = Page::new(
            "Playlist",
            (0..12)
                .map(|i| Item::new(format!("Song {i}"), Choice::Nothing))
                .collect(),
        );
        page.selected = 11;
        Extras::reveal(&mut page, 324.);
        let total = page.rows.iter().map(item_height).sum::<f32>();
        assert_eq!(page.scroll, total - 324.);
        assert_eq!(page.target, page.scroll);
        Extras::reveal(&mut page, 198.);
        assert_eq!(page.scroll, total - 198.);
        Extras::reveal(&mut page, 440.);
        assert_eq!(page.scroll, (total - 440.).max(0.));
    }
    #[test]
    fn background_cover_events_stay_out_of_normal_service_page_handlers() {
        let root =
            std::env::temp_dir().join(format!("betterpod-cover-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("music")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        let mut extras = Extras::new(root.join("music"), root.join("data")).unwrap();
        extras.push("Keep this page", vec![fact("Original row", "")]);
        let handle = extras.start_missing_covers(Vec::new()).unwrap();
        assert!(extras.start_missing_covers(Vec::new()).is_err());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut finished = false;
        while !finished && std::time::Instant::now() < deadline {
            let effects = extras.tick(0.);
            assert!(!effects
                .iter()
                .any(|effect| matches!(effect, Effect::Rescan)));
            while let Some(event) = extras.poll_cover_search() {
                match event {
                    Event::Progress { id, .. } => assert_eq!(id, handle.id),
                    Event::Finished { id, result } => {
                        assert_eq!(id, handle.id);
                        assert!(result.is_ok());
                        finished = true;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(finished);
        assert_eq!(extras.title(), "Keep this page");
        assert_eq!(extras.pages.last().unwrap().rows[0].title, "Original row");
        assert!(extras.cover_jobs.is_empty());
        extras.shutdown();
        drop(extras);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn original_archive_identifier_and_license_labels() {
        assert!(archive_identifier("Some-Live_2001.ogg"));
        assert!(!archive_identifier("ab"));
        assert!(!archive_identifier("../file"));
        assert!(!archive_identifier("has spaces"));
        assert_eq!(
            license_label("https://creativecommons.org/licenses/by-sa/4.0/"),
            "CC BY-SA 4.0"
        );
        assert_eq!(
            license_label("https://creativecommons.org/publicdomain/zero/1.0"),
            "CC0 (public domain)"
        );
        assert_eq!(license_label(""), "No license stated");
    }
}

impl Extras {
    pub(super) fn push_sheet(&mut self, title: impl Into<String>, mut rows: Vec<Item>) {
        rows.push(Item::new("Cancel", Choice::Dismiss));
        self.pages.push(Page::new(title, rows));
        self.pages.last_mut().unwrap().role = PageRole::Sheet;
    }
    pub(in crate::app) fn resolve_preview(
        &mut self,
        item: OnlineItem,
        queue: Vec<OnlineItem>,
        index: usize,
    ) {
        self.start_inline_listen(item, queue, index);
    }
    pub(super) fn push_confirmation(
        &mut self,
        title: impl Into<String>,
        note: &str,
        confirm: &str,
        choice: Choice,
    ) {
        let mut title = title.into();
        if !note.is_empty() {
            title.push_str(". ");
            title.push_str(note);
        }
        let action = Item::new(
            confirm,
            Choice::ConfirmAction(Box::new(choice), title.clone()),
        );
        let mut page = Page::new(title, vec![action, Item::new("Cancel", Choice::Dismiss)]);
        page.role = PageRole::Sheet;
        self.pages.push(page);
    }
    pub(super) fn search_rows(&self, provider: Provider, metadata: bool) -> Vec<Item> {
        let Some(state) = self
            .searches
            .iter()
            .find(|state| state.provider == provider && state.metadata == metadata)
        else {
            let mut rows = vec![search_row(provider, "", "")];
            if self.youtube_search_mode_available(provider, metadata) {
                rows[0].gear = Some(Choice::YouTubeSearchSettings(provider));
            }
            if let Some(kind) = self.youtube_search_kind_row(provider, metadata) {
                rows.push(kind);
            }
            return rows;
        };
        let mut rows = vec![search_row(
            provider,
            &state.query,
            if state.total > 0 {
                format!(
                    "{} {}",
                    state.total,
                    if state.total == 1 {
                        "result"
                    } else {
                        "results"
                    }
                )
            } else {
                String::new()
            },
        )];
        if self.youtube_search_mode_available(provider, metadata) {
            rows[0].gear = Some(Choice::YouTubeSearchSettings(provider));
        }
        if let Some(kind) = self.youtube_search_kind_row(provider, metadata) {
            rows.push(kind);
        }
        if state.query.trim().is_empty() {
            return rows;
        }
        if provider == Provider::Archive
            && !matches!(self.keyboard.as_ref(),Some((_,Typing::Search(p,m)))if *p==provider&&*m==metadata)
            && archive_identifier(&state.query)
            && !state.items.iter().any(|item| item.id == state.query)
        {
            let mut item = OnlineItem::new(provider);
            item.id = state.query.clone();
            item.title = state.query.clone();
            let mut row = Item::new(
                format!("Open item “{}”", state.query),
                Choice::Online(item, false),
            );
            row.detail = format!("By its identifier (archive.org/details/{})", state.query);
            rows.push(row);
        }
        for item in &state.items {
            let mut row = Item::new(
                &item.title,
                if metadata && self.cover_search {
                    Choice::UseCover(item.clone())
                } else {
                    self.video_search_choice(item, metadata)
                        .unwrap_or_else(|| Choice::Online(item.clone(), metadata))
                },
            );
            if self.video_search_choice(item, metadata).is_some() {
                row.options = self.video_download_options(item);
                row.visual.no_arrow = true;
                row.visual.thumbnail = true;
            }
            // Go's YouTube results keep a thumbnail slot before art arrives.
            let youtube =
                !metadata && matches!(provider, Provider::YouTube | Provider::YouTubePlaylists);
            if youtube {
                row.visual.thumbnail = true;
            }
            // Reserve podcast artwork before its asynchronous preview arrives.
            // The placeholder and the downloaded image must share one layout.
            if provider == Provider::Podcast {
                row.visual.thumbnail = true;
            }
            let mut sub = Vec::new();
            if !item.artist.is_empty() {
                sub.push(item.artist.clone());
            }
            if !item.year.is_empty() && !youtube {
                sub.push(item.year.clone());
            }
            if youtube {
                if item.kind == "playlist" {
                    sub.push(if item.count > 0 {
                        format!("{} videos", item.count)
                    } else {
                        "Playlist".into()
                    });
                } else if item.seconds > 0. {
                    sub.push(super::super::time_label(item.seconds));
                }
            }
            if provider == Provider::Archive {
                sub.push(license_label(&item.license));
            }
            row.detail = sub.join(" · ");
            rows.push(row);
        }
        if state.busy {
            rows.push(fact("Searching…", ""));
        } else if !state.error.is_empty() {
            let mut row = Item::new(
                &state.error,
                Choice::NextSearch(provider, state.query.clone(), state.page, metadata),
            );
            row.visual.no_arrow = true;
            rows.push(row);
        } else if state.items.len() < state.total {
            let mut row = Item::new(
                "More results…",
                Choice::NextSearch(provider, state.query.clone(), state.page + 1, metadata),
            );
            row.detail = format!("{} of {} shown", state.items.len(), state.total);
            rows.push(row);
        } else if state.items.is_empty() {
            rows.push(fact("No results", "Try another search"));
        }
        rows
    }
    fn youtube_search_kind_row(&self, provider: Provider, metadata: bool) -> Option<Item> {
        if metadata || !matches!(provider, Provider::YouTube | Provider::YouTubePlaylists) {
            return None;
        }
        let kind = if provider == Provider::YouTubePlaylists {
            "Playlists & Albums"
        } else {
            "Videos"
        };
        Some(header(format!("Showing: {kind}")))
    }
    pub(super) fn archive_rows(&self, item: &OnlineItem) -> Vec<Item> {
        let mut rows = vec![
            header(&item.title),
            fact("License", license_label(&item.license)),
        ];
        let Some(groups) = item.raw["groups"].as_object() else {
            rows.push(fact("Loading…", ""));
            return rows;
        };
        let queue = archive_preview_queue(item);
        if let Some(first) = queue.first() {
            let mut row = Item::new("Listen", Choice::Preview(first.clone(), queue.clone(), 0));
            row.detail = format!(
                "{} {}, before you download",
                queue.len(),
                if queue.len() == 1 { "song" } else { "songs" }
            );
            row.visual.no_arrow = true;
            rows.push(row);
        }
        for format in ["FLAC", "ALAC", "WAV", "AIFF", "MP3"] {
            let Some(files) = groups
                .get(format)
                .and_then(serde_json::Value::as_array)
                .filter(|files| !files.is_empty())
            else {
                continue;
            };
            let size = files
                .iter()
                .map(|file| number_field(file, "size") as u64)
                .sum::<u64>();
            let mut row = Item::new(
                format!(
                    "Download {format}{}",
                    if format == "MP3" { "" } else { " (lossless)" }
                ),
                Choice::Download(DownloadRequest::Archive {
                    item: item.clone(),
                    format: format.into(),
                }),
            );
            row.detail = format!(
                "{} {} · {}",
                files.len(),
                if files.len() == 1 { "track" } else { "tracks" },
                bytes_label(size)
            );
            if let Some(&(have, left)) = self
                .archive_status
                .get(&item.id)
                .and_then(|formats| formats.get(format))
            {
                if have == files.len() {
                    row.title = format!("{format} downloaded");
                    row.detail = format!(
                        "All {} {} are in your library · press [A] to check again",
                        files.len(),
                        if files.len() == 1 { "track" } else { "tracks" }
                    );
                } else if have > 0 {
                    row.title = format!("Resume {format} download");
                    row.detail = format!(
                        "{} of {} {} left · {}",
                        files.len() - have,
                        files.len(),
                        if files.len() == 1 { "track" } else { "tracks" },
                        bytes_label(left)
                    );
                }
            }
            rows.push(self.archive_download_row(&item.id, format, row));
        }
        let Some((format, files)) =
            ["FLAC", "ALAC", "WAV", "AIFF", "MP3"]
                .into_iter()
                .find_map(|format| {
                    groups
                        .get(format)
                        .and_then(serde_json::Value::as_array)
                        .filter(|files| !files.is_empty())
                        .map(|files| (format, files))
                })
        else {
            rows.push(fact("No audio files betterPod can play", ""));
            return rows;
        };
        for (index, file) in files.iter().enumerate() {
            let title = string_field(file, "title");
            let title = if title.is_empty() {
                string_field(file, "name")
            } else {
                title
            };
            let mut row = Item::new(
                title,
                if queue.len() == files.len() {
                    Choice::Preview(queue[index].clone(), queue.clone(), index)
                } else {
                    Choice::ArchiveTrack(
                        item.clone(),
                        format.into(),
                        file.clone(),
                        queue.clone(),
                        index,
                    )
                },
            );
            row.options = vec![Item::new(
                "Download This Track",
                Choice::ArchiveTrack(
                    item.clone(),
                    format.into(),
                    file.clone(),
                    queue.clone(),
                    index,
                ),
            )];
            row.visual.no_arrow = true;
            row.visual.value = super::super::time_label(number_field(file, "length"));
            rows.push(row);
        }
        rows
    }
    pub(super) fn podcast_home_rows(&self) -> Vec<Item> {
        let mut rows = vec![search_row(
            Provider::Podcast,
            "",
            if self.subscriptions.is_empty() {
                ""
            } else {
                "Your Podcasts"
            },
        )];
        if self.subscriptions.is_empty() {
            let mut row = fact("Find a podcast", "");
            row.detail = "Search above; the ones you download from are kept here".into();
            rows.push(row);
        }
        for podcast in &self.subscriptions {
            rows.push(podcast_row(podcast));
        }
        rows
    }
    pub(super) fn podcast_feed_rows(&self, podcast: &Podcast, episodes: &[Episode]) -> Vec<Item> {
        let mut rows = vec![header(&podcast.name)];
        let latest = episodes
            .iter()
            .filter(|episode| {
                !podcast.files.contains_key(episode.key()) && episode_playable(episode)
            })
            .take(3)
            .cloned()
            .collect::<Vec<_>>();
        if latest.len() > 1 {
            let mut row = Item::new(
                format!("Download the {} Latest", latest.len()),
                Choice::DownloadEpisodes(podcast.clone(), latest),
            );
            row.detail = "Episodes not downloaded yet".into();
            rows.push(row);
        }
        for episode in episodes {
            let mut row = Item::new(
                &episode.title,
                Choice::Episode(episode.clone(), podcast.clone()),
            );
            row.visual.thumbnail = true;
            let mut sub = Vec::new();
            if !episode.date.is_empty() {
                sub.push(podcast_date(
                    &episode.date,
                    chrono::Datelike::year(&chrono::DateTime::<chrono::Utc>::from(
                        std::time::SystemTime::now(),
                    )),
                ));
            }
            if episode.seconds > 0.0 {
                sub.push(podcast_length(episode.seconds));
            }
            if podcast.files.contains_key(episode.key()) {
                sub.push("Downloaded".into());
            } else if !episode_playable(episode) {
                sub.push(format!(
                    "{} can't be played here",
                    episode.extension().to_uppercase()
                ));
                row.choice = Choice::Nothing;
                row.visual.no_arrow = true;
            } else if episode.size > 0 {
                sub.push(bytes_label(episode.size));
            }
            row.detail = sub.join(" · ");
            rows.push(row);
        }
        if rows.len() == 1 {
            rows.push(fact("No episodes with audio", ""));
        }
        rows
    }
}
fn string_field(value: &serde_json::Value, key: &str) -> String {
    value[key].as_str().map(str::to_owned).unwrap_or_default()
}
fn number_field(value: &serde_json::Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .or_else(|| value[key].as_str()?.parse().ok())
        .unwrap_or(0.0)
}
fn archive_preview_queue(item: &OnlineItem) -> Vec<OnlineItem> {
    let Some(groups) = item.raw["groups"].as_object() else {
        return Vec::new();
    };
    let Some(files) = ["MP3", "FLAC", "WAV", "AIFF", "ALAC"]
        .into_iter()
        .find_map(|format| {
            groups
                .get(format)
                .and_then(serde_json::Value::as_array)
                .filter(|files| !files.is_empty())
        })
    else {
        return Vec::new();
    };
    files
        .iter()
        .map(|file| {
            let mut entry = OnlineItem::new(Provider::Archive);
            entry.url = string_field(file, "download_url");
            entry.title = string_field(file, "title");
            if entry.title.is_empty() {
                entry.title = string_field(file, "name");
            }
            entry.artist = item.artist.clone();
            entry.cover = item.cover.clone();
            entry.seconds = number_field(file, "length");
            entry
        })
        .collect()
}
pub(super) fn episode_playable(episode: &Episode) -> bool {
    matches!(
        episode.extension(),
        "mp3" | "m4a" | "aac" | "flac" | "ogg" | "opus" | "wav" | "aiff" | "aif" | "ape" | "wv"
    )
}
fn podcast_date(value: &str, current_year: i32) -> String {
    use chrono::Datelike;
    let date = chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.date_naive())
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(value.get(..10).unwrap_or(value), "%Y-%m-%d").ok()
        });
    date.map(|date| {
        date.format(if date.year() == current_year {
            "%b %-d"
        } else {
            "%b %-d, %Y"
        })
        .to_string()
    })
    .unwrap_or_default()
}
fn podcast_length(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round().max(1.0) as u64;
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {} min", minutes / 60, minutes % 60)
    }
}
fn podcast_row(podcast: &Podcast) -> Item {
    let mut row = Item::new(&podcast.name, Choice::Feed(podcast.clone()));
    row.visual.thumbnail = true;
    let mut sub = vec![podcast.author.clone(), podcast.genre.clone()];
    sub.retain(|s| !s.is_empty());
    if podcast.count > 0 {
        sub.push(format!(
            "{} {}",
            podcast.count,
            if podcast.count == 1 {
                "episode"
            } else {
                "episodes"
            }
        ));
    }
    row.detail = sub.join(" · ");
    row
}
impl Extras {
    pub(in crate::app) fn set_preview_state(
        &mut self,
        origin: &str,
        snapshot: &betterpod_rust::video::VideoSnapshot,
    ) {
        if self.preview_origin.as_deref() != Some(origin) {
            return;
        }
        if !self.preview_transport_accepts(snapshot.generation)
            || (!snapshot.playing
                && !snapshot.paused
                && !snapshot.loading
                && !snapshot.ended
                && snapshot.error.is_none())
        {
            return;
        }
        let mut snapshot = snapshot.clone();
        snapshot.position += self.listen_snapshot.offset;
        if self.listen_snapshot.duration > 0.0 {
            snapshot.duration = self.listen_snapshot.duration;
        }
        // Only row-sized status changes invalidate the projection; never every video sample.
        let changed = self.preview_state.as_ref().is_none_or(|old| {
            old.loading != snapshot.loading
                || old.paused != snapshot.paused
                || old.error != snapshot.error
                || (old.position * 4.0) as u64 != (snapshot.position * 4.0) as u64
                || old.duration as u64 != snapshot.duration as u64
        });
        self.preview_state = Some(snapshot.clone());
        if changed {
            self.invalidate_visual_rows();
        }
    }
    pub(in crate::app) fn stop_preview(&mut self) {
        if self.preview_origin.take().is_some() {
            self.listen.stop();
            self.listen_snapshot = Default::default();
            self.preview_ready = false;
            self.preview_queue.clear();
            self.preview_depth = 0;
            self.preview_state = None;
            self.effects.push(Effect::StopPreview);
            self.invalidate_visual_rows();
        }
    }
}
impl Extras {
    pub(in crate::app) fn tools_snapshot(&self) -> &betterpod_rust::downloads::ToolStatus {
        &self.tools_state
    }
    pub(in crate::app) fn refresh_tools(&mut self) {
        self.task(Task::ToolsStatus, Purpose::Tools);
    }
    pub(in crate::app) fn install_tools(&mut self) {
        self.task(Task::Tools, Purpose::ToolAction);
    }
    pub(super) fn tools_rows(&self) -> Vec<Item> {
        let mut toggle = Item::new("YouTube", Choice::ToggleYouTube);
        toggle.visual.value = if self.youtube { "On" } else { "Off" }.into();
        toggle.detail = "Watch in Videos, download in Music → Search".into();
        toggle.visual.no_arrow = true;
        let mut rows = vec![toggle];
        if self.tools_state.ready {
            let mut ready = fact("Tools Ready", "");
            ready.detail = format!(
                "yt-dlp {} · {}",
                self.tools_state.version, self.tools_state.runtime
            );
            rows.push(ready);
            let mut update = Item::new("Update Tools", Choice::Tools);
            update.detail = "YouTube changes often: update when downloads fail".into();
            rows.push(update);
            let mut remove = Item::new("Remove Tools", Choice::RemoveTools);
            remove.detail = "Frees their space; YouTube is turned off".into();
            remove.visual.no_arrow = true;
            rows.push(remove);
        } else {
            let mut get = Item::new("Get the Tools", Choice::Tools);
            get.detail = "yt-dlp and Deno from GitHub: 80 MB to get, 185 MB on the card".into();
            rows.push(get);
        }
        let mut notice = fact("Only download what you may", "");
        notice.detail = "Your own or freely licensed videos".into();
        rows.push(notice);
        rows
    }
}
impl Extras {
    pub(super) fn weather_rows(&self) -> Vec<Item> {
        let mut search = Item::new(&self.weather_query, Choice::WeatherSearch);
        search.gear = Some(Choice::WeatherSettings);
        search.visual.search_field = true;
        search.visual.no_arrow = true;
        search.visual.placeholder = "Search for a city or town".into();
        if let Some(results) = &self.weather_results {
            search.visual.value = format!(
                "{} {}",
                results.len(),
                if results.len() == 1 {
                    "place"
                } else {
                    "places"
                }
            );
        }
        let mut rows = vec![search];
        if self.weather_loading {
            rows.push(fact("Searching…", ""));
        } else if !self.weather_error.is_empty() {
            let mut row = Item::new("Couldn't search", Choice::WeatherRetry);
            row.detail = format!("{} · press [A] to retry", self.weather_error);
            rows.push(row);
        }
        if let Some(results) = &self.weather_results {
            for place in results {
                let mut row = Item::new(&place.name, Choice::WeatherAdd(place.clone()));
                row.detail = place.location();
                rows.push(row);
            }
            if results.is_empty() {
                let mut row = fact("No places found", "");
                row.detail = "Try another name".into();
                rows.push(row);
            }
            return rows;
        }
        for (index, place) in self.weather_state.places.iter().enumerate() {
            let mut row = Item::new(&place.name, Choice::WeatherPlace(index));
            row.detail = place.location();
            if let Some(forecast) = &place.forecast {
                row.visual.value = betterpod_rust::weather::temperature(
                    forecast.current.temperature_2m,
                    self.weather_state.fahrenheit,
                );
                if !row.detail.is_empty() {
                    row.detail.push_str(" · ");
                }
                row.detail.push_str(betterpod_rust::weather::label(
                    forecast.current.weather_code,
                ));
            }
            rows.push(row);
        }
        rows
    }
    pub(super) fn weather_settings_rows(&self) -> Vec<Item> {
        let mut units = Item::new("Units", Choice::WeatherUnits);
        units.visual.no_arrow = true;
        units.visual.value = if self.weather_state.fahrenheit {
            "Fahrenheit (°F)"
        } else {
            "Celsius (°C)"
        }
        .into();
        let mut rows = vec![units];
        for (title, sub) in [
            (
                "Weather data by Open-Meteo.com",
                "Free and open forecasts · CC BY 4.0",
            ),
            ("Place search by Open-Meteo", "GeoNames data · CC BY 4.0"),
            (
                "Forecasts may be wrong",
                "Updated every 30 minutes when online",
            ),
        ] {
            let mut row = fact(title, "");
            row.detail = sub.into();
            rows.push(row);
        }
        rows
    }
}
impl Extras {
    pub(in crate::app) fn open_cover_search_all(
        &mut self,
        paths: Vec<PathBuf>,
        album: String,
        artist: String,
    ) {
        self.edit_paths = paths;
        self.edit_title = album.clone();
        self.cover_search = true;
        self.cover_override = true;
        self.status.clear();
        self.pages.clear();
        let query = format!("{album} {artist}").trim().to_string();
        self.combined = Some(CombinedState {
            query: query.clone(),
            automatic_query: query,
            cover: true,
            cover_album: album.clone(),
            cover_artist: artist.clone(),
            cover_paths: Arc::new(self.edit_paths.clone()),
            cover_context: None,
            manual: false,
            generation: 0,
            hits: Vec::new(),
            pending: 0,
            errors: Vec::new(),
        });
        self.push(album, Vec::new());
        self.pages.last_mut().unwrap().role = PageRole::Combined;
        self.run_combined(None);
    }
    pub(super) fn run_combined(&mut self, query: Option<String>) {
        self.cancel_foreground();
        if query.as_ref().is_some_and(|query| query.trim().is_empty()) {
            if let Some(state) = self.combined.as_mut() {
                state.query.clear();
            }
            self.refresh_combined();
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        for (handle, purpose) in self.jobs.values() {
            if matches!(
                purpose,
                Purpose::Combined { .. } | Purpose::CoverContext { .. }
            ) {
                handle.cancel();
            }
        }
        let Some(state) = self.combined.as_mut() else {
            return;
        };
        state.manual = query.is_some();
        state.query = query
            .unwrap_or_else(|| state.automatic_query.clone())
            .trim()
            .to_string();
        if state.query.is_empty() {
            return;
        }
        state.generation = generation;
        state.pending = 4;
        state.hits.clear();
        state.errors.clear();
        let prepare = if state.cover && state.cover_context.is_none() {
            Some((
                Arc::clone(&state.cover_paths),
                state.cover_album.clone(),
                state.cover_artist.clone(),
            ))
        } else {
            None
        };
        while self.pages.last().is_some_and(|p| p.role == PageRole::Sheet) {
            self.pages.pop();
        }
        self.refresh_combined();
        if let Some(page) = self.pages.last_mut() {
            page.selected = 0;
        }
        if let Some((paths, album, artist)) = prepare {
            self.task(
                Task::PrepareCoverQuery {
                    paths,
                    album,
                    artist,
                    library: Arc::clone(&self.library_snapshot),
                },
                Purpose::CoverContext { generation },
            );
        } else {
            self.start_combined_requests();
        }
    }
    pub(super) fn start_combined_requests(&mut self) {
        let Some(state) = &self.combined else { return };
        let generation = state.generation;
        let query = state.query.clone();
        let manual = state.manual;
        let context = state.cover_context.clone();
        let cover = state.cover;
        self.refresh_combined();
        for provider in [
            Provider::MusicBrainz,
            Provider::Itunes,
            Provider::Deezer,
            Provider::Wikipedia,
        ] {
            let task = if cover {
                let Some(album) = context.clone() else {
                    continue;
                };
                Task::SearchCovers {
                    provider,
                    album,
                    query: manual.then(|| query.clone()),
                }
            } else {
                Task::Search {
                    provider,
                    query: query.clone(),
                    page: 1,
                }
            };
            self.task(task, Purpose::Combined { generation });
        }
    }
    pub(super) fn refresh_combined(&mut self) {
        if !self
            .pages
            .last()
            .is_some_and(|page| page.role == PageRole::Combined)
        {
            return;
        }
        let Some(state) = &self.combined else { return };
        let title = self.pages.last().unwrap().title.clone();
        let selected = self.pages.last().unwrap().selected;
        let mut search = Item::new(&state.query, Choice::CombinedSearch);
        search.visual.search_field = true;
        search.visual.no_arrow = true;
        search.visual.placeholder = "Search MusicBrainz, iTunes, Deezer…".into();
        if state.pending == 0 {
            search.visual.value = format!(
                "{} {}",
                state.hits.len(),
                if state.hits.len() == 1 {
                    "result"
                } else {
                    "results"
                }
            );
        }
        let mut rows = vec![search];
        if state.pending > 0 {
            let mut loading = fact("Searching…", "");
            loading.detail = "MusicBrainz, iTunes, Deezer, Wikipedia".into();
            rows.push(loading);
        } else if state.hits.is_empty() {
            let mut row = Item::new(
                if state.errors.is_empty() {
                    "No results"
                } else {
                    "Couldn't search"
                },
                Choice::CombinedAutomatic,
            );
            row.detail = state
                .errors
                .first()
                .cloned()
                .unwrap_or_else(|| "Try other words".into());
            rows.push(row);
        }
        for item in state
            .hits
            .iter()
            .filter(|_| !state.cover || state.pending == 0)
        {
            let mut row = Item::new(
                &item.title,
                if state.cover {
                    Choice::UseCover(item.clone())
                } else {
                    Choice::Online(item.clone(), true)
                },
            );
            let mut sub = vec![item.artist.clone(), item.year.clone()];
            if state.cover && !item.kind.is_empty() {
                sub.push(item.kind.clone());
            }
            if !state.cover && item.count > 0 {
                sub.push(format!("{} songs", item.count));
            }
            sub.push(item.provider.name().into());
            if state.cover {
                row.visual.icon = "default_album_artwork".into();
                let failed = self
                    .previews
                    .get(&item.cover)
                    .is_some_and(|image| image.is_none())
                    && !self.jobs.values().any(
                        |(_, purpose)| matches!(purpose,Purpose::Preview(url)if *url==item.cover),
                    );
                if failed {
                    sub.push("Cover not loaded".into());
                } else if item.raw["_cover_best"].as_bool().unwrap_or(false) {
                    sub.push("Best match".into());
                }
            }
            sub.retain(|s| !s.is_empty());
            row.detail = sub.join(" · ");
            rows.push(row);
        }
        if state.pending == 0 && !state.hits.is_empty() {
            let mut row = Item::new("None of these?", Choice::CombinedSearch);
            row.detail = "Edit the search and try other words".into();
            rows.push(row);
        }
        self.replace(title, rows);
        let page = self.pages.last_mut().unwrap();
        page.role = PageRole::Combined;
        page.selected = selected.min(page.rows.len().saturating_sub(1));
    }
}
impl Extras {
    /// PipeWire/PulseAudio's actual default sink, sampled on the bounded worker.
    pub(in crate::app) fn bluetooth_audio_connected(&self) -> Option<bool> {
        self.bt_state.audio_connected
    }
    pub(in crate::app) fn bluetooth_snapshot(&self) -> &BtState {
        &self.bt_state
    }
    pub(in crate::app) fn prevents_screensaver(&self) -> bool {
        self.share_view.as_ref().is_some_and(|view| view.busy())
    }
}
impl Extras {
    pub(super) fn editor_scope_paths(&self) -> Vec<PathBuf> {
        if let Some(edit) = &self.album_edit {
            edit.songs
                .iter()
                .enumerate()
                .filter(|(index, _)| edit.scope.is_none_or(|scope| scope == *index))
                .map(|(_, song)| song.path.clone())
                .collect()
        } else {
            self.edit_paths.clone()
        }
    }
    pub(super) fn album_info_rows(&self) -> Vec<Item> {
        let Some(edit) = &self.album_edit else {
            return Vec::new();
        };
        if edit.loading {
            return vec![fact("Loading album information…", "")];
        }
        if edit.saving {
            let mut row = fact("Saving…", "");
            row.detail = self.status.clone();
            return vec![row];
        }
        let mut rows = Vec::new();
        if !edit.status.is_empty() {
            rows.push(fact(&edit.status, ""));
        }
        let mut find = Item::new("Find Info Online", Choice::FindInfo);
        find.detail = "MusicBrainz, iTunes, Deezer, Wikipedia".into();
        find.visual.icon = "search_icon".into();
        rows.push(find);
        for field in [
            Field::Album,
            Field::AlbumArtist,
            Field::Artist,
            Field::Year,
            Field::Genre,
        ] {
            rows.push(self.editor_field_row(field));
        }
        let mut songs = Item::new("Songs", Choice::EditSongs);
        songs.detail = format!(
            "{} {}",
            edit.songs.len(),
            if edit.songs.len() == 1 {
                "song"
            } else {
                "songs"
            }
        );
        rows.push(songs);
        let mut cover = Item::new("Cover", Choice::ChooseEditedCover);
        cover.cover = self
            .edit_paths
            .first()
            .and_then(|path| {
                self.library_snapshot
                    .tracks
                    .iter()
                    .find(|track| track.path == *path)
            })
            .and_then(|track| track.cover.clone());
        cover.detail = if cover.cover.is_none() {
            "No cover · search for one"
        } else if cover.cover.as_ref().is_some_and(|path| {
            path.components()
                .any(|part| part.as_os_str() == "chosen-covers")
        }) {
            "Chosen by you · search for another one"
        } else {
            "Search for another one"
        }
        .into();
        if cover.cover.as_ref().is_some_and(|path| {
            path.components()
                .any(|part| part.as_os_str() == "chosen-covers")
        }) {
            cover.options = vec![Item::new(
                "Use the Album's Own Cover",
                Choice::ResetEditedCover,
            )];
        }
        rows.push(cover);
        let mut merge = Item::new("Add to Another Album", Choice::MergeEdited);
        merge.detail = "Merge it with an album seen as another one".into();
        rows.push(merge);
        let merged = self
            .edit_paths
            .iter()
            .filter(|path| {
                self.library_snapshot
                    .tracks
                    .iter()
                    .any(|track| track.path == **path && track.added_album)
            })
            .count();
        if merged > 0 {
            let mut row = Item::new("Separate Added Songs", Choice::SeparateEdited);
            row.detail = format!("{merged} songs added from other albums");
            rows.push(row);
        }
        let changes = edit.changes();
        if !changes.is_empty() {
            let writable = changes
                .iter()
                .filter(|(path, _)| can_write_tags_ui(path))
                .count();
            let mut save = Item::new("Save Changes", Choice::SaveTags);
            save.detail = if writable == 0 {
                "Only MP3, FLAC, Ogg and M4A tags can be saved".into()
            } else if writable < changes.len() {
                format!(
                    "{} · {} songs can't be saved (WAV, AIFF…)",
                    edit.summary(),
                    changes.len() - writable
                )
            } else {
                edit.summary()
            };
            rows.push(save);
            rows.push(Item::new("Undo Changes", Choice::UndoTags));
        }
        rows.push(Item::new("Delete Album", Choice::DeleteEditedAlbum));
        rows
    }
    pub(super) fn editor_field_row(&self, field: Field) -> Item {
        let Some(edit) = &self.album_edit else {
            return fact(metadata_edit::field_label(field), "");
        };
        let (value, same) = edit.common(field);
        let mut row = Item::new(metadata_edit::field_label(field), Choice::TagField(field));
        row.detail = if !same {
            "Different on each song".into()
        } else if value.is_empty() {
            "—".into()
        } else {
            value
        };
        if edit.changed_field(field) {
            row.visual.value = "changed".into();
        }
        if let Some((keyboard, Typing::Tag(typing))) = &self.keyboard {
            if *typing == field {
                row.detail = keyboard.with_caret();
                row.visual.typing = true;
            }
        }
        row
    }
    pub(super) fn editor_songs_rows(&self) -> Vec<Item> {
        self.album_edit
            .as_ref()
            .map(|edit| {
                edit.songs
                    .iter()
                    .enumerate()
                    .map(|(index, song)| {
                        let title = metadata_edit::field_text(&song.current, Field::Title);
                        let number = song.current.track.unwrap_or(0);
                        let mut row = Item::new(
                            if number > 0 {
                                format!("{number}. {title}")
                            } else {
                                title
                            },
                            Choice::EditSong(index),
                        );
                        row.detail = metadata_edit::field_text(&song.current, Field::Artist);
                        if metadata_edit::has_changes(&metadata_edit::diff(
                            &song.original,
                            &song.current,
                        )) {
                            row.visual.value = "changed".into();
                        }
                        row
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    pub(super) fn editor_song_rows(&self, index: usize) -> Vec<Item> {
        let mut rows = [Field::Title, Field::Artist, Field::Track, Field::Disc]
            .into_iter()
            .map(|field| self.editor_field_row(field))
            .collect::<Vec<_>>();
        if let Some(song) = self
            .album_edit
            .as_ref()
            .and_then(|edit| edit.songs.get(index))
        {
            let mut file = fact("File", "");
            file.detail = song.path.display().to_string();
            rows.push(file);
        }
        rows
    }
    pub(super) fn refresh_editor_page(&mut self) {
        self.invalidate_visual_rows();
        for index in 0..self.pages.len() {
            let role = self.pages[index].role;
            let rows = match role {
                PageRole::AlbumInfo => {
                    let old = self.album_edit.as_ref().and_then(|edit| edit.scope);
                    if let Some(edit) = self.album_edit.as_mut() {
                        edit.scope = None;
                    }
                    let rows = self.album_info_rows();
                    if let Some(edit) = self.album_edit.as_mut() {
                        edit.scope = old;
                    }
                    Some(rows)
                }
                PageRole::SongsInfo => Some(self.editor_songs_rows()),
                PageRole::SongInfo(song) => {
                    let old = self.album_edit.as_ref().and_then(|edit| edit.scope);
                    if let Some(edit) = self.album_edit.as_mut() {
                        edit.scope = Some(song);
                    }
                    let rows = self.editor_song_rows(song);
                    if let Some(edit) = self.album_edit.as_mut() {
                        edit.scope = old;
                    }
                    Some(rows)
                }
                _ => None,
            };
            if let Some(rows) = rows {
                let page = &mut self.pages[index];
                page.rows = rows;
                page.selected = page.selected.min(page.rows.len().saturating_sub(1));
                page.visual_rows = None;
            }
        }
    }
    pub(super) fn open_info_search(&mut self) {
        if let Some(edit) = self.album_edit.as_mut() {
            edit.scope = None;
        }
        let album = self
            .album_edit
            .as_ref()
            .map(|edit| edit.common(Field::Album).0)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.edit_title.clone());
        let artist = self
            .album_edit
            .as_ref()
            .map(|edit| {
                let (value, same) = edit.common(Field::AlbumArtist);
                if same && !value.is_empty() {
                    value
                } else {
                    edit.songs
                        .first()
                        .map(|song| metadata_edit::field_text(&song.current, Field::Artist))
                        .unwrap_or_default()
                }
            })
            .unwrap_or_default();
        let query = format!("{album} {artist}").trim().to_string();
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
        });
        self.push("Find Info Online", Vec::new());
        self.pages.last_mut().unwrap().role = PageRole::Combined;
        self.run_combined(None);
    }
}
pub(super) fn can_write_tags_ui(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp3" | "flac" | "ogg" | "oga" | "m4a"
            )
        })
}
pub(super) fn media_options(paths: Vec<PathBuf>) -> Vec<Item> {
    let mut next = Item::new("Play Next", Choice::MediaQueue(paths.clone(), true));
    next.visual.icon = "queue_next".into();
    let mut last = Item::new(
        "Add to End of Queue",
        Choice::MediaQueue(paths.clone(), false),
    );
    last.visual.icon = "queue_last".into();
    vec![
        next,
        last,
        Item::new("Add to Playlist…", Choice::MediaAddPlaylist(paths.clone())),
        Item::new("Share", Choice::MediaShare(paths)),
    ]
}
impl Extras {
    pub(in crate::app) fn set_library_snapshot(
        &mut self,
        library: Arc<betterpod_rust::library::Library>,
    ) {
        self.library_snapshot = library;
        self.refresh_editor_page();
        self.invalidate_visual_rows();
    }
}
impl Extras {
    pub(in crate::app) fn navigation_depth(&self) -> usize {
        self.pages.len().saturating_sub(1)
    }
    pub(in crate::app) fn send_paths_share(&mut self, paths: Vec<PathBuf>) {
        let track = paths.first().and_then(|path| {
            self.library_snapshot
                .tracks
                .iter()
                .find(|track| track.path == *path)
        });
        let info = ShareInfo {
            v: 1,
            from: self.sharing_name.clone(),
            album: track
                .map(|track| track.album.clone())
                .unwrap_or_else(|| "Shared Music".into()),
            artist: track.map(|track| track.artist.clone()).unwrap_or_default(),
            title: if paths.len() == 1 {
                track.map(|track| track.title.clone()).unwrap_or_default()
            } else {
                String::new()
            },
            songs: paths.len(),
            bytes: 0,
        };
        let cover = track.and_then(|track| track.cover.clone());
        self.send_share(paths, info, cover);
    }
}
impl Extras {
    pub(in crate::app) fn retain_current_visual(&mut self) {
        if let Some(view) = &self.share_view {
            self.retired_custom = Some(RetiredCustom::Share {
                title: self.title().to_string(),
                view: view.clone(),
            });
        } else if let Some(index) = self
            .forecast
            .filter(|_| self.pages.len() == self.forecast_depth)
        {
            if let Some(place) = self.weather_state.places.get(index) {
                self.retired_custom = Some(RetiredCustom::Weather {
                    title: self.title().to_string(),
                    place: place.clone(),
                    day: self.forecast_day,
                    fahrenheit: self.weather_state.fahrenheit,
                    loading: self.forecast_loading,
                    error: self.forecast_error.clone(),
                    view: self.weather_visual.clone(),
                });
            }
        }
    }
    pub(in crate::app) fn draw_previous(
        &mut self,
        page: &super::super::Page,
        r: &mut Renderer,
        _dark: bool,
        now: f64,
        art: &mut crate::art::Artwork,
        assets: &VisualAssets,
    ) -> bool {
        match self.retired_custom.as_mut() {
            Some(RetiredCustom::Share { title, view }) if title == &page.title => {
                view.draw(r, assets, art, now);
                true
            }
            Some(RetiredCustom::Weather {
                title,
                place,
                day,
                fahrenheit,
                loading,
                error,
                view,
            }) if title == &page.title => {
                view.draw(r, place, *day, *fahrenheit, *loading, error, now);
                true
            }
            _ => false,
        }
    }
    pub(in crate::app) fn release_previous_visual(&mut self) {
        self.retired_custom = None;
    }
}
impl Extras {
    pub(super) fn open_share_name_gate(&mut self, pending: PendingShare) {
        self.generation = self.generation.wrapping_add(1);
        self.pending_share = Some(pending);
        self.share_name_draft.clear();
        self.share_view = None;
        self.pages.clear();
        self.push("Name This betterPod", Vec::new());
        self.keyboard = Some((Keyboard::new(String::new()), Typing::SharingName));
        self.refresh_share_name_gate();
    }
    pub(super) fn refresh_share_name_gate(&mut self) {
        if self.pending_share.is_none()
            || !self
                .pages
                .last()
                .is_some_and(|page| page.title == "Name This betterPod")
        {
            return;
        }
        let mut name = Item::new(
            if self.share_name_draft.is_empty() {
                "Type a name"
            } else {
                &self.share_name_draft
            },
            Choice::EditSharingName,
        );
        name.detail = "The name your friends see when you share".into();
        name.visual.no_arrow = true;
        name.visual.typing = matches!(self.keyboard.as_ref(), Some((_, Typing::SharingName)));
        let mut rows = vec![name];
        if !self.share_name_draft.trim().is_empty() {
            let mut proceed = Item::new("Continue", Choice::FinishShareName);
            proceed.detail = "Or [START] on the keyboard".into();
            rows.push(proceed);
        }
        let page = self.pages.last_mut().unwrap();
        page.rows = rows;
        page.visual_rows = None;
        page.selected = page.selected.min(page.rows.len().saturating_sub(1));
    }
}

impl Extras {
    pub(super) fn refresh_archive_status(&mut self) {
        let Some(item) = self
            .detail_item
            .as_ref()
            .filter(|item| item.provider == Provider::Archive)
            .cloned()
        else {
            return;
        };
        if self.jobs.values().any(|(_, purpose)| matches!(purpose, Purpose::ArchiveStatus { item: id, generation } if *id == item.id && *generation == self.generation)) { return; }
        self.task(
            Task::ArchiveStatus { item: item.clone() },
            Purpose::ArchiveStatus {
                generation: self.generation,
                item: item.id,
            },
        );
    }
}

#[cfg(test)]
mod youtube_search_tests {
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
                "betterpod-youtube-ui-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(root.join("music")).unwrap();
            std::fs::create_dir_all(root.join("data")).unwrap();
            Self {
                extras: Extras::new(root.join("music"), root.join("data")).unwrap(),
                root,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.extras.shutdown();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn select(extras: &mut Extras, label: &str) {
        let page = extras.pages.last_mut().unwrap();
        page.selected = page
            .rows
            .iter()
            .position(|row| row.title == label)
            .unwrap_or_else(|| panic!("Missing {label}"));
        extras.handle(Action::Select);
    }
    fn finish_search_jobs(extras: &mut Extras) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while extras
            .jobs
            .values()
            .any(|(_, purpose)| matches!(purpose, Purpose::Search { .. }))
            && std::time::Instant::now() < deadline
        {
            extras.tick(0.);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    #[test]
    fn youtube_search_gear_has_go_focus_and_mode_settings_keep_the_query() {
        let mut fixture = Fixture::new("modes");
        let extras = &mut fixture.extras;
        extras.youtube = true;
        extras.open(ExtraPage::SearchSources);
        select(extras, "YouTube Videos");
        extras.handle(Action::BackHeld); // Close the initial keyboard; short B deletes text.
        extras.seed_youtube_query(Provider::YouTube, "Artist Café & Album");
        let owner = extras.pages.last().unwrap().key;
        extras.update_search_page(owner, Provider::YouTube, false, false);
        let page = extras.visual_page().unwrap();
        assert!(page.rows[0].visual.search_gear);
        assert!(!page.rows[0].visual.search_gear_focused);
        assert!(page.rows[1].visual.header && page.rows[1].label == "Showing: Videos");
        assert!(
            matches!(extras.pages.last().unwrap().rows[1].choice, Choice::Nothing),
            "Showing is the Go nonselectable header, never a settings row"
        );
        extras.handle(Action::Right);
        assert!(
            extras.visual_page().unwrap().rows[0]
                .visual
                .search_gear_focused
        );
        extras.handle(Action::Left);
        assert!(
            !extras.visual_page().unwrap().rows[0]
                .visual
                .search_gear_focused
        );
        extras.handle(Action::Select);
        assert!(extras.is_typing(), "A on the field edits the query");
        extras.handle(Action::BackHeld);
        extras.handle(Action::Right);
        extras.handle(Action::Down);
        assert!(
            !extras.pages.last().unwrap().gear_on,
            "Vertical navigation leaves gear focus"
        );
        extras.handle(Action::Up);
        extras.handle(Action::Right);
        let depth = extras.pages.len();
        extras.handle(Action::Select);
        assert_eq!(extras.title(), "Search Settings");
        assert_eq!(extras.pages.len(), depth + 1);
        assert!(extras.pages.last().unwrap().role == PageRole::YouTubeSearchSettings(owner));
        assert_eq!(extras.pages.last().unwrap().rows.len(), 2);
        assert!(extras.pages.last().unwrap().rows[0].visual.checked);
        let old_generation = extras.generation;
        let old = extras
            .services
            .submit(Task::Search {
                provider: Provider::YouTube,
                query: "Old mode".into(),
                page: 1,
            })
            .unwrap();
        extras.job_pages.insert(old.id, owner);
        extras.jobs.insert(
            old.id,
            (
                old.clone(),
                Purpose::Search {
                    provider: Provider::YouTube,
                    query: "Old mode".into(),
                    page: 1,
                    metadata: false,
                    generation: old_generation,
                },
            ),
        );
        select(extras, "Playlists & Albums");
        assert!(old.is_cancelled());
        assert_eq!(
            extras.title(),
            "Search Settings",
            "A mode pick stays in Go's full settings page"
        );
        assert!(extras.pages.last().unwrap().rows[1].visual.checked);
        let (generation, target) = extras
            .jobs
            .iter()
            .find_map(|(id, (_, purpose))| match purpose {
                Purpose::Search {
                    provider: Provider::YouTubePlaylists,
                    query,
                    generation,
                    metadata: false,
                    ..
                } if query == "Artist Café & Album" => {
                    Some((*generation, extras.job_pages.get(id).copied()))
                }
                _ => None,
            })
            .expect("The choice must submit playlist search with the retained query");
        assert_eq!(target, Some(owner));
        assert!(extras.owns_search_result(Provider::YouTubePlaylists, false, generation, target));
        assert!(!extras.owns_search_result(Provider::YouTube, false, old_generation, Some(owner)));
        assert!(!extras.owns_search_result(Provider::YouTube, false, generation, target));
        extras.update_search_page(owner, Provider::YouTubePlaylists, false, false);
        assert_eq!(
            extras.title(),
            "Search Settings",
            "Parent search refresh cannot replace its settings page"
        );
        finish_search_jobs(extras);
        extras.handle(Action::Back);
        assert!(
            extras.pages.last().unwrap().role
                == PageRole::Search(Provider::YouTubePlaylists, false)
        );
        assert_eq!(
            extras.title(),
            "YouTube",
            "Go keeps the YouTube title in playlist search mode"
        );
        assert_eq!(
            extras.pages.last().unwrap().rows[0].title,
            "Artist Café & Album"
        );
        assert!(extras.pages.last().unwrap().rows[1].visual.header);
        extras.handle(Action::Right);
        extras.handle(Action::Select);
        select(extras, "Videos");
        assert!(extras.jobs.values().any(|(_, purpose)| matches!(purpose, Purpose::Search { provider: Provider::YouTube, query, metadata: false, .. } if query == "Artist Café & Album")));
        finish_search_jobs(extras);
        extras.handle(Action::Back);
        extras.handle(Action::Back);
        assert_eq!(extras.title(), "Search Online");
        select(extras, "YouTube Albums");
        assert_eq!(
            extras.keyboard.as_ref().unwrap().0.query,
            "Artist Café & Album"
        );
    }
    #[test]
    fn youtube_and_weather_have_field_gears_but_watch_and_metadata_do_not() {
        let mut fixture = Fixture::new("routes");
        let extras = &mut fixture.extras;
        // This is also the reference Music → Search → YouTube destination.
        extras.open(ExtraPage::Online(Provider::YouTube));
        assert!(matches!(
            extras.pages.last().unwrap().rows[0].gear,
            Some(Choice::YouTubeSearchSettings(Provider::YouTube))
        ));
        assert!(extras.search_rows(Provider::YouTube, true)[0]
            .gear
            .is_none());
        extras.open(ExtraPage::Videos);
        extras.tools_state.ready = true;
        extras.video_action(VideoAction::Search);
        assert!(extras.pages.last().unwrap().role == PageRole::Search(Provider::YouTube, false));
        assert!(
            extras.pages.last().unwrap().rows[0].gear.is_none(),
            "Go's watching search intentionally stays video-only"
        );
        extras.keyboard = None;
        extras.push("Weather", extras.weather_rows());
        extras.pages.last_mut().unwrap().role = PageRole::WeatherHome;
        assert!(extras.visual_page().unwrap().rows[0].visual.search_gear);
        extras.handle(Action::Right);
        extras.handle(Action::Select);
        assert_eq!(extras.title(), "Weather Settings");
    }
    #[test]
    fn youtube_playlist_actions_download_an_album_and_play_a_child_queue() {
        let mut fixture = Fixture::new("playlist-actions");
        let extras = &mut fixture.extras;
        let item = betterpod_rust::online::youtube_item(
            serde_json::json!({
                "id":"PLfixture", "title":"Album", "_type":"playlist", "webpage_url":"https://www.youtube.com/playlist?list=PLfixture",
                "entries":[
                    {"id":"abcdefghijk", "title":"First", "url":"https://www.youtube.com/watch?v=abcdefghijk", "ie_key":"Youtube"},
                    null,
                    {"id":"private1234", "title":"[Private video]", "availability":"private"},
                    {"id":"PLnested", "title":"Nested", "ie_key":"YoutubePlaylist"},
                    {"id":"lmnopqrstuv", "title":"Second", "url":"https://www.youtube.com/watch?v=lmnopqrstuv", "ie_key":"Youtube"}
                ]
            }),
            Provider::YouTubePlaylists,
        );
        let rows = extras.details_rows(&item, false);
        assert!(rows.iter().any(|row| matches!(&row.choice, Choice::Download(DownloadRequest::YouTube { url, video:false, split_chapters:false }) if *url == item.url) && row.title == "Download as Album · 2 songs"));
        assert!(!rows
            .iter()
            .any(|row| matches!(&row.choice, Choice::Media(url, _, _) if *url == item.url)));
        assert!(!rows.iter().any(|row| matches!(
            row.choice,
            Choice::Download(DownloadRequest::YouTube { video: true, .. })
        )));
        assert_eq!(
            rows.iter()
                .filter(
                    |row| matches!(row.choice, Choice::Preview(_, _, _)) && row.title != "Listen"
                )
                .count(),
            2
        );
        let watch = rows
            .iter()
            .find(|row| row.title == "Watch the Videos")
            .unwrap()
            .choice
            .clone();
        extras.choose(watch);
        let Effect::VideoOpening {
            item: first,
            queue,
            index,
            ..
        } = extras
            .effects
            .iter()
            .find(|effect| matches!(effect, Effect::VideoOpening { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(first.id, "abcdefghijk");
        assert_eq!(
            queue
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["abcdefghijk", "lmnopqrstuv"]
        );
        assert_eq!(*index, 0);
        assert!(queue
            .iter()
            .all(|entry| entry.raw["_video_auto_next"] == true));
        assert!(extras.jobs.values().any(|(_, purpose)| matches!(purpose, Purpose::Stream { origin, queue, video:true, index:0, .. } if origin == "https://www.youtube.com/watch?v=abcdefghijk" && queue.len() == 2)));
        let listen = rows
            .iter()
            .find(|row| row.title == "Listen")
            .unwrap()
            .choice
            .clone();
        extras.choose(listen);
        assert_eq!(
            extras.preview_origin.as_deref(),
            Some("https://www.youtube.com/watch?v=abcdefghijk")
        );
        assert_eq!(extras.preview_queue.len(), 2);
        assert!(extras.effects.iter().any(|effect| matches!(effect, Effect::PreviewOpening { index:0, queue, .. } if queue.len() == 2)));
    }

    #[test]
    fn youtube_video_details_follow_go_order_without_a_crop_or_video_download_menu_row() {
        let mut fixture = Fixture::new("youtube-go-details");
        let extras = &mut fixture.extras;
        let item = betterpod_rust::online::youtube_item(
            serde_json::json!({
                "id":"abcdefghijk", "title":"A full album", "artist":"An artist", "duration":180,
                "thumbnail":"https://example.invalid/cover.jpg", "chapters":[
                    {"title":"First song","start_time":0,"end_time":60},
                    {"title":"Second song","start_time":60,"end_time":180}
                ]
            }),
            Provider::YouTube,
        );
        let rows = extras.details_rows(&item, false);
        assert_eq!(
            rows.iter()
                .map(|row| row.title.as_str())
                .collect::<Vec<_>>(),
            vec![
                "A full album",
                "Listen",
                "Watch Video",
                "Cover",
                "Download as Album · 2 tracks",
                "Download as One Song",
                "First song",
                "Second song"
            ]
        );
        assert!(rows[0].visual.thumbnail && rows[0].visual.no_arrow);
        assert!(matches!(rows[1].choice, Choice::Preview(_, _, 0)));
        assert!(matches!(rows[3].choice, Choice::Nothing));
        assert!(rows[3].visual.no_arrow);
        assert!(matches!(
            rows[4].choice,
            Choice::Download(DownloadRequest::YouTube {
                split_chapters: true,
                video: false,
                ..
            })
        ));
        assert_eq!(rows[6].visual.value, "1:00");
        assert_eq!(rows[7].visual.value, "2:00");
    }
}
