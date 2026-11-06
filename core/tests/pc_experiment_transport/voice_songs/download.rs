use super::{CARD, COOKIE, Error, Fixture, PlayerAction, STARTUP, json};
use cumulus_core::{
    client::dto::{DownloadCommand, SongDownloadState, VoiceSongDownloadDecision as D},
    download::service::DownloadOptions,
};
use std::time::Duration;

const LIKED: &str = "/api/song/like/get";
const AUTHORIZATION: &str = "/api/song/enhance/download/url/v1";

fn card() -> serde_json::Value {
    json!({"code":200,"tracks":[
        {"id":301,"name":"已喜欢普通歌曲","album":{},"privilege":{"id":301,"fee":0,"st":0,"dl":320000}},
        {"id":302,"name":"未喜欢普通歌曲","album":{},"privilege":{"id":302,"fee":0,"st":0,"dl":320000}},
        {"id":303,"name":"合作方下架","album":{},"privilege":{"id":303,"fee":0,"st":-100,"dl":320000}},
        {"id":304,"name":"另需权益","album":{},"privilege":{"id":304,"fee":4,"st":0,"dl":320000}},
        {"id":305,"name":"云盘资源","album":{},"privilege":{"id":305,"fee":0,"st":0,"dl":320000,"cs":true}},
        {"id":306,"name":"权益不完整","album":{},"privilege":{"id":306,"fee":0,"dl":320000}},
        {"id":307,"name":"权限指向另一个歌曲","album":{},"privilege":{"id":308,"fee":0,"st":0,"dl":320000}}
    ]})
}

async fn receipt(f: &Fixture) -> String {
    f.client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap()
        .receipt
        .unwrap()
}

async fn decision(f: &Fixture, receipt: &str, index: usize) -> D {
    f.client
        .player_voice_song_download_prepare(&f.epoch, &f.play_id, receipt, index)
        .await
        .unwrap()
        .decision
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
            .chain(paths.iter().copied())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn download_preflight_requires_known_liked_membership_and_never_skips_the_picker() {
    let f = Fixture::new(
        &[COOKIE],
        [card().into(), json!({"code":200,"ids":[301]}).into()],
    )
    .await;
    let receipt = receipt(&f).await;
    assert_eq!(decision(&f, &receipt, 0).await, D::MembershipUnknown);
    calls(&f, &[CARD]);
    f.client.liked_ids().await.unwrap();
    for (index, expected) in [
        D::Ready,
        D::PlaylistRequired,
        D::Unavailable,
        D::ExtendedRightsUnsupported,
        D::CloudUnsupported,
        D::Unavailable,
        D::Unavailable,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(decision(&f, &receipt, index).await, expected);
        if index != 0 {
            assert!(matches!(
                f.client
                    .player_voice_song_download_start(
                        &f.epoch,
                        &f.play_id,
                        &receipt,
                        index,
                        DownloadOptions::default()
                    )
                    .await,
                Err(Error::State(_))
            ));
        }
    }
    let view = f
        .client
        .player_voice_song_download_prepare(&f.epoch, &f.play_id, &receipt, 0)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(view).unwrap(),
        json!({"epoch":f.epoch,"playId":f.play_id,"trackId":"301","decision":"ready"})
    );
    calls(&f, &[CARD, LIKED]);
    assert!(
        f.store
            .values("25", "download_tasks_v1")
            .unwrap()
            .is_empty()
    );
    assert!(
        f.store
            .values("25", "resource-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn download_start_targets_the_exact_ordinary_row_and_preserves_independent_denial_and_existing_task()
 {
    let f = Fixture::new(
        &[COOKIE],
        [
            card().into(),
            json!({"code":200,"ids":[301]}).into(),
            json!({"code":200,"data":{"id":301,"code":403}}).into(),
        ],
    )
    .await;
    let receipt = receipt(&f).await;
    f.client.liked_ids().await.unwrap();
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
    let before = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    let started = f
        .client
        .player_voice_song_download_start(
            &f.epoch,
            &f.play_id,
            &receipt,
            0,
            DownloadOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(started.accepted.len(), 1);
    f.http.wait(STARTUP.len() + 3).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while f.client.song_download_state(&f.epoch, "301").await.unwrap()
            != SongDownloadState::Failed
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(decision(&f, &receipt, 0).await, D::ExistingTask);
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
    assert_eq!(f.store.values("25", "download_tasks_v1").unwrap().len(), 1);
    let task = f
        .store
        .get("25", "download_tasks_v1", &started.accepted[0])
        .unwrap()
        .unwrap();
    assert_eq!(task["song_id"], "301");
    assert_eq!(task["origin"]["kind"], "song");
    assert!(task["target"].is_null());
    let after = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    assert_eq!(before, after);
    calls(&f, &[CARD, LIKED, AUTHORIZATION]);
    let call = &f.http.calls()[STARTUP.len() + 2];
    assert_eq!(call.payload["id"], "301");
    assert_eq!(call.payload["level"], "standard");
    assert_eq!(call.payload["immerseType"], "c51");
}

#[tokio::test]
async fn old_play_and_replaced_receipt_cannot_reuse_a_ready_download_dialog() {
    let f = Fixture::new(
        &[COOKIE],
        [
            card().into(),
            json!({"code":200,"ids":[301]}).into(),
            card().into(),
        ],
    )
    .await;
    let old = receipt(&f).await;
    f.client.liked_ids().await.unwrap();
    assert_eq!(decision(&f, &old, 0).await, D::Ready);
    let current = receipt(&f).await;
    assert!(matches!(
        f.client
            .player_voice_song_download_start(
                &f.epoch,
                &f.play_id,
                &old,
                0,
                DownloadOptions::default()
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(decision(&f, &current, 0).await, D::Ready);
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(matches!(
        f.client
            .player_voice_song_download_start(
                &f.epoch,
                &f.play_id,
                &current,
                0,
                DownloadOptions::default()
            )
            .await,
        Err(Error::StaleResponse)
    ));
    calls(&f, &[CARD, LIKED, CARD]);
    assert!(
        f.store
            .values("25", "download_tasks_v1")
            .unwrap()
            .is_empty()
    );
}
