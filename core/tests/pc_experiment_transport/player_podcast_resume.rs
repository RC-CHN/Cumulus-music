use super::{
    ASSIGNMENT_PATH, assignment_arguments, gate, reply,
    voice_recommendations::{COOKIE, Fixture, STARTUP},
};
use cumulus_core::{
    Error,
    client::dto::{PlayerAction, PlayerPodcastResumeAction},
    podcast::{PodcastAction, PodcastService},
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};

const READ: &str = "/api/dj/user/setting/resumeplay/get";
const WRITE: &str = "/api/dj/user/setting/resumeplay/update";
const RADIO: &str = "800";

fn assignment(group: &str) -> Value {
    json!({"code":200,"data":[{"expName":"Podcast_ResumePlaySwitch","expGroupName":group}]})
}

fn setting(enabled: bool) -> Value {
    json!({"code":200,"data":{"resumePlayValue":u8::from(enabled)}})
}

async fn current(f: &Fixture, entry: &str, action: PlayerPodcastResumeAction) -> Value {
    let result = f
        .client
        .player_podcast_resume(&f.epoch, &f.play_id, entry, action)
        .await
        .unwrap();
    assert_eq!(result.epoch, f.epoch);
    serde_json::to_value(result.value).unwrap()
}

fn calls(f: &Fixture) -> Vec<String> {
    f.http.calls()[STARTUP.len() + 1..]
        .iter()
        .map(|call| call.path.clone())
        .collect()
}

