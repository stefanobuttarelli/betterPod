//! Go1.1 Videos / Downloaded / Recently Watched menu state. Catalog snapshots
//! come from Services; menu projection and input never touch files or probes.
use super::*;
use betterpod_rust::video_catalog;
#[derive(Clone, Copy)]
pub(super) enum VideoList {
    Saved,
    Recent,
}
#[derive(Clone)]
pub(super) enum VideoAction {
    Search,
    Play(VideoList, usize, f64, String),
    Watch(OnlineItem),
    Download(OnlineItem),
    Delete(PathBuf),
    Forget(String),
    GetFfmpeg,
}
#[derive(Clone)]
pub(super) enum VideoJob {
    Catalog(u64),
    Delete,
    Forget,
    Ffmpeg,
}
#[derive(Default)]
pub(super) struct Videos {
    pub snapshot: video_catalog::Snapshot,
    pub current: Option<OnlineItem>,
    pub watch_depth: Option<usize>,
    pub epoch: u64,
    pub ffmpeg_busy: bool,
    pub ffmpeg_error: String,
}
fn source_video_time(seconds: f64, duration: f64) -> String {
    let seconds = seconds.max(0.) as u64;
    if duration >= 3600. || seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}
fn source_video_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else if bytes >= 1_000_000 {
        format!("{:.0} MB", bytes as f64 / 1e6)
    } else if bytes > 0 {
        format!("{:.0} KB", bytes as f64 / 1e3)
    } else {
        "size unknown".into()
    }
}
impl Extras {
    pub(super) fn open_videos(&mut self, page: ExtraPage) {
        self.videos.watch_depth = None;
        match page {
            ExtraPage::Videos => {
                self.push("Videos", self.video_home_rows());
                self.pages.last_mut().unwrap().role = PageRole::VideoHome;
            }
            ExtraPage::SavedVideos => {
                self.push("Downloaded", self.video_list_rows(VideoList::Saved));
                self.pages.last_mut().unwrap().role = PageRole::SavedVideos;
            }
            ExtraPage::VideoHistory => {
                self.push("Recently Watched", self.video_list_rows(VideoList::Recent));
                self.pages.last_mut().unwrap().role = PageRole::RecentVideos;
            }
            _ => return,
        }
        self.refresh_video_catalog();
    }
    pub(super) fn refresh_video_catalog(&mut self) {
        self.videos.epoch = self.videos.epoch.wrapping_add(1);
        let epoch = self.videos.epoch;
        match self.services.submit(Task::VideoCatalog) {
            Ok(handle) => {
                self.jobs.insert(
                    handle.id,
                    (handle, Purpose::Video(VideoJob::Catalog(epoch))),
                );
            }
            Err(error) => self.effects.push(Effect::Notice(error)),
        }
    }
    fn video_home_rows(&self) -> Vec<Item> {
        let mut rows = vec![Item::new(
            "Search YouTube",
            Choice::Video(VideoAction::Search),
        )];
        if !self.videos.snapshot.recent.is_empty() {
            rows.push(Item::new(
                "Recently Watched",
                Choice::Page(ExtraPage::VideoHistory),
            ));
        }
        if !self.videos.snapshot.saved.is_empty() {
            rows.push(Item::new(
                "Downloaded",
                Choice::Page(ExtraPage::SavedVideos),
            ));
        }
        if self.has_downloads() {
            rows.push(Item::new(
                "Downloads",
                Choice::Page(ExtraPage::ActiveDownloads),
            ));
        }
        if self.videos.ffmpeg_busy {
            rows.push(visual::fact(
                "Downloading ffmpeg…",
                "ffmpeg, a static build from GitHub",
            ));
        } else if !self.videos.ffmpeg_error.is_empty() {
            let mut row = Item::new("Couldn't get ffmpeg", Choice::Video(VideoAction::GetFfmpeg));
            row.detail = format!("{} · press [A] to try again", self.videos.ffmpeg_error);
            row.visual.no_arrow = true;
            rows.push(row);
        } else if !self.videos.snapshot.ffmpeg_ready
            && !self.videos.snapshot.ffmpeg_error.is_empty()
        {
            let mut row = Item::new(
                if self.videos.snapshot.can_install_ffmpeg {
                    "Get ffmpeg"
                } else {
                    "ffmpeg unavailable"
                },
                if self.videos.snapshot.can_install_ffmpeg {
                    Choice::Video(VideoAction::GetFfmpeg)
                } else {
                    Choice::Nothing
                },
            );
            row.detail = if self.videos.snapshot.can_install_ffmpeg {
                "Videos need it: 26 MB to get, 51 MB on the card".into()
            } else {
                self.videos.snapshot.ffmpeg_error.clone()
            };
            rows.push(row);
        }
        if self.tools_state.ready
            && self
                .tools_state
                .runtime
                .to_ascii_lowercase()
                .starts_with("quickjs")
        {
            let mut row = Item::new("Open Videos Faster", Choice::Tools);
            row.detail = "Deno for yt-dlp: 40 MB to get, 85 MB on the card".into();
            rows.push(row);
        }
        for row in &mut rows {
            row.visual.preview = super::super::visual_menu::PreviewKind::Image {
                asset: "preview_videos".into(),
                title: "Videos".into(),
                subtitle: String::new(),
                size: 192,
            };
        }
        rows
    }
    fn video_list_rows(&self, list: VideoList) -> Vec<Item> {
        let items = match list {
            VideoList::Saved => &self.videos.snapshot.saved,
            VideoList::Recent => &self.videos.snapshot.recent,
        };
        if items.is_empty() {
            return vec![visual::fact(
                match list {
                    VideoList::Saved => "No videos downloaded",
                    VideoList::Recent => "No videos",
                },
                "",
            )];
        }
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let position = item.raw["position"].as_f64().unwrap_or(0.);
                let mut row = Item::new(
                    &item.title,
                    Choice::Video(VideoAction::Play(list, index, position, item.cover.clone())),
                );
                let mut details = Vec::new();
                if !item.artist.is_empty() {
                    details.push(item.artist.clone());
                }
                if matches!(list, VideoList::Recent) && position > 0. && item.seconds > 0. {
                    details.push(format!(
                        "{} of {}",
                        source_video_time(position, item.seconds),
                        source_video_time(item.seconds, item.seconds)
                    ));
                } else if item.seconds > 0. {
                    details.push(super::super::time_label(item.seconds));
                }
                if matches!(list, VideoList::Saved) {
                    if let Some(size) = item.raw["size"].as_u64().filter(|size| *size > 0) {
                        details.push(source_video_bytes(size));
                    }
                }
                row.detail = details.join(" · ");
                row.visual.thumbnail = true;
                row.cover = item.raw["cover_path"].as_str().map(PathBuf::from);
                row.visual.no_arrow = false;
                row.options.push(Item::new(
                    "Watch from the Start",
                    Choice::Video(VideoAction::Play(list, index, 0., item.cover.clone())),
                ));
                if matches!(list, VideoList::Recent) {
                    row.options.extend(self.video_download_options(item));
                    row.options.push(Item::new(
                        "Remove from Recently Watched",
                        Choice::Video(VideoAction::Forget(item.id.clone())),
                    ));
                } else if let Some(path) = item.raw["local_path"].as_str() {
                    row.options.push(Item::new(
                        "Delete Download",
                        Choice::Video(VideoAction::Delete(PathBuf::from(path))),
                    ));
                }
                row
            })
            .collect()
    }
    pub(super) fn video_download_options(&self, item: &OnlineItem) -> Vec<Item> {
        let mut rows = vec![Item::new(
            "Download",
            Choice::Video(VideoAction::Download(item.clone())),
        )];
        if let Some(saved) = self
            .videos
            .snapshot
            .saved
            .iter()
            .find(|saved| saved.id == item.id || saved.url == item.url)
        {
            if let Some(path) = saved.raw["local_path"].as_str() {
                rows.push(Item::new(
                    "Delete Download",
                    Choice::Video(VideoAction::Delete(PathBuf::from(path))),
                ));
            }
        }
        rows
    }
    pub(super) fn video_search_choice(&self, item: &OnlineItem, metadata: bool) -> Option<Choice> {
        (!metadata
            && matches!(
                item.provider,
                Provider::YouTube | Provider::YouTubePlaylists
            )
            && self
                .videos
                .watch_depth
                .is_some_and(|depth| self.pages.len() >= depth))
        .then(|| Choice::Video(VideoAction::Watch(item.clone())))
    }
    pub(super) fn video_action(&mut self, action: VideoAction) {
        match action {
            VideoAction::Search => {
                if !self.tools_state.ready {
                    self.push_page(ExtraPage::Tools);
                    return;
                }
                self.videos.watch_depth = Some(self.pages.len() + 1);
                self.push_page(ExtraPage::Online(Provider::YouTube));
            }
            VideoAction::Play(list, index, start, _) => {
                let queue = match list {
                    VideoList::Saved => self.videos.snapshot.saved.clone(),
                    VideoList::Recent => self.videos.snapshot.recent.clone(),
                };
                if let Some(item) = queue.get(index).cloned() {
                    self.play_video_item(item, queue, index, start);
                }
            }
            VideoAction::Watch(item) => {
                let queue = if self.media_queue.is_empty() {
                    vec![item.clone()]
                } else {
                    self.media_queue.clone()
                };
                let index = queue
                    .iter()
                    .position(|i| i.id == item.id && i.url == item.url)
                    .unwrap_or(0);
                self.play_video_item(item, queue, index, 0.);
            }
            VideoAction::Download(item) => {
                let saved = self
                    .videos
                    .snapshot
                    .saved
                    .iter()
                    .any(|saved| saved.id == item.id || saved.url == item.url);
                let mut rows = Vec::new();
                if !saved {
                    rows.push(Item::new(
                        "As Video",
                        Choice::Download(DownloadRequest::YouTube {
                            url: item.url.clone(),
                            video: true,
                            split_chapters: false,
                        }),
                    ));
                }
                rows.push(Item::new(
                    "As Audio Only",
                    Choice::Download(DownloadRequest::YouTube {
                        url: item.url.clone(),
                        video: false,
                        split_chapters: false,
                    }),
                ));
                let count = item.raw["chapters"].as_array().map_or(0, Vec::len);
                if count >= 2 {
                    rows.push(Item::new(
                        format!("As an Album ({count} tracks)"),
                        Choice::Download(DownloadRequest::YouTube {
                            url: item.url.clone(),
                            video: false,
                            split_chapters: true,
                        }),
                    ));
                }
                self.push_sheet("Download", rows);
            }
            VideoAction::Delete(path) => self.task(
                Task::DeleteSavedVideo { path },
                Purpose::Video(VideoJob::Delete),
            ),
            VideoAction::Forget(id) => {
                self.task(Task::ForgetVideo { id }, Purpose::Video(VideoJob::Forget))
            }
            VideoAction::GetFfmpeg => {
                self.videos.ffmpeg_busy = true;
                self.videos.ffmpeg_error.clear();
                self.task(Task::InstallFfmpeg, Purpose::Video(VideoJob::Ffmpeg));
                self.rebuild_video_pages();
            }
        }
    }
    pub(super) fn known_local_video(&self, item: &OnlineItem) -> Option<OnlineItem> {
        self.videos
            .snapshot
            .saved
            .iter()
            .chain(self.videos.snapshot.recent.iter())
            .find(|known| {
                (known.url == item.url || !item.id.is_empty() && known.id == item.id)
                    && known.raw["local_path"].is_string()
            })
            .cloned()
    }
    pub(super) fn play_video_item(
        &mut self,
        item: OnlineItem,
        queue: Vec<OnlineItem>,
        index: usize,
        start: f64,
    ) {
        let mut item = item;
        item.raw["_video_auto_next"] = false.into();
        let queue = queue
            .into_iter()
            .map(|mut item| {
                item.raw["_video_auto_next"] = false.into();
                item
            })
            .collect::<Vec<_>>();
        let saved = self.known_local_video(&item);
        if let Some(known) = saved.as_ref() {
            item.raw["local_path"] = known.raw["local_path"].clone();
            item.raw["cover_path"] = known.raw["cover_path"].clone();
        } else if let Some(raw) = item.raw.as_object_mut() {
            raw.remove("local_path");
            raw.remove("cover_path");
        }
        self.videos.current = Some(item.clone());
        if let Some(path) = saved
            .as_ref()
            .and_then(|known| known.raw["local_path"].as_str())
        {
            self.effects.push(Effect::VideoOpening {
                item: item.clone(),
                queue: queue.clone(),
                index,
                start,
            });
            self.effects.push(Effect::Stream {
                source: StreamSource {
                    video_url: path.into(),
                    audio_url: None,
                    headers: Vec::new(),
                    duration: item.seconds,
                    title: item.title.clone(),
                    available_at: None,
                    chapters: item.raw["chapters"].as_array().map_or(0, Vec::len),
                },
                video: true,
                origin: item.url,
                start,
                queue,
                index,
            });
        } else {
            self.resolve_media(item, queue, index, true, start);
        }
    }
    pub(super) fn remember_video_info(
        &mut self,
        url: String,
        title: String,
        position: f64,
        duration: f64,
    ) {
        let mut item = self
            .videos
            .current
            .as_ref()
            .filter(|item| item.url == url)
            .cloned()
            .unwrap_or_else(|| {
                let mut item = OnlineItem::new(Provider::YouTube);
                item.url = url.clone();
                item.title = title.clone();
                item
            });
        item.title = title;
        item.seconds = duration;
        match self
            .services
            .submit(Task::RememberVideoInfo { item, position })
        {
            Ok(handle) => {
                self.jobs.insert(handle.id, (handle, Purpose::Remember));
            }
            Err(error) => eprintln!("Remember video: {error}"),
        }
    }
    pub(super) fn video_finished(&mut self, job: VideoJob, result: Result<Output, String>) {
        match (job, result) {
            (VideoJob::Catalog(epoch), Ok(Output::VideoCatalog(snapshot))) => {
                if epoch != self.videos.epoch {
                    return;
                }
                self.videos.snapshot = snapshot;
                self.rebuild_video_pages();
            }
            (VideoJob::Ffmpeg, Ok(_)) => {
                self.videos.ffmpeg_busy = false;
                self.videos.ffmpeg_error.clear();
                self.refresh_video_catalog();
            }
            (VideoJob::Ffmpeg, Err(error)) => {
                self.videos.ffmpeg_busy = false;
                self.videos.ffmpeg_error = error;
                self.rebuild_video_pages();
            }
            (VideoJob::Delete | VideoJob::Forget, Ok(_)) => self.refresh_video_catalog(),
            (_, Err(error)) => self.effects.push(Effect::Notice(error)),
            _ => {}
        }
    }
    pub(super) fn rebuild_video_pages(&mut self) {
        for index in 0..self.pages.len() {
            let rows = match self.pages[index].role {
                PageRole::VideoHome => self.video_home_rows(),
                PageRole::SavedVideos => self.video_list_rows(VideoList::Saved),
                PageRole::RecentVideos => self.video_list_rows(VideoList::Recent),
                _ => continue,
            };
            let page = &mut self.pages[index];
            page.rows = rows;
            page.visual_rows = None;
            page.selected = page.selected.min(page.rows.len().saturating_sub(1));
        }
    }
}

