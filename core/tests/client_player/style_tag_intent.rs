use super::*;
use cumulus_core::client::style_tags::StyleTagPhase;

async fn revision(client: &MusicClient) -> String {
    client
        .style_recommendations("1")
        .await
        .unwrap()
        .value
        .revision
        .unwrap()
}

#[tokio::test]
async fn style_tag_intent_journals_before_send_and_reopened_client_never_replays() {
    let (dir, api, session, client, _) = fixture();
    api.style_tag_state.store(1, Ordering::SeqCst);
    api.style_tag_write_mode.store(1, Ordering::SeqCst);
    let revision = revision(&client).await;
    let id = uuid::Uuid::new_v4().to_string();
    let save = {
        let (client, id, revision) = (client.clone(), id.clone(), revision.clone());
        tokio::spawn(async move { client.save_style_tag("1", &id, &revision, "7", "2").await })
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        api.style_tag_started.notified(),
    )
    .await
    .unwrap();
    let pending = client.style_tag_operations("1").unwrap().value;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].phase, StyleTagPhase::Pending);
    save.abort();
    assert!(save.await.err().unwrap().is_cancelled());

    let reopened = MusicClient::with_session(
        session.clone(),
        ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap(),
    );
    let duplicate = reopened
        .save_style_tag("1", &id, &revision, "7", "2")
        .await
        .unwrap();
    assert_eq!(duplicate.value.phase, StyleTagPhase::Pending);
    assert!(matches!(
        reopened
            .save_style_tag("1", &uuid::Uuid::new_v4().to_string(), &revision, "7", "2")
            .await,
        Err(Error::MutationUncertain)
    ));
    let recovered = reopened.reconcile_style_tag("1", &id).await.unwrap();
    assert_eq!(recovered.value.phase, StyleTagPhase::Confirmed);
    assert_eq!(recovered.value.observed.unwrap().tag_ids, ["2"]);
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        reopened.style_tag_operations("1"),
        Err(Error::StaleResponse)
    ));
    assert!(reopened.style_tag_operations("2").unwrap().value.is_empty());
    assert_eq!(api.style_tag_writes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn style_tag_intent_requires_fresh_preference_and_can_accept_observed_mismatch() {
    let (_dir, api, _session, client, _) = fixture();
    api.style_tag_state.store(1, Ordering::SeqCst);
    api.style_tag_write_mode.store(2, Ordering::SeqCst);
    let displayed = revision(&client).await;
    api.style_tag_state.store(2, Ordering::SeqCst);
    let id = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        client.save_style_tag("1", &id, &displayed, "7", "2").await,
        Err(Error::QueueChanged)
    ));
    assert!(client.style_tag_operations("1").unwrap().value.is_empty());
    assert_eq!(api.style_tag_writes.load(Ordering::SeqCst), 0);
    api.style_tag_state.store(1, Ordering::SeqCst);
    let op = client
        .save_style_tag("1", &id, &displayed, "7", "2")
        .await
        .unwrap()
        .value;
    assert_eq!(op.phase, StyleTagPhase::Uncertain);
    let observed = op.observed.unwrap().revision;
    api.style_tag_state.store(0, Ordering::SeqCst);
    assert!(client.reconcile_style_tag("1", &id).await.is_err());
    assert!(
        client
            .accept_current_style_tag("1", &id, &observed)
            .await
            .is_err()
    );
    api.style_tag_state.store(1, Ordering::SeqCst);
    let accepted = client
        .accept_current_style_tag("1", &id, &observed)
        .await
        .unwrap();
    assert_eq!(accepted.value.phase, StyleTagPhase::AcceptedCurrentState);
    assert_eq!(api.style_tag_writes.load(Ordering::SeqCst), 1);
}
