use super::*;
use crate::{library::Track, metadata};
use std::{fs, io::Write, net::TcpStream, path::Path, sync::atomic::AtomicU64};

static NEXT_TEST: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    config: Config,
    library: Arc<Library>,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bp-wifi-library-{}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        ));
        let music = root.join("music");
        let data = root.join("data");
        fs::create_dir_all(&music).unwrap();
        fs::create_dir_all(&data).unwrap();
        let path = music.join("01 Test.flac");
        flac(&path);
        let config = Config {
            music,
            data,
            name: "Fixture betterPod".into(),
        };
        let library = Arc::new(Library::from_tracks(vec![Track {
            path: path.clone(),
            title: "Snapshot title".into(),
            album: "Snapshot album".into(),
            ..Track::default()
        }]));
        Self {
            root,
            config,
            library,
            path,
        }
    }
    fn controller(&self) -> Controller {
        let mut controller = Controller::new(self.config.clone());
        controller.start(self.library.clone(), Vec::new()).unwrap();
        controller
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn flac(path: &Path) {
    let mut bytes = b"fLaC\x00\x00\x00\x22".to_vec();
    let mut stream = [0u8; 34];
    stream[0..2].copy_from_slice(&4096u16.to_be_bytes());
    stream[2..4].copy_from_slice(&4096u16.to_be_bytes());
    stream[10..18].copy_from_slice(&((44100u64 << 44) | (15u64 << 36) | 16).to_be_bytes());
    bytes.extend_from_slice(&stream);
    let mut comment = 4u32.to_le_bytes().to_vec();
    comment.extend_from_slice(b"test");
    let fields = [
        "TITLE=Original title",
        "ALBUM=Original album",
        "ARTIST=Artist",
        "COMMENT=Keep this",
    ];
    comment.extend_from_slice(&(fields.len() as u32).to_le_bytes());
    for field in fields {
        comment.extend_from_slice(&(field.len() as u32).to_le_bytes());
        comment.extend_from_slice(field.as_bytes());
    }
    bytes.push(0x84);
    let len = comment.len() as u32;
    bytes.extend_from_slice(&len.to_be_bytes()[1..]);
    bytes.extend(comment);
    bytes.extend_from_slice(b"unchanged audio payload");
    fs::write(path, bytes).unwrap();
}
fn ready(controller: &Controller) -> (String, u16, String) {
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        match controller.poll() {
            Some(Event::Ready { code, port, id, .. }) => return (code, port, id),
            Some(Event::Error(error)) => panic!("{error}"),
            _ => thread::sleep(Duration::from_millis(10)),
        }
    }
    panic!("server did not become ready")
}
fn change(path: &Path, fields: Value) -> files::Change {
    let meta = fs::metadata(path).unwrap();
    let modified = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;
    serde_json::from_value(
        json!({"path":path,"size":meta.len(),"modified":modified,"fields":fields}),
    )
    .unwrap()
}
fn shared(library: Arc<Library>) -> Shared {
    Shared {
        snapshot: Mutex::new(Snapshot {
            library,
            protected: Vec::new(),
            revision: 1,
        }),
        socket: Mutex::new(None),
        cancelled: AtomicBool::new(false),
    }
}

#[test]
fn explicit_start_authenticates_before_reads_and_supports_reconnect() {
    let fixture = Fixture::new();
    let mut controller = fixture.controller();
    let (code, port, _) = ready(&controller);
    assert_eq!(code.len(), 8);
    assert!(code.bytes().all(|b| b.is_ascii_digit()));
    let client_state = shared(fixture.library.clone());
    let mut client = wire::connect(
        TcpStream::connect(("127.0.0.1", port)).unwrap(),
        &code,
        &client_state,
    )
    .unwrap();
    client
        .send(&json!({"op":"list","offset":0,"limit":256}), &client_state)
        .unwrap();
    let listed = client
        .receive(&client_state, Duration::from_secs(2))
        .unwrap();
    assert_eq!(listed["tracks"][0]["title"], "Original title");
    assert_eq!(listed["tracks"][0]["displayAlbum"], "Snapshot album");
    assert_eq!(listed["total"], 1);
    assert!(listed["revision"].is_string());
    client
        .send(&json!({"op":"disconnect"}), &client_state)
        .unwrap();
    assert_eq!(
        client
            .receive(&client_state, Duration::from_secs(2))
            .unwrap()["ok"],
        true
    );
    drop(client);
    let client = wire::connect(
        TcpStream::connect(("127.0.0.1", port)).unwrap(),
        &code,
        &client_state,
    )
    .unwrap();
    drop(client);
    controller.shutdown();
}

