//! Full-screen VideoView, directly from Go 1.1 ij_video.go. The UI samples
//! the existing video actor; rendering and controller callbacks never decode.
use super::*;
use betterpod_rust::{
    motion::{Anim, Ease},
    video::VideoSnapshot,
};

struct Opening {
    item: betterpod_rust::online::OnlineItem,
    queue: Vec<betterpod_rust::online::OnlineItem>,
    index: usize,
    start: f64,
    cover: Option<PathBuf>,
    stopped_full: bool,
}
pub(super) struct VideoVisual {
    controls: Anim,
    flash: Anim,
    until: f64,
    volume_until: f64,
    time: f64,
    seeking: Option<(f64, f64)>,
    seek_loading: Option<f64>,
    skip_at: f64,
    opening_at: f64,
    resolving: bool,
    resolve_error: Option<String>,
    links_until: Option<f64>,
    origin: String,
    start_position: f64,
    desired_paused: bool,
    chapters: usize,
    auto_next: bool,
    seen_playing: bool,
    was_paused: bool,
    closing: Option<VideoSnapshot>,
    opening: Option<Opening>,
    closing_back: Option<PathBuf>,
    closing_is_opening: bool,
    #[cfg(test)]
    fixture: Option<VideoSnapshot>,
    #[cfg(test)]
    phase_epoch: Option<i128>,
}
impl VideoVisual {
    pub fn new() -> Self {
        Self {
            controls: Anim::new(1.),
            flash: Anim::new(0.),
            until: 2.5,
            volume_until: -1.,
            time: 0.,
            seeking: None,
            seek_loading: None,
            skip_at: -100.,
            opening_at: 0.,
            resolving: false,
            resolve_error: None,
            links_until: None,
            origin: String::new(),
            start_position: 0.,
            desired_paused: false,
            chapters: 0,
            auto_next: false,
            seen_playing: false,
            was_paused: false,
            closing: None,
            opening: None,
            closing_back: None,
            closing_is_opening: false,
            #[cfg(test)]
            fixture: None,
            #[cfg(test)]
            phase_epoch: None,
        }
    }
    fn show(&mut self, now: f64) {
        self.until = now + 2.5;
        self.controls.set(1., 0.15, Ease::Css, 0., now);
    }
    fn reset(&mut self, origin: String, now: f64) {
        self.origin = origin;
        self.start_position = 0.;
        self.chapters = 0;
        self.auto_next = false;
        self.opening = None;
        self.closing_back = None;
        self.closing_is_opening = false;
        self.desired_paused = false;
        self.opening_at = now;
        self.resolving = false;
        self.resolve_error = None;
        self.links_until = None;
        self.seeking = None;
        self.seek_loading = None;
        self.seen_playing = false;
        self.was_paused = false;
        self.closing = None;
        self.flash.snap(0.);
        self.show(now);
    }
    fn seek_by(&mut self, position: f64, duration: f64, delta: f64, now: f64) {
        let (mut target, last) = self.seeking.unwrap_or((position, -100.));
        target += if now - last < 0.250 {
            delta * 0.5
        } else {
            delta
        };
        target = target.max(0.);
        if duration > 0. {
            target = target.min((duration - 1.).max(0.));
        }
        self.seeking = Some((target, now));
        self.show(now);
    }
}
impl App {
    #[cfg(test)]
    pub(super) fn visual_video_fixture(&mut self, key: &str, r: &Renderer) -> Result<(), String> {
        if !matches!(key, "video-playing" | "video-paused" | "video-loading") {
            return Err(format!("Unknown video visual fixture {key}"));
        }
        if let Some(texture) = self.video_texture.take() {
            r.delete_texture(texture);
        }
        self.video_visual = VideoVisual::new();
        self.video_visual.phase_epoch = Some(1_793_882_096_000_000_000);
        self.video_title = "A short film".into();
        self.video_origin = "https://www.youtube.com/watch?v=visual-fixture".into();
        self.video_audio_only = false;
        self.video_cover = None;
        self.pending_stream = None;
        self.inline_preview = false;
        self.video_open = true;
        self.video_visual.origin = self.video_origin.clone();
        self.video_visual.seen_playing = key != "video-loading";
        self.video_visual.was_paused = key == "video-paused";
        let loading = key == "video-loading";
        self.video_visual.resolving = loading;
        self.video_visual.opening_at = -1.;
        self.video_visual.fixture = Some(VideoSnapshot {
            playing: !loading,
            paused: key == "video-paused",
            loading,
            position: if loading { 0. } else { 65. },
            duration: 245.,
            title: self.video_title.clone(),
            ..Default::default()
        });
        if !loading {
            let mut pixels = vec![0_u8; 640 * 360 * 4];
            for y in 0..360 {
                for x in 0..640 {
                    let i = (y * 640 + x) * 4;
                    pixels[i] = (x * 255 / 639) as u8;
                    pixels[i + 1] = (y * 255 / 359) as u8;
                    pixels[i + 2] = (96 + ((x / 40 + y / 40) % 2) * 32) as u8;
                    pixels[i + 3] = 255;
                }
            }
            self.video_texture = Some(r.upload(640, 360, &pixels)?);
        }
        self.open(Screen::Video);
        if let Some(page) = self.pages.last_mut() {
            page.title = self.video_title.clone();
        }
        Ok(())
    }
    pub(super) fn visual_video_opening(
        &mut self,
        item: betterpod_rust::online::OnlineItem,
        queue: Vec<betterpod_rust::online::OnlineItem>,
        index: usize,
        start: f64,
    ) {
        let stopped_full =
            self.video_open || (!self.inline_preview && self.pending_stream.is_some());
        if stopped_full {
            self.remember_video();
            if let Some(player) = &self.video {
                player.command(VideoCommand::Stop);
            }
            self.pending_stream = None;
            self.video_open = false;
        }
        self.leave_saver();
        self.video_visual.reset(item.url.clone(), self.visual_time);
        self.video_visual.resolving = true;
        self.video_visual.chapters = item.raw["chapters"].as_array().map_or(0, Vec::len);
        self.video_visual.auto_next = item.raw["_video_auto_next"].as_bool().unwrap_or(false);
        self.video_visual.start_position = start;
        let title = item.title.clone();
        self.video_visual.opening = Some(Opening {
            item,
            queue,
            index,
            start,
            cover: None,
            stopped_full,
        });
        if self.screen() != Screen::Video {
            self.open(Screen::Video);
        }
        if let Some(page) = self.pages.last_mut() {
            page.title = title;
        }
    }
    pub(super) fn visual_video_download_origin(&self) -> &str {
        self.video_visual
            .opening
            .as_ref()
            .map_or(self.video_origin.as_str(), |opening| {
                opening.item.url.as_str()
            })
    }
    pub(super) fn visual_video_download_title(&self) -> &str {
        self.video_visual
            .opening
            .as_ref()
            .map_or(self.video_title.as_str(), |opening| {
                opening.item.title.as_str()
            })
    }
    pub(super) fn visual_video_artwork(&mut self, origin: &str, path: PathBuf) -> bool {
        if let Some(opening) = self
            .video_visual
            .opening
            .as_mut()
            .filter(|opening| opening.item.url == origin)
        {
            opening.cover = Some(path);
            true
        } else {
            false
        }
    }
    pub(super) fn visual_video_resolve_failed(&mut self, origin: &str, error: String) {
        if self.visual_video_download_origin() == origin && self.screen() == Screen::Video {
            self.video_visual.resolving = false;
            self.video_visual.resolve_error = Some(format!("Couldn't open it · {error}"));
            self.video_visual.show(self.visual_time);
        }
    }
    pub(super) fn visual_video_opened(&mut self, now: f64) {
        if self.inline_preview {
            return;
        }
        if self.video_visual.origin != self.video_origin {
            self.video_visual.reset(self.video_origin.clone(), now);
        }
        if let Some(opening) = self.video_visual.opening.take() {
            if opening.item.url == self.video_origin {
                if let Some(cover) = opening.cover {
                    self.video_cover = Some(cover);
                }
            }
        }
        self.video_visual.resolving = false;
        self.video_visual.resolve_error = None;
        self.video_visual.closing = None;
        self.pending_stream_paused |=
            self.video_visual.desired_paused || self.screen() != Screen::Video;
        if let Some((source, _, _)) = &self.pending_stream {
            self.video_visual.chapters = self.video_visual.chapters.max(source.chapters);
            let wall = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0., |d| d.as_secs_f64());
            self.video_visual.links_until = source
                .available_at
                .filter(|at| *at > wall)
                .map(|at| now + at - wall);
        }
        self.video_visual.show(now);
    }
    pub(super) fn visual_video_has_opening(&self) -> bool {
        self.video_visual.opening.is_some()
    }
    pub(super) fn visual_video_auto_next(&self) -> bool {
        self.video_visual.auto_next
    }
    pub(super) fn visual_video_waiting(&self) -> bool {
        self.video_visual.resolving && !self.video_visual.desired_paused
    }
    pub(super) fn visual_video_volume_changed(&mut self, now: f64) {
        if self.screen() == Screen::Video {
            self.video_visual.volume_until = now + 1.5;
            self.video_visual.show(now);
        }
    }
    fn visual_video_snapshot(&self) -> VideoSnapshot {
        #[cfg(test)]
        if let Some(state) = &self.video_visual.fixture {
            return state.clone();
        }
        if self.screen() != Screen::Video {
            if let Some(state) = &self.video_visual.closing {
                return state.clone();
            }
        }
        let mut state = self
            .video
            .as_ref()
            .map(|player| player.snapshot())
            .unwrap_or_default();
        if let Some((source, start, _)) = &self.pending_stream {
            state = VideoSnapshot {
                loading: true,
                paused: self.pending_stream_paused,
                position: *start,
                duration: source.duration,
                title: source.title.clone(),
                ..Default::default()
            };
        }
        if let Some(opening) = &self.video_visual.opening {
            state = VideoSnapshot {
                loading: self.video_visual.resolve_error.is_none(),
                title: opening.item.title.clone(),
                position: opening.start,
                duration: opening.item.seconds,
                ..Default::default()
            };
        }
        if state.duration <= 0. && self.video_visual.opening.is_none() {
            state.duration = self
                .video_queue
                .get(self.video_index)
                .map_or(0., |item| item.seconds);
        }
        if let Some(error) = &self.video_visual.resolve_error {
            state.loading = false;
            state.error = Some(error.clone());
        }
        state
    }
    pub(super) fn visual_video_tick(&mut self, now: f64) {
        self.video_visual.time = now;
        if self.screen() != Screen::Video {
            if self.video_open && !self.inline_preview {
                let state = self.visual_video_snapshot();
                if !state.paused && !state.ended {
                    if let Some(player) = &self.video {
                        player.command(VideoCommand::Pause(true));
                    }
                    self.pending_stream_paused = true;
                    self.video_visual.show(now);
                }
            }
            return;
        }
        let state = self.visual_video_snapshot();
        if let Some((target, at)) = self.video_visual.seeking {
            if now - at >= 0.450 {
                if let Some(player) = &self.video {
                    if player.command(VideoCommand::Seek(target)) {
                        if state.ended {
                            player.command(VideoCommand::Pause(false));
                        }
                        self.video_visual.seeking = None;
                        self.video_visual.seek_loading = Some(now);
                    }
                }
            }
        }
        if let Some(sent) = self.video_visual.seek_loading {
            if !state.loading && now - sent > 0.1 {
                self.video_visual.seek_loading = None;
            }
        }
        if state.paused && !self.video_visual.was_paused {
            self.video_visual.show(now);
        }
        if state.playing && !state.loading && !self.video_visual.seen_playing {
            self.video_visual.seen_playing = true;
            self.video_visual.show(now);
        }
        self.video_visual.was_paused = state.paused;
        let still = self.video_visual.resolving
            || state.paused
            || state.loading
            || state.ended
            || state.error.is_some()
            || self.pending_stream.is_some()
            || self.video_visual.seeking.is_some();
        if !still && now > self.video_visual.until {
            self.video_visual.controls.set(0., 0.3, Ease::Css, 0., now);
        }
    }
    fn visual_video_toggle(&mut self) {
        let now = self.visual_time;
        let state = self.visual_video_snapshot();
        self.video_visual.show(now);
        if state.error.is_some() {
            self.visual_video_retry();
            return;
        }
        if self.video_visual.resolving {
            return;
        }
        if self.pending_stream.is_some() {
            self.pending_stream_paused = !self.pending_stream_paused;
            return;
        }
        if let Some(player) = &self.video {
            if state.ended {
                player.command(VideoCommand::Seek(0.));
                player.command(VideoCommand::Pause(false));
            } else {
                if state.paused {
                    self.video_visual.flash.snap(1.);
                    self.video_visual.flash.set(0., 0.45, Ease::Css, 0.35, now);
                }
                player.command(VideoCommand::Pause(!state.paused));
            }
        }
    }
    pub(super) fn visual_video_restart(&mut self) {
        self.video_visual.seeking = None;
        self.video_visual.show(self.visual_time);
        if let Some((_, start, _)) = self.pending_stream.as_mut() {
            *start = 0.;
            self.pending_stream_paused = false;
            return;
        }
        if let Some(player) = &self.video {
            player.command(VideoCommand::Seek(0.));
            player.command(VideoCommand::Pause(false));
            self.video_visual.seek_loading = Some(self.visual_time);
        }
    }
    fn visual_video_retry(&mut self) {
        if let Some(opening) = &self.video_visual.opening {
            self.extras.resolve_media(
                opening.item.clone(),
                opening.queue.clone(),
                opening.index,
                true,
                opening.start,
            );
            return;
        }
        let state = self.visual_video_snapshot();
        let item = self
            .video_queue
            .get(self.video_index)
            .cloned()
            .unwrap_or_else(|| {
                let mut item = betterpod_rust::online::OnlineItem::new(
                    betterpod_rust::online::Provider::YouTube,
                );
                item.url = self.video_origin.clone();
                item.title = self.video_title.clone();
                item.kind = "video".into();
                item.seconds = state.duration;
                item
            });
        self.extras.resolve_media(
            item,
            self.video_queue.clone(),
            self.video_index,
            true,
            state.position,
        );
    }
    pub(super) fn visual_video_stop(&mut self) {
        if !self.video_open && !self.video_visual.resolving && self.screen() != Screen::Video {
            return;
        }
        self.video_visual.closing = Some(self.visual_video_snapshot());
        self.video_visual.closing_is_opening = self.video_visual.opening.is_some();
        self.video_visual.closing_back = self
            .video_visual
            .opening
            .as_ref()
            .and_then(|opening| opening.cover.clone());
        if let Some(opening) = self.video_visual.opening.take() {
            self.video_visual.resolving = false;
            self.video_visual.resolve_error = None;
            self.extras.cancel_media();
            if opening.stopped_full {
                if let Some(saved) = self.stream_restore.as_mut() {
                    saved.paused = true;
                }
                self.restore_stream_music();
            }
            return;
        }
        self.remember_video();
        self.video_visual.resolving = false;
        self.pending_stream = None;
        self.extras.cancel_media();
        if let Some(player) = &self.video {
            player.command(VideoCommand::Stop);
        }
        self.video_open = false;
        // Full-screen Videos pause the user's music; unlike Listen they do
        // not resume it on close (ij_video.go pauseMusicFor / VideoView.stop).
        if let Some(saved) = self.stream_restore.as_mut() {
            saved.paused = true;
        }
        self.restore_stream_music();
    }
    fn visual_video_skip(&mut self, delta: isize, throttle: bool) {
        let now = self.visual_time;
        if throttle && now - self.video_visual.skip_at <= 0.600 {
            return;
        }
        self.video_visual.skip_at = now;
        if let Some(opening) = &self.video_visual.opening {
            let index = opening.index as isize + delta;
            if index < 0 || index as usize >= opening.queue.len() {
                self.notify("No other video in the list");
                return;
            }
            self.extras.resolve_media(
                opening.queue[index as usize].clone(),
                opening.queue.clone(),
                index as usize,
                true,
                0.,
            );
            return;
        }
        let index = self.video_index as isize + delta;
        if index < 0 || index as usize >= self.video_queue.len() {
            self.notify("No other video in the list");
            return;
        }
        self.video_next(delta);
    }
    pub(super) fn visual_video_media(&mut self, action: Action) -> bool {
        if self.screen() != Screen::Video {
            return false;
        }
        match action {
            Action::MediaPlayPause => self.visual_video_toggle(),
            Action::MediaPlay => {
                let state = self.visual_video_snapshot();
                self.video_visual.desired_paused = false;
                if !self.video_visual.resolving {
                    if state.ended || state.error.is_some() {
                        self.visual_video_toggle();
                    } else if state.paused {
                        self.pending_stream_paused = false;
                        if let Some(player) = &self.video {
                            player.command(VideoCommand::Pause(false));
                        }
                        self.video_visual.flash.snap(1.);
                        self.video_visual
                            .flash
                            .set(0., 0.45, Ease::Css, 0.35, self.visual_time);
                        self.video_visual.show(self.visual_time);
                    }
                }
            }
            Action::MediaUnplug => {
                self.video_visual.desired_paused = true;
                return false;
            }
            Action::MediaPause => {
                self.video_visual.desired_paused = true;
                if !self.video_visual.resolving {
                    self.pending_stream_paused = true;
                    if let Some(player) = &self.video {
                        player.command(VideoCommand::Pause(true));
                    }
                    self.video_visual.show(self.visual_time);
                }
            }
            Action::MediaNext => self.visual_video_skip(1, false),
            Action::MediaPrevious => self.visual_video_skip(-1, false),
            _ => return false,
        }
        true
    }
    pub(super) fn visual_video_handle(&mut self, action: Action) -> bool {
        if self.screen() != Screen::Video || self.options.is_some() {
            return false;
        }
        let now = self.visual_time;
        match action {
            Action::Select => {
                let state = self.visual_video_snapshot();
                if state.playing
                    && !state.paused
                    && !state.loading
                    && state.error.is_none()
                    && self.video_visual.controls.target() == 0.
                {
                    self.video_visual.show(now);
                } else {
                    self.visual_video_toggle();
                }
            }
            Action::PlayPause | Action::Start => self.visual_video_toggle(),
            Action::Left | Action::Right => {
                let state = self.visual_video_snapshot();
                if self.video_visual.opening.is_none()
                    && state.error.is_none()
                    && self.video.is_some()
                {
                    self.video_visual.seek_by(
                        state.position,
                        state.duration,
                        if action == Action::Left { -10. } else { 10. },
                        now,
                    );
                }
            }
            Action::Up | Action::Down | Action::VolumeUp | Action::VolumeDown => {
                let delta = if matches!(action, Action::Up | Action::VolumeUp) {
                    0.04
                } else {
                    -0.04
                };
                let next = (self.settings.volume + delta).clamp(0.01, 1.);
                self.set_volume(next - self.settings.volume);
                self.visual_video_volume_changed(now);
            }
            Action::Next => self.visual_video_skip(1, true),
            Action::Previous => self.visual_video_skip(-1, true),
            Action::Options | Action::SelectHeld => self.visual_video_options(),
            Action::Back => {
                self.visual_video_stop();
                if self.pages.len() > 1 {
                    let previous = self.pages.pop();
                    self.visual_menu_navigation(previous, false);
                }
            }
            _ => {
                if !matches!(action, Action::MenuDown | Action::MenuUp | Action::Quit) {
                    self.video_visual.show(now);
                }
                return false;
            }
        }
        true
    }
    fn visual_video_options(&mut self) {
        let state = self.visual_video_snapshot();
        let mut rows = Vec::new();
        if !self.video_visual.resolving && state.error.is_none() && self.video.is_some() {
            rows.push(Row::new("Watch from the Start", Intent::VideoRestart));
        }
        if self.visual_video_download_origin().starts_with("http://")
            || self.visual_video_download_origin().starts_with("https://")
        {
            rows.push(Row::new("Download", Intent::VideoDownloadMenu));
        }
        if rows.is_empty() {
            return;
        }
        rows.push(Row::new("Cancel", Intent::CloseOptions));
        self.options = Some(video_sheet(self.visual_video_download_title(), rows));
    }
    pub(super) fn visual_video_download_options(&mut self) {
        let mut rows = vec![
            Row::new("As Video", Intent::DownloadMedia(true)),
            Row::new("As Audio Only", Intent::DownloadMedia(false)),
        ];
        let count = self.video_visual.chapters.max(
            self.video_queue
                .get(self.video_index)
                .and_then(|item| item.raw["chapters"].as_array())
                .map_or(0, Vec::len),
        );
        if count >= 2 {
            rows.push(Row::new(
                format!("As an Album ({count} tracks)"),
                Intent::DownloadVideoChapters,
            ));
        }
        rows.push(Row::new("Cancel", Intent::CloseOptions));
        self.options = Some(video_sheet("Download", rows));
    }
    pub(super) fn visual_video_draw(&mut self, r: &mut Renderer, now: f64) {
        let state = self.visual_video_snapshot();
        let dark = self.dark();
        r.rect(0., 0., 640., 480., [0., 0., 0., 1.]);
        let resolving_display = self.video_visual.resolving
            || (self.screen() != Screen::Video
                && self.video_visual.closing_is_opening
                && state.error.is_none());
        if !resolving_display {
            if let Some(texture) = self.video_texture {
                let (w, h) = fit(texture.width, texture.height, 640, 480);
                r.image(
                    texture,
                    (640. - w as f32) / 2.,
                    (480. - h as f32) / 2.,
                    w as f32,
                    h as f32,
                    1.,
                );
            } else {
                self.visual_video_back(r);
            }
        } else {
            self.visual_video_back(r);
        }
        let waiting = state.loading
            || self.pending_stream.is_some()
            || self.video_visual.seek_loading.is_some()
            || (!self.video_visual.seen_playing && !state.playing && !state.ended && !state.paused);
        if let Some(error) = &state.error {
            let error = if error.contains(" · ") {
                error.clone()
            } else {
                format!("Couldn't play it · {error}")
            };
            let (first, second) = error.split_once(" · ").unwrap_or((&error, ""));
            centered_lines(r, 212., &[first, second, "Press [A] to try again"]);
        } else if resolving_display {
            spinner(r, 320., 212., 24., self.video_visual.spinner_step(now));
            centered_lines(
                r,
                260.,
                &[
                    "Opening the video",
                    &format!(
                        "Starting yt-dlp · {} s",
                        (now - self.video_visual.opening_at).max(0.) as u64
                    ),
                ],
            );
        } else if waiting {
            let label = if self
                .video_visual
                .links_until
                .is_some_and(|until| now < until)
            {
                format!(
                    "Waiting for YouTube · {} s",
                    (self.video_visual.links_until.unwrap() - now).ceil() as u64
                )
            } else if self.video_visual.seen_playing && self.video_visual.seek_loading.is_none() {
                "Buffering…".into()
            } else {
                "Loading…".into()
            };
            spinner(r, 320., 224., 24., self.video_visual.spinner_step(now));
            centered_lines(r, 272., &[&label]);
        } else if state.ended {
            centered_lines(r, 224., &["Press [A] to watch again"]);
        } else if state.paused && self.video_visual.seeking.is_none() {
            pause_glyph(r, 320., 240., 1.);
        } else {
            let flash = self.video_visual.flash.value(now) as f32;
            if flash > 0.01 {
                play_glyph(r, 320., 240., flash);
            }
        }
        let opacity = self.video_visual.controls.value(now) as f32;
        if opacity > 0.01 {
            r.push_transform(0., 0., 1., 1., opacity);
            r.gradient_stops(
                0.,
                0.,
                640.,
                80.,
                &[
                    (0., [0., 0., 0., 0.8]),
                    (0.6, [0., 0., 0., 0.55]),
                    (1., [0., 0., 0., 0.]),
                ],
            );
            let title = ellipsize(
                r,
                if state.title.is_empty() {
                    self.visual_video_download_title()
                } else {
                    &state.title
                },
                26.,
                700,
                592.,
            );
            text_shadow(
                r,
                &title,
                24.,
                18. + crate::render::line_box(26.).1,
                26.,
                700,
                [1.; 4],
                592.,
            );
            r.gradient_stops(
                0.,
                400.,
                640.,
                80.,
                &[
                    (0., [0., 0., 0., 0.]),
                    (0.35, [0., 0., 0., 0.6]),
                    (1., [0., 0., 0., 0.8]),
                ],
            );
            if now < self.video_visual.volume_until {
                super::app_visual_menu::draw_volume_icon(
                    r,
                    24.,
                    426.,
                    self.visual_menu.status.headphones,
                    [1.; 4],
                );
                super::app_visual_player::progress(
                    r,
                    &self.visual_assets,
                    68.,
                    424.,
                    548.,
                    self.settings.volume as f64,
                    false,
                    dark,
                );
            } else {
                let position = self.video_visual.seeking.map_or(
                    if state.ended {
                        state.duration
                    } else {
                        state.position
                    },
                    |(target, _)| target,
                );
                let current = video_time(position, state.duration);
                let remaining = if state.duration > 0. {
                    format!(
                        "-{}",
                        video_time((state.duration - position).max(0.), state.duration)
                    )
                } else {
                    "--:--".into()
                };
                let lw = r
                    .text_width_weighted(&current, 24., 700)
                    .max(r.text_width_weighted(&remaining, 24., 700));
                let bx = 44. + lw;
                r.text_baseline(
                    &current,
                    24.,
                    425. + crate::render::line_box(24.).1,
                    24.,
                    700,
                    [1.; 4],
                    lw + 2.,
                );
                let rw = r.text_width_weighted(&remaining, 24., 700);
                r.text_baseline(
                    &remaining,
                    616. - rw,
                    425. + crate::render::line_box(24.).1,
                    24.,
                    700,
                    [1.; 4],
                    rw + 1.,
                );
                super::app_visual_player::progress(
                    r,
                    &self.visual_assets,
                    bx,
                    424.,
                    640. - 2. * bx,
                    if state.duration > 0. {
                        position / state.duration
                    } else {
                        0.
                    },
                    self.video_visual.seeking.is_some(),
                    dark,
                );
            }
            r.pop_transform();
        }
    }
    fn visual_video_back(&mut self, r: &mut Renderer) {
        let path = if self.screen() != Screen::Video && self.video_visual.closing_is_opening {
            self.video_visual.closing_back.clone()
        } else {
            self.video_visual
                .opening
                .as_ref()
                .and_then(|opening| opening.cover.clone())
                .or_else(|| {
                    if self.video_visual.opening.is_none() {
                        self.video_cover.clone()
                    } else {
                        None
                    }
                })
        };
        if let Some(path) = path.as_deref() {
            let texture = self.art.get(Some(path));
            if texture.id != self.art.placeholder.id {
                let (w, h) = fit(texture.width, texture.height, 640, 360);
                let (x, y) = ((640. - w as f32) / 2., (480. - h as f32) / 2.);
                r.image(texture, x, y, w as f32, h as f32, 1.);
                r.rect(x, y, w as f32, h as f32, [0., 0., 0., 0.55]);
            }
        }
    }
}
impl VideoVisual {
    fn spinner_step(&self, now: f64) -> usize {
        #[cfg(test)]
        if let Some(epoch) = self.phase_epoch {
            return (((epoch + (now * 1e9).round() as i128).rem_euclid(833_333_333) * 12)
                / 833_333_333) as usize;
        }
        let _ = now;
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        ((ns % 833_333_333) * 12 / 833_333_333) as usize
    }
}
fn video_sheet(title: &str, rows: Vec<Row>) -> Page {
    Page {
        screen: Screen::Video,
        kind: ViewKind::Full,
        mini: false,
        title: title.into(),
        rows: Arc::new(rows),
        selected: 0,
        scroll: 0.,
        target_scroll: 0.,
        enter: 0.,
    }
}
fn fit(width: u32, height: u32, box_width: u32, box_height: u32) -> (u32, u32) {
    let (w, h) = if width == 0 || height == 0 {
        (16_u64, 9_u64)
    } else {
        (width as u64, height as u64)
    };
    let (mut fw, mut fh) = (box_width as u64, box_width as u64 * h / w);
    if fh > box_height as u64 {
        fw = box_height as u64 * w / h;
        fh = box_height as u64;
    }
    ((fw as u32 & !1).max(2), (fh as u32 & !1).max(2))
}
fn video_time(seconds: f64, duration: f64) -> String {
    let s = if seconds.is_finite() {
        seconds.max(0.) as u64
    } else {
        0
    };
    if duration >= 3600. || s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}
