//! Snapshot only; commands use the ordinary App::handle input path.
use super::*;
use serde_json::{json, Value};
impl App {
    pub(crate) fn remote_qa_state(&mut self) -> Value {
        let audio = self.audio_telemetry();
        let page = if let Some(p) = self.options.as_ref() {
            Some(p.clone())
        } else {
            match self.screen() {
                Screen::Extra => self
                    .extras
                    .visual_options()
                    .or_else(|| self.extras.visual_page()),
                Screen::Files => Some(self.files.visual_page()),
                Screen::Curation => self.curation.visual_page(),
                Screen::Maintenance => self.maintenance.as_mut().and_then(|m| m.visual_page()),
                _ => self.pages.last().cloned(),
            }
        };
        json!({"screen":format!("{:?}",self.screen()),"stack":self.pages.iter().map(|p|format!("{:?}",p.screen)).collect::<Vec<_>>(),
            "page":page.as_ref().map(|p|json!({"title":p.title,"kind":format!("{:?}",p.kind),"selected":p.selected,"rows":p.rows.iter().map(|r|json!({"label":r.label,"detail":r.detail,"value":r.visual.value,"icon":r.visual.icon})).collect::<Vec<_>>() })),
            "options":self.options.is_some(),"keyboard":self.keyboard.is_some()||self.extras.is_typing(),"held":self.held,"screen_off":self.screen_off,"saver":self.auto_saver,
            "weather_icons":betterpod_rust::weather::icon_cache_diagnostics(),
            "playing":self.audio_snapshot.playing,"paused":self.audio_snapshot.paused,"position":self.audio_snapshot.position,"tracks":self.library.tracks.len(),"albums":self.library.albums.len(),
            "audio":json!({"callbacks":audio.callbacks,"last_interval_ms":audio.last_interval_ms,"max_interval_ms":audio.max_interval_ms,"rate":audio.rate,"chunk_frames":audio.chunk_frames,"pcm_underruns":audio.underruns,"device_statuses":self.audio.diagnostic_device_statuses(),"driver":self.audio.diagnostic_driver(),"path":self.audio_snapshot.path.as_ref().map(|p|p.to_string_lossy()),"error":self.audio_snapshot.error.as_deref(),"loading":self.audio_snapshot.loading,"duration":self.audio_snapshot.duration}),
            "preview_focus":self.preview_focus,"preview_selected":self.preview_selected,"preview_rows":self.preview_rows.iter().map(|r|json!({"label":r.label,"detail":r.detail,"value":r.visual.value})).collect::<Vec<_>>(),
            "elapsed":self.visual_time,"video":self.video.as_ref().map(|v|{let s=v.snapshot();let t=v.telemetry();json!({"playing":s.playing,"paused":s.paused,"loading":s.loading,"position":s.position,"duration":s.duration,"error":s.error,"frames":t.frames,"frame_pts":t.frame_pts,"underruns":s.underruns})}),
            "np_mode":self.np_mode,"inline_preview":self.inline_preview,"video_open":self.video_open,"menu":self.visual_menu_remote_state()})
    }
}