#[test]
fn incorrect_code_never_opens_the_library() {
    let fixture = Fixture::new();
    let mut controller = fixture.controller();
    let (code, port, _) = ready(&controller);
    let wrong = if code == "00000000" {
        "00000001"
    } else {
        "00000000"
    };
    let client_state = shared(fixture.library.clone());
    assert!(wire::connect(
        TcpStream::connect(("127.0.0.1", port)).unwrap(),
        wrong,
        &client_state
    )
    .is_err());
    assert_eq!(
        metadata::read_tag_fields(&fixture.path)
            .unwrap()
            .title
            .as_deref(),
        Some("Original title")
    );
    controller.shutdown();
}

#[test]
fn oversized_plain_frames_are_refused_before_allocation() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let mut client = TcpStream::connect(address).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    client
        .write_all(&((wire::MAX_FRAME + 1) as u32).to_be_bytes())
        .unwrap();
    let state = shared(Arc::new(Library::default()));
    assert!(
        wire::read_frame(&mut server, &state, Duration::from_secs(1), wire::MAX_FRAME)
            .unwrap_err()
            .contains("limit")
    );
}

#[test]
fn save_preserves_audio_and_rejects_stale_requests_and_outside_paths() {
    let fixture = Fixture::new();
    let roots = files::Roots::new(&fixture.config).unwrap();
    let state = shared(fixture.library.clone());
    let edit = change(
        &fixture.path,
        json!({"title":"Changed ✓","album":"Correct album"}),
    );
    files::save(&fixture.config, &roots, &state, &fixture.library, &edit).unwrap();
    let fields = metadata::read_tag_fields(&fixture.path).unwrap();
    assert_eq!(fields.title.as_deref(), Some("Changed ✓"));
    assert_eq!(fields.album.as_deref(), Some("Correct album"));
    assert!(fs::read(&fixture.path)
        .unwrap()
        .ends_with(b"unchanged audio payload"));
    let after = fs::read(&fixture.path).unwrap();
    assert!(
        files::save(&fixture.config, &roots, &state, &fixture.library, &edit)
            .unwrap_err()
            .contains("changed")
    );
    assert_eq!(fs::read(&fixture.path).unwrap(), after);
    let outside = fixture.root.join("outside.flac");
    flac(&outside);
    let outside_edit = change(&outside, json!({"title":"No"}));
    assert!(files::save(
        &fixture.config,
        &roots,
        &state,
        &fixture.library,
        &outside_edit
    )
    .unwrap_err()
    .contains("not in"));
    assert_eq!(
        fs::read_dir(&fixture.config.music).unwrap().count(),
        1,
        "staging files must be cleaned"
    );
}

#[test]
fn decoder_paths_and_symlinks_are_read_only() {
    let fixture = Fixture::new();
    let roots = files::Roots::new(&fixture.config).unwrap();
    let state = shared(fixture.library.clone());
    state
        .snapshot
        .lock()
        .unwrap()
        .protected
        .push(fixture.path.clone());
    let edit = change(&fixture.path, json!({"title":"Do not change"}));
    assert!(
        files::save(&fixture.config, &roots, &state, &fixture.library, &edit)
            .unwrap_err()
            .contains("playback")
    );
    let protected = HashSet::from([fixture.path.clone()]);
    let record = files::record(&fixture.library.tracks[0], &roots, false, &protected);
    assert_eq!(record["writable"], false);
    assert!(record["warning"].as_str().unwrap().contains("playback"));
    #[cfg(unix)]
    {
        let link = fixture.config.music.join("link.flac");
        std::os::unix::fs::symlink(&fixture.path, &link).unwrap();
        let library = Library::from_tracks(vec![Track {
            path: link.clone(),
            ..Track::default()
        }]);
        let edit = change(&link, json!({"title":"No"}));
        assert!(
            files::save(&fixture.config, &roots, &state, &library, &edit)
                .unwrap_err()
                .contains("Symbolic")
        );
    }
}

