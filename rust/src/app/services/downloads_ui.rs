//! Downloads outlive the page that started them. Their state never doubles as
//! the foreground browser's loading state.
use super::*;
mod history;

#[derive(Clone)]
pub(super) struct DownloadState {
    pub(super) key: String,
    pub(super) title: String,
    pub(super) creator: String,
    pub(super) source: String,
    format: String,
    retry: Task,
    shown: f64,
    last_draw: Option<f64>,
    sampled: Option<(f64, u64)>,
    speed: f64,
    archive: Option<String>,
    rescan_partial: bool,
    pub(super) done: u64,
    pub(super) total: Option<u64>,
    pub(super) status: String,
    pub(super) active: bool,
    pub(super) started: bool,
    pub(super) dismissed: bool,
    completed: Option<usize>,
    error: Option<String>,
}
impl DownloadState {
    pub(super) fn use_youtube_info(&mut self, item: &OnlineItem) {
        let request = match &self.retry {
            Task::Download(request)
            | Task::DownloadWithCover { request, .. }
            | Task::DownloadWithAutomaticCover { request, .. } => request,
            _ => return,
        };
        if matches!(request, DownloadRequest::YouTube { url, .. } if *url == item.url) {
            self.title = item.title.clone();
            self.creator = item.artist.clone();
        }
    }
    pub(super) fn from_task(task: &Task) -> Option<Self> {
        let request = match task {
            Task::Download(request)
            | Task::DownloadWithCover { request, .. }
            | Task::DownloadWithAutomaticCover { request, .. } => request,
            _ => return None,
        };
        let rescan_partial = matches!(request, DownloadRequest::YouTube { video: false, .. });
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
            DownloadRequest::YouTube {
                url,
                video,
                split_chapters,
            } => (
                format!("youtube:{url}:{video}:{split_chapters}"),
                "YouTube download".into(),
                if *video { "MP4" } else { "M4A" }.into(),
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
            creator: match request {
                DownloadRequest::Archive { item, .. } => item.artist.clone(),
                DownloadRequest::Episodes { podcast, .. } => podcast.author.clone(),
                _ => String::new(),
            },
            source: match request {
                DownloadRequest::YouTube { .. } => "YouTube",
                DownloadRequest::Archive { .. } => "Internet Archive",
                DownloadRequest::Episodes { .. } => "Podcasts",
                _ => "Download",
            }
            .into(),
            format,
            retry: task.clone(),
            shown: 0.0,
            last_draw: None,
            sampled: None,
            speed: 0.0,
            archive,
            rescan_partial,
            done: 0,
            total,
            status: "Queued".into(),
            active: true,
            started: false,
            dismissed: false,
            completed: None,
            error: None,
        })
    }
    pub(super) fn request_cancel(&mut self) {
        if !self.started {
            self.active = false;
            self.error = Some("Cancelled".into());
        } else {
            self.status = "Cancelling…".into();
        }
    }
    fn progress_fraction(&self) -> f64 {
        if self.completed.is_some() {
            return 1.0;
        }
        // Archive reports cumulative album bytes. YouTube playlist totals
        // describe only the current file and must also include track position.
        if self.source != "YouTube" {
            if let Some(total) = self.total.filter(|n| *n > 0) {
                return (self.done as f64 / total as f64).clamp(0., 1.);
            }
        }
        // A bounded playlist may not know its byte total. Show the completed
        // tracks rather than pretending a current file is the whole album.
        let words = self.status.split_whitespace().take(4).collect::<Vec<_>>();
        if let [index, "of", total, "tracks"] = words.as_slice() {
            if let (Ok(index), Ok(total)) = (index.parse::<u64>(), total.parse::<u64>()) {
                if total > 0 {
                    return (index as f64 / total as f64).clamp(0., 1.);
                }
            }
        }
        if let ["Track", index, "of", total] = words.as_slice() {
            if let (Ok(index), Ok(total)) = (index.parse::<u64>(), total.parse::<u64>()) {
                if total > 0 {
                    let current = self
                        .total
                        .filter(|n| *n > 0)
                        .map_or(0., |n| (self.done as f64 / n as f64).clamp(0., 1.));
                    return ((index.saturating_sub(1) as f64 + current) / total as f64)
                        .clamp(0.0, 1.0);
                }
            }
        }
        if let Some(total) = self.total.filter(|total| *total > 0) {
            return (self.done as f64 / total as f64).clamp(0.0, 1.0);
        }
        0.0
    }
    fn detail(&self) -> String {
        if let Some(error) = &self.error {
            return if error.contains("Cancelled") {
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
        if self.queued() {
            return "Queued".into();
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
    fn queued(&self) -> bool {
        self.active && !self.started
    }
    fn row(&self, id: u64) -> Item {
        let mut row = Item::new(
            format!("{} · {}", self.format, self.title),
            Choice::ShowDownload(id),
        );
        row.detail = self.detail();
        row.visual.value = self.format.clone();
        row.visual.progress_visible = self.active;
        row.visual.progress = self.total.filter(|n| *n > 0).map_or(0.0, |total| {
            (self.done as f64 / total as f64).clamp(0., 1.) as f32
        });
        row
    }
}
fn update_archive_row(row: &mut Item, id: u64, state: &DownloadState, format: &str) {
    row.title = if state.queued() {
        format!("Queued {format}")
    } else if state.active {
        format!("Downloading {format}…")
    } else if state.error.is_some() {
        format!("Retry {format} download")
    } else {
        format!("{format} downloaded")
    };
    row.detail = state.detail();
    row.visual.progress_visible = state.active && !state.queued();
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
    #[cfg(test)]
    pub(in crate::app) fn qa_download_view(&mut self, outcome: &str) {
        self.open(ExtraPage::ActiveDownloads);
        let request = DownloadRequest::YouTube {
            url: "https://example.invalid/qa-video".into(),
            video: false,
            split_chapters: true,
        };
        let mut state = DownloadState::from_task(&Task::Download(request)).unwrap();
        state.title = "Timely!!".into();
        state.creator = "Anri".into();
        state.status = "Track 3 of 8 · Downloading audio".into();
        state.started = true;
        state.done = 32 * 1024 * 1024;
        state.total = Some(64 * 1024 * 1024);
        state.shown = 0.5;
        state.speed = 256.0 * 1024.0;
        if outcome != "active" {
            state.active = false;
            match outcome {
                "done" => {
                    state.completed = Some(8);
                    state.done = state.total.unwrap();
                    state.shown = 1.0;
                }
                "cancelled" => state.error = Some("Cancelled".into()),
                _ => state.error = Some("Connection interrupted".into()),
            }
        }
        self.download_states.insert(999_999, state);
        self.show_download(999_999);
    }
    pub(super) fn apply_service_progress(
        &mut self,
        id: u64,
        done: u64,
        total: Option<u64>,
        status: String,
    ) {
        if self.foreground_job(id) {
            return;
        }
        if let Some(state) = self.download_states.get_mut(&id) {
            if !state.active || state.dismissed {
                return;
            }
            state.started = true;
            if let Some((at, previous)) = state.sampled {
                let dt = self.visual_time - at;
                if dt > 0.2 && done >= previous {
                    let sample = (done - previous) as f64 / dt;
                    state.speed = if state.speed == 0.0 {
                        sample
                    } else {
                        state.speed * 0.8 + sample * 0.2
                    };
                    state.sampled = Some((self.visual_time, done));
                } else if done < previous {
                    state.sampled = Some((self.visual_time, done));
                    state.speed = 0.0;
                }
            } else {
                state.sampled = Some((self.visual_time, done));
            }
            state.done = done;
            let new_youtube_track = state.source == "YouTube"
                && status.starts_with("Track ")
                && state.status.split('·').next() != status.split('·').next();
            if total.is_some() || new_youtube_track {
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
        self.cancel_foreground();
        self.cancel_external_foreground();
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
            .filter(|state| state.active && !state.dismissed)
            .count()
    }
    pub(in crate::app) fn has_downloads(&self) -> bool {
        self.download_states.values().any(|state| !state.dismissed)
    }
    pub(super) fn download_rows(&self) -> Vec<Item> {
        let mut rows: Vec<_> = self
            .download_states
            .iter()
            .filter(|(_, s)| s.active && !s.dismissed)
            .map(|(&id, state)| {
                let mut row = state.row(id);
                if let Some(position) = self.download_queue_position(id) {
                    row.detail = format!("Queued · position {position}");
                    row.visual.progress_visible = false;
                }
                row
            })
            .collect();
        let finished: Vec<_> = self
            .download_states
            .iter()
            .rev()
            .filter(|(_, s)| !s.active && !s.dismissed)
            .take(8)
            .map(|(&id, state)| state.row(id))
            .collect();
        if !finished.is_empty() {
            rows.push(visual::header("Recent downloads"));
            rows.extend(finished);
        }
        if rows.is_empty() {
            rows.push(visual::fact("No downloads", ""));
        }
        rows
    }
    fn download_queue_position(&self, id: u64) -> Option<usize> {
        self.download_states
            .get(&id)
            .filter(|state| state.queued() && !state.dismissed)?;
        Some(
            1 + self
                .download_states
                .range(..id)
                .filter(|(_, state)| state.active)
                .count(),
        )
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
        if page
            .rows
            .get(page.selected)
            .is_none_or(|row| !matches!(row.choice, Choice::ShowDownload(_)))
        {
            page.selected = page
                .rows
                .iter()
                .position(|row| matches!(row.choice, Choice::ShowDownload(_)))
                .unwrap_or(0);
        }
    }
    pub(super) fn show_download(&mut self, id: u64) {
        let Some(state) = self.download_states.get(&id) else {
            return;
        };
        if state.dismissed {
            return;
        }
        let title = state.source.clone();
        if self
            .pages
            .last()
            .is_some_and(|page| matches!(page.role, PageRole::Download(_)))
        {
            self.replace(title, Vec::new());
        } else {
            self.push(title, Vec::new());
        }
        self.pages.last_mut().unwrap().role = PageRole::Download(id);
    }
    pub(super) fn handle_download_view(&mut self, action: Action) -> bool {
        let Some(PageRole::Download(id)) = self.pages.last().map(|page| page.role) else {
            return false;
        };
        if matches!(action, Action::Back | Action::BackHeld) {
            if self.pages.len() > 1 {
                self.pages.pop();
            } else {
                self.effects.push(Effect::Exit);
            }
        } else if action == Action::Select {
            let Some(state) = self.download_states.get(&id) else {
                return true;
            };
            if state.active {
                self.choose(Choice::CancelDownload(id));
            } else if state
                .error
                .as_ref()
                .is_some_and(|error| !error.contains("Cancelled"))
            {
                let retry = state.retry.clone();
                self.pages.pop();
                self.task(retry, Purpose::Download);
            } else if self.pages.len() > 1 {
                self.pages.pop();
            } else {
                self.effects.push(Effect::Exit);
            }
        }
        true
    }
    pub(super) fn draw_download_view(
        &mut self,
        r: &mut Renderer,
        assets: &VisualAssets,
        dark: bool,
        now: f64,
    ) -> bool {
        let Some(PageRole::Download(id)) = self.pages.last().map(|page| page.role) else {
            return false;
        };
        let queue_position = self.download_queue_position(id);
        let Some(state) = self.download_states.get_mut(&id) else {
            return false;
        };
        let rgb = |r: u8, g: u8, b: u8| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0];
        let fg = if dark {
            rgb(238, 239, 241)
        } else {
            rgb(0, 0, 0)
        };
        let sub = if dark {
            rgb(150, 153, 158)
        } else {
            rgb(100, 100, 100)
        };
        r.rect(
            0.0,
            40.0,
            640.0,
            440.0,
            if dark { rgb(22, 23, 25) } else { [1.0; 4] },
        );
        let center = |r: &mut Renderer, text: &str, y: f32, size: f32, weight: u16, color| {
            let baseline = y + betterpod_rust::render::line_box(size).1;
            let text = super::super::visual_menu::ellipsize(r, text, size, weight, 560.0);
            let width = r.text_width_weighted(&text, size, weight);
            r.text_baseline(
                &text,
                320.0 - width * 0.5,
                baseline,
                size,
                weight,
                color,
                560.0,
            );
        };
        center(r, &state.title, 84.0, 32.0, 700, fg);
        if !state.creator.is_empty() {
            center(r, &state.creator, 124.0, 26.0, 500, sub);
        }
        let status = if queue_position.is_some() {
            "Waiting in queue".into()
        } else if let Some(files) = state.completed {
            format!(
                "Added to Music · {files} {} · {}",
                if files == 1 { "track" } else { "tracks" },
                state.format
            )
        } else if state.error.is_some() {
            "Stopped".into()
        } else {
            state.status.clone()
        };
        center(r, &status, 192.0, 26.0, 600, fg);
        if state.active && queue_position.is_none() {
            super::super::visual_menu::draw_spinner_in(r, 320., 174., 10., now, sub);
        }
        let detail = if let Some(position) = queue_position {
            if position == 1 {
                "Starting next".into()
            } else {
                format!(
                    "{} {} ahead",
                    position - 1,
                    if position == 2 {
                        "download"
                    } else {
                        "downloads"
                    }
                )
            }
        } else if let Some(error) = &state.error {
            error.clone()
        } else {
            state.total.filter(|total| *total > 0).map_or_else(
                || {
                    if state.done > 0 {
                        format!("{} received", download_bytes(state.done))
                    } else {
                        String::new()
                    }
                },
                |total| {
                    let mut text = format!(
                        "{} of {} · {}",
                        download_bytes(state.done),
                        download_bytes(total),
                        state.format
                    );
                    if state.speed > 0.0 {
                        text.push_str(&format!(" · {}/s", download_bytes(state.speed as u64)));
                    }
                    text
                },
            )
        };
        center(r, &detail, 228.0, 24.0, 500, sub);
        let fraction = state.progress_fraction();
        let dt = state.last_draw.map_or(0.5, |at| (now - at).clamp(0.0, 0.5));
        state.last_draw = Some(now);
        state.shown = if fraction < state.shown {
            fraction
        } else {
            state.shown + (fraction - state.shown) * (dt * 10.0).min(1.0)
        };
        super::super::visual_player::progress(
            r,
            assets,
            80.0,
            272.0,
            480.0,
            state.shown,
            false,
            dark,
        );
        let mut percentage = queue_position.map_or_else(
            || {
                if state.active && state.total.is_none() && fraction == 0. {
                    "Working…".into()
                } else {
                    format!("{}%", (state.shown * 100.0 + 0.5) as u32)
                }
            },
            |position| format!("Queue position {position}"),
        );
        if state.active && state.speed > 0.0 {
            if let Some(total) = state.total.filter(|total| *total > state.done) {
                let seconds = ((total - state.done) as f64 / state.speed).ceil() as u64;
                percentage.push_str(&format!(" · {}:{:02} left", seconds / 60, seconds % 60));
            }
        }
        center(r, &percentage, 332.0, 24.0, 700, sub);
        let retry = !state.active
            && state
                .error
                .as_ref()
                .is_some_and(|error| !error.contains("Cancelled"));
        if retry {
            center(
                r,
                "Tracks already downloaded are kept · [B] to close",
                360.0,
                24.0,
                500,
                sub,
            );
        }
        if state.active {
            center(
                r,
                if queue_position.is_some() {
                    "[B] Back · stays in queue"
                } else {
                    "[B] Back · download continues"
                },
                360.0,
                24.0,
                500,
                sub,
            );
        }
        draw_job_button(
            r,
            if state.active {
                "Cancel"
            } else if retry {
                "Try Again"
            } else {
                "Done"
            },
            true,
            dark,
            fg,
        );
        true
    }
    pub(super) fn refresh_download_pages(&mut self) {
        self.rebuild_video_pages();
        // Finished records are bounded. Active transfers are bounded by the
        // dedicated service queue and must not be evicted.
        let completed: Vec<_> = self
            .download_states
            .iter()
            .rev()
            .filter(|(_, s)| !s.active && !s.dismissed)
            .skip(8)
            .map(|(&id, _)| id)
            .collect();
        for id in completed {
            self.download_states.remove(&id);
        }
        let download_rows = self.download_rows();
        let mut updates = std::collections::BTreeMap::new();
        for (&id, state) in &self.download_states {
            if state.dismissed {
                continue;
            }
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
                if page
                    .rows
                    .get(page.selected)
                    .is_some_and(|row| row.visual.header)
                {
                    page.selected = (page.selected + 1).min(page.rows.len().saturating_sub(1));
                }
                Self::reveal(page, self.viewport_height);
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
                !state.dismissed
                    && state.archive.as_deref() == Some(item)
                    && state.format == format
                    && state.active
            })
            .or_else(|| {
                self.download_states.iter().rev().find(|(_, state)| {
                    !state.dismissed
                        && state.archive.as_deref() == Some(item)
                        && state.format == format
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
        // Published files must enter the library even if their job was removed
        // while its final callback was in flight.
        if matches!(result, Ok(Output::Files(_))) {
            self.effects.push(Effect::Rescan);
        }
        if self
            .download_states
            .get(&id)
            .is_some_and(|state| state.dismissed)
        {
            let state = self.download_states.remove(&id).unwrap();
            if state.rescan_partial && result.is_err() {
                self.effects.push(Effect::Rescan);
            }
            self.refresh_download_pages();
            return;
        }
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
                Err(error) => {
                    state.error = Some(error.clone());
                    // Playlist/chapter audio jobs publish completed songs one
                    // at a time. A later failure or cancellation keeps them.
                    if state.rescan_partial {
                        self.effects.push(Effect::Rescan);
                    }
                }
                _ => state.error = Some("Download returned no files".into()),
            }
            if notify
                && !self
                    .pages
                    .last()
                    .is_some_and(|page| page.role == PageRole::Download(id))
            {
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

// Go's formatBytes uses decimal units on the dedicated download screen.
fn download_bytes(bytes: u64) -> String {
    match bytes {
        bytes if bytes >= 1_000_000_000 => format!("{:.1} GB", bytes as f64 / 1_000_000_000.0),
        bytes if bytes >= 1_000_000 => format!("{:.0} MB", bytes as f64 / 1_000_000.0),
        bytes if bytes > 0 => format!("{:.0} KB", bytes as f64 / 1_000.0),
        _ => "size unknown".into(),
    }
}

// The original drawJobButton geometry and gradients, shared by the Go-style jobs.
pub(in crate::app) fn draw_job_button(
    r: &mut Renderer,
    label: &str,
    primary: bool,
    dark: bool,
    fg: betterpod_rust::render::Color,
) {
    let rgb = |r: u8, g: u8, b: u8| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0];
    let gradient = if primary {
        vec![(0.0, rgb(60, 184, 255)), (1.0, rgb(52, 122, 181))]
    } else if dark {
        vec![
            (0.0, rgb(85, 88, 95)),
            (0.4844, rgb(52, 55, 61)),
            (1.0, rgb(60, 63, 69)),
        ]
    } else {
        vec![
            (0.0, [1.0; 4]),
            (0.4844, rgb(203, 204, 206)),
            (1.0, rgb(223, 223, 223)),
        ]
    };
    r.rounded_gradient(80.0, 408.0, 480.0, 56.0, 18.0, &gradient);
    r.stroke_rounded(
        80.0,
        408.0,
        480.0,
        56.0,
        18.0,
        2.0,
        if primary {
            rgb(58, 120, 173)
        } else if dark {
            rgb(21, 23, 26)
        } else {
            rgb(62, 66, 73)
        },
    );
    let width = r.text_width_weighted(label, 28.0, 700);
    r.text_baseline(
        label,
        320.0 - width * 0.5,
        436.0 + 28.0 * 0.727 * 0.5,
        28.0,
        700,
        if primary { WHITE } else { fg },
        480.0,
    );
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
    fn download_byte_labels_match_go_decimal_units() {
        assert_eq!(download_bytes(5_000_000), "5 MB");
        assert_eq!(download_bytes(10_000_000), "10 MB");
        assert_eq!(download_bytes(1_500_000_000), "1.5 GB");
        assert_eq!(download_bytes(256_000), "256 KB");
        assert_eq!(download_bytes(0), "size unknown");
    }
    #[test]
    fn youtube_download_has_one_job_view_and_cancel_done_keep_the_source_page() {
        let mut fixture = Fixture::new("go-download");
        let extras = &mut fixture.extras;
        assert!(!extras.has_downloads());
        extras.push("YouTube", vec![Item::new("A full album", Choice::Nothing)]);
        let owner = extras.pages.last().unwrap().key;
        let task = Task::Download(DownloadRequest::YouTube {
            url: "https://example.invalid/video".into(),
            video: false,
            split_chapters: true,
        });
        let mut state = DownloadState::from_task(&task).unwrap();
        state.title = "A full album".into();
        state.creator = "An artist".into();
        state.status = "Track 2 of 4 · Downloading".into();
        state.started = true;
        assert_eq!(state.progress_fraction(), 0.25);
        state.done = 50;
        state.total = Some(100);
        assert_eq!(
            state.progress_fraction(),
            0.375,
            "Album progress includes the current file"
        );
        let mut archive = state.clone();
        archive.source = "Internet Archive".into();
        assert_eq!(
            archive.progress_fraction(),
            0.5,
            "Archive bytes already describe the complete album"
        );
        let handle = extras.services.submit(Task::Downloaded).unwrap();
        extras
            .jobs
            .insert(handle.id, (handle.clone(), Purpose::Download));
        extras.download_states.insert(handle.id, state);
        assert!(extras.has_downloads());
        extras.show_download(handle.id);
        assert!(extras.has_custom_visual());
        assert!(extras.visual_page().is_none());
        assert_eq!(extras.title(), "YouTube");
        extras.handle(Action::Back);
        assert_eq!(extras.pages.last().unwrap().key, owner);
        assert!(
            !handle.is_cancelled(),
            "B closes the view while the download continues"
        );
        extras.show_downloads(None);
        assert!(extras.pages.last().unwrap().role == PageRole::ActiveDownloads);
        let reopen = extras
            .pages
            .last()
            .unwrap()
            .rows
            .iter()
            .find_map(|row| {
                matches!(row.choice, Choice::ShowDownload(id) if id == handle.id)
                    .then(|| row.choice.clone())
            })
            .unwrap();
        extras.choose(reopen);
        assert!(extras.pages.last().unwrap().role == PageRole::Download(handle.id));
        assert_eq!(
            extras.download_states[&handle.id].status,
            "Track 2 of 4 · Downloading"
        );
        extras.handle(Action::Back);
        extras.handle(Action::Back);
        assert_eq!(extras.pages.last().unwrap().key, owner);
        extras.show_download(handle.id);
        extras.handle(Action::Select);
        assert!(handle.is_cancelled(), "The one Cancel button stops the job");
        extras.download_finished(handle.id, &Err("Cancelled".into()));
        assert!(
            extras.has_downloads(),
            "Recent stopped jobs keep the Downloads entry available"
        );
        extras.handle(Action::Select);
        assert_eq!(extras.pages.last().unwrap().key, owner);
        assert_eq!(extras.pages.last().unwrap().rows[0].title, "A full album");
    }
    #[test]
    fn failed_or_cancelled_youtube_album_rescans_kept_songs_without_reporting_completion() {
        let mut fixture = Fixture::new("partial-youtube");
        let extras = &mut fixture.extras;
        extras.push(
            "Unrelated page",
            vec![Item::new("Keep this row", Choice::Nothing)],
        );
        let owner = extras.pages.last().unwrap().key;
        for error in ["The second song failed; the first was saved", "Cancelled"] {
            let task = Task::Download(DownloadRequest::YouTube {
                url: "https://www.youtube.com/playlist?list=PLfixture".into(),
                video: false,
                split_chapters: false,
            });
            let mut state = DownloadState::from_task(&task).unwrap();
            state.status = "Downloading song 2 of 3".into();
            extras.download_states.insert(91, state);
            extras.effects.clear();
            extras.download_finished(91, &Err(error.into()));
            assert_eq!(
                extras
                    .effects
                    .iter()
                    .filter(|effect| matches!(effect, Effect::Rescan))
                    .count(),
                1
            );
            let state = &extras.download_states[&91];
            assert!(!state.active && state.completed.is_none());
            assert_eq!(state.error.as_deref(), Some(error));
            assert!(!state.detail().starts_with("Complete"));
            assert!(!extras.effects.iter().any(|effect| matches!(effect, Effect::Notice(text) if text.starts_with("Download complete"))));
            assert_eq!(extras.pages.last().unwrap().key, owner);
            assert_eq!(extras.title(), "Unrelated page");
            assert_eq!(extras.pages.last().unwrap().rows[0].title, "Keep this row");
        }
        extras.effects.clear();
        let video = Task::Download(DownloadRequest::YouTube {
            url: "https://www.youtube.com/watch?v=abcdefghijk".into(),
            video: true,
            split_chapters: false,
        });
        extras
            .download_states
            .insert(92, DownloadState::from_task(&video).unwrap());
        extras.download_finished(92, &Err("Failed before saving".into()));
        assert!(
            !extras
                .effects
                .iter()
                .any(|effect| matches!(effect, Effect::Rescan)),
            "Video failures must not trigger an audio-library rescan"
        );
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
            .find(|r| r.label.starts_with("Queued FLAC"))
            .unwrap();
        assert!(row.detail.contains("Queued"));
        assert!(!row.visual.progress_visible);
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
        assert!(page.rows[0].visual.search_field);
        assert!(page.rows.iter().any(|row| row.label == "Showing: Videos"));
        assert!(!page.rows.iter().any(|row| row.visual.progress_visible));
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
        state.started = true;
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
    #[test]
    fn new_youtube_requests_open_their_own_queued_job_and_cancel_only_that_job() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::mpsc,
            time::{Duration, Instant},
        };
        let mut fixture = Fixture::new("fifo-navigation");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/first.m4a", listener.local_addr().unwrap());
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
            ready_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nblocked",
            );
        });
        let extras = &mut fixture.extras;
        let mut album = OnlineItem::new(Provider::Archive);
        album.id = "first-archive".into();
        album.title = "First Album".into();
        album.raw = serde_json::json!({"groups":{"M4A":[{"name":"first.m4a","download_url":url,"size":7}]}});
        extras.push(
            "Internet Archive",
            vec![Item::new("First Album", Choice::Nothing)],
        );
        extras.start_download(DownloadRequest::Archive {
            item: album,
            format: "M4A".into(),
        });
        ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let first = *extras.download_states.keys().next().unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while !extras.download_states[&first].started && Instant::now() < until {
            extras.tick(0.01);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(extras.download_states[&first].started);
        extras.push("YouTube", vec![Item::new("Second Album", Choice::Nothing)]);
        let owner = extras.pages.last().unwrap().key;
        let mut video = OnlineItem::new(Provider::YouTube);
        video.title = "Second Album".into();
        video.url = "https://www.youtube.com/watch?v=first000001".into();
        extras.detail_item = Some(video.clone());
        let request = DownloadRequest::YouTube {
            url: video.url.clone(),
            video: false,
            split_chapters: false,
        };
        extras.start_download(request.clone());
        let second = *extras.download_states.keys().next_back().unwrap();
        assert_ne!(first, second);
        assert!(extras.pages.last().unwrap().role == PageRole::Download(second));
        assert_eq!(extras.download_states[&second].title, "Second Album");
        assert_eq!(extras.download_queue_position(second), Some(2));
        assert_eq!(extras.active_download_count(), 2);
        extras.start_download(request);
        assert_eq!(
            extras.active_download_count(),
            2,
            "Reopening an identical request must not duplicate it"
        );
        assert!(extras.pages.last().unwrap().role == PageRole::Download(second));
        let rows = extras.download_rows();
        assert!(matches!(rows[0].choice, Choice::ShowDownload(id) if id == first));
        assert!(matches!(rows[1].choice, Choice::ShowDownload(id) if id == second));
        assert!(rows[1].detail.contains("position 2"));
        extras.handle(Action::Back);
        assert_eq!(extras.pages.last().unwrap().key, owner);
        assert!(
            !extras.jobs[&second].0.is_cancelled(),
            "B leaves the job queued"
        );
        extras.start_download(DownloadRequest::YouTube {
            url: video.url,
            video: false,
            split_chapters: true,
        });
        let third = *extras.download_states.keys().next_back().unwrap();
        assert_ne!(second, third, "Album and single-song requests are distinct");
        assert_eq!(extras.download_queue_position(third), Some(3));
        extras.show_download(second);
        extras.handle(Action::Select);
        assert!(extras.jobs[&second].0.is_cancelled());
        assert!(!extras.jobs[&first].0.is_cancelled());
        assert!(!extras.jobs[&third].0.is_cancelled());
        assert_eq!(extras.download_queue_position(third), Some(2));
        extras.choose(Choice::CancelDownload(third));
        extras.choose(Choice::CancelDownload(first));
        release_tx.send(()).unwrap();
        server.join().unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while extras
            .jobs
            .values()
            .any(|(_, purpose)| matches!(purpose, Purpose::Download))
            && Instant::now() < until
        {
            extras.tick(0.01);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(extras.active_download_count(), 0);
        assert!(extras.download_states.values().all(|state| state
            .error
            .as_ref()
            .is_some_and(|error| error.contains("Cancelled"))));
    }
    fn history_state(title: &str) -> DownloadState {
        let mut state = DownloadState::from_task(&Task::Download(DownloadRequest::YouTube {
            url: format!("https://example.invalid/{title}"),
            video: false,
            split_chapters: false,
        }))
        .unwrap();
        state.title = title.into();
        state.active = false;
        state.error = Some("Fixture failure".into());
        state
    }
    #[test]
    fn y_removes_one_history_entry_and_keeps_saved_files_and_selection() {
        let mut fixture = Fixture::new("remove-history");
        let saved = fixture.root.join("saved.m4a");
        std::fs::write(&saved, b"keep downloaded audio").unwrap();
        let extras = &mut fixture.extras;
        extras
            .download_states
            .insert(10, history_state("Failed album"));
        let mut complete = history_state("Saved album");
        complete.error = None;
        complete.completed = Some(1);
        extras.download_states.insert(9, complete);
        extras.show_downloads(None);
        extras.handle(Action::Options);
        assert!(extras.pages.last().unwrap().role == PageRole::Sheet);
        assert_eq!(
            extras.pages.last().unwrap().rows[0].title,
            "Remove from List"
        );
        extras.handle(Action::Select);
        assert!(!extras.download_states.contains_key(&10));
        let page = extras.pages.last().unwrap();
        assert!(page.role == PageRole::ActiveDownloads);
        assert!(matches!(
            page.rows[page.selected].choice,
            Choice::ShowDownload(9)
        ));
        extras.handle(Action::Options);
        extras.handle(Action::Select);
        assert!(!extras.has_downloads());
        extras.handle(Action::Options);
        assert!(extras.pages.last().unwrap().role == PageRole::ActiveDownloads);
        assert_eq!(std::fs::read(saved).unwrap(), b"keep downloaded audio");
    }
    #[test]
    fn clear_list_confirms_cancels_owned_work_and_does_not_resurrect_late_results() {
        let mut fixture = Fixture::new("clear-history");
        let extras = &mut fixture.extras;
        let first = extras.services.submit(Task::Downloaded).unwrap();
        let queued = extras.services.submit(Task::Downloaded).unwrap();
        for (handle, started) in [(&first, true), (&queued, false)] {
            let mut state = history_state("Pending album");
            state.active = true;
            state.started = started;
            state.error = None;
            extras.download_states.insert(handle.id, state);
            extras
                .jobs
                .insert(handle.id, (handle.clone(), Purpose::Download));
        }
        extras
            .download_states
            .insert(100, history_state("Failed album"));
        extras.show_downloads(None);
        extras.handle(Action::Options);
        assert_eq!(
            extras.pages.last().unwrap().rows[0].title,
            "Cancel and Remove"
        );
        extras.handle(Action::Down);
        extras.handle(Action::Select);
        assert!(extras.pages.last().unwrap().title.contains("Files stay"));
        extras.handle(Action::Back);
        assert!(!first.is_cancelled() && !queued.is_cancelled());
        assert!(extras.has_downloads());
        extras.handle(Action::Options);
        extras.handle(Action::Down);
        extras.handle(Action::Select);
        extras.handle(Action::Select);
        assert!(first.is_cancelled() && queued.is_cancelled());
        assert!(!extras.has_downloads());
        assert_eq!(extras.active_download_count(), 0);
        assert_eq!(
            extras.pending_download_count(),
            2,
            "Stopping workers retain idle protection"
        );
        assert!(extras.download_queue_position(queued.id).is_none());
        extras.show_download(first.id);
        assert!(extras.pages.last().unwrap().role == PageRole::ActiveDownloads);
        for id in 101..120 {
            extras
                .download_states
                .insert(id, history_state("Other history"));
        }
        extras.refresh_download_pages();
        assert!(
            extras.download_states[&queued.id].dismissed,
            "History pruning must retain a stopping queued job"
        );
        extras.apply_service_progress(first.id, 10, Some(20), "Late progress".into());
        assert_ne!(extras.download_states[&first.id].status, "Late progress");
        extras.effects.clear();
        extras.jobs.remove(&first.id);
        extras.download_finished(first.id, &Err("Stopped after one track: Cancelled".into()));
        assert!(extras
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::Rescan)));
        assert!(!extras
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::Notice(_))));
        assert!(!extras.download_states.contains_key(&first.id));
        extras.jobs.remove(&queued.id);
        extras.download_finished(queued.id, &Ok(Output::Files(vec![])));
        assert!(!extras.download_states.contains_key(&queued.id));
        assert_eq!(extras.pending_download_count(), 0);
    }
}
