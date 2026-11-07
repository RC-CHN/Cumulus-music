use super::*;
use cumulus_core::{
    client::dto::{
        DownloadResult, VoiceSongGroupDownloadConfirmation as C, VoiceSongGroupDownloadPlan,
    },
    protocol::pc_experiments::ASSIGNMENT_PATH,
};

async fn prepare(
    f: &Fixture,
    card: &str,
    anchor: usize,
    order: &[usize],
) -> VoiceSongGroupDownloadPlan {
    f.client
        .player_voice_songs_download_prepare(&f.epoch, &f.play_id, card, anchor, order)
        .await
        .unwrap()
}

async fn confirm(
    f: &Fixture,
    card: &str,
    receipt: &str,
    quality: Quality,
) -> cumulus_core::Result<C> {
    f.client
        .player_voice_songs_download_confirm_quality(&f.epoch, &f.play_id, card, receipt, quality)
        .await
}

fn denial() -> HttpResponse {
    json!({"code":200,"data":{"code":403}}).into()
}

fn no_tasks(f: &Fixture) {
    assert!(f.store.values("25", TASKS).unwrap().is_empty());
    assert!(f.store.values("25", BATCH).unwrap().is_empty());
}

fn calls(f: &Fixture, expected: &[&str]) {
    assert_eq!(
        f.http
            .calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        STARTUP
            .into_iter()
            .chain([CARD])
            .chain(expected.iter().copied())
            .collect::<Vec<_>>()
    );
}

async fn assert_tier(
    f: &Fixture,
    result: &DownloadResult,
    request_offset: usize,
    songs: &[&str],
    quality: Quality,
) {
    let new = DownloadService::new(f.session.clone())
        .unwrap()
        .list()
        .unwrap()
        .into_iter()
        .filter(|task| result.accepted.contains(&task.id))
        .collect::<Vec<_>>();
    assert_eq!(
        new.iter().map(|task| task.id.as_str()).collect::<Vec<_>>(),
        result.accepted
    );
    assert_eq!(
        new.iter()
            .map(|task| task.song_id.as_str())
            .collect::<Vec<_>>(),
        songs
    );
    assert!(new.iter().all(|task| task.options.quality == quality));
    f.http.wait(request_offset + songs.len()).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        for song in songs {
            while f.client.song_download_state(&f.epoch, song).await.unwrap()
                != SongDownloadState::Failed
            {
                tokio::task::yield_now().await;
            }
        }
    })
    .await
    .unwrap();
    let observed = f.http.calls();
    let mut ids = observed[request_offset..]
        .iter()
        .map(|call| {
            assert_eq!(call.path, AUTH);
            assert_eq!(call.payload["level"], quality.as_str());
            assert_eq!(call.payload["immerseType"], "c51");
            assert!(
                call.cookie
                    .as_deref()
                    .unwrap()
                    .contains("MUSIC_U=synthetic-voice")
            );
            let header: serde_json::Value =
                serde_json::from_str(call.payload["header"].as_str().unwrap()).unwrap();
            assert_eq!(header["os"], "pc");
            assert_eq!(header["clientSign"], "synthetic-signature");
            call.payload["id"].as_str().unwrap()
        })
        .collect::<Vec<_>>();
    ids.sort_unstable();
    let mut expected = songs.to_vec();
    expected.sort_unstable();
    assert_eq!(ids, expected);
}

