//! Removing history never deletes media or loses ownership of a stopping job.
use super::*;

impl Extras {
    pub(in crate::app) fn pending_download_count(&self) -> usize {
        self.jobs
            .values()
            .filter(|(_, purpose)| matches!(purpose, Purpose::Download))
            .count()
    }

    pub(in crate::app) fn show_download_history_options(&mut self) -> bool {
        let Some(page) = self
            .pages
            .last()
            .filter(|page| page.role == PageRole::ActiveDownloads)
        else {
            return false;
        };
        let selected = page.rows.get(page.selected).and_then(|row| {
            if let Choice::ShowDownload(id) = row.choice {
                Some(id)
            } else {
                None
            }
        });
        if !self.has_downloads() {
            return true;
        }
        let mut options = Vec::new();
        if let Some(id) = selected {
            if let Some(state) = self
                .download_states
                .get(&id)
                .filter(|state| !state.dismissed)
            {
                options.push(Item::new(
                    if state.active {
                        "Cancel and Remove"
                    } else {
                        "Remove from List"
                    },
                    Choice::RemoveDownload(id),
                ));
            }
        }
        options.push(Item::new("Clear Download List…", Choice::ClearDownloads));
        self.push_sheet("Downloads", options);
        true
    }

    pub(in crate::app) fn remove_download_history(&mut self, ids: &[u64]) {
        for id in ids {
            if let Some((handle, Purpose::Download)) = self.jobs.get(id) {
                handle.cancel();
                if let Some(state) = self.download_states.get_mut(id) {
                    state.dismissed = true;
                    state.request_cancel();
                }
            } else {
                self.download_states.remove(id);
            }
        }
        self.pages
            .retain(|page| !matches!(page.role, PageRole::Download(id) if ids.contains(&id)));
        self.pending_navigation = None;

        // Source pages must no longer offer actions for removed history records.
        let archive_pages: Vec<_> = self
            .pages
            .iter()
            .enumerate()
            .filter(|(_, page)| page.role == PageRole::ArchiveItem)
            .filter_map(|(index, page)| {
                page.rows.iter().find_map(|row| {
                    if let Choice::Download(DownloadRequest::Archive { item, .. }) = &row.choice {
                        Some((index, item.clone()))
                    } else {
                        None
                    }
                })
            })
            .collect();
        for (index, item) in archive_pages {
            let rows = self.archive_rows(&item);
            let page = &mut self.pages[index];
            page.rows = rows;
            page.selected = page.selected.min(page.rows.len().saturating_sub(1));
            page.visual_rows = None;
        }
        self.refresh_download_pages();
    }
}