#[test]
fn cancelled_and_invalid_edits_leave_the_original_intact() {
    let fixture = Fixture::new();
    let roots = files::Roots::new(&fixture.config).unwrap();
    let state = shared(fixture.library.clone());
    let bytes = fs::read(&fixture.path).unwrap();
    let invalid = change(&fixture.path, json!({"track":70000}));
    assert!(files::save(&fixture.config, &roots, &state, &fixture.library, &invalid).is_err());
    let edit = change(&fixture.path, json!({"title":"Cancelled"}));
    state.cancelled.store(true, Ordering::Release);
    assert!(
        files::save(&fixture.config, &roots, &state, &fixture.library, &edit)
            .unwrap_err()
            .contains("stopped")
    );
    assert_eq!(fs::read(&fixture.path).unwrap(), bytes);
    assert_eq!(fs::read_dir(&fixture.config.music).unwrap().count(), 1);
}

#[test]
fn successful_physical_album_edit_clears_old_private_grouping_only() {
    let fixture = Fixture::new();
    let roots = files::Roots::new(&fixture.config).unwrap();
    let state = shared(fixture.library.clone());
    let custom = json!({"tracks":{(fixture.path.to_string_lossy().to_string()):{"album":"Wrong private album","artist":"Private artist","group":"merged","disc":3,"cover":"keep-art","added":true}}});
    fs::write(
        fixture.config.data.join("curation.json"),
        serde_json::to_vec(&custom).unwrap(),
    )
    .unwrap();
    let edit = change(&fixture.path, json!({"album":"Correct physical album"}));
    files::save(&fixture.config, &roots, &state, &fixture.library, &edit).unwrap();
    let custom: Value =
        serde_json::from_slice(&fs::read(fixture.config.data.join("curation.json")).unwrap())
            .unwrap();
    let row = &custom["tracks"][fixture.path.to_str().unwrap()];
    assert_eq!(row["group"], "");
    assert!(row["album"].is_null());
    assert_eq!(row["cover"], "keep-art");
    assert_eq!(row["added"], false);
}

