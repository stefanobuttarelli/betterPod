//! Playback ownership for full-screen video and in-list Listen previews.
use super::*;
impl App {
    pub(super) fn begin_inline_preview(
        &mut self,
        item: betterpod_rust::online::OnlineItem,
        queue: Vec<betterpod_rust::online::OnlineItem>,
        index: usize,
    ) {
        self.leave_saver();
        self.pending_music = None;
        self.pending_stream = None;
        self.pending_stream_paused = false;
        if self.stream_restore.is_none() {
            self.stream_restore = Some(SavedPlayback {
                queue: Arc::clone(&self.queue),
                index: self.queue_index,
                position: if self.restored {
                    self.restored_position
                } else {
                    self.audio_snapshot.position
                },
                paused: !(self.audio_snapshot.playing
                    || self.audio_snapshot.loading
                    || self.idle_play_pending.is_some()),
            });
        }
        self.send(AudioCommand::Pause);
        if let Some(video) = self.video.as_ref() {
            self.extras
                .preview_transport_starting(video.snapshot().generation);
            video.command(VideoCommand::Stop);
        }
        self.video_queue = queue;
        self.video_index = index;
        self.video_origin = item.url;
        self.video_title = item.title;
        self.video_audio_only = true;
        self.video_open = false;
        self.inline_preview = true;
    }
    pub(super) fn inline_preview_failed(&mut self, error: String) {
        self.pending_stream = None;
        if let Some(player) = self.video.as_ref() {
            player.command(VideoCommand::Stop);
        }
        self.extras.preview_failed(error);
    }
    pub(super) fn pause_media(&mut self) {
        self.pending_stream_paused = true;
        if self.inline_preview {
            self.extras.preview_set_paused(true);
        }
        if let Some(saved) = self.stream_restore.as_mut() {
            saved.paused = true;
        }
        if let Some(saved) = self.quiz_restore.as_mut() {
            saved.paused = true;
        }
        if let Some(video) = self.video.as_ref() {
            if self.inline_preview || self.video_open || self.pending_stream.is_some() {
                video.command(VideoCommand::Pause(true));
            }
        }
        if self.audio_snapshot.playing
            || self.audio_snapshot.loading
            || self.idle_play_pending.is_some()
        {
            self.send(AudioCommand::Pause);
            self.save_playback();
        }
    }
    pub(super) fn begin_stream(
        &mut self,
        source: StreamSource,
        video: bool,
        origin: String,
        start: f64,
        queue: Vec<betterpod_rust::online::OnlineItem>,
        index: usize,
        inline: bool,
        renderer: &Renderer,
    ) -> Result<(), String> {
        self.leave_saver();
        self.pending_music = None;
        if !inline {
            self.pending_stream_paused = false;
        }
        if self.video.is_none() {
            self.video = Some(VideoPlayer::with_data(self.data_dir.clone())?);
        }
        if inline {
            self.extras.preview_transport_starting(
                self.video
                    .as_ref()
                    .map_or(0, |video| video.snapshot().generation),
            );
        } else if self.inline_preview {
            // Go stopListenFor: opening a video must not resume the saved song.
            self.extras.stop_preview();
            self.inline_preview = false;
            if let Some(saved) = self.stream_restore.as_mut() {
                saved.paused = true;
            }
        }
        self.remember_video();
        if self.stream_restore.is_none() {
            self.stream_restore = Some(SavedPlayback {
                queue: Arc::clone(&self.queue),
                index: self.queue_index,
                position: if self.restored {
                    self.restored_position
                } else {
                    self.audio_snapshot.position
                },
                paused: !self.audio_snapshot.playing,
            });
        }
        self.send(AudioCommand::Pause);
        if self.video_open || self.inline_preview {
            if let Some(player) = self.video.as_ref() {
                player.command(VideoCommand::Stop);
            }
        }
        if let Some(texture) = self.video_texture.take() {
            renderer.delete_texture(texture);
        }
        self.video_queue = queue;
        self.video_index = index;
        self.video_cover = None;
        self.video_title = source.title.clone();
        self.video_origin = origin;
        self.video_audio_only = !video;
        self.inline_preview = inline;
        self.pending_stream = Some((source, start, !video));
        self.video_overlay = Instant::now();
        if !inline {
            if !self.pages.iter().any(|page| page.screen == Screen::Video) {
                self.open(Screen::Video);
            }
            if let Some(page) = self
                .pages
                .iter_mut()
                .rfind(|page| page.screen == Screen::Video)
            {
                page.title = if video {
                    self.video_title.clone()
                } else {
                    "Listen".into()
                };
            }
            self.visual_video_opened(self.visual_time);
        }
        Ok(())
    }
    pub(super) fn stop_inline_preview(&mut self) {
        if !self.inline_preview {
            return;
        }
        self.inline_preview = false;
        self.pending_stream = None;
        if let Some(player) = self.video.as_ref() {
            player.command(VideoCommand::Stop);
        }
        self.video_open = false;
        self.extras.stop_preview();
        self.restore_stream_music();
    }
}