#[tokio::test]
async fn signed_resume_menu_is_passive_and_changes_only_the_current_radio() {
    let f = Fixture::with_existing_voice_targets(
        &[COOKIE],
        [
            assignment("t1"),
            setting(false),
            json!({"code":200}),
            setting(true),
            setting(true),
            assignment("c"),
        ]
        .map(Into::into),
    )
    .await;
    let before = f
        .client
        .player_action(&f.epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let entry = before.entry_id.as_deref().unwrap();
    assert!(
        current(&f, entry, PlayerPodcastResumeAction::View)
            .await
            .is_null()
    );
    assert!(calls(&f).is_empty());
    assert!(
        f.client
            .refresh_pc_presentation()
            .await
            .unwrap()
            .podcast_resume
    );
    assert_eq!(
        current(&f, entry, PlayerPodcastResumeAction::View).await,
        json!({"radioId":RADIO,"enabled":false,"pending":false})
    );
    assert_eq!(calls(&f), [ASSIGNMENT_PATH]);
    assert_eq!(
        current(
            &f,
            entry,
            PlayerPodcastResumeAction::Change { enabled: true }
        )
        .await,
        json!({"radioId":RADIO,"enabled":true,"pending":false})
    );
    let after = f.client.player_snapshot(&f.epoch).await.unwrap();
    assert_eq!(before.play_id, after.play_id);
    assert_eq!(before.entry_id, after.entry_id);
    assert_eq!(before.queue_revision, after.queue_revision);
    assert_eq!(before.state, after.state);
    assert_eq!(before.position, after.position);
    assert!(
        !f.client
            .refresh_pc_presentation()
            .await
            .unwrap()
            .podcast_resume
    );
    assert!(
        current(&f, entry, PlayerPodcastResumeAction::View)
            .await
            .is_null()
    );
    assert_eq!(
        calls(&f),
        [ASSIGNMENT_PATH, READ, WRITE, READ, READ, ASSIGNMENT_PATH]
    );
    for call in &f.http.calls()[STARTUP.len() + 1..] {
        let mut payload = call.payload.clone();
        assert!(
            payload
                .as_object_mut()
                .unwrap()
                .remove("header")
                .unwrap()
                .is_string()
        );
        assert_eq!(
            payload,
            match call.path.as_str() {
                ASSIGNMENT_PATH => assignment_arguments(),
                READ => json!({"radioId":RADIO}),
                WRITE => json!({"radioId":RADIO,"resumePlayValue":"1"}),
                _ => unreachable!(),
            }
        );
        assert!(
            call.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-voice-rcmd")
        );
    }
}

#[tokio::test]
async fn stopping_during_preflight_sends_no_update_and_does_not_leave_a_pending_write() {
    let (held, answer) = gate();
    let f = Fixture::with_existing_voice_targets(&[COOKIE], [assignment("t1").into(), held]).await;
    f.client.refresh_pc_presentation().await.unwrap();
    let before = f.client.player_snapshot(&f.epoch).await.unwrap();
    let entry = before.entry_id.unwrap();
    let task = f.client.player_podcast_resume(
        &f.epoch,
        &f.play_id,
        &entry,
        PlayerPodcastResumeAction::Change { enabled: true },
    );
    let (result, ()) = tokio::join!(task, async {
        f.http.wait(STARTUP.len() + 3).await;
        f.client
            .player_action(&f.epoch, PlayerAction::Stop)
            .await
            .unwrap();
        assert!(answer.send(Ok(reply(setting(false)))).is_ok());
    });
    assert!(matches!(result, Err(Error::StaleResponse)));
    assert_eq!(calls(&f), [ASSIGNMENT_PATH, READ]);
    assert!(
        PodcastService::new(f.session.clone())
            .unwrap()
            .operation_for(&f.session.token().unwrap(), PodcastAction::Resume, RADIO)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn ambiguous_resume_update_reconciles_by_reads_without_replaying() {
    let (held, answer) = gate();
    let f = Fixture::with_existing_voice_targets(
        &[COOKIE],
        [
            assignment("t1").into(),
            setting(false).into(),
            held,
            setting(true).into(),
            setting(true).into(),
        ],
    )
    .await;
    f.client.refresh_pc_presentation().await.unwrap();
    let before = f.client.player_snapshot(&f.epoch).await.unwrap();
    let entry = before.entry_id.unwrap();
    let task = f.client.player_podcast_resume(
        &f.epoch,
        &f.play_id,
        &entry,
        PlayerPodcastResumeAction::Change { enabled: true },
    );
    let (result, ()) = tokio::join!(task, async {
        f.http.wait(STARTUP.len() + 4).await;
        assert!(
            answer
                .send(Err(Error::Transport("lost synthetic reply".into())))
                .is_ok()
        );
    });
    assert!(matches!(result, Err(error) if matches!(error.root(), Error::Transport(_))));
    assert_eq!(
        current(&f, &entry, PlayerPodcastResumeAction::View).await,
        json!({"radioId":RADIO,"enabled":false,"pending":true})
    );
    assert!(matches!(
        f.client
            .player_podcast_resume(
                &f.epoch,
                &f.play_id,
                &entry,
                PlayerPodcastResumeAction::Change { enabled: false }
            )
            .await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        current(&f, &entry, PlayerPodcastResumeAction::Reconcile).await,
        json!({"radioId":RADIO,"enabled":true,"pending":false})
    );
    assert_eq!(calls(&f), [ASSIGNMENT_PATH, READ, WRITE, READ, READ]);
}

#[tokio::test]
async fn scoped_update_cookie_cannot_borrow_the_assignment_or_read_authority() {
    let f = Fixture::with_existing_voice_targets(&[COOKIE], [assignment("t1").into()]).await;
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(
            &target,
            &[
                "MUSIC_U=foreign-synthetic; Path=/eapi/dj/user/setting/resumeplay/update; Secure"
                    .into(),
            ],
            None,
            &revision,
        )
        .unwrap();
    assert!(
        f.client
            .refresh_pc_presentation()
            .await
            .unwrap()
            .podcast_resume
    );
    let entry = f
        .client
        .player_snapshot(&f.epoch)
        .await
        .unwrap()
        .entry_id
        .unwrap();
    assert!(
        current(&f, &entry, PlayerPodcastResumeAction::View)
            .await
            .is_null()
    );
    assert!(matches!(
        f.client
            .player_podcast_resume(
                &f.epoch,
                &f.play_id,
                &entry,
                PlayerPodcastResumeAction::Change { enabled: true }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(calls(&f), [ASSIGNMENT_PATH]);
}

#[tokio::test(start_paused = true)]
async fn expired_assignment_keeps_known_menu_but_cannot_dispatch_until_fresh_same_group() {
    let f = Fixture::with_existing_voice_targets(
        &[COOKIE],
        [assignment("t1"), assignment("t1")].map(Into::into),
    )
    .await;
    f.client.refresh_pc_presentation().await.unwrap();
    let before = f.client.player_snapshot(&f.epoch).await.unwrap();
    let entry = before.entry_id.as_deref().unwrap();
    let menu = current(&f, entry, PlayerPodcastResumeAction::View).await;
    assert_eq!(
        menu,
        json!({"radioId":RADIO,"enabled":false,"pending":false})
    );
    tokio::time::advance(std::time::Duration::from_secs(1800)).await;
    assert_eq!(
        current(&f, entry, PlayerPodcastResumeAction::View).await,
        menu
    );
    assert!(matches!(
        f.client
            .player_podcast_resume(
                &f.epoch,
                &f.play_id,
                entry,
                PlayerPodcastResumeAction::Change { enabled: true },
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(calls(&f), [ASSIGNMENT_PATH]);
    f.client.refresh_pc_presentation().await.unwrap();
    assert_eq!(
        current(&f, entry, PlayerPodcastResumeAction::View).await,
        menu
    );
    assert_eq!(calls(&f), [ASSIGNMENT_PATH, ASSIGNMENT_PATH]);
}
