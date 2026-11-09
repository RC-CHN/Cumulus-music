use super::*;
use std::sync::Mutex;
use tokio::sync::Notify;

#[derive(Default)]
struct SongLyrics {
    reads: Mutex<Vec<Value>>,
    hold: AtomicBool,
    entered: Notify,
    release: Notify,
}

impl Api for SongLyrics {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(
                path, "/api/song/lyric/v1",
                "local playback must not request another API"
            );
            assert!(!cache);
            let id = payload["id"].as_str().unwrap().to_owned();
            self.reads.lock().unwrap().push(payload);
            if self.hold.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            if id == "404" {
                return Ok(json!({"code":200}));
            }
            Ok(json!({"code":200,"sgc":true,"qfy":true,
                "lrc":{"version":17,"lyric":format!("[00:01]歌曲{id}第一行\n[00:08]歌曲{id}第二行")}}))
        })
    }
}

#[tokio::test]
async fn matched_local_cue_lyrics_follow_account_mapping_and_keep_the_song_clock() {
    use cumulus_core::{lyrics::LyricBountyKind, preferences::desktop_lyrics::Appearance};
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("album.wav"), b"synthetic local CUE audio").unwrap();
    std::fs::write(
        media.join("album.lrc"),
        "[00:00]整碟开始\n[00:35]整碟第二首",
    )
    .unwrap();
    std::fs::write(media.join("album.cue"),
        "FILE \"album.wav\" WAVE\nTRACK 01 AUDIO\nTITLE \"First\"\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nTITLE \"Second\"\nINDEX 01 00:30:00\n").unwrap();
    let (session, client, library) = client(&root.path().join("state"), Arc::new(Probe::default()));
    let api = Arc::new(SongLyrics::default());
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    library.scan(&media).unwrap();
    let page = client.local_page(LocalQuery::default()).await.unwrap();
    let id = page
        .tracks
        .iter()
        .find(|row| row.track.name == "Second")
        .unwrap()
        .track
        .id
        .clone();
    let select = || PlaySelection::Local {
        query: LocalQuery::default(),
        revision: page.revision.clone(),
        start_id: Some(id.clone()),
    };
    let initial = client.play_selection("1", select()).await.unwrap();
    let play = initial.play_id.unwrap();
    assert_eq!(initial.timeline.unwrap().media_start, 30.0);
    let local = client.player_lyrics("1", &play).await.unwrap();
    assert!(local.from_sidecar);
    assert_eq!(local.lines[1].time, Some(5.0));
    assert!(api.reads.lock().unwrap().is_empty());

    library.map_online(&session, &id, &json!(123)).unwrap();
    let mapped = client.player_lyrics("1", &play).await.unwrap();
    assert!(!mapped.from_sidecar && !mapped.from_embedded);
    assert_eq!(mapped.lines[0].time, Some(1.0));
    assert_eq!(mapped.lines[1].time, Some(8.0));
    assert_eq!(mapped.lines[0].lyric, "歌曲123第一行");
    assert!(mapped.contribution.is_none() && mapped.toolbar_contributions.is_empty());
    assert!(
        client
            .prepare_lyric_bounty("1", &play, LyricBountyKind::Translation)
            .await
            .is_err()
    );
    let current = client.player_snapshot("1").await.unwrap();
    assert_eq!(current.play_id.as_deref(), Some(play.as_str()));
    assert_eq!(current.queue_revision, initial.queue_revision);
    assert_eq!(current.timeline.unwrap().media_start, 30.0);

    let mut desktop = client.desktop_lyric_document("1", &play).await.unwrap();
    let detail = client
        .desktop_lyric_frame(&desktop, &Appearance::default())
        .unwrap();
    assert_eq!(detail.play_id, play);
    assert!(!detail.info.from_sidecar);
    assert_eq!(
        *api.reads.lock().unwrap(),
        [json!({"id":"123","lv":-1,"tv":-1,"rv":-1,"yv":-1})]
    );

    library.map_online(&session, &id, &json!(321)).unwrap();
    assert!(matches!(
        client.desktop_lyric_offset(&mut desktop, 0.5).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.refresh_desktop_lyric_document(&mut desktop).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.desktop_lyric_frame(&desktop, &Appearance::default()),
        Err(Error::StaleResponse)
    ));
    assert!(client.player_cached_lyrics("1", &play).await.is_err());
    assert_eq!(api.reads.lock().unwrap().len(), 1);
    api.hold.store(true, Ordering::SeqCst);
    let client = Arc::new(client);
    let pending = tokio::spawn({
        let client = client.clone();
        let play = play.clone();
        async move { client.player_lyrics("1", &play).await }
    });
    tokio::time::timeout(Duration::from_secs(4), api.entered.notified())
        .await
        .unwrap();
    library.map_online(&session, &id, &json!(999)).unwrap();
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    let current = client.player_lyrics("1", &play).await.unwrap();
    assert_eq!(current.lines[0].lyric, "歌曲999第一行");
    assert_eq!(current.offset, 0.0);

    library.map_online(&session, &id, &json!(404)).unwrap();
    let empty = client.player_lyrics("1", &play).await.unwrap();
    assert!(empty.lines.is_empty() && !empty.from_sidecar);
    let calls = api.reads.lock().unwrap().len();
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    let new = client.play_selection("2", select()).await.unwrap();
    let isolated = client
        .player_lyrics("2", new.play_id.as_deref().unwrap())
        .await
        .unwrap();
    assert!(isolated.from_sidecar);
    assert_eq!(isolated.lines[1].lyric, "整碟第二首");
    assert_eq!(api.reads.lock().unwrap().len(), calls);
}
