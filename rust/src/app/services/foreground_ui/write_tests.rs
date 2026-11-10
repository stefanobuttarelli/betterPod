//! Coordinator regressions use local read-only jobs; no provider or media write.
use super::*;

struct Fixture {
    root: PathBuf,
    extras: Extras,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bp-foreground-write-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("music")).unwrap();
        let extras = Extras::new(root.join("music"), root.join("data")).unwrap();
        Self { root, extras }
    }
    fn job(&mut self, purpose: Purpose) -> JobHandle {
        let handle = self.extras.services.submit(Task::Downloaded).unwrap();
        self.extras
            .jobs
            .insert(handle.id, (handle.clone(), purpose));
        handle
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.extras.shutdown();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn details(generation: u64) -> Purpose {
    Purpose::Details {
        generation,
        metadata: false,
        item: OnlineItem::new(Provider::YouTube),
    }
}

#[test]
fn grouped_lookup_finishes_members_individually_and_back_cancels_only_remaining_jobs() {
    let mut fixture = Fixture::new();
    fixture.extras.push(
        "Find Info Online",
        vec![Item::new("Album and Artist", Choice::CombinedSearch)],
    );
    let owner = fixture.extras.pages.last().unwrap().key;
    fixture.extras.pages.last_mut().unwrap().role = PageRole::Combined;
    let generation = fixture.extras.generation;
    fixture.extras.combined = Some(CombinedState {
        cover_album: String::new(),
        cover_artist: String::new(),
        cover_paths: Arc::new(Vec::new()),
        cover_context: None,
        query: "Album and Artist".into(),
        automatic_query: "Album and Artist".into(),
        cover: false,
        manual: true,
        generation,
        hits: Vec::new(),
        pending: 3,
        errors: Vec::new(),
    });
    let purpose = Purpose::Combined { generation };
    let context_purpose = Purpose::CoverContext { generation };
    let first = fixture.job(purpose.clone());
    let second = fixture.job(purpose.clone());
    let third = fixture.job(context_purpose.clone());
    let background = fixture.job(Purpose::Downloads);
    let extras = &mut fixture.extras;
    extras.claim_foreground(first.id, owner, purpose.clone());
    extras.claim_foreground(second.id, owner, purpose.clone());
    extras.claim_foreground(third.id, owner, context_purpose.clone());
    for handle in [&first, &second, &third] {
        assert!(extras.foreground_job(handle.id));
        assert!(
            !handle.is_cancelled(),
            "Joining a lookup must not cancel earlier members"
        );
    }
    assert_eq!(extras.foreground_busy_row(), Some((owner, 0)));
    extras.jobs.remove(&first.id);
    assert!(extras.complete_foreground_job(first.id, Some(owner), &purpose, false));
    assert!(!extras.foreground_job(first.id));
    assert!(extras.foreground_job(second.id) && extras.foreground_job(third.id));
    assert!(
        extras.foreground_busy_row().is_some(),
        "Partial lookup results do not finish the group"
    );
    let mut result = OnlineItem::new(Provider::Itunes);
    result.id = "completed-result".into();
    result.title = "Completed lookup".into();
    extras.combined.as_mut().unwrap().hits.push(result);
    extras.combined.as_mut().unwrap().pending = 2;
    extras.handle(Action::Back);
    assert!(!first.is_cancelled());
    assert!(second.is_cancelled() && third.is_cancelled());
    assert!(!background.is_cancelled());
    assert!(extras.foreground_busy_row().is_none());
    assert_eq!(extras.pages.last().unwrap().key, owner);
    assert_eq!(extras.combined.as_ref().unwrap().pending, 0);
    assert_eq!(
        extras.combined.as_ref().unwrap().hits[0].id,
        "completed-result"
    );
    assert!(!extras.complete_foreground_job(second.id, Some(owner), &purpose, false));
    assert!(
        !extras
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::Rescan)),
        "Cancelled read-only results do not report library writes"
    );
}

#[test]
fn grouped_lookup_releases_its_spinner_only_after_the_last_owned_result() {
    let mut fixture = Fixture::new();
    fixture.extras.push(
        "Find Info Online",
        vec![Item::new("Album and Artist", Choice::CombinedSearch)],
    );
    let owner = fixture.extras.pages.last().unwrap().key;
    let purpose = Purpose::Combined {
        generation: fixture.extras.generation,
    };
    let first = fixture.job(purpose.clone());
    let second = fixture.job(purpose.clone());
    let extras = &mut fixture.extras;
    extras.claim_foreground(first.id, owner, purpose.clone());
    extras.claim_foreground(second.id, owner, purpose.clone());
    assert!(!extras.complete_foreground_job(999_999, Some(owner), &purpose, false));
    assert!(extras.complete_foreground_job(first.id, Some(owner), &purpose, false));
    assert_eq!(extras.foreground_busy_row(), Some((owner, 0)));
    assert!(extras.complete_foreground_job(second.id, Some(owner), &purpose, false));
    assert!(extras.foreground_busy_row().is_none());
    assert!(!extras.pages.last().unwrap().rows[0].visual.busy);
}

