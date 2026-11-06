use super::{
    gate, reply,
    voice_recommendations::{COOKIE, Fixture, HOLD, SONG, STARTUP, VOICE},
};
use cumulus_core::{
    Error, Result,
    client::dto::{PlayerAction, VinylVoiceDetailView},
    protocol::{api::Api, http::ApiOrigin},
};
use serde_json::{Value, json};
use std::{future::poll_fn, task::Poll};

const DETAIL: &str = "/api/dj/program/detail";

async fn read(f: &Fixture) -> Result<VinylVoiceDetailView> {
    f.client
        .player_vinyl_voice_detail(&f.epoch, &f.play_id)
        .await
}

fn fresh() -> Value {
    json!({"code":200,"program":{
        "id":VOICE,"programId":VOICE,"resourceId":VOICE,
        "mainTrackId":SONG,"trackId":SONG,"mainSong":{"id":SONG,"private":"DO_NOT_EXPORT"},
        "name":"","voiceName":"新读标题",
        "coverUrl":"https://p1.music.126.net/not-the-selected-artwork.png",
        "listenerCount":0,"playCount":"00123","adjustedPlayCount":1000,
        "description":"第一段\n\n第二段",
        "createTime":1700049600000_u64,
        "dj":{"userId":25,"nickname":"新读主播","private":"DO_NOT_EXPORT"},
        "radio":{"id":"801","voiceListName":"新读播单","subCount":7,
            "picUrl":"https://p1.music.126.net/radio.png?key=DO_NOT_EXPORT"},
        "audio":{"id":"802","name":"不可信的备用播单"},
        "authorization":"DO_NOT_EXPORT"
    }})
}

fn assert_wire(f: &Fixture, expected: usize) {
    let calls = f.http.calls();
    let requests = &calls[STARTUP.len()..];
    assert_eq!(requests.len(), expected);
    for call in requests {
        assert_eq!(call.path, DETAIL);
        let mut payload = call.payload.clone();
        assert!(
            payload
                .as_object_mut()
                .unwrap()
                .remove("header")
                .unwrap()
                .is_string()
        );
        assert_eq!(payload, json!({"id":VOICE}));
        assert!(
            call.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-voice-rcmd")
        );
        assert!(call.login_chain.is_none());
    }
}

#[tokio::test]
async fn fresh_voice_detail_projects_signed_display_only_without_changing_playback() {
    let minimal = json!({"code":200,"program":{
        "voiceId":VOICE,"trackId":SONG,"listenerCount":0,
        "radio":{"id":"not-a-radio-id"},"audio":{"id":"803","name":"伪造播单"}
    }});
    let f = Fixture::new(&[COOKIE], [fresh().into(), minimal.into()]).await;
    let before = f
        .client
        .player_action(&f.epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(read(&f).await.unwrap()).unwrap(),
        json!({
            "epoch":f.epoch,"playId":f.play_id,"entryId":before.entry_id,"voiceId":VOICE,"title":"新读标题",
            "detail":{"id":VOICE,"creator":{"id":"25","name":"新读主播"},"releaseTime":1700049600000_u64,
                "description":"第一段\n\n第二段","coverUrl":"https://p1.music.126.net/current.png",
                "playCount":"00123","radio":{"id":"801","name":"新读播单",
                    "coverUrl":"https://p1.music.126.net/radio.png?param=320y320",
                    "subscriberCount":7,"ownedByViewer":true}}
        })
    );
    assert_eq!(
        serde_json::to_value(read(&f).await.unwrap()).unwrap(),
        json!({
            "epoch":f.epoch,"playId":f.play_id,"entryId":before.entry_id,"voiceId":VOICE,
            "detail":{"id":VOICE,"creator":null,"releaseTime":null,
                "coverUrl":"https://p1.music.126.net/current.png"}
        })
    );
    let after = f.client.player_snapshot(&f.epoch).await.unwrap();
    assert_eq!(after.voice_detail, before.voice_detail);
    assert_eq!(after.play_id, before.play_id);
    assert_eq!(after.entry_id, before.entry_id);
    assert_eq!(after.queue_revision, before.queue_revision);
    assert_eq!(after.state, before.state);
    assert_eq!(after.position, before.position);
    assert_wire(&f, 2);
}

