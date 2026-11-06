use super::{
    HttpResponse, gate, reply,
    voice_detail::{fresh, read},
    voice_recommendations::{COOKIE, Fixture, HOLD, STARTUP},
};
use cumulus_core::{
    Error,
    account::resources::MutationPhase,
    client::dto::{PlayerAction, VinylVoiceDetailTarget},
    podcast::{PodcastAction, PodcastMutation, PodcastService},
    protocol::{api::Api, http::ApiOrigin},
};
use serde_json::{Value, json};
use std::{future::poll_fn, task::Poll};

const DETAIL: &str = "/api/dj/program/detail";
const RADIO: &str = "/api/djradio/v3/get";
const SUB: &str = "/api/djradio/sub";
const UNSUB: &str = "/api/djradio/unsub";

fn other_episode() -> Value {
    let mut value = fresh();
    value["program"]["dj"]["userId"] = json!(55);
    value
}

fn radio(subscribed: bool) -> Value {
    json!({"code":200,"data":{"id":"801","name":"合成新播单","subed":subscribed,
        "dj":{"userId":55,"nickname":"合成主播"}}})
}

fn journal(f: &Fixture) -> Option<PodcastMutation> {
    PodcastService::new(f.session.clone())
        .unwrap()
        .operation_for(&f.session.token().unwrap(), PodcastAction::Subscribe, "801")
        .unwrap()
}

fn wire(f: &Fixture, paths: &[&str]) {
    wire_after(f, STARTUP.len(), paths);
}

fn wire_after(f: &Fixture, startup: usize, paths: &[&str]) {
    let calls = f.http.calls();
    let calls = &calls[startup..];
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        paths
    );
    for call in calls {
        let mut body = call.payload.clone();
        let header = body.as_object_mut().unwrap().remove("header").unwrap();
        assert!(header.is_string());
        assert!(
            call.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-voice-rcmd")
        );
        assert_eq!(
            body,
            match call.path.as_str() {
                DETAIL => json!({"id":"71"}),
                RADIO | SUB | UNSUB => json!({"id":"801"}),
                HOLD => json!({}),
                _ => unreachable!(),
            }
        );
    }
}

fn scoped_cookie(f: &Fixture, cookie: &str) {
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(&target, &[cookie.into()], None, &revision)
        .unwrap();
}

#[tokio::test]
async fn fresh_voice_detail_actions_route_and_write_only_the_bound_fresh_ids() {
    let f = Fixture::with_existing_voice_targets(
        &[COOKIE],
        [
            other_episode().into(),
            radio(false).into(),
            json!({"code":200}).into(),
            radio(true).into(),
            radio(true).into(),
            radio(true).into(),
            json!({"code":200}).into(),
            radio(false).into(),
            radio(false).into(),
        ],
    )
    .await;
    let original = f
        .client
        .player_snapshot(&f.epoch)
        .await
        .unwrap()
        .voice_detail
        .unwrap();
    assert_eq!(
        original.creator.and_then(|creator| creator.id).as_deref(),
        Some("54")
    );
    assert_eq!(original.radio.map(|radio| radio.id).as_deref(), Some("800"));
    let receipt = read(&f).await.unwrap().action_receipt.unwrap();
    let resolve = |target| {
        f.client
            .player_vinyl_voice_detail_target(&f.epoch, &f.play_id, &receipt, target)
    };
    assert_eq!(resolve(VinylVoiceDetailTarget::Creator).unwrap(), "55");
    assert_eq!(resolve(VinylVoiceDetailTarget::Radio).unwrap(), "801");
    assert!(matches!(
        f.client.player_vinyl_voice_detail_target(
            &f.epoch,
            &f.play_id,
            "forged",
            VinylVoiceDetailTarget::Radio
        ),
        Err(Error::StaleResponse)
    ));
    wire_after(&f, STARTUP.len() + 1, &[DETAIL]);
    for expected in [false, true] {
        let updated = f
            .client
            .change_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &receipt, expected)
            .await
            .unwrap();
        assert_eq!(updated.epoch, f.epoch);
        assert_eq!(updated.value.id, "801");
        assert_eq!(journal(&f).unwrap().phase, MutationPhase::Confirmed);
        assert_eq!(journal(&f).unwrap().observed, Some(!expected));
    }
    let current = f.client.player_snapshot(&f.epoch).await.unwrap();
    assert_eq!(current.play_id.as_deref(), Some(f.play_id.as_str()));
    wire_after(
        &f,
        STARTUP.len() + 1,
        &[DETAIL, RADIO, SUB, RADIO, RADIO, RADIO, UNSUB, RADIO, RADIO],
    );
}

