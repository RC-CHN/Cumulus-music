use super::{
    Api, ApiOrigin, CARD, COOKIE, Error, Fixture, HOLD, HttpResponse, PlayerAction, STARTUP, gate,
    json, reply,
};
use cumulus_core::{
    client::dto::DownloadCommand, download::service::DownloadOptions, playlist::PlaylistPhase,
    protocol::mconfig::Snapshot, storage::StateStore,
};
use std::{
    future::{Future, poll_fn},
    task::Poll,
    time::Duration,
};

#[path = "picker_creation.rs"]
mod creation;

const LIKED: &str = "/api/song/like/get";
const DETAIL: &str = "/api/v6/playlist/detail";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";
const AUTH: &str = "/api/song/enhance/download/url/v1";
const PLAYLIST: &str = "81";

fn card() -> HttpResponse {
    json!({"code":200,"tracks":[
        {"id":301,"name":"指定普通歌曲","album":{},"privilege":{"id":301,"fee":0,"st":0,"dl":320000}},
        {"id":302,"name":"旁边的歌曲","album":{},"privilege":{"id":302,"fee":0,"st":0,"dl":320000,"subp":0}},
        {"id":303,"name":"不可收藏","album":{},"privilege":{"id":303,"fee":0,"st":0,"dl":320000,"subp":-1}}
    ]}).into()
}

fn detail(owner: u64, present: bool) -> serde_json::Value {
    json!({"code":200,"playlist":{"id":81,"name":"合成自建歌单","userId":owner,"privacy":10,
        "tags":[],"trackIds":if present {vec![json!({"id":301})]} else {vec![]}}})
}

pub(super) fn config(f: &Fixture) {
    let store = StateStore::open(f.root.path().join("config")).unwrap();
    store
        .put(
            "transport",
            "pc-mconfig-v1",
            "pc",
            &serde_json::to_value(Snapshot {
                origin: ApiOrigin::Pc,
                version: 1,
                bucket: String::new(),
                generation: 1,
                source_url: Some("https://p1.music.126.net/obj/fixture.data".into()),
                data: Some(json!({})),
            })
            .unwrap(),
        )
        .unwrap();
}

async fn fixture(replies: impl IntoIterator<Item = HttpResponse>) -> (Fixture, String) {
    let f = Fixture::new(
        &[COOKIE],
        [card(), json!({"code":200,"ids":[]}).into()]
            .into_iter()
            .chain(replies),
    )
    .await;
    config(&f);
    let receipt = f
        .client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap()
        .receipt
        .unwrap();
    f.client.liked_ids().await.unwrap();
    (f, receipt)
}