#[tokio::test]
async fn mixed_group_uses_one_descending_purchase_union_and_one_exact_tier_for_all_rows() {
    use Quality::*;
    let (f, card) = fixture([
        json!({"code":200,"ids":[303]}).into(),
        denial(),
        denial(),
        denial(),
    ])
    .await;
    f.client.liked_ids().await.unwrap();
    configure(&f).await;
    existing(&f, 313, "complete");
    let queue = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    let preference = f.client.download_quality_preferences(&f.epoch).unwrap();

    let plan = prepare(&f, &card, 2, &[11, 2, 0, 13, 10]).await;
    assert_eq!(plan.decision, D::PurchasedQualityRequired);
    assert_eq!(plan.eligible_indices, [11, 0, 10]);
    assert_eq!(plan.omitted_indices, [2, 13]);
    // Sky belongs only to 311; 310 has HiRes, and 301 is ordinary.
    // The completed 313 must not contribute JyMaster/Lossless.
    assert_eq!(plan.purchased_qualities, [Sky, HiRes, ExHigh, Standard]);
    let receipt = plan.selection_receipt.unwrap();
    assert!(matches!(
        f.client
            .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &receipt)
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        confirm(&f, &card, &receipt, JyMaster).await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(f.store.values("25", TASKS).unwrap().len(), 1);
    calls(&f, &[LIKED]);

    let C::Started { result } = confirm(&f, &card, &receipt, Sky).await.unwrap() else {
        panic!("expected explicit group start");
    };
    assert_eq!(result.operation_id.as_deref(), Some(receipt.as_str()));
    assert_tier(&f, &result, STARTUP.len() + 2, &["311", "301", "310"], Sky).await;
    let C::Started { result: replay } = confirm(&f, &card, &receipt, Sky).await.unwrap() else {
        panic!("expected committed result");
    };
    assert_eq!(replay.accepted, result.accepted);
    assert!(matches!(
        confirm(&f, &card, &receipt, HiRes).await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(f.store.values("25", BATCH).unwrap().len(), 1);
    assert_eq!(f.store.values("25", TASKS).unwrap().len(), 4);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
    let after = f.client.download_quality_preferences(&f.epoch).unwrap();
    assert_eq!(after.revision, preference.revision);
    assert_eq!(after.options.quality, preference.options.quality);
    assert_eq!(
        queue,
        serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap()
    );
    calls(&f, &[LIKED, AUTH, AUTH, AUTH]);
}

#[tokio::test]
async fn purchased_only_group_checks_clicked_liked_anchor_and_directory_after_explicit_quality() {
    use Quality::*;
    let (f, card) = fixture([
        json!({"code":200,"ids":[310]}).into(),
        json!({"code":200,"ids":[311]}).into(),
        denial(),
        denial(),
    ])
    .await;
    for (anchor, rows) in [(10, &[10, 2][..]), (10, &[10, 12]), (10, &[10, 8])] {
        let rejected = prepare(&f, &card, anchor, rows).await;
        assert_eq!(rejected.decision, D::UnsupportedSelection);
        assert!(rejected.selection_receipt.is_none());
        assert!(rejected.purchased_qualities.is_empty());
    }
    let plan = prepare(&f, &card, 11, &[11, 10]).await;
    assert_eq!(plan.decision, D::PurchasedQualityRequired);
    assert_eq!(plan.purchased_qualities, [Sky, HiRes, ExHigh, Standard]);
    let receipt = plan.selection_receipt.unwrap();
    assert!(matches!(
        confirm(&f, &card, &receipt, HiRes).await.unwrap(),
        C::MembershipUnknown
    ));
    no_tasks(&f);
    calls(&f, &[]);

    f.client.liked_ids().await.unwrap();
    assert!(matches!(
        confirm(&f, &card, &receipt, HiRes).await.unwrap(),
        C::PlaylistRequired
    ));
    f.client.liked_ids().await.unwrap();
    assert!(matches!(
        confirm(&f, &card, &receipt, HiRes).await.unwrap(),
        C::DirectoryRequired
    ));
    no_tasks(&f);
    assert!(!f.root.path().join("output").exists());
    calls(&f, &[LIKED, LIKED]);

    configure(&f).await;
    let C::Started { result } = confirm(&f, &card, &receipt, HiRes).await.unwrap() else {
        panic!("configured direct group should start");
    };
    assert_tier(&f, &result, STARTUP.len() + 3, &["311", "310"], HiRes).await;
    calls(&f, &[LIKED, LIKED, AUTH, AUTH]);
}

fn assignment(quality_group: &str) -> HttpResponse {
    json!({"code":200,"data":[
        {"expName":"PH-PC-hyyinzhi","expGroupName":quality_group},
        {"expName":"PH-PC-zhenyingn","expGroupName":"t1"}
    ]})
    .into()
}

#[tokio::test]
async fn purchased_group_requires_current_vivid_cohort_and_revokes_cancelled_or_rotated_receipts() {
    use Quality::*;
    let (f, card) = fixture([assignment("t1"), assignment("c")]).await;
    let cold = prepare(&f, &card, 0, &[10, 0]).await;
    assert!(!cold.purchased_qualities.contains(&Vivid));
    f.client
        .official_pc_ordinary_skip_assignment(&f.epoch)
        .await
        .unwrap();
    let enabled = prepare(&f, &card, 0, &[10, 0]).await;
    assert_eq!(
        enabled.purchased_qualities,
        [Vivid, HiRes, ExHigh, Standard]
    );
    assert!(matches!(
        confirm(&f, &card, &cold.selection_receipt.unwrap(), HiRes).await,
        Err(Error::StaleResponse)
    ));
    let receipt = enabled.selection_receipt.unwrap();
    f.client.refresh_pc_presentation().await.unwrap();
    assert!(matches!(
        confirm(&f, &card, &receipt, Vivid).await,
        Err(Error::StaleResponse)
    ));
    f.client
        .player_voice_songs_download_cancel(&f.epoch, &f.play_id, &card, &receipt)
        .unwrap();
    assert!(matches!(
        confirm(&f, &card, &receipt, HiRes).await,
        Err(Error::StaleResponse)
    ));

    let latest = prepare(&f, &card, 0, &[10, 0]).await;
    assert_eq!(latest.purchased_qualities, [HiRes, ExHigh, Standard]);
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(
            &target,
            &["MUSIC_U=rotated-synthetic-group; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    assert!(matches!(
        confirm(&f, &card, &latest.selection_receipt.unwrap(), HiRes).await,
        Err(Error::StaleResponse)
    ));
    no_tasks(&f);
    calls(&f, &[ASSIGNMENT_PATH, ASSIGNMENT_PATH]);
}
