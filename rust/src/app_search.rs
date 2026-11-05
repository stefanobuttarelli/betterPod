//! Go 1.1's title-only search categories, constructed on the search worker.
use super::*;
use betterpod_rust::playlists::Playlist;
pub(super) struct SearchResults {
    pub rows: Arc<Vec<Row>>,
    pub categories: [Arc<Vec<Row>>; 4],
}
pub(super) fn search_field(query: &str) -> Row {
    let mut row = Row::new(query, Intent::SearchKeyboard);
    row.arrow = false;
    row.visual.search_field = true;
    row.visual.placeholder = "Search your music".into();
    row
}
pub(super) fn search(library: &Library, playlists: &[Playlist], query: &str) -> SearchResults {
    let mut categories: [Vec<Row>; 4] = std::array::from_fn(|_| Vec::new());
    let needle = query.trim().to_lowercase();
    if !needle.is_empty() {
        for (i, artist) in library
            .artists
            .iter()
            .enumerate()
            .filter(|(_, a)| a.to_lowercase().contains(&needle))
        {
            let mut row = Row::new(artist, Intent::Open(Screen::Artist(i)));
            let album = library.albums.iter().find(|a| a.artist == *artist);
            row.cover = album.and_then(|album| album.cover.clone());
            row.visual.thumbnail = album.is_some();
            if album.is_none() {
                row.visual.icon = "artists_icon".into();
            }
            categories[0].push(row);
        }
        for (i, album) in library
            .albums
            .iter()
            .enumerate()
            .filter(|(_, a)| a.name.to_lowercase().contains(&needle))
        {
            let mut row = Row::new(&album.name, Intent::Open(Screen::Album(i)));
            row.detail = album.artist.clone();
            row.cover = album.cover.clone();
            row.visual.thumbnail = true;
            categories[1].push(row);
        }
        for (i, track) in library
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.title.to_lowercase().contains(&needle))
        {
            let mut row = Row::new(&track.title, Intent::Song(i));
            row.detail = format!("{} • {}", track.artist, track.album);
            row.cover = track.cover.clone();
            row.visual.thumbnail = true;
            row.arrow = false;
            categories[2].push(row);
        }
        for playlist in playlists
            .iter()
            .filter(|p| p.name.to_lowercase().contains(&needle))
        {
            let mut row = Row::new(&playlist.name, Intent::Playlist(playlist.clone()));
            row.detail = format!(
                "{} {}",
                playlist.tracks.len(),
                if playlist.tracks.len() == 1 {
                    "song"
                } else {
                    "songs"
                }
            );
            row.visual.icon = "playlist_icon".into();
            categories[3].push(row);
        }
    }
    let mut rows = vec![search_field(query)];
    for (i, (title, singular, icon)) in [
        ("Artists", "artist", "artists_icon"),
        ("Albums", "album", "albums_icon"),
        ("Songs", "song", "song_icon"),
        ("Playlists", "playlist", "playlist_icon"),
    ]
    .into_iter()
    .enumerate()
    {
        let count = categories[i].len();
        if count == 0 {
            continue;
        }
        let mut row = Row::new(title, Intent::Open(Screen::SearchResults(i as u8)));
        row.detail = format!("{count} {singular}{}", if count == 1 { "" } else { "s" });
        row.visual.icon = icon.into();
        rows.push(row);
    }
    SearchResults {
        rows: Arc::new(rows),
        categories: categories.map(Arc::new),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_keeps_source_categories_and_title_only_matches() {
        let library = Library::from_tracks(vec![
            Track {
                title: "Needle Song".into(),
                artist: "Artist".into(),
                album: "Other".into(),
                path: "/music/a.mp3".into(),
                ..Default::default()
            },
            Track {
                title: "Unrelated".into(),
                artist: "Needle Artist".into(),
                album: "Needle Album".into(),
                path: "/music/b.mp3".into(),
                ..Default::default()
            },
        ]);
        let result = search(&library, &[], "  NEEDLE ");
        assert_eq!(
            result
                .rows
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>(),
            vec!["  NEEDLE ", "Artists", "Albums", "Songs"]
        );
        assert_eq!(result.categories[2].len(), 1);
        assert_eq!(result.categories[2][0].label, "Needle Song");
        assert_eq!(search(&library, &[], " ").rows.len(), 1);
    }
}
