//! Menu contracts from Go 1.1's ij_library.go, ij_columns.go and ij_screensaver.go.
use super::*;
use app_visual_menu::PreviewKind;
fn setting(label: &str, value: impl Into<String>, intent: Intent) -> Row {
    let mut row = Row::new(label, intent);
    row.visual.value = value.into();
    row.visual.preview = PreviewKind::Settings;
    row
}
fn fact(label: &str, value: impl Into<String>) -> Row {
    let mut row = setting(label, value, Intent::Noop);
    row.arrow = false;
    row
}
fn detail(label: &str, subtitle: impl Into<String>, intent: Intent) -> Row {
    let mut row = Row::new(label, intent);
    row.detail = subtitle.into();
    row
}
impl App {
    pub(super) fn database_label(&self) -> String {
        self.maintenance
            .as_ref()
            .map(|m| m.database_label())
            .unwrap_or_else(|| {
                self.data_dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
    }
    pub(super) fn ensure_selection_viewport(&mut self) {
        let height = self
            .pages
            .last()
            .map(|p| self.visual_menu_content_height(p))
            .unwrap_or(440.0);
        let changed = self.pages.last().is_some_and(|p| {
            self.selection_layout
                .as_ref()
                .is_none_or(|(rows, selected, screen, h)| {
                    !Arc::ptr_eq(rows, &p.rows)
                        || *selected != p.selected
                        || *screen != p.screen
                        || *h != height.to_bits()
                })
        });
        if changed {
            self.visual_menu_reveal_current_selection();
            self.selection_layout = self
                .pages
                .last()
                .map(|p| (Arc::clone(&p.rows), p.selected, p.screen, height.to_bits()));
        }
        if self.screen() == Screen::Search
            && matches!(self.keyboard_context, KeyboardContext::Library)
        {
            let label = self
                .keyboard
                .as_ref()
                .filter(|k| !k.is_exiting())
                .map(Keyboard::with_caret)
                .unwrap_or_else(|| self.search_query.clone());
            if let Some(page) = self.pages.last_mut() {
                if let Some(row) = page.rows.first() {
                    if row.label != label {
                        Arc::make_mut(&mut page.rows)[0].label = label;
                    }
                }
            }
        }
    }
    pub(super) fn refresh_playback_menus(&mut self) {
        for i in 0..self.pages.len() {
            if matches!(
                self.pages[i].screen,
                Screen::Home | Screen::Music | Screen::Queue
            ) {
                let screen = self.pages[i].screen;
                let selected = self.pages[i]
                    .rows
                    .get(self.pages[i].selected)
                    .map(|r| r.label.clone());
                let old_index = self.pages[i].selected;
                let old_scroll = self.pages[i].scroll;
                let mut page = self.page(screen);
                page.selected = selected
                    .and_then(|s| page.rows.iter().position(|r| r.label == s))
                    .unwrap_or(old_index)
                    .min(page.rows.len().saturating_sub(1));
                page.scroll = old_scroll;
                page.target_scroll = old_scroll;
                let height = self.visual_menu_content_height(&page);
                page.reveal_selection_in(height);
                self.pages[i] = page;
            }
        }
    }
    pub(super) fn enqueue_paths(&mut self, paths: Vec<PathBuf>, next: bool) {
        if paths.is_empty() {
            return;
        }
        for path in &paths {
            self.queued_marks.insert(
                path.clone(),
                if next { "queue_next" } else { "queue_last" }.into(),
            );
        }
        if self.queue.is_empty() {
            self.queue = Arc::new(paths);
            self.queue_original = None;
            self.queue_index = 0;
            self.play_current(0.0);
        } else {
            if let Some(original) = self.queue_original.as_mut() {
                Arc::make_mut(original).extend(paths.iter().cloned());
            }
            let at = if next {
                self.detached_next
                    .unwrap_or(self.queue_index + 1)
                    .min(self.queue.len())
            } else {
                self.queue.len()
            };
            Arc::make_mut(&mut self.queue).splice(at..at, paths);
        }
        self.save_playback();
        self.notify(if next {
            "Playing Next"
        } else {
            "Added to Queue"
        });
        if self.screen() == Screen::Queue {
            self.refresh_page();
        }
    }
    pub(super) fn reference_confirm(&mut self, title: &str, label: &str, intent: Intent) {
        self.options = Some(Page {
            screen: self.screen(),
            kind: ViewKind::Full,
            mini: false,
            title: title.into(),
            rows: Arc::new(vec![
                Row::new(label, intent),
                Row::new("Cancel", Intent::CloseOptions),
            ]),
            selected: 0,
            scroll: 0.0,
            target_scroll: 0.0,
            enter: 0.0,
        });
    }
    pub(super) fn reference_queue_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        let first = self
            .detached_next
            .unwrap_or_else(|| self.queue_index.saturating_add(1));
        let end = self.queue.len().min(first + 300);
        if self.now_track.is_some() {
            for (i, path) in self.queue.iter().enumerate().take(end).skip(first) {
                let track = self
                    .by_path
                    .get(path)
                    .and_then(|&i| self.library.tracks.get(i));
                let mut row = Row::new(
                    track.map(|t| t.title.clone()).unwrap_or_else(|| {
                        path.file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    }),
                    Intent::QueueSong(i),
                );
                row.detail = track.map(|t| t.artist.clone()).unwrap_or_default();
                row.cover = track.and_then(|t| t.cover.clone());
                row.visual.thumbnail = track.is_some();
                row.arrow = false;
                row.visual.queue_mark = self.queued_marks.get(path).cloned().unwrap_or_default();
                rows.push(row);
            }
        }
        if self.queue.len() > end {
            let mut row = Row::new(format!("and {} more", self.queue.len() - end), Intent::Noop);
            row.arrow = false;
            rows.push(row);
        }
        let left = self.queue.len().saturating_sub(first);
        if left > 1 {
            let mut row = Row::new("Clear Up Next", Intent::AskClearUpcoming);
            row.detail = format!("{left} songs");
            row.arrow = false;
            rows.push(row);
        }
        if rows.is_empty() {
            let mut row = Row::new("Nothing up next", Intent::Noop);
            row.arrow = false;
            rows.push(row);
        }
        if self.now_track.is_some() {
            let mut row = Row::new("Clear Now Playing", Intent::AskClearNowPlaying);
            row.detail = "Stop the music and empty the queue".into();
            row.arrow = false;
            rows.push(row);
        }
        rows
    }
    pub(super) fn remove_queued(&mut self, index: usize) {
        if index < self.detached_next.unwrap_or(self.queue_index + 1) || index >= self.queue.len() {
            return;
        }
        let path = Arc::make_mut(&mut self.queue).remove(index);
        self.queued_marks.remove(&path);
        if let Some(original) = self.queue_original.as_mut() {
            if let Some(i) = original.iter().rposition(|p| p == &path) {
                Arc::make_mut(original).remove(i);
            }
        }
        self.save_playback();
        self.refresh_page();
    }
    pub(super) fn move_queued_next(&mut self, index: usize) {
        let first = self.detached_next.unwrap_or(self.queue_index + 1);
        if index > first && index < self.queue.len() {
            let path = Arc::make_mut(&mut self.queue).remove(index);
            Arc::make_mut(&mut self.queue).insert(first, path);
            self.save_playback();
            self.refresh_page();
        }
        self.notify("Playing Next");
    }
    pub(super) fn reference_home_rows(&self) -> Vec<Row> {
        let mut rows = vec![
            Row::new("Cover Flow", Intent::Open(Screen::CoverFlow)),
            Row::new("Music", Intent::Open(Screen::Music)),
        ];
        if !self.settings.hide_podcasts {
            rows.push(Row::new("Podcasts", Intent::Extra(ExtraPage::Podcasts)));
        }
        if self.settings.youtube {
            let mut videos = Row::new("Videos", Intent::Extra(ExtraPage::Videos));
            videos.visual.preview = PreviewKind::Image {
                asset: "preview_videos".into(),
                title: "Videos".into(),
                subtitle: String::new(),
                size: 192,
            };
            rows.push(videos);
        }
        let downloads = self.extras.active_download_count();
        if self.extras.has_downloads() {
            let mut row = Row::new("Downloads", Intent::Extra(ExtraPage::ActiveDownloads));
            row.visual.value = if downloads > 0 {
                downloads.to_string()
            } else {
                "Recent".into()
            };
            row.visual.preview = PreviewKind::None;
            rows.push(row);
        }
        let mut apps = Row::new("Apps", Intent::Open(Screen::Apps));
        apps.visual.preview = PreviewKind::Apps;
        rows.push(apps);
        let mut settings = Row::new("Settings", Intent::Open(Screen::Settings));
        settings.visual.preview = PreviewKind::Settings;
        rows.push(settings);
        if self.reference_mini_visible() {
            let mut row = Row::new("Now Playing", Intent::Open(Screen::NowPlaying));
            row.visual.preview = PreviewKind::NowPlaying;
            rows.push(row);
        } else if !self.library.tracks.is_empty() {
            rows.push(Row::new("Genius", Intent::Genius));
        }
        if self
            .maintenance
            .as_ref()
            .is_some_and(|m| m.update_available())
        {
            rows.push(Row::new(
                "Update Available",
                Intent::Maintenance(MaintenanceMode::Update),
            ));
        }
        rows
    }
    pub(super) fn reference_music_rows(&self) -> Vec<Row> {
        let mut rows = vec![
            Row::new("Cover Flow", Intent::Open(Screen::CoverFlow)),
            Row::new("Genius", Intent::Genius),
            Row::new("Playlists", Intent::Extra(ExtraPage::Playlists)),
            Row::new("Artists", Intent::Open(Screen::Artists)),
            Row::new("Albums", Intent::Open(Screen::Albums)),
            Row::new("Search", Intent::Open(Screen::SearchHub)),
            Row::new("Receive", Intent::Extra(ExtraPage::Receive)),
            Row::new("Wi-Fi Library", Intent::Open(Screen::WifiLibrary)),
        ];
        if self.reference_mini_visible() {
            for (name, screen) in [
                ("Now playing", Screen::NowPlaying),
                ("Up Next", Screen::Queue),
            ] {
                let mut row = Row::new(name, Intent::Open(screen));
                row.visual.preview = PreviewKind::NowPlaying;
                rows.push(row);
            }
        }
        rows
    }
    pub(super) fn reference_settings_rows(&self) -> Vec<Row> {
        vec![
            setting("About", "", Intent::Open(Screen::About)),
            setting("Software Update", "", Intent::SoftwareUpdate),
            setting("Shuffle", self.shuffle_label(), Intent::Shuffle),
            setting(
                "Repeat",
                match self.settings.repeat.as_str() {
                    "all" => "All",
                    "one" => "One",
                    _ => "Off",
                },
                Intent::Repeat,
            ),
            setting(
                "Theme",
                match self.settings.theme.as_str() {
                    "dark" => "Dark",
                    "auto" => "Automatic",
                    _ => "Light",
                },
                Intent::Theme,
            ),
            setting(
                "Effects",
                if self.settings.effects == "light" {
                    "Light"
                } else {
                    "Full"
                },
                Intent::Effects,
            ),
            setting(
                "Screensaver",
                if self.settings.screensaver_seconds == 0 {
                    "Never".into()
                } else {
                    format!("{} min", self.settings.screensaver_seconds / 60)
                },
                Intent::Open(Screen::Screensaver),
            ),
            setting("Turn it Apple", on_off(self.settings.apple), Intent::Apple),
            setting("Library", "", Intent::Open(Screen::Library)),
            setting("Wi-Fi Library", "", Intent::Open(Screen::WifiLibrary)),
            setting(
                "Database",
                self.database_label(),
                Intent::Maintenance(MaintenanceMode::Database),
            ),
            setting(
                "Podcasts",
                on_off(!self.settings.hide_podcasts),
                Intent::Podcasts,
            ),
            setting(
                "YouTube",
                on_off(self.settings.youtube),
                Intent::YouTubeSettings,
            ),
            setting("Bluetooth", "", Intent::Extra(ExtraPage::Bluetooth)),
            setting(
                "Sharing",
                if self.settings.sharing_name.is_empty() {
                    "Not set"
                } else {
                    &self.settings.sharing_name
                },
                Intent::SharingSettings,
            ),
            setting("Quit betterPod", "", Intent::QuitColumn),
        ]
    }
    pub(super) fn reference_apps_rows(&self) -> Vec<Row> {
        let mut rows = vec![
            Row::new("Brick", Intent::Game(GameKind::Brick)),
            Row::new("Solitaire", Intent::Game(GameKind::Solitaire)),
            Row::new("2048", Intent::Game(GameKind::TwentyFortyEight)),
            Row::new("Five Letters", Intent::Game(GameKind::FiveLetters)),
            Row::new("Guess the Song", Intent::Quiz),
            Row::new("Puzzles", Intent::Open(Screen::Puzzles)),
            Row::new("Weather", Intent::Extra(ExtraPage::Weather)),
        ];
        for (row, (asset, size)) in rows.iter_mut().zip([
            ("preview_brick", 224),
            ("preview_solitaire", 224),
            ("preview_2048", 192),
            ("preview_words", 192),
            ("preview_quiz", 192),
            ("preview_puzzles", 192),
        ]) {
            row.visual.preview = PreviewKind::Image {
                asset: asset.into(),
                title: row.label.clone(),
                subtitle: String::new(),
                size,
            };
        }
        rows.last_mut().unwrap().visual.preview = PreviewKind::Weather;
        rows
    }
    pub(super) fn reference_saver_rows(&self) -> Vec<Row> {
        let mut rows = vec![
            detail("Start Now", "Anywhere: [SELECT] + [Y]", Intent::StartSaver),
            detail(
                "Style",
                if self.settings.screensaver_style == "viz" {
                    "Moves with the music · [◀] [▶] change it"
                } else {
                    "Covers turning over"
                },
                Intent::SaverStyle,
            ),
            detail(
                "Dim the Screen",
                "[L2] / [R2] set it while it shows",
                Intent::SaverDim,
            ),
        ];
        rows[1].visual.value = if self.settings.screensaver_style == "viz" {
            "Visualizer"
        } else {
            "Cover Flip"
        }
        .into();
        rows[2].visual.value = on_off(self.settings.screensaver_dim > 0).into();
        for (minutes, label) in [
            (1, "Start After 1 min"),
            (2, "Start After 2 min"),
            (5, "Start After 5 min"),
            (10, "Start After 10 min"),
            (0, "Never Start by Itself"),
        ] {
            let mut row = Row::new(label, Intent::Setting(SettingValue::Delay(minutes * 60)));
            row.visual.checked = self.settings.screensaver_seconds == minutes * 60;
            rows.push(row);
        }
        for row in &mut rows {
            row.arrow = false;
        }
        rows
    }
    pub(super) fn reference_library_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.scanning {
            rows.push(fact(
                "Scanning…",
                format!("{} songs", self.library.tracks.len()),
            ));
        } else {
            rows.push(Row::new("Rescan now", Intent::Rescan));
            let missing = self
                .library
                .albums
                .iter()
                .filter(|a| a.cover.is_none())
                .count();
            rows.push(detail(
                "Missing Covers",
                if missing == 0 {
                    "Every album has a cover".into()
                } else {
                    format!("{missing} albums without a cover")
                },
                Intent::Curation(CurationMode::MissingCovers),
            ));
            rows.push(Row::new(
                "Missing Info",
                Intent::Curation(CurationMode::MissingInfo),
            ));
            rows.push(detail(
                "Browse Files",
                "See and delete files and folders",
                Intent::Files(FilesMode::Browse),
            ));
            if !self.library.scan_folders.is_empty() {
                rows.push(detail(
                    "Scanned Folders",
                    format!(
                        "{} folders · {} with music",
                        self.library.scan_folders.len(),
                        self.library
                            .scan_folders
                            .iter()
                            .filter(|f| f.tracks > 0)
                            .count()
                    ),
                    Intent::Files(FilesMode::Scanned),
                ));
            }
        }
        let mut folder = detail(
            "Music Folder",
            self.music_dir.display().to_string(),
            Intent::Noop,
        );
        folder.arrow = false;
        rows.push(folder);
        rows.push(detail(
            "Move Music to Another Card",
            "Or copy it there, or use the music already there",
            Intent::Maintenance(MaintenanceMode::Music),
        ));
        for (label, count) in [
            ("Songs", self.library.tracks.len()),
            ("Albums", self.library.albums.len()),
            ("Artists", self.library.artists.len()),
            ("Playlists", self.extras.playlists_snapshot().len()),
        ] {
            rows.push(fact(label, count.to_string()));
        }
        for info in self.files.library_information().iter().filter(|info| {
            !matches!(
                info.title.as_str(),
                "Songs" | "Albums" | "Artists" | "Playlists"
            )
        }) {
            let mut row = Row::new(&info.title, Intent::Noop);
            row.detail = info.subtitle.clone();
            row.visual.value = info.value.clone();
            row.arrow = false;
            rows.push(row);
        }
        rows
    }
    pub(super) fn reference_search_rows(&self) -> Vec<Row> {
        let mut rows = vec![
            Row::new("Library", Intent::Open(Screen::Search)),
            Row::new(
                "Internet Archive",
                Intent::Extra(ExtraPage::Online(betterpod_rust::online::Provider::Archive)),
            ),
        ];
        if self.settings.youtube {
            rows.push(Row::new(
                "YouTube",
                Intent::Extra(ExtraPage::Online(betterpod_rust::online::Provider::YouTube)),
            ));
        }
        rows
    }
    pub(super) fn reference_mini_visible(&self) -> bool {
        self.now_track.is_some()
            && (self.audio_snapshot.playing
                || self.restored
                || self.visual_time - self.paused_at < 600.0)
    }
    pub(super) fn shuffle_label(&self) -> &'static str {
        if !self.settings.shuffle {
            "Off"
        } else if self.settings.shuffle_mode == "albums" {
            "Albums"
        } else {
            "Songs"
        }
    }
}
fn on_off(value: bool) -> &'static str {
    if value {
        "On"
    } else {
        "Off"
    }
}