#[tokio::test]
async fn fresh_voice_detail_rejects_foreign_episode_and_backing_track_claims() {
    let mut replies = vec![json!({"code":200,"program":{"id":VOICE}}).into()];
    for field in ["id", "programId", "resourceId", "mainTrackId", "trackId"] {
        let mut invalid = fresh();
        invalid["program"][field] = json!("902");
        replies.push(invalid.into());
    }
    let mut invalid = fresh();
    invalid["program"]["mainSong"]["id"] = json!("902");
    replies.push(invalid.into());
    let expected = replies.len();
    let f = Fixture::new(&[COOKIE], replies).await;
    for _ in 0..expected {
        assert!(matches!(
            read(&f).await,
            Err(Error::Response("vinyl voice detail identity mismatch"))
        ));
    }
    assert_wire(&f, expected);
}

#[tokio::test]
async fn fresh_voice_detail_drops_held_replies_after_stop_cookie_rotation_and_account_adopt() {
    for action in ["stop", "cookie", "account"] {
        let (held, release) = gate();
        let f = Fixture::new(&[COOKIE], [held]).await;
        let mut pending = Box::pin(read(&f));
        assert!(
            poll_fn(|cx| Poll::Ready(pending.as_mut().poll(cx)))
                .await
                .is_pending()
        );
        f.http.wait(STARTUP.len() + 1).await;
        match action {
            "stop" => {
                f.client
                    .player_action(&f.epoch, PlayerAction::Stop)
                    .await
                    .unwrap();
            }
            "cookie" => f.rotate(),
            "account" => {
                f.session
                    .adopt(
                        f.native.clone(),
                        json!({"userId":26}),
                        &f.session.token().unwrap(),
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(release.send(Ok(reply(fresh()))).is_ok());
        assert!(
            matches!(pending.await, Err(Error::StaleResponse)),
            "{action}"
        );
        assert_wire(&f, 1);
    }
}

#[tokio::test]
async fn fresh_voice_detail_does_not_dispatch_for_a_stale_play_or_mismatched_cookie_scope() {
    let f = Fixture::new(&[COOKIE], []).await;
    assert!(matches!(
        f.client.player_vinyl_voice_detail(&f.epoch, "stale").await,
        Err(Error::StaleResponse)
    ));
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(
            &target,
            &["MUSIC_U=other-scoped-cookie; Path=/eapi/dj/program/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    assert!(matches!(read(&f).await, Err(Error::StaleResponse)));
    assert_wire(&f, 0);
}

#[tokio::test]
async fn fresh_voice_detail_checks_scope_again_after_waiting_for_native_http() {
    let (held, release) = gate();
    let f = Fixture::new(&[COOKIE], [held]).await;
    let native = f.native.clone();
    let occupying = tokio::spawn(async move { native.request(HOLD, json!({}), false).await });
    f.http.wait(STARTUP.len() + 1).await;
    let mut pending = Box::pin(read(&f));
    assert!(
        poll_fn(|cx| Poll::Ready(pending.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(release.send(Ok(reply(json!({"code":200})))).is_ok());
    occupying.await.unwrap().unwrap();
    assert!(matches!(pending.await, Err(Error::StaleResponse)));
    assert_eq!(f.http.calls().len(), STARTUP.len() + 1);
    assert_eq!(f.http.calls().last().unwrap().path, HOLD);
}

#[tokio::test]
async fn a_new_fresh_voice_detail_request_supersedes_a_reply_for_the_same_play() {
    let (held, release) = gate();
    let mut newer = fresh();
    newer["program"]["name"] = json!("随后打开的节目详情");
    let f = Fixture::new(&[COOKIE], [held, newer.into()]).await;
    let mut old = Box::pin(read(&f));
    assert!(
        poll_fn(|cx| Poll::Ready(old.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.http.wait(STARTUP.len() + 1).await;
    let mut next = Box::pin(read(&f));
    assert!(
        poll_fn(|cx| Poll::Ready(next.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(release.send(Ok(reply(fresh()))).is_ok());
    assert!(matches!(old.await, Err(Error::StaleResponse)));
    assert_eq!(
        next.await.unwrap().title.as_deref(),
        Some("随后打开的节目详情")
    );
    assert_wire(&f, 2);
}
