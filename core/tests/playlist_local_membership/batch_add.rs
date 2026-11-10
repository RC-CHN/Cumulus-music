use super::*;
use cumulus_core::client::dto::PlaylistMixedBatchSource;

fn selection(revision: &str, ids: &[&str]) -> PlaylistMixedBatchSource {
    serde_json::from_value(json!({"playlistId":"74","revision":revision,"ids":ids})).unwrap()
}

async fn source_revision(client: &MusicClient) -> String {
    client
        .playlist_page(
            "74",
            PageRequest {
                offset: 0,
                limit: 100,
                revision: None,
            },
        )
        .await
        .unwrap()
        .value
        .revision
        .unwrap()
}

fn queued(fixture: &Fixture, id: &str) {
    ProfileRegistry::open(&fixture.profile, ApiOrigin::Pc)
        .unwrap()
        .store()
        .put(
            "9",
            "playback",
            "queue",
            &json!({
                "items":[{"entry_id":"retained","resource":{"id":id,"song":{"id":id}}}],
                "order":["retained"],"current":"retained",
            }),
        )
        .unwrap();
}

#[tokio::test]
async fn official_batch_admission_order_and_lost_ack_preserve_local_membership() {
    let fixture = Fixture::new(&[10, 20], Reply::Lost).await;
    fixture.api.batch_privileges.lock().unwrap().extend([
        ("50".to_string(), json!(0)),
        ("60".to_string(), json!(3)),
        ("70".to_string(), json!(-1)),
    ]);
    let (client, session) = fixture.open();
    // An earlier admitted track is in the queue; only the last admitted
    // track 60 selects M/3160's queue-attribution branch.
    queued(&fixture, "40");
    let revision = source_revision(&client).await;
    let selected = ["70", "60", "50", "40"];
    let operation = "a".repeat(32);
    let before = fixture.api.current(&session);
    let result = client
        .collect_playlist_batch_into_mixed("1", "73", selection(&revision, &selected), &operation)
        .await
        .unwrap();
    let result = serde_json::to_value(result.value.unwrap()).unwrap();
    assert_eq!(result["receipt"]["phase"], "uncertain");
    assert_eq!(result["receipt"]["operationId"], operation);
    assert_eq!(result["currentSelection"], true);
    assert_eq!(
        &*fixture.api.writes.lock().unwrap(),
        &[json!({
            "pid":"73","trackIds":"[\"40\",\"60\"]","op":"add",
        })]
    );
    let requests = fixture.api.batch_requests.lock().unwrap().clone();
    assert_eq!(
        requests,
        [json!({"c":json!([
        {"id":"40","v":0},{"id":"50","v":0},{"id":"60","v":0},{"id":"70","v":0},
    ]).to_string(),"trialMode":-1})]
    );
    assert_eq!(
        fixture.api.current(&session).generation(),
        before.generation()
    );
    drop(client);
    let (client, session) = fixture.open();
    let reopened = client
        .collect_playlist_batch_into_mixed(
            "1",
            "73",
            selection(&revision, &["40", "50", "70", "60"]),
            &"b".repeat(32),
        )
        .await
        .unwrap();
    let reopened = serde_json::to_value(reopened.value.unwrap()).unwrap();
    assert_eq!(reopened, result);
    let earlier = client
        .collect_playlist_batch_into_mixed(
            "1",
            "73",
            selection(&revision, &["60"]),
            &"c".repeat(32),
        )
        .await
        .unwrap();
    let earlier = serde_json::to_value(earlier.value.unwrap()).unwrap();
    assert_eq!(earlier["receipt"], result["receipt"]);
    assert_eq!(earlier["currentSelection"], false);
    let confirmed = client
        .reconcile_playlist_mixed_add("1", "73", &operation)
        .await
        .unwrap()
        .value;
    assert_eq!(confirmed.phase, PlaylistPhase::Confirmed);
    assert_eq!(
        confirmed.evidence,
        Some(PlaylistMixedReorderEvidence::ChangedReadback)
    );
    assert_eq!(fixture.api.writes.lock().unwrap().len(), 1);
    assert_eq!(
        fixture.api.current(&session).rows,
        [
            PlaylistLocalRow::Remote { index: 0 },
            PlaylistLocalRow::Remote { index: 1 },
            PlaylistLocalRow::Local(fixture.rows[0].clone()),
            PlaylistLocalRow::Local(fixture.rows[1].clone()),
            PlaylistLocalRow::Remote { index: 2 },
            PlaylistLocalRow::Remote { index: 3 },
        ]
    );
}

