use super::*;
use cumulus_core::client::dto::{
    PlayerAction, PlaylistDurationOrder, PlaylistDurationScope, PlaylistDurationSource, TrackPage,
};
use cumulus_core::membership::Quality;

#[derive(Default)]
struct Source {
    reads: Mutex<Vec<(String, Vec<String>)>>,
    song_trials: Mutex<Vec<Option<i64>>>,
    offline: std::sync::atomic::AtomicBool,
}

impl Api for Source {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            use std::sync::atomic::Ordering::Acquire;
            let ids: Vec<String> = if path == "/api/v3/song/detail" {
                self.song_trials
                    .lock()
                    .unwrap()
                    .push(payload["trialMode"].as_i64());
                serde_json::from_str::<Vec<Value>>(payload["c"].as_str().unwrap())
                    .unwrap()
                    .iter()
                    .map(|row| row["id"].as_str().unwrap().to_owned())
                    .collect()
            } else {
                Vec::new()
            };
            self.reads.lock().unwrap().push((path.into(), ids.clone()));
            if self.offline.load(Acquire) {
                return Err(Error::Transport("synthetic transport cut".into()));
            }
            match path {
                "/api/v6/playlist/detail" => Ok(json!({"code":200,"playlist":{
                    "id":73,"name":"Synthetic duration playlist","userId":9,"privacy":0,"specialType":0,
                    "trackIds":(1..=605).map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>(),
                }})),
                "/api/v3/song/detail" => Ok(json!({"code":200,"songs":ids.iter().map(|id| {
                    let duration = match id.as_str() { "605" => 0, "604" => 2_000,
                        "2" | "503" => 9_000, _ => 53_000 };
                    json!({"id":id,"name":format!("Song {id}"),"dt":duration,"ar":[],"al":{"name":"Synthetic album"}})
                }).collect::<Vec<_>>() })),
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                    Ok(json!({"code":200,"data":ids.iter().map(|id| {
                        json!({"id":id,"code":200,"url":format!("https://m1.music.126.net/fixture-{}.mp3",id.as_str().unwrap_or("audio")),
                            "type":"mp3","level":"standard","size":12345,"br":128000,"md5":"abcd","expi":60})
                    }).collect::<Vec<_>>() }))
                }
                path => panic!("unexpected synthetic request: {path}"),
            }
        })
    }
}

fn page(offset: usize, before: Option<&TrackPage>) -> PageRequest {
    PageRequest {
        offset,
        limit: 100,
        revision: before.and_then(|before| before.revision.clone()),
    }
}

fn source(page: &TrackPage) -> PlaylistDurationSource {
    PlaylistDurationSource {
        revision: page.revision.clone().unwrap(),
        local_revision: page
            .playlist_local
            .as_ref()
            .map(|local| local.revision.clone()),
        cached_at_ms: page
            .playlist_offline
            .as_ref()
            .map(|offline| offline.cached_at_ms),
    }
}

fn sorted(receipt: &str, order: PlaylistDurationOrder) -> PlaylistDurationScope {
    PlaylistDurationScope {
        receipt: receipt.into(),
        order,
    }
}