#[tokio::test]
async fn fresh_voice_detail_actions_use_fresh_episode_owner_and_keep_the_radio_link() {
    let f = Fixture::new(&[COOKIE], [fresh().into()]).await;
    let view = read(&f).await.unwrap();
    assert!(view.detail.radio.unwrap().owned_by_viewer);
    let receipt = view.action_receipt.unwrap();
    assert_eq!(
        f.client
            .player_vinyl_voice_detail_target(
                &f.epoch,
                &f.play_id,
                &receipt,
                VinylVoiceDetailTarget::Radio
            )
            .unwrap(),
        "801"
    );
    assert!(matches!(
        f.client
            .change_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &receipt, false)
            .await,
        Err(Error::InvalidInput("cannot subscribe to an owned podcast"))
    ));
    assert!(journal(&f).is_none());
    wire(&f, &[DETAIL]);
}

#[tokio::test]
async fn fresh_voice_detail_actions_do_not_mint_from_a_foreign_profile_cookie_scope() {
    let f = Fixture::new(&[COOKIE], [other_episode().into()]).await;
    scoped_cookie(
        &f,
        "MUSIC_U=synthetic-other; Path=/eapi/w/nuser/account/; Secure",
    );
    let view = read(&f).await.unwrap();
    assert_eq!(view.detail.radio.unwrap().id, "801");
    assert!(view.action_receipt.is_none());
    wire(&f, &[DETAIL]);
}

#[tokio::test]
async fn fresh_voice_detail_actions_reject_a_foreign_write_cookie_before_post() {
    let f = Fixture::new(&[COOKIE], [other_episode().into(), radio(false).into()]).await;
    scoped_cookie(
        &f,
        "MUSIC_U=synthetic-other; Path=/eapi/djradio/sub; Secure",
    );
    let receipt = read(&f).await.unwrap().action_receipt.unwrap();
    let Err(error) = f
        .client
        .change_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &receipt, false)
        .await
    else {
        panic!("foreign cookie authorized the write")
    };
    assert!(matches!(error, Error::RequestNotSent(_)), "{error:?}");
    assert_eq!(journal(&f).unwrap().phase, MutationPhase::Rejected);
    wire(&f, &[DETAIL, RADIO]);
}

