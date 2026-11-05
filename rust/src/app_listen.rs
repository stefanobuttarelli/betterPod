//! Inline Listen status and source cache bridge. All IO lives in ListenClient.
use super::*;
use betterpod_rust::{
    listen::{Phase, Snapshot},
    video::VideoSnapshot,
};
#[derive(Debug, PartialEq)]
pub(super) enum RowTap {
    Ignore,
    Toggle,
    Retry,
}
pub(super) fn row_tap(ready: bool, state: Option<&VideoSnapshot>) -> RowTap {
    match state {
        Some(state) if state.error.is_some() => RowTap::Retry,
        Some(state) if ready && !state.loading => RowTap::Toggle,
        _ => RowTap::Ignore,
    }
}
fn accepts_transport(ready: bool, floor: u64, generation: u64, failed: bool) -> bool {
    ready && generation > floor && !failed
}
impl Extras {
    #[cfg(test)]
    pub(in crate::app) fn qa_listen_page(&mut self, items: Vec<OnlineItem>) {
        self.stop_preview();
        self.keyboard = None;
        self.pages.clear();
        let rows = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut row = Item::new(
                    item.title.clone(),
                    Choice::Preview(item.clone(), items.clone(), index),
                );
                row.visual.no_arrow = true;
                row
            })
            .collect();
        self.pages.push(Page::new("Listen fixture", rows));
    }
    #[cfg(test)]
    pub(in crate::app) fn qa_listen_worker_phase(&self) -> Phase {
        self.listen.snapshot().phase
    }
    pub(in crate::app) fn preview_can_open(&self, origin: &str) -> bool {
        self.preview_origin.as_deref() == Some(origin)
            && self.preview_ready
            && self
                .preview_state
                .as_ref()
                .is_some_and(|state| state.error.is_none())
    }
    pub(in crate::app) fn preview_cache_snapshot(&self) -> Snapshot {
        self.listen_snapshot.clone()
    }
    pub(in crate::app) fn preview_row_state(&self) -> Option<VideoSnapshot> {
        self.preview_state.clone()
    }
    pub(super) fn start_inline_listen(
        &mut self,
        item: OnlineItem,
        mut queue: Vec<OnlineItem>,
        mut index: usize,
    ) {
        let retry = self.preview_origin.as_ref() == Some(&item.url)
            && self
                .preview_state
                .as_ref()
                .is_some_and(|s| s.error.is_some());
        if queue
            .get(index)
            .is_none_or(|candidate| candidate.url != item.url)
        {
            queue = vec![item.clone()];
            index = 0;
        }
        self.cancel_media();
        self.preview_origin = Some(item.url.clone());
        if self.preview_depth == 0 {
            self.preview_depth = self.pages.len();
        }
        self.preview_queue = queue.clone();
        self.preview_ready = false;
        self.preview_state = Some(VideoSnapshot {
            loading: true,
            title: item.title.clone(),
            duration: item.seconds,
            ..Default::default()
        });
        self.active_media_art = Some((item.url.clone(), item.cover.clone()));
        self.request_preview(item.cover.clone());
        self.effects.push(Effect::PreviewOpening {
            item,
            queue: queue.clone(),
            index,
        });
        let result = if retry {
            self.listen.retry(queue, index)
        } else {
            self.listen.start(queue, index)
        };
        if let Err(error) = result {
            self.preview_failed(error);
        }
        self.listen_snapshot = self.listen.snapshot();
        self.invalidate_visual_rows();
    }
    pub(in crate::app) fn preview_transport_starting(&mut self, generation: u64) {
        self.preview_decoder_floor = generation;
    }
    pub(in crate::app) fn preview_transport_accepts(&self, generation: u64) -> bool {
        accepts_transport(
            self.preview_ready,
            self.preview_decoder_floor,
            generation,
            self.preview_state
                .as_ref()
                .is_some_and(|state| state.error.is_some()),
        )
    }
    pub(in crate::app) fn preview_waiting(&self) -> bool {
        self.preview_origin.is_some()
            && self
                .preview_state
                .as_ref()
                .is_some_and(|state| state.loading && state.error.is_none() && !state.paused)
    }
    pub(in crate::app) fn preview_set_paused(&mut self, paused: bool) {
        if let Some(state) = self.preview_state.as_mut() {
            state.paused = paused;
        }
        self.invalidate_visual_rows();
    }
    pub(in crate::app) fn preview_failed(&mut self, error: String) {
        self.preview_ready = false;
        if let Some(state) = self.preview_state.as_mut() {
            state.loading = false;
            state.playing = false;
            state.error = Some(error);
        }
        self.invalidate_visual_rows();
    }
    pub(in crate::app) fn advance_preview(&mut self) {
        if self.preview_origin.is_none() {
            return;
        }
        self.preview_ready = false;
        if let Some(state) = self.preview_state.as_mut() {
            state.loading = true;
            state.playing = false;
        }
        if let Err(error) = self.listen.advance() {
            self.preview_failed(error);
        }
        self.invalidate_visual_rows();
    }
    pub(super) fn poll_inline_listen(&mut self) {
        if self.preview_origin.is_none() {
            return;
        }
        let snapshot = self.listen.snapshot();
        let changed = self.listen_snapshot.generation != snapshot.generation
            || self.listen_snapshot.index != snapshot.index
            || self.listen_snapshot.part != snapshot.part
            || self.listen_snapshot.phase != snapshot.phase
            || self.listen_snapshot.done != snapshot.done
            || self.listen_snapshot.total != snapshot.total
            || self.listen_snapshot.error != snapshot.error;
        if changed {
            self.listen_snapshot = snapshot.clone();
            match snapshot.phase {
                Phase::Ended => {
                    self.stop_preview();
                    return;
                }
                Phase::Failed => self.preview_failed(
                    snapshot
                        .error
                        .clone()
                        .unwrap_or_else(|| "Can't play it".into()),
                ),
                Phase::Loading => {
                    let paused = self
                        .preview_state
                        .as_ref()
                        .is_some_and(|state| state.paused);
                    if let Some(item) = self.preview_queue.get(snapshot.index) {
                        self.preview_origin = Some(item.url.clone());
                        self.preview_state = Some(VideoSnapshot {
                            loading: true,
                            paused,
                            title: item.title.clone(),
                            duration: snapshot.duration,
                            position: snapshot.offset,
                            ..Default::default()
                        });
                    }
                }
                _ => {}
            }
            self.invalidate_visual_rows();
        }
        if let Some(ready) = self.listen.take_ready() {
            let Some(item) = self.preview_queue.get(ready.index).cloned() else {
                return;
            };
            self.preview_origin = Some(item.url.clone());
            self.listen_snapshot = Snapshot {
                offset: ready.offset,
                duration: ready.duration,
                index: ready.index,
                part: ready.part,
                ..snapshot
            };
            self.preview_ready = true;
            let paused = self
                .preview_state
                .as_ref()
                .is_some_and(|state| state.paused);
            self.preview_state = Some(VideoSnapshot {
                loading: true,
                paused,
                title: item.title.clone(),
                duration: ready.duration,
                position: ready.offset,
                ..Default::default()
            });
            self.active_media_art = Some((item.url.clone(), item.cover.clone()));
            self.effects.push(Effect::Preview {
                source: ready.source,
                origin: item.url,
                queue: self.preview_queue.clone(),
                index: ready.index,
            });
            self.emit_media_art();
            self.invalidate_visual_rows();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_ready_current_generation_can_end_or_fail_a_preview() {
        assert!(!accepts_transport(false, 8, 7, false));
        assert!(!accepts_transport(false, 8, 9, false));
        assert!(!accepts_transport(true, 8, 8, false));
        assert!(!accepts_transport(true, 8, 7, false));
        assert!(accepts_transport(true, 8, 9, false));
        assert!(!accepts_transport(true, 8, 9, true));
    }
    #[test]
    fn source_row_a_ignores_loading_and_retries_errors() {
        assert_eq!(row_tap(false, None), RowTap::Ignore);
        assert_eq!(
            row_tap(
                true,
                Some(&VideoSnapshot {
                    loading: true,
                    ..Default::default()
                })
            ),
            RowTap::Ignore
        );
        assert_eq!(
            row_tap(
                true,
                Some(&VideoSnapshot {
                    playing: true,
                    ..Default::default()
                })
            ),
            RowTap::Toggle
        );
        assert_eq!(
            row_tap(
                false,
                Some(&VideoSnapshot {
                    error: Some("offline".into()),
                    ..Default::default()
                })
            ),
            RowTap::Retry
        );
    }
}
