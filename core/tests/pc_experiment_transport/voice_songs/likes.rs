use super::{
    ApiOrigin, CARD, COOKIE, Error, Fixture, PlayerAction, STARTUP, gate, json, playable_card,
    reply,
};
use cumulus_core::{account::resources::MutationPhase, client::dto::LikeRecoveryCommand};

const LIKED: &str = "/api/song/like/get";
const LIKE: &str = "/api/song/like";

async fn receipt(f: &Fixture) -> String {
    f.client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap()
        .receipt
        .unwrap()
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
async fn like_sends_only_the_selected_ordinary_song_without_changing_episode_playback() {
    let f = Fixture::new(
        &[COOKIE],
        [
            playable_card().into(),
            json!({"code":200,"ids":[]}).into(),
            json!({"code":200}).into(),
            json!({"code":200,"ids":[301]}).into(),
            json!({"code":200,"ids":[301]}).into(),
        ],
    )
    .await;
    let receipt = receipt(&f).await;
    let before = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    // Row zero's audio is unavailable online; liking is still an ordinary
    // account operation and cannot target the program/backing song instead.
    let result = f
        .client
        .player_voice_song_like(&f.epoch, &f.play_id, &receipt, 0, true)
        .await
        .unwrap();
    assert_eq!(result.epoch, f.epoch);
    assert_eq!(result.value.id, "301");
    assert_eq!(result.value.phase, MutationPhase::Confirmed);
    assert_eq!(result.value.observed, Some(true));
    let unchanged = f
        .client
        .player_voice_song_like(&f.epoch, &f.play_id, &receipt, 0, true)
        .await
        .unwrap();
    assert_eq!(unchanged.value.phase, MutationPhase::Unchanged);
    assert_eq!(unchanged.value.observed, Some(true));
    let after = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    assert_eq!(before, after);
    assert_eq!(
        f.client
            .player_snapshot(&f.epoch)
            .await
            .unwrap()
            .play_id
            .as_deref(),
        Some(f.play_id.as_str())
    );
    calls(&f, &[CARD, LIKED, LIKE, LIKED, LIKED]);
    let write = &f.http.calls()[STARTUP.len() + 2];
    assert_eq!(write.payload["trackId"], "301");
    assert_eq!(write.payload["userid"], "25");
    assert_eq!(write.payload["like"], "true");
    assert!(write.payload.get("rqRefer").is_none());
    assert!(write.payload.get("songRqRefer").is_none());
    assert_eq!(f.client.like_recovery(&f.epoch, 0).unwrap().value.total, 0);
}

#[tokio::test]
async fn like_rejects_replaced_receipt_invalid_index_and_rotated_same_account_credentials() {
    let f = Fixture::new(&[COOKIE], [playable_card().into(), playable_card().into()]).await;
    let old = receipt(&f).await;
    let current = receipt(&f).await;
    assert_ne!(old, current);
    assert!(matches!(
        f.client
            .player_voice_song_like(&f.epoch, &f.play_id, &old, 0, true)
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        f.client
            .player_voice_song_like(&f.epoch, &f.play_id, &current, 2, true)
            .await,
        Err(Error::InvalidInput(_))
    ));
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(
            &target,
            &["MUSIC_U=revoked-old-voice-like; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    assert!(matches!(
        f.client
            .player_voice_song_like(&f.epoch, &f.play_id, &current, 0, true)
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.session.token().unwrap().generation.to_string(), f.epoch);
    calls(&f, &[CARD, CARD]);
    assert!(
        f.store
            .values("25", "resource-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn like_cannot_dispatch_after_the_displayed_episode_stops_during_membership_read() {
    let (held, release) = gate();
    let f = Fixture::new(&[COOKIE], [playable_card().into(), held]).await;
    let receipt = receipt(&f).await;
    let (client, epoch, play_id) = (f.client.clone(), f.epoch.clone(), f.play_id.clone());
    let old_receipt = receipt.clone();
    let action = tokio::spawn(async move {
        client
            .player_voice_song_like(&epoch, &play_id, &receipt, 0, true)
            .await
    });
    f.http.wait(STARTUP.len() + 2).await;
    let stopped = f
        .client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert_ne!(stopped.play_id.as_deref(), Some(f.play_id.as_str()));
    assert!(
        release
            .send(Ok(reply(json!({"code":200,"ids":[]}))))
            .is_ok()
    );
    assert!(matches!(action.await.unwrap(), Err(Error::StaleResponse)));
    assert!(matches!(
        f.client
            .player_voice_song_like(&f.epoch, &f.play_id, &old_receipt, 0, true)
            .await,
        Err(Error::StaleResponse)
    ));
    calls(&f, &[CARD, LIKED]);
    assert!(
        f.store
            .values("25", "resource-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn like_preflight_cookie_rotation_revokes_the_card_without_switching_account() {
    let (held, release) = gate();
    let f = Fixture::new(&[COOKIE], [playable_card().into(), held]).await;
    let receipt = receipt(&f).await;
    let (client, epoch, play_id) = (f.client.clone(), f.epoch.clone(), f.play_id.clone());
    let action = tokio::spawn(async move {
        client
            .player_voice_song_like(&epoch, &play_id, &receipt, 0, true)
            .await
    });
    f.http.wait(STARTUP.len() + 2).await;
    let target = ApiOrigin::Pc.url();
    let prior = f.credentials.request_credentials(&target).unwrap().revision;
    let mut response = reply(json!({"code":200,"ids":[]}));
    response.set_cookies = vec!["MUSIC_U=preflight-rotation; Path=/; Secure".into()];
    assert!(release.send(Ok(response)).is_ok());
    assert!(matches!(action.await.unwrap(), Err(Error::StaleResponse)));
    assert_ne!(
        f.credentials.request_credentials(&target).unwrap().revision,
        prior
    );
    assert_eq!(f.session.token().unwrap().generation.to_string(), f.epoch);
    assert_eq!(
        f.client
            .player_snapshot(&f.epoch)
            .await
            .unwrap()
            .play_id
            .as_deref(),
        Some(f.play_id.as_str())
    );
    calls(&f, &[CARD, LIKED]);
    assert!(
        f.store
            .values("25", "resource-mutations")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn lost_song_card_ack_uses_the_existing_independent_read_back_without_replay() {
    let (held, release) = gate();
    let f = Fixture::new(
        &[COOKIE],
        [
            playable_card().into(),
            json!({"code":200,"ids":[]}).into(),
            held,
            json!({"code":200,"ids":[301]}).into(),
        ],
    )
    .await;
    let receipt = receipt(&f).await;
    let (client, epoch, play_id) = (f.client.clone(), f.epoch.clone(), f.play_id.clone());
    let current_receipt = receipt.clone();
    let action = tokio::spawn(async move {
        client
            .player_voice_song_like(&epoch, &play_id, &receipt, 0, true)
            .await
    });
    f.http.wait(STARTUP.len() + 3).await;
    assert!(
        release
            .send(Err(Error::Transport(
                "synthetic lost like acknowledgment".into()
            )))
            .is_ok()
    );
    assert!(matches!(action.await.unwrap(), Err(Error::Transport(_))));
    assert!(matches!(
        f.client
            .player_voice_song_like(&f.epoch, &f.play_id, &current_receipt, 0, false)
            .await,
        Err(Error::MutationUncertain)
    ));
    calls(&f, &[CARD, LIKED, LIKE]);

    // Recovery outlives the SongCard/episode and accepts no replacement write.
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    let pending = f.client.like_recovery(&f.epoch, 0).unwrap().value;
    assert_eq!(pending.total, 1);
    let [pending] = pending.operations.as_slice() else {
        panic!("exactly one pending song like")
    };
    assert_eq!(pending.id, "301");
    assert_eq!(pending.phase, MutationPhase::Uncertain);
    let resolved = f
        .client
        .recover_like(
            &f.epoch,
            LikeRecoveryCommand::Reconcile {
                id: pending.id.clone(),
                revision: pending.revision.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(resolved.value.phase, MutationPhase::Confirmed);
    assert_eq!(resolved.value.observed, Some(true));
    assert_eq!(f.client.like_recovery(&f.epoch, 0).unwrap().value.total, 0);
    calls(&f, &[CARD, LIKED, LIKE, LIKED]);
}
