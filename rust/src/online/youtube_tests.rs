use super::*;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, sync::atomic::AtomicU64};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture(PathBuf);
impl Fixture {
    fn new(raw: Value) -> Self {
        let root = std::env::temp_dir().join(format!(
            "bp-youtube-online-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("tools")).unwrap();
        let tool = root.join("tools/yt-dlp");
        fs::write(
            &tool,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\n/bin/cat \"$0.response\"\n",
        )
        .unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            root.join("tools/yt-dlp.response"),
            serde_json::to_vec(&raw).unwrap(),
        )
        .unwrap();
        Self(root)
    }
    fn args(&self) -> String {
        fs::read_to_string(self.0.join("tools/yt-dlp.args")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn playlist_search_preserves_the_youtube_filter_and_stops_at_the_last_page() {
    let fixture = Fixture::new(
        json!({"entries":[{"_type":"url","id":"PLfixture","url":"https://www.youtube.com/playlist?list=PLfixture","title":"An album","playlist_count":2}]}),
    );
    let (items, total) = search(
        &Context::standalone(),
        &fixture.0,
        Provider::YouTubePlaylists,
        "Artist & album",
        1,
    )
    .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, "playlist");
    assert_eq!(total, 1, "A short final page must not show More forever");
    let args = fixture.args();
    assert!(args.contains("--flat-playlist\n--playlist-end\n26\n"));
    assert!(args.contains("search_query=Artist%20%26%20album&sp=EgIQAw%3D%3D"));
    assert!(!args.contains("ytsearch"));
}

#[test]
fn video_search_pages_remain_video_searches_with_one_lookahead_result() {
    let entries = (0..51)
        .map(|index| json!({"id":format!("video{index:06}"),"title":format!("Video {index}")}))
        .collect::<Vec<_>>();
    let fixture = Fixture::new(json!({"entries":entries}));
    let (items, total) = search(
        &Context::standalone(),
        &fixture.0,
        Provider::YouTube,
        "fixture",
        2,
    )
    .unwrap();
    assert_eq!(items.len(), 25);
    assert_eq!(items[0].id, "video000025");
    assert_eq!(total, 51);
    assert!(fixture.args().contains("ytsearch51:fixture"));
    let (items, total) = search(
        &Context::standalone(),
        &fixture.0,
        Provider::YouTube,
        "fixture",
        9,
    )
    .unwrap();
    assert!(items.is_empty());
    assert_eq!(total, 0);
}

#[test]
fn filtered_previous_rows_and_empty_final_pages_do_not_leave_more_results() {
    let mut entries = (0..25)
        .map(|index| json!({"id":format!("video{index:06}"),"title":format!("Video {index}")}))
        .collect::<Vec<_>>();
    entries[0] = json!({"id":"private0001","title":"[Private video]"});
    entries.push(json!({"id":"last0000001","title":"Last result"}));
    let fixture = Fixture::new(json!({"entries":entries}));
    let (items, total) = search(
        &Context::standalone(),
        &fixture.0,
        Provider::YouTube,
        "fixture",
        2,
    )
    .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        total, 25,
        "Filtered rows are not counted as visible results"
    );
    let (items, total) = search(
        &Context::standalone(),
        &fixture.0,
        Provider::YouTube,
        "fixture",
        3,
    )
    .unwrap();
    assert!(items.is_empty());
    assert_eq!(total, 0, "An empty final page must immediately end paging");
}

#[test]
fn playlist_details_are_flat_keep_valid_order_and_do_not_resolve_child_formats() {
    let fixture = Fixture::new(
        json!({"_type":"playlist","id":"PLfixture","title":"Album","entries":[
            {"id":"first000001","title":"First","url":"first000001"},
            {"id":"gone0000001","title":"[Private video]"},null,
            {"id":"second00001","title":"Second","url":"second00001"},
            {"id":"first000001","title":"First repeated","url":"first000001"}
        ]}),
    );
    let item = youtube_item(
        json!({"id":"PLfixture","url":"https://www.youtube.com/playlist?list=PLfixture","title":"Album"}),
        Provider::YouTubePlaylists,
    );
    let opened = details(&Context::standalone(), &fixture.0, item).unwrap();
    assert_eq!(opened.count, 3);
    let entries = opened.raw["entries"].as_array().unwrap();
    assert_eq!(entries[0]["id"], "first000001");
    assert_eq!(entries[1]["id"], "second00001");
    assert_eq!(
        entries[2]["id"], "first000001",
        "Repeated playlist positions remain intact"
    );
    let child = youtube_item(entries[0].clone(), Provider::YouTube);
    assert_eq!(child.url, "https://www.youtube.com/watch?v=first000001");
    assert_eq!(child.kind, "video");
    assert!(fixture
        .args()
        .contains("--flat-playlist\n--playlist-end\n501\n"));
    assert!(!fixture.args().contains("--no-playlist"));
}

#[test]
fn playlists_are_identified_from_urls_and_video_links_inside_playlists_stay_videos() {
    let list = youtube_item(
        json!({"id":"PLfixture","url":"https://www.youtube.com/playlist?list=PLfixture","entry_count":12}),
        Provider::YouTube,
    );
    assert_eq!(list.kind, "playlist");
    assert_eq!(list.count, 12);
    let video = youtube_item(
        json!({"id":"abcdefghijk","url":"https://www.youtube.com/watch?v=abcdefghijk&list=PLfixture"}),
        Provider::YouTube,
    );
    assert_eq!(video.kind, "video");
    assert!(
        youtube_playlist_entries(&json!({"entries":[],"playlist_count":900}))
            .unwrap_err()
            .contains("500")
    );
}

#[test]
fn cancelled_playlist_search_does_not_start_the_tool() {
    let fixture = Fixture::new(json!({"entries":[]}));
    let context = Context::standalone();
    context.cancel();
    assert!(
        search(&context, &fixture.0, Provider::YouTubePlaylists, "album", 1)
            .unwrap_err()
            .contains("Cancelled")
    );
    assert!(!fixture.0.join("tools/yt-dlp.args").exists());
}