#[cfg(test)]
impl Extras {
    pub(in crate::app) fn visual_videos_fixture(
        &mut self,
        key: &str,
    ) -> Result<super::super::Page, String> {
        let mut first = OnlineItem::new(Provider::YouTube);
        first.id = "fixture_first".into();
        first.url = "https://www.youtube.com/watch?v=fixture_first".into();
        first.title = "A short film".into();
        first.artist = "Fixture Studio".into();
        first.seconds = 245.;
        first.kind = "video".into();
        first.raw = serde_json::json!({"position":65.,"size":12345678,"at":20,"local_path":"/fixture/videos/fixture_first.mp4"});
        let mut second = first.clone();
        second.id = "fixture_second".into();
        second.url = "https://www.youtube.com/watch?v=fixture_second".into();
        second.title = "Another journey".into();
        second.seconds = 130.;
        second.raw = serde_json::json!({"position":0.,"size":23456789,"at":10,"local_path":"/fixture/videos/fixture_second.mp4"});
        self.videos.snapshot = video_catalog::Snapshot {
            saved: vec![first.clone(), second.clone()],
            recent: vec![first, second],
            ffmpeg_ready: true,
            ..Default::default()
        };
        self.videos.ffmpeg_error.clear();
        self.videos.ffmpeg_busy = false;
        self.status.clear();
        self.pages.clear();
        self.keyboard = None;
        self.tools_state.ready = false;
        let (title, role, rows) = match key {
            "videos-home" => ("Videos", PageRole::VideoHome, self.video_home_rows()),
            "videos-saved" => (
                "Downloaded",
                PageRole::SavedVideos,
                self.video_list_rows(VideoList::Saved),
            ),
            "videos-recent" => (
                "Recently Watched",
                PageRole::RecentVideos,
                self.video_list_rows(VideoList::Recent),
            ),
            _ => return Err("Unknown Videos fixture".into()),
        };
        self.push(title, rows);
        self.pages.last_mut().unwrap().role = role;
        self.visual_page()
            .ok_or("Missing Videos fixture page".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_video_actions_keep_source_menus_and_play_offline_without_resolution() {
        let root = std::env::temp_dir().join(format!("rust-video-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("music")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        let mut extras = Extras::new(root.join("music"), root.join("data")).unwrap();
        let page = extras.visual_videos_fixture("videos-saved").unwrap();
        assert_eq!(page.title, "Downloaded");
        assert_eq!(source_video_bytes(12345678), "12 MB");
        assert_eq!(source_video_time(65., 245.), "01:05");
        assert!(page.rows.iter().all(|r| r.visual.thumbnail));
        let rows = extras.video_list_rows(VideoList::Saved);
        assert_eq!(
            rows[0]
                .options
                .iter()
                .map(|r| r.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Watch from the Start", "Delete Download"]
        );
        let recent = extras.video_list_rows(VideoList::Recent);
        assert_eq!(
            recent[0]
                .options
                .iter()
                .map(|r| r.title.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Watch from the Start",
                "Download",
                "Delete Download",
                "Remove from Recently Watched"
            ]
        );
        let item = extras.videos.snapshot.saved[0].clone();
        let jobs = extras.jobs.len();
        extras.play_video_item(item.clone(), vec![item.clone()], 0, 65.);
        assert_eq!(
            extras.jobs.len(),
            jobs,
            "offline playback must not enqueue a resolver"
        );
        assert!(matches!(extras.effects[0], Effect::VideoOpening { .. }));
        let Effect::Stream {
            source,
            origin,
            start,
            queue,
            ..
        } = &extras.effects[1]
        else {
            panic!("saved video must open directly")
        };
        assert_eq!(source.video_url, "/fixture/videos/fixture_first.mp4");
        assert_eq!(origin, &item.url);
        assert_eq!(*start, 65.);
        assert_eq!(queue[0].raw["_video_auto_next"], false);
        let mut untrusted = OnlineItem::new(Provider::YouTube);
        untrusted.url = "https://www.youtube.com/watch?v=untrusted".into();
        untrusted.raw = serde_json::json!({"local_path":"/outside/foreign.mp4"});
        assert!(extras.known_local_video(&untrusted).is_none());
        extras.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }
}