fn ellipsize(r: &mut Renderer, text: &str, size: f32, weight: u16, width: f32) -> String {
    let chars = text.chars().take(4096).collect::<Vec<_>>();
    let full = chars.iter().collect::<String>();
    if r.text_width_weighted(&full, size, weight) <= width {
        return full;
    }
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let candidate = format!("{}…", chars[..mid].iter().collect::<String>());
        if r.text_width_weighted(&candidate, size, weight) <= width {
            lo = mid
        } else {
            hi = mid - 1
        }
    }
    format!("{}…", chars[..lo].iter().collect::<String>().trim_end())
}
fn text_shadow(
    r: &mut Renderer,
    text: &str,
    x: f32,
    y: f32,
    size: f32,
    weight: u16,
    color: Color,
    width: f32,
) {
    for (dx, dy) in [(-1., 0.), (1., 0.), (0., -1.), (0., 1.)] {
        r.text_baseline(
            text,
            x + dx,
            y + dy,
            size,
            weight,
            [0., 0., 0., 0.35],
            width,
        );
    }
    r.text_baseline(text, x, y, size, weight, color, width);
}
fn centered_lines(r: &mut Renderer, mut y: f32, lines: &[&str]) {
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let (size, weight) = if i == 0 { (28., 700) } else { (24., 500) };
        let text = ellipsize(r, line, size, weight, 600.);
        let width = r.text_width_weighted(&text, size, weight);
        let (height, ascent) = crate::render::line_box(size);
        text_shadow(
            r,
            &text,
            (640. - width) / 2.,
            y + ascent,
            size,
            weight,
            [1.; 4],
            600.,
        );
        y += height + 4.;
    }
}
fn disc(r: &mut Renderer, x: f32, y: f32, alpha: f32) {
    r.rounded_rect(x - 48., y - 48., 96., 96., 48., [0., 0., 0., 0.45 * alpha]);
}
fn pause_glyph(r: &mut Renderer, x: f32, y: f32, alpha: f32) {
    disc(r, x, y, alpha);
    for xx in [x - 16., x + 4.] {
        r.rounded_gradient(
            xx,
            y - 20.,
            12.,
            40.,
            4.,
            &[
                (0., [1., 1., 1., 0.95 * alpha]),
                (1., [225. / 255., 225. / 255., 225. / 255., 0.95 * alpha]),
            ],
        );
    }
}
fn play_glyph(r: &mut Renderer, x: f32, y: f32, alpha: f32) {
    disc(r, x, y, alpha);
    r.triangle(
        [[x - 12., y - 22.], [x - 12., y + 22.], [x + 24., y]],
        [1., 1., 1., 0.95 * alpha],
    );
}
fn spinner(r: &mut Renderer, cx: f32, cy: f32, size_css: f32, step: usize) {
    let scale = size_css * 2. / 2400.;
    let stroke = 200. * scale;
    for (i, alpha) in [
        1., 0.917, 0.833, 0.75, 0.667, 0.583, 0.5, 0.417, 0.333, 0.25, 0.167, 0.083,
    ]
    .into_iter()
    .enumerate()
    {
        let angle = (-(i as f32) * 30. + step as f32 * 30.).to_radians();
        let (c, s) = (angle.cos(), angle.sin());
        r.push_projective([[c, -s, cx], [s, c, cy], [0., 0., 1.]], 1.);
        r.rounded_rect(
            -stroke / 2.,
            -1100. * scale - stroke / 2.,
            stroke,
            500. * scale + stroke,
            stroke / 2.,
            [1., 1., 1., alpha],
        );
        r.pop_transform();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_video_layout_is_even_and_centred() {
        assert_eq!(fit(1920, 1080, 640, 480), (640, 360));
        assert_eq!(fit(1080, 1920, 640, 480), (270, 480));
        assert_eq!(fit(0, 0, 640, 480), (640, 360));
    }
    #[test]
    fn seek_accumulates_held_steps_and_clamps_before_the_last_second() {
        let mut v = VideoVisual::new();
        v.seek_by(30., 60., 10., 1.);
        assert_eq!(v.seeking.unwrap().0, 40.);
        v.seek_by(30., 60., 10., 1.1);
        assert_eq!(v.seeking.unwrap().0, 45.);
        v.seek_by(30., 60., 10., 1.5);
        assert_eq!(v.seeking.unwrap().0, 55.);
        v.seek_by(30., 60., 10., 2.);
        assert_eq!(v.seeking.unwrap().0, 59.);
    }
    #[test]
    fn source_video_time_uses_duration_width() {
        assert_eq!(video_time(65.9, 245.), "01:05");
        assert_eq!(video_time(65., 3600.), "0:01:05");
        assert_eq!(video_time(-1., 0.), "00:00");
    }
}

#[cfg(test)]
#[path = "app_visual_video_qa.rs"]
mod qa;