#[tokio::test]
async fn fresh_voice_detail_actions_reject_a_receipt_during_new_read_and_after_rotation() {
    let (held, release) = gate();
    let f = Fixture::new(&[COOKIE], [other_episode().into(), held]).await;
    let old = read(&f).await.unwrap().action_receipt.unwrap();
    let mut reread = Box::pin(read(&f));
    assert!(
        poll_fn(|cx| Poll::Ready(reread.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.http.wait(STARTUP.len() + 2).await;
    assert!(matches!(
        f.client.player_vinyl_voice_detail_target(
            &f.epoch,
            &f.play_id,
            &old,
            VinylVoiceDetailTarget::Creator
        ),
        Err(Error::StaleResponse)
    ));
    assert!(release.send(Ok(reply(other_episode()))).is_ok());
    let new = reread.await.unwrap().action_receipt.unwrap();
    assert_ne!(old, new);
    f.rotate();
    assert!(matches!(
        f.client.player_vinyl_voice_detail_target(
            &f.epoch,
            &f.play_id,
            &new,
            VinylVoiceDetailTarget::Creator
        ),
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        f.client
            .change_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &new, false)
            .await,
        Err(Error::StaleResponse)
    ));
    wire(&f, &[DETAIL, DETAIL]);
}

#[tokio::test]
async fn fresh_voice_detail_actions_recheck_play_at_actual_write_gate() {
    let (read_held, read_release) = gate();
    let (gate_held, gate_release) = gate();
    let f = Fixture::new(&[COOKIE], [other_episode().into(), read_held, gate_held]).await;
    let receipt = read(&f).await.unwrap().action_receipt.unwrap();
    let mut write = Box::pin(
        f.client
            .change_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &receipt, false),
    );
    assert!(
        poll_fn(|cx| Poll::Ready(write.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.http.wait(STARTUP.len() + 2).await;
    let mut occupying = Box::pin(f.native.request(HOLD, json!({}), false));
    assert!(
        poll_fn(|cx| Poll::Ready(occupying.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(read_release.send(Ok(reply(radio(false)))).is_ok());
    assert!(
        poll_fn(|cx| Poll::Ready(write.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(
        poll_fn(|cx| Poll::Ready(occupying.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.http.wait(STARTUP.len() + 3).await;
    assert_eq!(journal(&f).unwrap().phase, MutationPhase::Sending);
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(gate_release.send(Ok(reply(json!({"code":200})))).is_ok());
    occupying.await.unwrap();
    assert!(matches!(write.await, Err(Error::StaleResponse)));
    assert_eq!(journal(&f).unwrap().phase, MutationPhase::Rejected);
    wire(&f, &[DETAIL, RADIO, HOLD]);
}

#[tokio::test]
async fn fresh_voice_detail_actions_reconcile_lost_ack_without_replaying_the_write() {
    let f = Fixture::new(
        &[COOKIE],
        [
            other_episode().into(),
            radio(false).into(),
            HttpResponse::Ready(Err(Error::Transport("sealed lost acknowledgement".into()))),
            radio(true).into(),
            radio(true).into(),
        ],
    )
    .await;
    let receipt = read(&f).await.unwrap().action_receipt.unwrap();
    let change = || {
        f.client
            .change_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &receipt, false)
    };
    assert!(matches!(change().await, Err(Error::Transport(_))));
    assert_eq!(journal(&f).unwrap().phase, MutationPhase::Uncertain);
    assert!(matches!(change().await, Err(Error::MutationUncertain)));
    let recovered = f
        .client
        .reconcile_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &receipt)
        .await
        .unwrap();
    assert_eq!(recovered.value.id, "801");
    assert_eq!(journal(&f).unwrap().phase, MutationPhase::Confirmed);
    assert_eq!(journal(&f).unwrap().observed, Some(true));
    wire(&f, &[DETAIL, RADIO, SUB, RADIO, RADIO]);
}

#[tokio::test]
async fn fresh_voice_detail_actions_keep_ack_when_the_same_episode_detail_is_replaced() {
    let (held, release) = gate();
    let f = Fixture::new(
        &[COOKIE],
        [
            other_episode().into(),
            radio(false).into(),
            held,
            other_episode().into(),
            radio(true).into(),
            radio(true).into(),
        ],
    )
    .await;
    let old = read(&f).await.unwrap().action_receipt.unwrap();
    let mut write = Box::pin(
        f.client
            .change_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &old, false),
    );
    assert!(
        poll_fn(|cx| Poll::Ready(write.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.http.wait(STARTUP.len() + 3).await;
    let mut reread = Box::pin(read(&f));
    assert!(
        poll_fn(|cx| Poll::Ready(reread.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    assert!(matches!(write.await, Err(Error::StaleResponse)));
    let pending = journal(&f).unwrap();
    assert!(pending.acknowledged);
    assert_eq!(pending.phase, MutationPhase::Verifying);
    let new = reread.await.unwrap().action_receipt.unwrap();
    assert_ne!(old, new);
    assert!(matches!(
        f.client
            .reconcile_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &old)
            .await,
        Err(Error::StaleResponse)
    ));
    let result = f
        .client
        .reconcile_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &new)
        .await
        .unwrap();
    assert_eq!(result.value.phase, MutationPhase::Confirmed);
    wire(&f, &[DETAIL, RADIO, SUB, DETAIL, RADIO, RADIO]);
}

#[tokio::test]
async fn fresh_voice_detail_actions_record_ack_only_for_the_original_account() {
    let (held, release) = gate();
    let f = Fixture::new(
        &[COOKIE],
        [other_episode().into(), radio(false).into(), held],
    )
    .await;
    let receipt = read(&f).await.unwrap().action_receipt.unwrap();
    let mut write = Box::pin(
        f.client
            .change_fresh_vinyl_podcast_subscription(&f.epoch, &f.play_id, &receipt, false),
    );
    assert!(
        poll_fn(|cx| Poll::Ready(write.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.http.wait(STARTUP.len() + 3).await;
    let adopt = |uid| {
        f.session.adopt(
            f.native.clone(),
            json!({"userId":uid}),
            &f.session.token().unwrap(),
        )
    };
    adopt(26).unwrap();
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    assert!(matches!(write.await, Err(Error::StaleResponse)));
    assert!(journal(&f).is_none());
    adopt(25).unwrap();
    let original = journal(&f).unwrap();
    assert_eq!(original.phase, MutationPhase::Verifying);
    assert!(original.acknowledged);
    wire(&f, &[DETAIL, RADIO, SUB]);
}