impl App {
    pub(super) fn reference_about_rows(&self) -> Vec<Row> {
        [
            ("betterPod 1.1", "betterpod.xyz · r/betterPod"),
            ("Knulli", "Anbernic handhelds with the Allwinner H700"),
            ("Interface: iPod.js", "Tanner Villarete · MIT"),
            ("Original audio engine", "Danilo Fragoso · MIT"),
            (
                "Puzzles: Portable Puzzle Collection",
                "Simon Tatham and contributors · MIT",
            ),
            ("2048: the game's rules", "Gabriele Cirulli · MIT"),
            (
                "Five Letters: the words",
                "SCOWL (Kevin Atkinson) · ENABLE, public domain",
            ),
            ("Font: Inter", "Rasmus Andersson · OFL"),
            (
                "An independent fan project",
                "iPod is a trademark of Apple Inc.; betterPod is not affiliated with Apple",
            ),
        ]
        .into_iter()
        .map(|(label, sub)| {
            let mut r = detail(label, sub, Intent::Noop);
            r.arrow = false;
            r
        })
        .collect()
    }
    pub(super) fn open_youtube_settings(&mut self) {
        self.extras.open(ExtraPage::Tools);
        self.extras.refresh_tools();
        self.open(Screen::Extra);
    }
    pub(super) fn reference_youtube_rows(&self) -> Vec<Row> {
        let mut row = detail(
            "YouTube",
            "Watch in Videos, download in Music → Search",
            Intent::YouTube,
        );
        row.visual.value = on_off(self.settings.youtube).into();
        row.arrow = false;
        vec![
            row,
            detail(
                "Get the Tools",
                "yt-dlp and Deno from GitHub: 80 MB to get, 185 MB on the card",
                Intent::Extra(ExtraPage::Tools),
            ),
            detail(
                "Only download what you may",
                "Your own or freely licensed videos",
                Intent::Noop,
            ),
        ]
    }
    pub(super) fn set_shuffle_mode(&mut self, mode: &str) {
        let current = self.queue.get(self.queue_index).cloned();
        if mode == "off" {
            if let Some(original) = self.queue_original.take() {
                let mut a = original.as_ref().clone();
                let mut b = self.queue.as_ref().clone();
                a.sort();
                b.sort();
                if a == b {
                    self.queue = original;
                    self.queue_index = current
                        .and_then(|p| self.queue.iter().position(|q| q == &p))
                        .unwrap_or(0);
                }
            }
        } else {
            if self.queue_original.is_none() {
                self.queue_original = Some(Arc::clone(&self.queue));
            }
            self.settings.shuffle_mode = mode.into();
            self.reference_shuffle_remaining();
        }
        if mode == "off" && self.detached_next.is_some() {
            self.detached_next = Some(0);
        }
        self.settings.shuffle = mode != "off";
        self.settings.shuffle_mode = mode.into();
        self.save_playback();
    }
    pub(super) fn reference_shuffle_remaining(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        // Keep a physical queue so Off can restore it without restarting audio.
        if self.queue_original.as_ref().is_none_or(|original| {
            let mut a = original.as_ref().clone();
            let mut b = self.queue.as_ref().clone();
            a.sort();
            b.sort();
            a != b
        }) {
            self.queue_original = Some(Arc::clone(&self.queue));
        }
        let original = self.queue_original.as_ref().unwrap();
        let current = if self.detached_next.is_some() {
            None
        } else {
            Some(
                self.queue
                    .get(self.queue_index)
                    .and_then(|current| original.iter().position(|p| p == current))
                    .unwrap_or(0),
            )
        };
        let library = &self.library;
        let by_path = &self.by_path;
        let result = super::app_queue_order::order(
            original,
            current,
            self.settings.shuffle_mode == "albums",
            |path| {
                by_path
                    .get(path)
                    .and_then(|&i| library.tracks.get(i))
                    .map(betterpod_rust::library::album_key)
                    .unwrap_or_else(|| path.display().to_string())
            },
            &mut self.rng,
        );
        self.queue = Arc::new(result);
        self.queue_index = 0;
        if self.detached_next.is_some() {
            self.detached_next = Some(0);
        }
    }
}