#[test]
fn cancelling_tag_write_keeps_wanted_edits_and_late_completion_only_requests_a_rescan() {
    let mut fixture = Fixture::new();
    let path = fixture.root.join("music/song.m4a");
    let mut edit = metadata_edit::AlbumEdit::loading();
    edit.install(vec![(
        path,
        TagEdit {
            title: Some("Original title".into()),
            ..Default::default()
        },
    )]);
    edit.set(Field::Title, "Wanted title".into()).unwrap();
    edit.saving = true;
    fixture.extras.album_edit = Some(edit);
    fixture.extras.push(
        "Album Info",
        vec![Item::new("Save Changes", Choice::SaveTagBatch)],
    );
    fixture.extras.pages.last_mut().unwrap().role = PageRole::AlbumInfo;
    let owner = fixture.extras.pages.last().unwrap().key;
    let purpose = Purpose::WriteTags {
        generation: fixture.extras.generation,
    };
    let write = fixture.job(purpose.clone());
    let background = fixture.job(Purpose::Download);
    fixture
        .extras
        .claim_foreground(write.id, owner, purpose.clone());
    fixture.extras.handle(Action::Back);
    assert!(write.is_cancelled());
    assert!(!background.is_cancelled());
    let edit = fixture.extras.album_edit.as_ref().unwrap();
    assert!(!edit.saving);
    assert_eq!(
        edit.songs[0].original.title.as_deref(),
        Some("Original title")
    );
    assert_eq!(edit.songs[0].current.title.as_deref(), Some("Wanted title"));
    assert_eq!(
        edit.changes().len(),
        1,
        "Cancel stops remaining work without discarding wanted edits"
    );
    assert_eq!(fixture.extras.pages.last().unwrap().key, owner);
    let next_purpose = details(fixture.extras.generation);
    let next = fixture.job(next_purpose.clone());
    fixture
        .extras
        .claim_foreground(next.id, owner, next_purpose);
    fixture.extras.effects.clear();
    assert!(!fixture
        .extras
        .complete_foreground_job(write.id, Some(owner), &purpose, false));
    assert_eq!(
        fixture
            .extras
            .effects
            .iter()
            .filter(|effect| matches!(effect, Effect::Rescan))
            .count(),
        1
    );
    assert!(
        fixture.extras.foreground_job(next.id),
        "An old write result cannot release a newer source row"
    );
    assert_eq!(
        fixture.extras.foreground_busy_row(),
        Some((owner, fixture.extras.pages.last().unwrap().selected))
    );
    assert_eq!(
        fixture.extras.album_edit.as_ref().unwrap().songs[0]
            .current
            .title
            .as_deref(),
        Some("Wanted title")
    );
}

#[test]
fn failed_mutating_actions_rescan_and_passive_tools_do_not_claim_foreground() {
    let mut fixture = Fixture::new();
    fixture.extras.push(
        "Album Info",
        vec![Item::new("Apply Changes", Choice::Nothing)],
    );
    let owner = fixture.extras.pages.last().unwrap().key;
    for purpose in [
        Purpose::Tags,
        Purpose::Merge,
        Purpose::Separate,
        Purpose::CoverReset,
        Purpose::Cover,
    ] {
        assert!(Extras::foreground_purpose(&purpose));
        let handle = fixture.job(purpose.clone());
        fixture
            .extras
            .claim_foreground(handle.id, owner, purpose.clone());
        fixture.extras.effects.clear();
        assert!(fixture
            .extras
            .complete_foreground_job(handle.id, Some(owner), &purpose, true));
        assert!(fixture
            .extras
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::Rescan)));
        assert!(fixture.extras.foreground_busy_row().is_none());
    }
    assert!(Extras::foreground_purpose(&Purpose::ToolAction));
    assert!(!Extras::foreground_purpose(&Purpose::Tools));
    assert!(!Extras::foreground_purpose(&Purpose::Download));
}

#[test]
fn cancelled_cover_completion_rescans_committed_art_without_releasing_a_newer_action() {
    let mut fixture = Fixture::new();
    fixture
        .extras
        .push("Album Info", vec![Item::new("Cover", Choice::Nothing)]);
    let owner = fixture.extras.pages.last().unwrap().key;
    let cover = fixture.job(Purpose::Cover);
    fixture
        .extras
        .claim_foreground(cover.id, owner, Purpose::Cover);
    fixture.extras.handle(Action::Back);
    assert!(cover.is_cancelled());
    let next_purpose = details(fixture.extras.generation);
    let next = fixture.job(next_purpose.clone());
    fixture
        .extras
        .claim_foreground(next.id, owner, next_purpose);
    fixture.extras.effects.clear();
    assert!(!fixture
        .extras
        .complete_foreground_job(cover.id, Some(owner), &Purpose::Cover, false));
    assert_eq!(
        fixture
            .extras
            .effects
            .iter()
            .filter(|effect| matches!(effect, Effect::Rescan))
            .count(),
        1
    );
    assert!(fixture.extras.foreground_job(next.id));
    assert_eq!(fixture.extras.foreground_busy_row(), Some((owner, 0)));
}