#[test]
fn saved_cover_survives_folder_art_import_cache_rescan_and_reconnect() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let fixture = Fixture::new();
    let roots = files::Roots::new(&fixture.config).unwrap();
    let state = shared(fixture.library.clone());
    let folder_cover = fixture.config.music.join("cover.jpg");
    fs::write(&folder_cover, b"old folder artwork").unwrap();
    let original_folder = fs::read(&folder_cover).unwrap();
    let before = Library::load_or_scan(&fixture.config.music, &fixture.config.data, None).unwrap();
    assert_eq!(before.tracks[0].cover.as_ref(), Some(&folder_cover));

    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        16,
        16,
        image::Rgb([230, 20, 80]),
    ))
    .write_to(&mut encoded, image::ImageFormat::Png)
    .unwrap();
    let picture = encoded.into_inner();
    let mut edit = change(&fixture.path, json!({"title":"Saved from Mac"}));
    edit.cover = Some(STANDARD.encode(&picture));
    files::save(&fixture.config, &roots, &state, &fixture.library, &edit).unwrap();

    // A newer Go index must not reintroduce its old art or metadata after the
    // remote save. A warm Rust cache and a new session must agree as well.
    let import = fixture.root.join("go-library.json");
    fs::write(&import, serde_json::to_vec(&json!({
        "tracks":[{"path":fixture.path,"title":"Stale imported title","album":"Original album","artist":"Artist"}],
        "albums":[{"name":"Original album","artist":"Artist","artPath":folder_cover}]
    })).unwrap()).unwrap();
    let mut saved_cover = None;
    for _ in 0..2 {
        let scanned =
            Library::load_or_scan(&fixture.config.music, &fixture.config.data, Some(&import))
                .unwrap();
        let track = &scanned.tracks[0];
        assert_eq!(track.title, "Saved from Mac");
        let cover = track.cover.as_ref().unwrap();
        assert_ne!(cover, &folder_cover);
        assert_eq!(fs::read(cover).unwrap(), picture);
        if let Some(previous) = &saved_cover {
            assert_eq!(cover, previous);
        }
        saved_cover = Some(cover.clone());
        let response = files::cover(track, &roots, false).unwrap();
        assert_eq!(
            STANDARD
                .decode(response["cover"].as_str().unwrap())
                .unwrap(),
            picture
        );
    }
    assert_eq!(fs::read(&folder_cover).unwrap(), original_folder);
    assert!(fs::read(&fixture.path)
        .unwrap()
        .ends_with(b"unchanged audio payload"));

    let mut remove = change(&fixture.path, json!({}));
    remove.remove_cover = true;
    files::save(&fixture.config, &roots, &state, &fixture.library, &remove).unwrap();
    let removed =
        Library::load_or_scan(&fixture.config.music, &fixture.config.data, Some(&import)).unwrap();
    assert!(
        removed.tracks[0].cover.is_none(),
        "An explicit removal must not restore old folder artwork"
    );
    assert_eq!(fs::read(&folder_cover).unwrap(), original_folder);
}

#[test]
fn authenticated_batch_reports_partial_success_and_read_boundary() {
    let fixture = Fixture::new();
    let roots = files::Roots::new(&fixture.config).unwrap();
    let state = Arc::new(shared(fixture.library.clone()));
    let (tx, _rx) = mpsc::channel();
    let mut tags = HashSet::new();
    let mut artwork = HashSet::new();
    let meta = fs::metadata(&fixture.path).unwrap();
    let modified = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;
    let request = json!({"op":"save","tracks":[{"path":fixture.path,"size":meta.len(),"modified":modified,"fields":{"title":"Saved"}},{"path":fixture.root.join("outside.flac"),"size":0,"modified":0,"fields":{"title":"No"}}]});
    let response = handle(
        &fixture.config,
        &roots,
        &state,
        &tx,
        &request,
        &mut tags,
        &mut artwork,
    )
    .unwrap();
    assert_eq!(response["saved"].as_array().unwrap().len(), 1);
    assert_eq!(response["failures"].as_array().unwrap().len(), 1);
    let read = handle(
        &fixture.config,
        &roots,
        &state,
        &tx,
        &json!({"op":"read","paths":[fixture.path,fixture.root.join("outside.flac")]}),
        &mut tags,
        &mut artwork,
    )
    .unwrap();
    assert_eq!(read["tracks"][0]["title"], "Saved");
    assert_eq!(
        read["tracks"][0]["displayAlbum"], "Snapshot album",
        "A title-only edit keeps the player's existing album display"
    );
    assert!(
        tags.is_empty(),
        "A title-only edit must not clear album grouping"
    );
    assert_eq!(read["skipped"].as_array().unwrap().len(), 1);
}

#[test]
fn display_album_identity_keeps_distinct_folders_and_reflects_edited_tags() {
    let fixture = Fixture::new();
    let roots = files::Roots::new(&fixture.config).unwrap();
    let track = &fixture.library.tracks[0];
    let protected = HashSet::new();
    let first = files::record(track, &roots, false, &protected);
    let mut other = track.clone();
    other.path = fixture.config.music.join("another album").join("song.flac");
    let second = files::record(&other, &roots, false, &protected);
    assert_ne!(
        first["displayAlbumID"], second["displayAlbumID"],
        "Same tags in different physical albums stay distinct"
    );
    let mut grouped = track.clone();
    grouped.album_group = "private-group".into();
    assert_eq!(
        files::record(&grouped, &roots, false, &protected)["displayAlbumID"],
        "private-group"
    );
    let edited = files::record(&grouped, &roots, true, &protected);
    assert_ne!(edited["displayAlbumID"], "private-group");
    assert!(edited["displayAlbum"].is_null());
    assert!(change(&fixture.path, json!({"album":"Corrected"})).changes_grouping());
    assert!(!change(&fixture.path, json!({"title":"Corrected"})).changes_grouping());
}

