//! Deterministic YouTube state only. The real menu, input and render code run.
use super::*;

fn entry(id: &str, title: &str) -> serde_json::Value {
    serde_json::json!({"id":id,"title":title,"channel":"Fixture Artist","duration":245,"url":format!("https://www.youtube.com/watch?v={id}"),"ie_key":"Youtube"})
}
impl Extras {
    pub(in crate::app) fn visual_youtube_fixture(
        &mut self,
        key: &str,
    ) -> Result<super::super::Page, String> {
        self.cancel_foreground();
        self.pages.clear();
        self.keyboard = None;
        self.closing_keyboard = None;
        self.pending_navigation = None;
        self.status.clear();
        self.status_job = None;
        self.status_page = None;
        self.videos.watch_depth = None;
        self.searches.clear();
        let video = betterpod_rust::online::youtube_item(
            entry("abcdefghijk", "Fixture Video"),
            Provider::YouTube,
        );
        let playlist = betterpod_rust::online::youtube_item(
            serde_json::json!({
                "id":"PLfixture","title":"Fixture Album","channel":"Fixture Artist","_type":"playlist",
                "webpage_url":"https://www.youtube.com/playlist?list=PLfixture",
                "entries":[entry("abcdefghijk","First Song"),entry("lmnopqrstuv","Second Song"),entry("wxyz1234567","Third Song")]
            }),
            Provider::YouTubePlaylists,
        );
        match key {
            "youtube-search" | "youtube-gear" | "youtube-settings" | "youtube-row-loader" => {
                let provider = if key == "youtube-settings" {
                    Provider::YouTubePlaylists
                } else {
                    Provider::YouTube
                };
                self.searches.push(SearchState {
                    provider,
                    metadata: false,
                    query: "Fixture Artist".into(),
                    items: vec![video.clone(), playlist],
                    total: 2,
                    page: 1,
                    error: String::new(),
                    busy: false,
                    generation: self.generation,
                });
                self.push(provider.name(), self.search_rows(provider, false));
                self.pages.last_mut().unwrap().role = PageRole::Search(provider, false);
                if key == "youtube-settings" {
                    self.youtube_search_settings(provider);
                } else if key == "youtube-gear" {
                    self.handle(Action::Right);
                } else if key == "youtube-row-loader" {
                    let page = self.pages.last_mut().unwrap();
                    page.selected = 2;
                    page.rows[2].visual.busy = true;
                    page.visual_rows = None;
                }
            }
            "youtube-video-detail" | "youtube-playlist-detail" => {
                let mut item = if key == "youtube-playlist-detail" {
                    playlist
                } else {
                    video
                };
                if key == "youtube-video-detail" {
                    item.raw["chapters"] = serde_json::json!([
                        {"title":"First Song","start_time":0,"end_time":80},
                        {"title":"Second Song","start_time":80,"end_time":160},
                        {"title":"Third Song","start_time":160,"end_time":245}
                    ]);
                }
                self.detail_item = Some(item.clone());
                self.push("YouTube", self.details_rows(&item, false));
                self.pages.last_mut().unwrap().selected = 1;
            }
            "youtube-download" | "youtube-download-error" | "youtube-download-queued" => {
                let task = Task::Download(DownloadRequest::YouTube {
                    url: playlist.url.clone(),
                    video: false,
                    split_chapters: false,
                });
                let mut state =
                    DownloadState::from_task(&task).ok_or("Missing download fixture state")?;
                state.title = "Fixture Album".into();
                state.creator = "Fixture Artist".into();
                state.status = "Track 2 of 3 · Downloading".into();
                state.started = true;
                state.done = 5_000_000;
                state.total = Some(10_000_000);
                if key == "youtube-download-queued" {
                    let first = state.clone();
                    self.download_states.insert(6, first);
                    state.started = false;
                    state.done = 0;
                    state.status = "Queued".into();
                    state.title = "Next Album".into();
                }
                self.download_states.insert(7, state);
                if key == "youtube-download-error" {
                    self.download_finished(7, &Err("Fixture connection failed".into()));
                }
                self.show_download(7);
            }
            _ => return Err("Unknown YouTube visual fixture".into()),
        }
        self.pending_navigation = None;
        if key.starts_with("youtube-download") {
            return Ok(super::super::Page {
                screen: super::super::Screen::Extra,
                kind: super::super::ViewKind::Full,
                mini: false,
                title: "YouTube".into(),
                rows: Arc::new(Vec::new()),
                selected: 0,
                scroll: 0.,
                target_scroll: 0.,
                enter: 0.,
            });
        }
        self.visual_page()
            .ok_or("Missing YouTube fixture page".into())
    }
}