fn calls(f: &Fixture, paths: &[&str]) {
    assert_eq!(
        f.http
            .calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        STARTUP
            .into_iter()
            .chain([CARD, LIKED])
            .chain(paths.iter().copied())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn exact_destination_completion_true_or_false_can_unlock_only_the_selected_row_once() {
    for (code, after, expected) in [
        (200, true, PlaylistPhase::Confirmed),
        (403, false, PlaylistPhase::Rejected),
        (0, true, PlaylistPhase::Unchanged),
    ] {
        let playlist = if code == 0 {
            vec![detail(25, true).into()]
        } else {
            vec![
                detail(25, false).into(),
                json!({"code":code}).into(),
                detail(25, after).into(),
            ]
        };
        let (f, receipt) = fixture(
            playlist
                .into_iter()
                .chain([json!({"code":200,"data":{"id":301,"code":403}}).into()]),
        )
        .await;
        let chosen = f
            .client
            .player_voice_song_download_choose_playlist(&f.epoch, &f.play_id, &receipt, 0, PLAYLIST)
            .await
            .unwrap();
        assert_eq!(chosen.phase, expected);
        assert_eq!(chosen.track_id, "301");
        let permit = chosen.continuation.unwrap();
        let repeated = f
            .client
            .player_voice_song_download_choose_playlist(&f.epoch, &f.play_id, &receipt, 0, PLAYLIST)
            .await
            .unwrap();
        assert_eq!(repeated.operation_id, chosen.operation_id);
        assert_eq!(repeated.continuation.as_deref(), Some(permit.as_str()));
        assert!(matches!(
            f.client
                .player_voice_song_download_start_from_playlist(
                    &f.epoch,
                    &f.play_id,
                    &receipt,
                    1,
                    &permit,
                    DownloadOptions::default()
                )
                .await,
            Err(Error::StaleResponse)
        ));
        assert!(matches!(
            f.client
                .player_voice_song_download_start(
                    &f.epoch,
                    &f.play_id,
                    &receipt,
                    0,
                    DownloadOptions::default()
                )
                .await,
            Err(Error::State(_))
        ));
        f.client
            .download_command(
                &f.epoch,
                DownloadCommand::Configure {
                    root: f.root.path().join("output").to_str().unwrap().into(),
                    path_type: 0,
                    file_type: 0,
                },
            )
            .await
            .unwrap();
        let started = f
            .client
            .player_voice_song_download_start_from_playlist(
                &f.epoch,
                &f.play_id,
                &receipt,
                0,
                &permit,
                DownloadOptions::default(),
            )
            .await
            .unwrap();
        assert_eq!(started.accepted.len(), 1);
        assert!(matches!(
            f.client
                .player_voice_song_download_start_from_playlist(
                    &f.epoch,
                    &f.play_id,
                    &receipt,
                    0,
                    &permit,
                    DownloadOptions::default()
                )
                .await,
            Err(Error::StaleResponse)
        ));
        let consumed = f
            .client
            .player_voice_song_download_picker_status(&f.epoch, &f.play_id, &receipt, 0, false)
            .await
            .unwrap();
        assert!(consumed.continuation.is_none());
        let paths = if code == 0 {
            vec![DETAIL, AUTH]
        } else {
            vec![DETAIL, ADD, DETAIL, AUTH]
        };
        f.http.wait(STARTUP.len() + 2 + paths.len()).await;
        calls(&f, &paths);
        if code != 0 {
            let write = &f.http.calls()[STARTUP.len() + 3];
            assert_eq!(write.payload["pid"], PLAYLIST);
            assert_eq!(write.payload["trackIds"], r#"["301"]"#);
            assert_eq!(write.payload["op"], "add");
            assert!(write.payload.get("imme").is_none());
        }
        assert_eq!(f.http.calls().last().unwrap().payload["id"], "301");
    }
}

#[tokio::test]
async fn lost_ack_cannot_unlock_download_and_explicit_readback_does_not_replay_add() {
    let (f, receipt) = fixture([
        detail(25, false).into(),
        HttpResponse::Ready(Err(Error::Transport("synthetic lost playlist ack".into()))),
        detail(25, false).into(),
        detail(25, true).into(),
    ])
    .await;
    let first = f
        .client
        .player_voice_song_download_choose_playlist(&f.epoch, &f.play_id, &receipt, 0, PLAYLIST)
        .await
        .unwrap();
    assert_eq!(first.phase, PlaylistPhase::Uncertain);
    assert!(first.continuation.is_none());
    assert!(matches!(
        f.client
            .player_voice_song_download_start_from_playlist(
                &f.epoch,
                &f.play_id,
                &receipt,
                0,
                &first.operation_id,
                DownloadOptions::default()
            )
            .await,
        Err(Error::StaleResponse)
    ));
    let repeated = f
        .client
        .player_voice_song_download_choose_playlist(&f.epoch, &f.play_id, &receipt, 0, PLAYLIST)
        .await
        .unwrap();
    assert_eq!(first.operation_id, repeated.operation_id);
    calls(&f, &[DETAIL, ADD, DETAIL]);
    let verified = f
        .client
        .player_voice_song_download_picker_status(&f.epoch, &f.play_id, &receipt, 0, true)
        .await
        .unwrap();
    assert_eq!(verified.operation_id, first.operation_id);
    assert_eq!(verified.phase, PlaylistPhase::Confirmed);
    assert!(verified.continuation.is_some());
    calls(&f, &[DETAIL, ADD, DETAIL, DETAIL]);
    assert!(
        f.store
            .values("25", "download_tasks_v1")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn foreign_playlist_or_old_play_during_detail_cannot_create_an_add_journal() {
    let (held, release) = gate();
    let (f, receipt) = fixture([detail(26, false).into(), held]).await;
    assert!(matches!(
        f.client
            .player_voice_song_download_choose_playlist(&f.epoch, &f.play_id, &receipt, 2, PLAYLIST)
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        f.client
            .player_voice_song_download_choose_playlist(&f.epoch, &f.play_id, &receipt, 0, PLAYLIST)
            .await,
        Err(Error::InvalidInput(_))
    ));
    let (client, epoch, play_id) = (f.client.clone(), f.epoch.clone(), f.play_id.clone());
    let pending = tokio::spawn(async move {
        client
            .player_voice_song_download_choose_playlist(&epoch, &play_id, &receipt, 0, PLAYLIST)
            .await
    });
    f.http.wait(STARTUP.len() + 4).await;
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(release.send(Ok(reply(detail(25, false)))).is_ok());
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    calls(&f, &[DETAIL, DETAIL]);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn same_revision_with_a_distinct_unique_playlist_write_cookie_is_not_a_picker_scope() {
    let f = Fixture::new(
        &[
            "MUSIC_U=card-account; Path=/eapi/dj; Secure",
            "MUSIC_U=card-account; Path=/eapi/song; Secure",
            "MUSIC_U=card-account; Path=/eapi/v1/dj/program; Secure",
            "MUSIC_U=card-account; Path=/eapi/rtrs/abt/front; Secure",
            "MUSIC_U=card-account; Path=/eapi/w/nuser/account; Secure",
            "MUSIC_U=card-account; Path=/eapi/v6/playlist/detail; Secure",
            "MUSIC_U=another-account; Path=/eapi/v1/playlist/manipulate/tracks; Secure",
        ],
        [card(), json!({"code":200,"ids":[]}).into()],
    )
    .await;
    config(&f);
    let receipt = f
        .client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap()
        .receipt
        .unwrap();
    f.client.liked_ids().await.unwrap();
    assert_eq!(
        f.client
            .player_voice_song_download_prepare(&f.epoch, &f.play_id, &receipt, 0)
            .await
            .unwrap()
            .decision,
        cumulus_core::client::dto::VoiceSongDownloadDecision::PlaylistRequired
    );
    assert!(matches!(
        f.client
            .player_voice_song_download_choose_playlist(&f.epoch, &f.play_id, &receipt, 0, PLAYLIST)
            .await,
        Err(Error::StaleResponse)
    ));
    calls(&f, &[]);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn stale_after_sending_journal_but_before_http_dispatch_is_known_unsent() {
    let (detail_wait, detail_release) = gate();
    let (blocker_wait, blocker_release) = gate();
    let (f, receipt) = fixture([detail_wait, blocker_wait]).await;
    let (client, epoch, play_id) = (f.client.clone(), f.epoch.clone(), f.play_id.clone());
    let pending = tokio::spawn(async move {
        client
            .player_voice_song_download_choose_playlist(&epoch, &play_id, &receipt, 0, PLAYLIST)
            .await
    });
    f.http.wait(STARTUP.len() + 3).await;
    let native = f.native.clone();
    let mut blocker = Box::pin(async move { native.request(HOLD, json!({}), false).await });
    assert!(
        poll_fn(|cx| Poll::Ready(blocker.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    let blocker = tokio::spawn(blocker);
    assert!(detail_release.send(Ok(reply(detail(25, false)))).is_ok());
    f.http.wait(STARTUP.len() + 4).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if f.store
                .values("25", "playlist-mutations")
                .unwrap()
                .into_values()
                .any(|record| record["phase"] == "sending")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(
        blocker_release
            .send(Ok(reply(json!({"code":200,"profile":{"userId":25}}))))
            .is_ok()
    );
    blocker.await.unwrap().unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    calls(&f, &[DETAIL, HOLD]);
    let records = f.store.values("25", "playlist-mutations").unwrap();
    assert_eq!(records.len(), 1);
    let record = records.values().next().unwrap();
    assert_eq!(record["phase"], "rejected");
    assert_eq!(record["error_class"], "not_sent");
    assert!(
        f.store
            .values("25", "download_tasks_v1")
            .unwrap()
            .is_empty()
    );
}