#[tokio::test]
async fn batch_zero_is_used_only_as_the_first_fallback_and_queue_representative_is_fenced() {
    let fixture = Fixture::new(&[10, 20], Reply::Ack).await;
    fixture.api.batch_privileges.lock().unwrap().extend([
        ("40".to_string(), json!(-1)),
        ("50".to_string(), json!(0)),
        ("60".to_string(), json!(null)),
        ("70".to_string(), json!("0")),
    ]);
    let (client, _) = fixture.open();
    let revision = source_revision(&client).await;
    let op = "a".repeat(32);
    assert!(matches!(
        client
            .collect_playlist_batch_into_mixed("1", "73", selection(&revision, &["50", "40"]), &op)
            .await,
        Err(Error::InvalidInput(
            "selected catalog songs cannot be collected"
        )),
    ));
    queued(&fixture, "50");
    assert!(matches!(
        client
            .collect_playlist_batch_into_mixed(
                "1",
                "73",
                selection(&revision, &["70", "60", "50"]),
                &op
            )
            .await,
        Err(Error::InvalidInput(
            "queued song requires its original collection context"
        )),
    ));
    assert!(fixture.api.writes.lock().unwrap().is_empty());
    queued(&fixture, "60");
    let reply = client
        .collect_playlist_batch_into_mixed(
            "1",
            "73",
            selection(&revision, &["70", "60", "50"]),
            &op,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(reply.value.unwrap()).unwrap()["receipt"]["phase"],
        "confirmed"
    );
    assert_eq!(
        &*fixture.api.writes.lock().unwrap(),
        &[json!({"pid":"73","trackIds":"[\"50\"]","op":"add"})]
    );
}

#[tokio::test]
async fn batch_source_must_be_a_complete_fresh_ordinary_catalogue() {
    let fixture = Fixture::new(&[10, 20], Reply::Ack).await;
    let (client, _) = fixture.open();
    let revision = source_revision(&client).await;
    let op = "a".repeat(32);
    assert!(matches!(
        client
            .collect_playlist_batch_into_mixed("1", "73", selection(&revision, &["40", "40"]), &op)
            .await,
        Err(Error::InvalidInput(
            "invalid mixed playlist batch selection"
        )),
    ));
    fixture.api.source_detail.lock().unwrap()["trackIds"]
        .as_array_mut()
        .unwrap()
        .swap(1, 2);
    assert!(matches!(
        client
            .collect_playlist_batch_into_mixed("1", "73", selection(&revision, &["40", "60"]), &op)
            .await,
        Err(Error::QueueChanged),
    ));
    fixture.api.source_detail.lock().unwrap()["trackIds"][1]["v"] = json!(-1);
    assert!(matches!(
        client
            .collect_playlist_batch_into_mixed("1", "73", selection(&revision, &["40"]), &op)
            .await,
        Err(Error::InvalidInput(
            "this batch requires an ordinary catalog playlist source"
        )),
    ));
    assert!(fixture.api.batch_requests.lock().unwrap().is_empty());
    assert!(fixture.api.writes.lock().unwrap().is_empty());
    assert!(
        client
            .collect_playlist_batch_into_mixed("1", "74", selection(&revision, &["40"]), &op)
            .await
            .unwrap()
            .value
            .is_none()
    );
}
