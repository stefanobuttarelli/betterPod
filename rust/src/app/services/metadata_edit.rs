//! Album Info's editable snapshots. Unchanged fields never become write jobs.
use super::*;
#[derive(Clone)]
pub(super) struct SongEdit {
    pub path: PathBuf,
    pub original: TagEdit,
    pub current: TagEdit,
}
pub(super) struct AlbumEdit {
    pub songs: Vec<SongEdit>,
    pub scope: Option<usize>,
    pub status: String,
    pub loading: bool,
    pub saving: bool,
}
impl AlbumEdit {
    pub(super) fn loading() -> Self {
        Self {
            songs: Vec::new(),
            scope: None,
            status: String::new(),
            loading: true,
            saving: false,
        }
    }
    pub(super) fn install(&mut self, tags: Vec<(PathBuf, TagEdit)>) {
        self.songs = tags
            .into_iter()
            .map(|(path, tags)| SongEdit {
                path,
                original: tags.clone(),
                current: tags,
            })
            .collect();
        self.loading = false;
        self.scope = None;
    }
    pub(super) fn common(&self, field: Field) -> (String, bool) {
        let mut songs = self
            .songs
            .iter()
            .enumerate()
            .filter(|(index, _)| self.scope.is_none_or(|scope| scope == *index));
        let Some((_, first)) = songs.next() else {
            return (String::new(), true);
        };
        let value = field_text(&first.current, field);
        let same = songs.all(|(_, song)| field_text(&song.current, field) == value);
        (value, same)
    }
    pub(super) fn set(&mut self, field: Field, value: String) -> Result<(), String> {
        let value = value.trim().to_string();
        if matches!(field, Field::Year | Field::Track | Field::Disc)
            && !value.is_empty()
            && value.parse::<u32>().is_err()
        {
            return Err(format!("{}: type a number", field_label(field)));
        }
        let (old, same) = self.common(field);
        if same && old == value || !same && value.is_empty() {
            return Ok(());
        }
        for (index, song) in self
            .songs
            .iter_mut()
            .enumerate()
            .filter(|(index, _)| self.scope.is_none_or(|scope| scope == *index))
        {
            let _ = index;
            set_value(&mut song.current, field, &value);
        }
        self.status.clear();
        Ok(())
    }
    pub(super) fn changed_field(&self, field: Field) -> bool {
        self.songs
            .iter()
            .enumerate()
            .filter(|(index, _)| self.scope.is_none_or(|scope| scope == *index))
            .any(|(_, song)| field_text(&song.current, field) != field_text(&song.original, field))
    }
    pub(super) fn changes(&self) -> Vec<(PathBuf, TagEdit)> {
        self.songs
            .iter()
            .filter_map(|song| {
                let patch = diff(&song.original, &song.current);
                has_changes(&patch).then(|| (song.path.clone(), patch))
            })
            .collect()
    }
    pub(super) fn undo(&mut self) {
        for song in &mut self.songs {
            song.current = song.original.clone();
        }
        self.status.clear();
    }
    pub(super) fn apply_preview(&mut self, edits: Vec<(PathBuf, TagEdit)>) {
        for (path, patch) in edits {
            if let Some(song) = self.songs.iter_mut().find(|song| song.path == path) {
                merge(&mut song.current, patch);
            }
        }
    }
    pub(super) fn summary(&self) -> String {
        let fields = [
            Field::Title,
            Field::Artist,
            Field::Track,
            Field::Disc,
            Field::Album,
            Field::AlbumArtist,
            Field::Year,
            Field::Genre,
        ];
        let mut parts = Vec::new();
        for field in fields {
            let n = self
                .songs
                .iter()
                .filter(|song| {
                    field_text(&song.current, field) != field_text(&song.original, field)
                })
                .count();
            if n == 0 {
                continue;
            }
            let label = match field {
                Field::Title => "Titles",
                Field::Artist => "Artists",
                Field::Track => "Numbers",
                Field::Disc => "Discs",
                Field::Album => "Album",
                Field::AlbumArtist => "Album Artist",
                Field::Year => "Year",
                Field::Genre => "Genre",
            };
            parts.push(
                if n < self.songs.len()
                    && matches!(
                        field,
                        Field::Title | Field::Artist | Field::Track | Field::Disc
                    )
                {
                    format!("{label} of {n}")
                } else {
                    label.into()
                },
            );
        }
        if parts.is_empty() {
            "No changes".into()
        } else {
            parts.join(" · ")
        }
    }
}
pub(super) fn field_label(field: Field) -> &'static str {
    match field {
        Field::Title => "Title",
        Field::Artist => "Artist",
        Field::Album => "Album",
        Field::AlbumArtist => "Album Artist",
        Field::Year => "Year",
        Field::Genre => "Genre",
        Field::Track => "Track Number",
        Field::Disc => "Disc Number",
    }
}
pub(super) fn field_text(tags: &TagEdit, field: Field) -> String {
    match field {
        Field::Title => tags.title.clone().unwrap_or_default(),
        Field::Artist => tags.artist.clone().unwrap_or_default(),
        Field::Album => tags.album.clone().unwrap_or_default(),
        Field::AlbumArtist => tags.album_artist.clone().unwrap_or_default(),
        Field::Genre => tags.genre.clone().unwrap_or_default(),
        Field::Year => tags
            .year
            .filter(|n| *n > 0)
            .map(|n| n.to_string())
            .unwrap_or_default(),
        Field::Track => tags
            .track
            .filter(|n| *n > 0)
            .map(|n| n.to_string())
            .unwrap_or_default(),
        Field::Disc => tags
            .disc
            .filter(|n| *n > 0)
            .map(|n| n.to_string())
            .unwrap_or_default(),
    }
}
fn set_value(tags: &mut TagEdit, field: Field, value: &str) {
    match field {
        Field::Title => tags.title = Some(value.into()),
        Field::Artist => tags.artist = Some(value.into()),
        Field::Album => tags.album = Some(value.into()),
        Field::AlbumArtist => tags.album_artist = Some(value.into()),
        Field::Genre => tags.genre = Some(value.into()),
        Field::Year => tags.year = Some(value.parse().unwrap_or(0)),
        Field::Track => tags.track = Some(value.parse().unwrap_or(0)),
        Field::Disc => tags.disc = Some(value.parse().unwrap_or(0)),
    }
}
pub(super) fn has_changes(patch: &TagEdit) -> bool {
    patch.title.is_some()
        || patch.artist.is_some()
        || patch.album.is_some()
        || patch.album_artist.is_some()
        || patch.genre.is_some()
        || patch.year.is_some()
        || patch.track.is_some()
        || patch.track_total.is_some()
        || patch.disc.is_some()
        || patch.cover.is_some()
}
pub(super) fn diff(original: &TagEdit, current: &TagEdit) -> TagEdit {
    let mut patch = TagEdit::default();
    for field in [
        Field::Title,
        Field::Artist,
        Field::Album,
        Field::AlbumArtist,
        Field::Genre,
        Field::Year,
        Field::Track,
        Field::Disc,
    ] {
        let value = field_text(current, field);
        if field_text(original, field) != value {
            set_value(&mut patch, field, &value);
        }
    }
    if original.track_total.unwrap_or(0) != current.track_total.unwrap_or(0) {
        patch.track_total = Some(current.track_total.unwrap_or(0));
    }
    if original.cover != current.cover {
        patch.cover = current.cover.clone();
    }
    patch
}
fn merge(tags: &mut TagEdit, patch: TagEdit) {
    macro_rules! set{($($field:ident),*)=>{$(if patch.$field.is_some(){tags.$field=patch.$field;})*}}
    set!(
        title,
        artist,
        album,
        album_artist,
        genre,
        year,
        track,
        track_total,
        disc,
        cover
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn common_mixed_values_and_song_edits_preserve_other_songs() {
        let mut edit = AlbumEdit::loading();
        edit.install(vec![
            (
                "a.flac".into(),
                TagEdit {
                    artist: Some("One".into()),
                    title: Some("A".into()),
                    track_total: Some(12),
                    ..Default::default()
                },
            ),
            (
                "b.flac".into(),
                TagEdit {
                    artist: Some("Two".into()),
                    title: Some("B".into()),
                    track_total: Some(12),
                    ..Default::default()
                },
            ),
        ]);
        assert!(!edit.common(Field::Artist).1);
        edit.set(Field::Artist, String::new()).unwrap();
        assert!(edit.changes().is_empty());
        edit.scope = Some(1);
        edit.set(Field::Title, "Changed".into()).unwrap();
        let changes = edit.changes();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, PathBuf::from("b.flac"));
        assert!(changes[0].1.artist.is_none());
        assert!(changes[0].1.track_total.is_none());
        edit.undo();
        assert!(edit.changes().is_empty());
    }
    #[test]
    fn invalid_numbers_do_not_erase_existing_metadata() {
        let mut edit = AlbumEdit::loading();
        edit.install(vec![(
            "x.mp3".into(),
            TagEdit {
                year: Some(2001),
                ..Default::default()
            },
        )]);
        assert!(edit.set(Field::Year, "not a year".into()).is_err());
        assert!(edit.changes().is_empty());
        edit.set(Field::Year, String::new()).unwrap();
        assert_eq!(edit.changes()[0].1.year, Some(0));
    }
}