#[test]
fn confirmed_join_is_stable_across_folders_and_survives_a_real_rescan() {
    let fixture = Fixture::new();
    let second = fixture.config.music.join("Other folder/02 Other.flac");
    fs::create_dir_all(second.parent().unwrap()).unwrap();
    flac(&second);
    let first = fs::canonicalize(&fixture.path).unwrap();
    let second = fs::canonicalize(&second).unwrap();
    let library =
        Arc::new(Library::load_or_scan(&fixture.config.music, &fixture.config.data, None).unwrap());
    assert_eq!(
        library.albums.len(),
        2,
        "Equal tags alone do not merge source folders"
    );
    let roots = files::Roots::new(&fixture.config).unwrap();
    let state = Arc::new(shared(library.clone()));
    let (tx, _rx) = mpsc::channel();
    let mut changed = HashSet::new();
    let mut artwork = HashSet::new();
    let changes = [&first, &second].map(|path| {
        let meta = fs::metadata(path).unwrap();
        json!({"path":path,"size":meta.len(),"modified":meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as i64,
            "fields":{"album":"Confirmed album","albumArtist":"Confirmed artist"},"joinAlbum":true})
    });
    let response = handle(
        &fixture.config,
        &roots,
        &state,
        &tx,
        &json!({"op":"save","tracks":changes}),
        &mut changed,
        &mut artwork,
    )
    .unwrap();
    assert_eq!(response["saved"].as_array().unwrap().len(), 2);
    assert_eq!(response["joined"].as_array().unwrap().len(), 2);
    assert!(response["failures"].as_array().unwrap().is_empty());
    let read = handle(
        &fixture.config,
        &roots,
        &state,
        &tx,
        &json!({"op":"read","paths":[first,second]}),
        &mut changed,
        &mut artwork,
    )
    .unwrap();
    let group = crate::curation::remote_album_group("Confirmed album", "Confirmed artist").unwrap();
    assert_eq!(read["tracks"][0]["displayAlbumID"], group);
    assert_eq!(read["tracks"][1]["displayAlbumID"], group);
    assert_eq!(read["tracks"][0]["albumGroup"], group);
    assert_eq!(read["capabilities"], json!(["album-join-v1"]));
    let refreshed =
        Library::load_or_scan(&fixture.config.music, &fixture.config.data, None).unwrap();
    assert_eq!(
        refreshed.albums.len(),
        1,
        "The console's real grouping matches the Mac after rescan"
    );
    for track in &refreshed.tracks {
        assert_eq!(track.album_group, group);
        assert!(fs::read(&track.path)
            .unwrap()
            .ends_with(b"unchanged audio payload"));
    }
}

