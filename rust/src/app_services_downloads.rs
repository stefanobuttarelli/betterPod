//! Downloads outlive the page that started them. Their state never doubles as
//! the foreground browser's loading state.
use super::*;

#[derive(Clone)]
pub(super) struct DownloadState {
    pub(super) key: String,
    pub(super) title: String,
    format: String,
    archive: Option<String>,
    pub(super) done: u64,
    pub(super) total: Option<u64>,
    pub(super) status: String,
    pub(super) active: bool,
    completed: Option<usize>,
    error: Option<String>,
}
impl DownloadState {
    pub(super) fn from_task(task: &Task) -> Option<Self> {
        let request = match task {
            Task::Download(request) | Task::DownloadWithCover { request, .. } => request,
            _ => return None,
        };
        let (key, title, format, archive, total) = match request {
            DownloadRequest::Archive { item, format } => {
                let files = item.raw["groups"][format].as_array();
                let names = files
                    .map(|files| {
                        files
                            .iter()
                            .map(|f| f["name"].as_str().unwrap_or(""))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                let total = files.and_then(|files| {
                    files.iter().try_fold(0u64, |sum, file| {
                        let bytes = betterpod_rust::online::number(file, "size").max(0.0) as u64;
                        (bytes > 0).then(|| sum.saturating_add(bytes))
                    })
                });
                (
                    format!("archive:{}:{format}:{names}", item.id),
                    item.title.clone(),
                    format.clone(),
                    Some(item.id.clone()),
                    total,
                )
            }
            DownloadRequest::YouTube { url, video, .. } => (
                format!("youtube:{url}:{video}"),
                "YouTube download".into(),
                if *video { "Video" } else { "Audio" }.into(),
                None,
                None,
            ),
            DownloadRequest::Episodes { podcast, episodes } => (
                format!(
                    "podcast:{}:{}",
                    podcast.feed,
                    episodes
                        .iter()
                        .map(|e| e.url.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                podcast.name.clone(),
                format!(
                    "{} {}",
                    episodes.len(),
                    if episodes.len() == 1 {
                        "episode"
                    } else {
                        "episodes"
                    }
                ),
                None,
                None,
            ),
            DownloadRequest::File {
                url,
                name,
                category,
                ..
            } => (
                format!("file:{url}"),
                name.clone(),
                category.clone(),
                None,
                None,
            ),
        };
        Some(Self {
            key,
            title,
            format,
            archive,
            done: 0,
            total,
            status: "Queued".into(),
            active: true,
            completed: None,
            error: None,
        })
    }
    pub(super) fn request_cancel(&mut self) {
        if self.status == "Queued" {
            self.active = false;
            self.error = Some("Cancelled".into());
        } else {
            self.status = "Cancelling…".into();
        }
    }
    fn detail(&self) -> String {
        if let Some(error) = &self.error {
            return if error == "Cancelled" {
                "Cancelled".into()
            } else {
                format!("Failed · {error}")
            };
        }
        if let Some(files) = self.completed {
            return format!(
                "Complete · {files} {} saved",
                if files == 1 { "file" } else { "files" }
            );
        }
        let bytes = self.total.filter(|n| *n > 0).map_or_else(
            || {
                if self.done > 0 {
                    format!("{} received", visual::bytes_label(self.done))
                } else {
                    String::new()
                }
            },
            |total| {
                format!(
                    "{} / {} · {}%",
                    visual::bytes_label(self.done),
                    visual::bytes_label(total),
                    self.done.saturating_mul(100).saturating_div(total).min(100)
                )
            },
        );
        if bytes.is_empty() {
            self.status.clone()
        } else {
            format!("{bytes} · {}", self.status)
        }
    }
    fn row(&self, id: u64) -> Item {
        let mut row = Item::new(
            format!("{} · {}", self.format, self.title),
            Choice::ShowDownload(id),
        );
        row.detail = self.detail();
        row.visual.value = self.format.clone();
        row.visual.no_arrow = true;
        row.visual.progress_visible = self.active;
        row.visual.progress = self.total.filter(|n| *n > 0).map_or(0.0, |total| {
            (self.done as f64 / total as f64).clamp(0., 1.) as f32
        });
        if self.active {
            row.options = vec![Item::new("Cancel Download", Choice::CancelDownload(id))];
        }
        row
    }
}
fn update_archive_row(row: &mut Item, id: u64, state: &DownloadState, format: &str) {
    row.title = if state.active {
        format!("Downloading {format}…")
    } else if state.error.is_some() {
        format!("Retry {format} download")
    } else {
        format!("{format} downloaded")
    };
    row.detail = state.detail();
    row.visual.progress_visible = state.active;
    row.visual.progress = state
        .total
        .filter(|n| *n > 0)
        .map_or(0., |n| (state.done as f64 / n as f64).clamp(0., 1.) as f32);
    row.options = if state.active {
        vec![
            Item::new("View Download", Choice::ShowDownload(id)),
            Item::new("Cancel Download", Choice::CancelDownload(id)),
        ]
    } else {
        Vec::new()
    };
}

impl Extras {
    pub(super) fn apply_service_progress(
        &mut self,
        id: u64,
        done: u64,
        total: Option<u64>,
        status: String,
    ) {
        if let Some(state) = self.download_states.get_mut(&id) {
            if !state.active {
                return;
            }
            state.done = done;
            if total.is_some() {
                state.total = total;
            }
            state.status = status;
            self.refresh_download_pages();
            return;
        }
        if self
            .jobs
            .get(&id)
            .is_some_and(|(handle, _)| handle.is_cancelled())
        {
            return;
        }
        let page = self.pages.last().map(|page| page.key);
        if self.job_pages.get(&id).copied() != page || page.is_none() {
            return;
        }
        self.status_job = Some(id);
        self.status_page = page;
        self.status = total.filter(|n| *n > 0).map_or(status.clone(), |total| {
            format!(
                "{status} - {}%",
                done.saturating_mul(100).saturating_div(total).min(100)
            )
        });
    }
    pub(super) fn cancel_browse_jobs(&mut self) {
        for (handle, purpose) in self.jobs.values() {
            if matches!(
                purpose,
                Purpose::Search { .. }
                    | Purpose::Details { .. }
                    | Purpose::Feed { .. }
                    | Purpose::Combined { .. }
                    | Purpose::CoverContext { .. }
                    | Purpose::ArchiveStatus { .. }
                    | Purpose::History
                    | Purpose::Downloads
            ) {
                handle.cancel();
                self.job_pages.remove(&handle.id);
            }
        }
        let searches: Vec<_> = self
            .searches
            .iter_mut()
            .filter_map(|state| {
                let was_busy = state.busy;
                state.busy = false;
                was_busy.then_some((state.provider, state.metadata))
            })
            .collect();
        for (provider, metadata) in searches {
            let rows = self.search_rows(provider, metadata);
            for page in &mut self.pages {
                if page.role == PageRole::Search(provider, metadata) {
                    page.rows = rows.clone();
                    page.selected = page.selected.min(page.rows.len().saturating_sub(1));
                    page.visual_rows = None;
                }
            }
        }
    }
    pub(in crate::app) fn leave_browse_context(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending_navigation = None;
        self.status.clear();
        self.status_job = None;
        self.status_page = None;
        self.keyboard = None;
        self.closing_keyboard = None;
        self.cancel_browse_jobs();
    }
    pub(in crate::app) fn active_download_count(&self) -> usize {
        self.download_states
            .values()
            .filter(|state| state.active)
            .count()
    }
    pub(super) fn download_rows(&self) -> Vec<Item> {
        let mut rows: Vec<_> = self
            .download_states
            .iter()
            .rev()
            .filter(|(_, s)| s.active)
            .map(|(&id, state)| state.row(id))
            .collect();
        let finished: Vec<_> = self
            .download_states
            .iter()
            .rev()
            .filter(|(_, s)| !s.active)
            .take(8)
            .map(|(&id, state)| state.row(id))
            .collect();
        if !finished.is_empty() {
            rows.push(visual::header("Recent downloads"));
            rows.extend(finished);
        }
        if rows.is_empty() {
            rows.push(visual::fact("No active downloads", ""));
        }
        rows
    }
    pub(super) fn show_downloads(&mut self, selected: Option<u64>) {
        let rows = self.download_rows();
        if self
            .pages
            .last()
            .is_some_and(|page| page.role == PageRole::ActiveDownloads)
        {
            self.replace("Downloads", rows);
        } else {
            self.push("Downloads", rows);
        }
        let page = self.pages.last_mut().unwrap();
        page.role = PageRole::ActiveDownloads;
        if let Some(id) = selected {
            if let Some(index) = page
                .rows
                .iter()
                .position(|row| matches!(row.choice, Choice::ShowDownload(other) if other == id))
            {
                page.selected = index;
            }
        }
    }
    pub(super) fn refresh_download_pages(&mut self) {
        // Finished records are bounded. Active transfers are bounded by the
        // dedicated service queue and must not be evicted.
        let completed: Vec<_> = self
            .download_states
            .iter()
            .rev()
            .filter(|(_, s)| !s.active)
            .skip(8)
            .map(|(&id, _)| id)
            .collect();
        for id in completed {
            self.download_states.remove(&id);
        }
        let download_rows = self.download_rows();
        let mut updates = std::collections::BTreeMap::new();
        for (&id, state) in &self.download_states {
            if let Some(item) = &state.archive {
                let key = (item.clone(), state.format.clone());
                if state.active
                    || !updates
                        .get(&key)
                        .is_some_and(|(_, old): &(u64, &DownloadState)| old.active)
                {
                    updates.insert(key, (id, state));
                }
            }
        }
        for page in &mut self.pages {
            if page.role == PageRole::ActiveDownloads {
                page.rows = download_rows.clone();
                page.visual_rows = None;
                page.selected = page.selected.min(page.rows.len().saturating_sub(1));
            } else if page.role == PageRole::ArchiveItem {
                for row in &mut page.rows {
                    let Choice::Download(DownloadRequest::Archive { item, format }) = &row.choice
                    else {
                        continue;
                    };
                    if page.resource.as_deref() != Some(item.id.as_str()) {
                        continue;
                    }
                    let Some(&(id, state)) = updates.get(&(item.id.clone(), format.clone())) else {
                        continue;
                    };
                    let format = format.clone();
                    update_archive_row(row, id, state, &format);
                    page.visual_rows = None;
                }
            }
        }
    }
    pub(super) fn archive_download(
        &self,
        item: &str,
        format: &str,
    ) -> Option<(u64, &DownloadState)> {
        self.download_states
            .iter()
            .rev()
            .find(|(_, state)| {
                state.archive.as_deref() == Some(item) && state.format == format && state.active
            })
            .or_else(|| {
                self.download_states.iter().rev().find(|(_, state)| {
                    state.archive.as_deref() == Some(item) && state.format == format
                })
            })
            .map(|(&id, state)| (id, state))
    }
    pub(super) fn archive_download_row(&self, item: &str, format: &str, mut row: Item) -> Item {
        if let Some((id, state)) = self.archive_download(item, format) {
            update_archive_row(&mut row, id, state, format);
        }
        row
    }
    pub(super) fn download_finished(&mut self, id: u64, result: &Result<Output, String>) {
        if let Some(state) = self.download_states.get_mut(&id) {
            let notify = state.active || result.is_ok();
            state.active = false;
            match result {
                Ok(Output::Files(files)) => {
                    state.error = None;
                    state.completed = Some(files.len());
                    state.status = "Complete".into();
                    state.done = state.total.unwrap_or(state.done);
                }
                Err(error) => state.error = Some(error.clone()),
                _ => state.error = Some("Download returned no files".into()),
            }
            if notify {
                let notice = match result {
                    Ok(Output::Files(files)) => {
                        format!("Download complete · {} files saved", files.len())
                    }
                    Err(error) if error == "Cancelled" => "Download cancelled".into(),
                    Err(error) => format!("Download failed: {error}"),
                    _ => "Download failed".into(),
                };
                self.effects.push(Effect::Notice(notice));
            }
        }
        self.refresh_download_pages();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        extras: Extras,
    }
    impl Fixture {
        fn new(label: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "betterpod-service-ui-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(root.join("music")).unwrap();
            let extras = Extras::new(root.join("music"), root.join("data")).unwrap();
            Self { root, extras }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.extras.shutdown();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn archive_feedback_is_immediate_and_does_not_follow_the_user_to_youtube() {
        let mut fixture = Fixture::new("archive-feedback");
        let extras = &mut fixture.extras;
        let mut album = OnlineItem::new(Provider::Archive);
        album.id = "fixture-album".into();
        album.title = "Fixture Album".into();
        album.raw = serde_json::json!({"groups":{"FLAC":[{"name":"song.flac","size":1000}]}});
        extras.detail_item = Some(album.clone());
        extras.push("Internet Archive", extras.archive_rows(&album));
        let page = extras.pages.last_mut().unwrap();
        page.role = PageRole::ArchiveItem;
        page.resource = Some(album.id.clone());
        let request = Task::Download(DownloadRequest::Archive {
            item: album.clone(),
            format: "FLAC".into(),
        });
        extras
            .download_states
            .insert(77, DownloadState::from_task(&request).unwrap());
        extras.refresh_download_pages();
        let page = extras.visual_page().unwrap();
        let row = page
            .rows
            .iter()
            .find(|r| r.label.starts_with("Downloading FLAC"))
            .unwrap();
        assert!(row.detail.contains("Queued"));
        assert!(row.visual.progress_visible);
        assert_eq!(row.visual.progress, 0.);
        // An Archive receipt refresh used to silently turn this into a generic
        // page, preventing every subsequent progress update.
        extras.replace("Internet Archive", extras.archive_rows(&album));
        extras.apply_service_progress(77, 500, Some(1000), "Track 1 of 1".into());
        assert!(extras
            .visual_page()
            .unwrap()
            .rows
            .iter()
            .any(|r| r.label.starts_with("Downloading FLAC") && r.detail.contains("50%")));
        extras.open(ExtraPage::Online(Provider::YouTube));
        extras.apply_service_progress(77, 750, Some(1000), "Downloading song.flac".into());
        let page = extras.visual_page().unwrap();
        assert_eq!(page.title, "YouTube");
        assert_eq!(page.rows.len(), 1);
        assert!(page.rows[0].visual.search_field);
        assert!(extras.status.is_empty());
        assert_eq!(extras.active_download_count(), 1);
        extras.leave_browse_context();
        assert_eq!(extras.active_download_count(), 1);
        extras.download_finished(77, &Ok(Output::Files(vec![PathBuf::from("saved.flac")])));
        assert_eq!(extras.active_download_count(), 0);
        assert_eq!(extras.title(), "YouTube");
        extras.open(ExtraPage::ActiveDownloads);
        assert!(extras
            .visual_page()
            .unwrap()
            .rows
            .iter()
            .any(|r| r.detail.contains("Complete · 1 file saved")));
    }
    #[test]
    fn an_old_foreground_progress_event_cannot_write_into_a_new_page() {
        let mut fixture = Fixture::new("page-owner");
        let extras = &mut fixture.extras;
        extras.push("Internet Archive", vec![visual::fact("Album", "")]);
        let key = extras.pages.last().unwrap().key;
        extras.job_pages.insert(91, key);
        extras.apply_service_progress(91, 5, Some(10), "Opening album".into());
        assert!(extras
            .visual_page()
            .unwrap()
            .rows
            .iter()
            .any(|r| r.label.contains("Opening album")));
        extras.push(
            "YouTube",
            vec![visual::search_row(Provider::YouTube, "", "")],
        );
        extras.apply_service_progress(91, 9, Some(10), "Opening album".into());
        assert_eq!(extras.visual_page().unwrap().rows.len(), 1);
        extras.searches.push(SearchState {
            provider: Provider::YouTube,
            metadata: false,
            query: "Retained query".into(),
            items: vec![],
            total: 0,
            page: 1,
            error: String::new(),
            busy: true,
            generation: extras.generation,
        });
        extras.replace("YouTube", extras.search_rows(Provider::YouTube, false));
        extras.pages.last_mut().unwrap().role = PageRole::Search(Provider::YouTube, false);
        assert!(extras
            .visual_page()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.label.contains("Searching")));
        extras.leave_browse_context();
        assert!(!extras
            .visual_page()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.label.contains("Searching")));
        assert_eq!(extras.searches[0].query, "Retained query");
    }
    #[test]
    fn delayed_tools_status_does_not_replace_the_music_youtube_search() {
        let mut fixture = Fixture::new("youtube-tools");
        let extras = &mut fixture.extras;
        extras.open(ExtraPage::Online(Provider::YouTube));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while extras
            .jobs
            .values()
            .any(|(_, purpose)| matches!(purpose, Purpose::Tools))
            && std::time::Instant::now() < deadline
        {
            extras.tick(0.);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!extras
            .jobs
            .values()
            .any(|(_, purpose)| matches!(purpose, Purpose::Tools)));
        let page = extras.visual_page().unwrap();
        assert_eq!(page.title, "YouTube");
        assert!(page.rows[0].visual.search_field);
        extras.youtube = true;
        extras.open(ExtraPage::SearchSources);
        let labels: Vec<_> = extras
            .pages
            .last()
            .unwrap()
            .rows
            .iter()
            .map(|r| r.title.as_str())
            .collect();
        assert!(labels.contains(&"YouTube Albums"));
        assert!(labels.contains(&"YouTube Videos"));
    }
    #[test]
    fn unknown_size_and_completion_are_explicit() {
        let request = Task::Download(DownloadRequest::File {
            url: "https://example.invalid/file".into(),
            name: "Album".into(),
            category: "Music".into(),
            tags: None,
        });
        let mut state = DownloadState::from_task(&request).unwrap();
        state.status = "Downloading track".into();
        state.done = 1_048_576;
        assert!(state.detail().contains("received"));
        assert!(!state.detail().contains("Loading"));
        state.total = Some(2_097_152);
        state.status = format!("Downloading {}.flac", "A long filename ".repeat(20));
        let detail = state.detail();
        assert!(detail.find("50%").unwrap() < detail.find("Downloading").unwrap());
        assert!(state.row(1).title.starts_with("Music · "));
        state.completed = Some(12);
        state.active = false;
        assert_eq!(state.detail(), "Complete · 12 files saved");
        state.error = Some("Connection failed".into());
        assert_eq!(state.detail(), "Failed · Connection failed");
    }
    #[test]
    fn archive_identity_distinguishes_format_and_track_selection() {
        let mut item = OnlineItem::new(Provider::Archive);
        item.id = "album".into();
        item.title = "Album".into();
        item.raw = serde_json::json!({"groups":{"FLAC":[{"name":"one.flac","size":100},{"name":"two.flac","size":200}]}});
        let task = |item| {
            Task::Download(DownloadRequest::Archive {
                item,
                format: "FLAC".into(),
            })
        };
        let album = DownloadState::from_task(&task(item.clone())).unwrap();
        assert_eq!(album.total, Some(300));
        item.raw["groups"]["FLAC"] = serde_json::json!([{"name":"one.flac","size":100}]);
        let track = DownloadState::from_task(&task(item)).unwrap();
        assert_ne!(album.key, track.key);
        assert_eq!(track.total, Some(100));
    }
}
