//! One user-requested foreground operation owns the source row until it ends.
//! Downloads, Listen and passive artwork jobs keep their separate lifecycles.
use super::*;

pub(super) struct Foreground {
    id: u64,
    ids: Vec<u64>,
    owner: u64,
    target: u64,
    row: usize,
    generation: u64,
    purpose: Purpose,
}

#[cfg(test)]
mod write_tests;

impl Extras {
    pub(in crate::app) fn service_foreground_token(&self) -> Option<u64> {
        self.foreground
            .as_ref()
            .and_then(|pending| pending.ids.first().copied())
    }
    pub(in crate::app) fn service_write_result(&self, token: u64) -> Option<bool> {
        self.service_write_result
            .filter(|(id, _)| *id == token)
            .map(|(_, success)| success)
    }
    pub(in crate::app) fn dismiss_service_source_view(&mut self) {
        if self
            .pages
            .last()
            .is_some_and(|page| page.title == "Album Changes")
        {
            self.pages.pop();
        }
    }
    pub(in crate::app) fn cancel_service_foreground(&mut self, token: u64) {
        if self.foreground_job(token) {
            self.cancel_foreground();
            self.dismiss_service_source_view();
        }
    }
    fn grouped_purpose(purpose: &Purpose) -> Option<u64> {
        match purpose {
            Purpose::Combined { generation } | Purpose::CoverContext { generation } => {
                Some(*generation)
            }
            _ => None,
        }
    }
    fn joins_foreground(&self, purpose: &Purpose) -> bool {
        let Some(generation) = Self::grouped_purpose(purpose) else {
            return false;
        };
        self.foreground.as_ref().is_some_and(|pending| {
            Self::grouped_purpose(&pending.purpose) == Some(generation)
                && pending.generation == self.generation
                && self
                    .pages
                    .last()
                    .is_some_and(|page| page.key == pending.owner)
        })
    }
    pub(super) fn prepare_foreground_purpose(&mut self, purpose: &Purpose) -> Option<(u64, usize)> {
        if self.joins_foreground(purpose) {
            self.foreground_busy_row()
        } else {
            self.prepare_foreground_row()
        }
    }
    pub(super) fn foreground_busy_row(&self) -> Option<(u64, usize)> {
        self.foreground
            .as_ref()
            .map(|pending| (pending.owner, pending.row))
    }
    pub(super) fn mutating_foreground(purpose: &Purpose) -> bool {
        matches!(
            purpose,
            Purpose::Tags
                | Purpose::WriteTags { .. }
                | Purpose::Merge
                | Purpose::Separate
                | Purpose::CoverReset
                | Purpose::Cover
        )
    }
    pub(super) fn complete_foreground_job(
        &mut self,
        id: u64,
        owner: Option<u64>,
        purpose: &Purpose,
        failed: bool,
    ) -> bool {
        let accepted = !Self::foreground_purpose(purpose) || self.finish_foreground(id, owner);
        if accepted && Self::mutating_foreground(purpose) {
            self.service_write_result = Some((id, !failed));
        }
        if Self::mutating_foreground(purpose) && (!accepted || failed) {
            self.effects.push(Effect::Rescan);
        }
        if matches!(purpose, Purpose::ToolAction) && (!accepted || failed) {
            self.task(Task::ToolsStatus, Purpose::Tools);
        }
        accepted
    }
    pub(super) fn foreground_submission_failed(&mut self, purpose: &Purpose, error: &str) {
        if let Some(generation) = Self::grouped_purpose(purpose) {
            if let Some(state) = self
                .combined
                .as_mut()
                .filter(|state| state.generation == generation)
            {
                state.pending = if matches!(purpose, Purpose::CoverContext { .. }) {
                    0
                } else {
                    state.pending.saturating_sub(1)
                };
                state.errors.push(error.into());
            }
            self.refresh_combined();
        }
        if matches!(
            purpose,
            Purpose::WriteTags { .. } | Purpose::ReadTags { .. }
        ) {
            if let Some(edit) = self.album_edit.as_mut() {
                edit.loading = false;
                edit.saving = false;
                edit.status = error.into();
            }
            self.refresh_editor_page();
        }
    }
    pub(super) fn prepare_foreground_row(&mut self) -> Option<(u64, usize)> {
        self.cancel_foreground();
        let page = self.pages.last_mut()?;
        let row = page.selected;
        if let Some(item) = page.rows.get_mut(row) {
            item.visual.busy = true;
        }
        page.visual_rows = None;
        Some((page.key, row))
    }
    pub(super) fn clear_prepared_foreground_row(&mut self, prepared: Option<(u64, usize)>) {
        if let Some((owner, row)) = prepared {
            if let Some(page) = self.pages.iter_mut().find(|page| page.key == owner) {
                if let Some(item) = page.rows.get_mut(row) {
                    item.visual.busy = false;
                }
                page.visual_rows = None;
            }
        }
    }
    pub(in crate::app) fn initial_album_read_token(&self) -> Option<u64> {
        if !self.album_edit.as_ref().is_some_and(|edit| edit.loading) {
            return None;
        }
        self.foreground
            .as_ref()
            .filter(|pending| {
                matches!(pending.purpose, Purpose::ReadTags { .. })
                    && self
                        .jobs
                        .get(&pending.id)
                        .is_some_and(|(handle, _)| !handle.is_cancelled())
            })
            .map(|pending| pending.id)
    }
    #[cfg(test)]
    pub(in crate::app) fn initial_album_read_pending(&self) -> bool {
        self.initial_album_read_token().is_some()
    }
    pub(in crate::app) fn cancel_initial_album_read(&mut self) {
        if self.initial_album_read_token().is_none() {
            return;
        }
        self.cancel_foreground();
        self.generation = self.generation.wrapping_add(1);
        self.album_edit = None;
        self.edit_paths.clear();
        self.pages.clear();
        self.pending_navigation = None;
        self.status.clear();
        self.status_job = None;
        self.status_page = None;
    }
    pub(super) fn foreground_purpose(purpose: &Purpose) -> bool {
        matches!(
            purpose,
            Purpose::Details { .. }
                | Purpose::Feed { .. }
                | Purpose::Search { .. }
                | Purpose::ReadTags { .. }
                | Purpose::InfoPreview { .. }
                | Purpose::Tags
                | Purpose::WriteTags { .. }
                | Purpose::Merge
                | Purpose::Separate
                | Purpose::CoverReset
                | Purpose::Cover
                | Purpose::ToolAction
                | Purpose::Combined { .. }
                | Purpose::CoverContext { .. }
                | Purpose::YouTubeCover { .. }
        )
    }
    pub(super) fn claim_foreground(&mut self, id: u64, owner: u64, purpose: Purpose) {
        if self.joins_foreground(&purpose)
            && self
                .foreground
                .as_ref()
                .is_some_and(|pending| pending.target == owner)
        {
            if let Some(pending) = self.foreground.as_mut() {
                if !pending.ids.contains(&id) {
                    pending.ids.push(id);
                }
            }
            return;
        }
        self.cancel_foreground();
        if let Purpose::Search {
            provider, metadata, ..
        } = &purpose
        {
            if let Some(state) = self
                .searches
                .iter_mut()
                .find(|state| state.provider == *provider && state.metadata == *metadata)
            {
                state.busy = true;
            }
        }
        let target = owner;
        let owner = self.pages.last().map_or(owner, |page| page.key);
        let Some(page) = self.pages.iter_mut().find(|page| page.key == owner) else {
            return;
        };
        let row = page.selected;
        if let Some(item) = page.rows.get_mut(row) {
            item.visual.busy = true;
        }
        page.visual_rows = None;
        self.foreground = Some(Foreground {
            id,
            ids: vec![id],
            owner,
            target,
            row,
            generation: self.generation,
            purpose,
        });
    }
    pub(super) fn foreground_job(&self, id: u64) -> bool {
        self.foreground
            .as_ref()
            .is_some_and(|pending| pending.ids.contains(&id))
    }
    fn clear_foreground_row(&mut self, pending: &Foreground) {
        if let Some(page) = self.pages.iter_mut().find(|page| page.key == pending.owner) {
            if let Some(item) = page.rows.get_mut(pending.row) {
                item.visual.busy = false;
            }
            page.visual_rows = None;
        }
    }
    pub(super) fn cancel_foreground(&mut self) {
        let Some(pending) = self.foreground.take() else {
            return;
        };
        for id in &pending.ids {
            if let Some((handle, _)) = self.jobs.get(id) {
                handle.cancel();
            }
        }
        self.clear_foreground_row(&pending);
        if Self::mutating_foreground(&pending.purpose) {
            if let Some(edit) = self.album_edit.as_mut() {
                edit.saving = false;
            }
            if let Some(page) = self.pages.iter_mut().find(|page| page.key == pending.owner) {
                if let Some(item) = page.rows.get_mut(pending.row) {
                    item.detail = "Cancelled · completed file changes are kept".into();
                }
                page.visual_rows = None;
            }
        }
        if Self::grouped_purpose(&pending.purpose).is_some() {
            if let Some(state) = self.combined.as_mut() {
                state.pending = 0;
            }
            self.refresh_combined();
        }
        if let Purpose::Search {
            provider, metadata, ..
        } = pending.purpose
        {
            if let Some(state) = self
                .searches
                .iter_mut()
                .find(|state| state.provider == provider && state.metadata == metadata)
            {
                state.busy = false;
            }
            self.update_search_page(pending.target, provider, metadata, false);
        }
        if self.status_job.is_some_and(|id| pending.ids.contains(&id)) {
            self.status.clear();
            self.status_job = None;
            self.status_page = None;
        }
    }
    pub(super) fn finish_foreground(&mut self, id: u64, owner: Option<u64>) -> bool {
        if !self
            .foreground
            .as_ref()
            .is_some_and(|pending| pending.ids.contains(&id))
        {
            return false;
        }
        let mut pending = self.foreground.take().unwrap();
        let accepts = owner == Some(pending.target)
            && pending.generation == self.generation
            && self
                .pages
                .last()
                .is_some_and(|page| page.key == pending.owner)
            && self.pages.iter().any(|page| page.key == pending.target);
        pending.ids.retain(|other| *other != id);
        if accepts && !pending.ids.is_empty() {
            self.foreground = Some(pending);
        } else {
            for other in &pending.ids {
                if let Some((handle, _)) = self.jobs.get(other) {
                    handle.cancel();
                }
            }
            self.clear_foreground_row(&pending);
        }
        accepts
    }
    pub(super) fn foreground_error(&mut self, id: u64, error: &str) {
        let Some(pending) = self
            .foreground
            .as_ref()
            .filter(|pending| pending.ids.contains(&id))
        else {
            return;
        };
        if pending.generation != self.generation
            || self
                .pages
                .last()
                .is_none_or(|page| page.key != pending.owner)
        {
            return;
        }
        if let Some(page) = self.pages.iter_mut().find(|page| page.key == pending.owner) {
            if let Some(item) = page.rows.get_mut(pending.row) {
                item.detail = format!("{error} · press [A] to try again");
            }
            page.visual_rows = None;
        }
    }
    pub(super) fn handle_foreground(&mut self, action: Action) -> bool {
        if self.foreground.is_none() {
            return false;
        }
        match action {
            Action::Back | Action::BackHeld => {
                self.cancel_foreground();
                true
            }
            Action::Up
            | Action::Down
            | Action::Left
            | Action::Right
            | Action::Select
            | Action::SelectHeld
            | Action::Options
            | Action::Search
            | Action::MenuDown
            | Action::MenuUp => true,
            _ => false,
        }
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
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "bp-foreground-{}-{}",
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
    fn detail() -> Purpose {
        Purpose::Details {
            metadata: false,
            generation: 0,
        }
    }
    #[test]
    fn foreground_keeps_the_source_list_blocks_navigation_and_back_releases_only_its_row() {
        let mut fixture = Fixture::new();
        let extras = &mut fixture.extras;
        extras.push(
            "YouTube",
            vec![
                Item::new("First result", Choice::Nothing),
                Item::new("Second result", Choice::Nothing),
            ],
        );
        let owner = extras.pages.last().unwrap().key;
        let foreground = extras.services.submit(Task::Downloaded).unwrap();
        let background = extras.services.submit(Task::Downloaded).unwrap();
        extras
            .jobs
            .insert(foreground.id, (foreground.clone(), detail()));
        extras
            .jobs
            .insert(background.id, (background.clone(), Purpose::Downloads));
        extras.claim_foreground(foreground.id, owner, detail());
        assert!(extras.pages.last().unwrap().rows[0].visual.busy);
        let visual = extras.visual_page().unwrap();
        assert!(visual.rows[0].visual.busy);
        for action in [Action::Down, Action::Select, Action::Options, Action::Right] {
            extras.handle(action);
        }
        assert_eq!(extras.pages.last().unwrap().selected, 0);
        assert_eq!(extras.pages.len(), 1);
        assert!(extras.effects.is_empty());
        assert!(!extras.handle_foreground(Action::VolumeUp));
        extras.handle(Action::Back);
        assert!(foreground.is_cancelled());
        assert!(
            !background.is_cancelled(),
            "Back only cancels the foreground request"
        );
        assert_eq!(extras.pages.len(), 1);
        assert_eq!(extras.pages.last().unwrap().key, owner);
        assert!(!extras.pages.last().unwrap().rows[0].visual.busy);
        assert!(
            !extras.finish_foreground(foreground.id, Some(owner)),
            "Cancelled output must not open a page"
        );
        extras.handle(Action::Down);
        assert_eq!(extras.pages.last().unwrap().selected, 1);
    }
    #[test]
    fn settings_loader_uses_display_owner_but_completes_only_its_target_job_and_generation() {
        let mut fixture = Fixture::new();
        let extras = &mut fixture.extras;
        extras.push("YouTube", vec![Item::new("Search", Choice::Nothing)]);
        let target = extras.pages.last().unwrap().key;
        extras.push(
            "Search Settings",
            vec![
                Item::new("Videos", Choice::Nothing),
                Item::new("Playlists & Albums", Choice::Nothing),
            ],
        );
        extras.pages.last_mut().unwrap().selected = 1;
        let display = extras.pages.last().unwrap().key;
        extras.claim_foreground(102, target, detail());
        assert!(extras.pages.last().unwrap().rows[1].visual.busy);
        assert!(!extras.pages[0].rows[0].visual.busy);
        assert!(!extras.finish_foreground(77, Some(target)));
        assert!(extras.foreground.is_some());
        assert!(extras.finish_foreground(102, Some(target)));
        assert_eq!(extras.pages.last().unwrap().key, display);
        assert!(!extras.pages.last().unwrap().rows[1].visual.busy);
        extras.claim_foreground(103, target, detail());
        extras.generation += 1;
        assert!(!extras.finish_foreground(103, Some(target)));
        assert!(extras.foreground.is_none());
    }
}