#[test]
fn joining_requires_explicit_identity_and_obeys_original_write_boundaries() {
    let fixture = Fixture::new();
    let roots = files::Roots::new(&fixture.config).unwrap();
    let state = shared(fixture.library.clone());
    let original = fs::read(&fixture.path).unwrap();
    for fields in [
        json!({"album":"Only album"}),
        json!({"albumArtist":"Only artist"}),
        json!({"album":"  ","albumArtist":"Artist"}),
    ] {
        let mut edit = change(&fixture.path, fields);
        edit.join_album = true;
        assert!(files::save(&fixture.config, &roots, &state, &fixture.library, &edit).is_err());
        assert_eq!(fs::read(&fixture.path).unwrap(), original);
    }
    let mut edit = change(
        &fixture.path,
        json!({"album":"Album","albumArtist":"Artist"}),
    );
    edit.join_album = true;
    state
        .snapshot
        .lock()
        .unwrap()
        .protected
        .push(fixture.path.clone());
    assert!(
        files::save(&fixture.config, &roots, &state, &fixture.library, &edit)
            .unwrap_err()
            .contains("playback")
    );
    state.snapshot.lock().unwrap().protected.clear();
    let outside = fixture.root.join("outside.flac");
    flac(&outside);
    let mut escape = change(&outside, json!({"album":"Album","albumArtist":"Artist"}));
    escape.join_album = true;
    assert!(
        files::save(&fixture.config, &roots, &state, &fixture.library, &escape)
            .unwrap_err()
            .contains("not in")
    );
    let mut stale = change(
        &fixture.path,
        json!({"album":"Album","albumArtist":"Artist"}),
    );
    stale.join_album = true;
    files::save(&fixture.config, &roots, &state, &fixture.library, &edit).unwrap();
    let after = fs::read(&fixture.path).unwrap();
    assert!(
        files::save(&fixture.config, &roots, &state, &fixture.library, &stale)
            .unwrap_err()
            .contains("changed")
    );
    assert_eq!(fs::read(&fixture.path).unwrap(), after);
}

#[test]
fn join_identity_normalizes_case_width_and_spaces_but_keeps_distinct_artists() {
    let group = |album, artist| crate::curation::remote_album_group(album, artist).unwrap();
    assert_eq!(
        group("  Ａlbum  Name ", "Ａrtist\t Name"),
        group("album name", "artist name")
    );
    assert_ne!(group("Album", "Artist one"), group("Album", "Artist two"));
    assert_ne!(group("Album one", "Artist"), group("Album two", "Artist"));
    assert_ne!(group("Cafe", "Artist"), group("Café", "Artist"));
}

#[test]
fn join_preserves_existing_disc_and_cover_until_explicitly_edited() {
    let fixture = Fixture::new();
    let custom = json!({"tracks":{(fixture.path.to_string_lossy().to_string()):{"album":"Old album","group":"old-private","disc":3,"cover":"keep-art","added":true}}});
    fs::write(
        fixture.config.data.join("curation.json"),
        serde_json::to_vec(&custom).unwrap(),
    )
    .unwrap();
    let roots = files::Roots::new(&fixture.config).unwrap();
    let state = shared(fixture.library.clone());
    let mut edit = change(
        &fixture.path,
        json!({"album":"Joined","albumArtist":"Artist"}),
    );
    edit.join_album = true;
    files::save(&fixture.config, &roots, &state, &fixture.library, &edit).unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(fixture.config.data.join("curation.json")).unwrap())
            .unwrap();
    let entry = &manifest["tracks"][fixture.path.to_str().unwrap()];
    assert_eq!(entry["disc"], 3);
    assert_eq!(entry["cover"], "keep-art");
    assert_eq!(
        entry["group"],
        crate::curation::remote_album_group("Joined", "Artist").unwrap()
    );
}

#[test]
fn grouping_acknowledgement_requires_a_successful_manifest_commit() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.config.data.join("curation.json")).unwrap();
    let roots = files::Roots::new(&fixture.config).unwrap();
    let state = Arc::new(shared(fixture.library.clone()));
    let (tx, _rx) = mpsc::channel();
    let meta = fs::metadata(&fixture.path).unwrap();
    let request = json!({"op":"save","tracks":[{"path":fixture.path,"size":meta.len(),"modified":meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as i64,
        "fields":{"album":"Saved physical album","albumArtist":"Artist"},"joinAlbum":true}]});
    let response = handle(
        &fixture.config,
        &roots,
        &state,
        &tx,
        &request,
        &mut HashSet::new(),
        &mut HashSet::new(),
    )
    .unwrap();
    assert_eq!(
        response["saved"].as_array().unwrap().len(),
        1,
        "The physical tags were saved"
    );
    assert!(
        response["joined"].as_array().unwrap().is_empty(),
        "A failed private grouping must never be acknowledged"
    );
    assert_eq!(response["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(
        metadata::read_tag_fields(&fixture.path)
            .unwrap()
            .album
            .as_deref(),
        Some("Saved physical album")
    );
}