#[tokio::test]
async fn complete_native_sort_hydrates_only_suffix_and_drives_both_live_actions() {
    let temp = tempfile::tempdir().unwrap();
    let api = Arc::new(Source::default());
    let (client, _, _) = open(temp.path(), api.clone(), Some(9));
    let first = client
        .playlist_page_at("73", page(0, None), None)
        .await
        .unwrap()
        .value;
    let authority = source(&first);
    assert_eq!(first.tracks.len(), 100);
    let prepared = client
        .playlist_duration_prepare("1", "73", authority.clone())
        .await
        .unwrap()
        .value;
    assert_eq!(prepared.page.tracks.len(), 605);
    assert_eq!(prepared.page.tracks[0].id, "1");
    let ids = |ordinals: &[usize]| {
        ordinals
            .iter()
            .map(|&i| prepared.page.tracks[i].id.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        &ids(&prepared.ascending)[..5],
        ["605", "604", "2", "503", "1"]
    );
    let desc = ids(&prepared.descending);
    assert_eq!(&desc[desc.len() - 4..], ["2", "503", "604", "605"]);
    let reads = api.reads.lock().unwrap().clone();
    let batches = reads
        .iter()
        .filter(|(path, _)| path == "/api/v3/song/detail")
        .map(|(_, ids)| ids)
        .collect::<Vec<_>>();
    assert_eq!(
        batches.iter().map(|ids| ids.len()).collect::<Vec<_>>(),
        [100, 500, 5]
    );
    assert_eq!(batches[1][0], "101");
    assert_eq!(batches[2][0], "601");
    assert_eq!(*api.song_trials.lock().unwrap(), [None, Some(-1), Some(-1)]);

    let newer = client
        .playlist_duration_prepare("1", "73", authority)
        .await
        .unwrap()
        .value;
    assert_eq!(
        *api.reads.lock().unwrap(),
        reads,
        "complete source needs no reread"
    );
    let before = client.player_snapshot("1").await.unwrap();
    let action = |receipt, expected_queue_revision| PlayerAction::QueueSortedPlaylist {
        scope: sorted(receipt, PlaylistDurationOrder::Ascending),
        quality: Quality::Standard,
        expected_queue_revision,
    };
    assert!(matches!(
        client
            .player_action(
                "1",
                action(&prepared.receipt, before.queue_revision.clone())
            )
            .await,
        Err(Error::QueueChanged)
    ));
    client
        .play_selection(
            "1",
            PlaySelection::SortedPlaylist {
                scope: sorted(&newer.receipt, PlaylistDurationOrder::Ascending),
                start: None,
                double_click: false,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let sorted_state = client.player_snapshot("1").await.unwrap();
    assert_eq!(sorted_state.track.as_ref().unwrap().id, "605");
    let paused = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let insert = client
        .player_action(
            "1",
            PlayerAction::QueueSortedPlaylist {
                scope: sorted(&newer.receipt, PlaylistDurationOrder::Descending),
                quality: Quality::Standard,
                expected_queue_revision: paused.queue_revision.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(insert.play_id, paused.play_id);
    assert_eq!(insert.position, paused.position);
    assert_eq!(insert.state, paused.state);
    let queue = ProfileRegistry::open(temp.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    let rows = queue["items"].as_array().unwrap();
    assert_eq!(rows[0]["resource"]["id"], "605");
    assert_eq!(rows[1]["resource"]["id"], "1");
    assert_eq!(rows.last().unwrap()["resource"]["id"], "604");
    assert_eq!(
        api.reads
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == "/api/v3/song/detail")
            .count(),
        3
    );
}

#[tokio::test]
async fn mixed_and_cached_sort_bind_the_same_indexed_cue_without_catalog_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("fixture.wav"), b"synthetic cue container").unwrap();
    std::fs::write(
        media.join("fixture.cue"),
        concat!(
            "FILE \"fixture.wav\" WAVE\n",
            "TRACK 01 AUDIO\nTITLE \"Duration cue\"\nINDEX 01 00:00:00\n",
            "TRACK 02 AUDIO\nTITLE \"Next cue\"\nINDEX 01 00:15:00\n",
        ),
    )
    .unwrap();

    let api = Arc::new(Source::default());
    let (client, _, library) = open(&profile, api.clone(), Some(9));
    assert_eq!(library.scan(&media).unwrap().cue_updated, 1);
    let chosen = client
        .local_page(LocalQuery::default())
        .await
        .unwrap()
        .tracks
        .into_iter()
        .find(|row| row.track.name == "Duration cue")
        .unwrap();
    let cue = PlaylistLocalRef {
        id: chosen.track.id,
        revision: chosen.revision,
    };
    let page_before = client
        .playlist_page_at("73", page(0, None), None)
        .await
        .unwrap()
        .value;
    client
        .mutate_playlist_local(
            "1",
            "73",
            &page_before.playlist_local.unwrap().revision,
            add(std::slice::from_ref(&cue)),
        )
        .await
        .unwrap();
    let first = client
        .playlist_page_at("73", page(0, None), None)
        .await
        .unwrap()
        .value;
    let live = client
        .playlist_duration_prepare("1", "73", source(&first))
        .await
        .unwrap()
        .value;
    let local = live
        .page
        .tracks
        .iter()
        .position(|row| row.id == cue.id && row.local)
        .unwrap();
    assert_eq!(live.page.tracks[local].duration_ms, 15_000);
    assert_eq!(live.ascending[4], local);
    let local_selection = |receipt| PlaySelection::SortedPlaylist {
        scope: sorted(receipt, PlaylistDurationOrder::Ascending),
        start: Some(MixedPlaylistStart {
            id: cue.id.clone(),
            local: true,
        }),
        double_click: false,
        quality: Quality::Standard,
    };
    let selected = client
        .play_selection("1", local_selection(&live.receipt))
        .await
        .unwrap();
    assert_eq!(selected.track.as_ref().unwrap().id, cue.id);
    assert_eq!(selected.resource_kind, Some(PlayerResourceKind::Local));

    api.offline
        .store(true, std::sync::atomic::Ordering::Release);
    let cached_prefix = client
        .playlist_page_at("73", page(0, None), None)
        .await
        .unwrap()
        .value;
    assert!(cached_prefix.more && cached_prefix.playlist_offline.is_some());
    let requests = api.reads.lock().unwrap().len();
    assert!(matches!(
        client
            .play_selection("1", local_selection(&live.receipt))
            .await,
        Err(Error::QueueChanged)
    ));
    let cached = client
        .playlist_duration_prepare("1", "73", source(&cached_prefix))
        .await
        .unwrap()
        .value;
    assert_eq!(cached.ascending, live.ascending);
    assert_eq!(cached.descending, live.descending);
    let selected = client
        .play_selection("1", local_selection(&cached.receipt))
        .await
        .unwrap();
    assert_eq!(selected.track.as_ref().unwrap().id, cue.id);
    assert_eq!(selected.resource_kind, Some(PlayerResourceKind::Local));
    let queue = ProfileRegistry::open(&profile, ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    let rows = queue["items"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "unlicensed cached metadata is never queued");
    assert_eq!(rows[0]["resource"]["local_id"], cue.id);
    assert_eq!(api.reads.lock().unwrap().len(), requests);
}
