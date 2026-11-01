use super::*;
use cumulus_core::cloud::library::CloudDeletionKind;
use sha2::{Digest, Sha256};

#[tokio::test]
async fn deletion_classification_uses_the_official_action_inputs_and_is_bound_to_the_snapshot() {
    let oracle: Value =
        serde_json::from_str(include_str!("../fixtures/cloud-delete-ui.json")).unwrap();
    let cases = oracle["cases"].as_array().unwrap();
    let f = Fixture::new(cases.len() as u64);
    for (row, sample) in f.api.rows.lock().unwrap().iter_mut().zip(cases) {
        row["simpleSong"] = sample["raw"].clone();
    }
    let client = f.client();
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let before = page.revision.unwrap();
    for (index, sample) in cases.iter().enumerate() {
        let row = page
            .tracks
            .iter()
            .find(|row| row.track.id == (index + 1).to_string())
            .unwrap();
        assert_eq!(
            serde_json::to_value(row.deletion_kind).unwrap(),
            sample["kind"]
        );
    }
    f.api.rows.lock().unwrap()[2]["simpleSong"]["t"] = json!(1);
    let next = client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    assert_ne!(next.revision.as_ref(), Some(&before));
    assert_eq!(
        next.tracks
            .iter()
            .find(|row| row.track.id == "3")
            .unwrap()
            .deletion_kind,
        CloudDeletionKind::Private
    );
    assert!(matches!(
        client
            .cloud_command(
                "1",
                CloudCommand::Delete {
                    ids: vec!["3".into()],
                    revision: before
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(
        !f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|(path, _)| path == "/api/cloud/del")
    );
}

#[tokio::test]
async fn legacy_snapshots_without_type_evidence_show_unknown_instead_of_guessing_from_share_or_playability()
 {
    let f = Fixture::new(1);
    let client = f.client();
    client
        .cloud_page("1", query(CloudTab::Uploaded), true)
        .await
        .unwrap();
    let store = ProfileRegistry::open(f.root.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let mut snapshot = store
        .get("1", "cloud_library_v1", "snapshot")
        .unwrap()
        .unwrap();
    snapshot["items"][0]["song"]
        .as_object_mut()
        .unwrap()
        .remove("_cloud_delete_kind");
    let mut canonical = json!([snapshot["items"], snapshot["size"], snapshot["max_size"]]);
    for row in canonical[0].as_array_mut().unwrap() {
        if let Some(album) = row["song"]["al"].as_object_mut() {
            album.remove("picUrl");
        }
    }
    canonical.sort_all_objects();
    snapshot["revision"] = json!(hex::encode(Sha256::digest(
        serde_json::to_vec(&canonical).unwrap()
    )));
    store
        .put("1", "cloud_library_v1", "snapshot", &snapshot)
        .unwrap();
    let page = client
        .cloud_page("1", query(CloudTab::Uploaded), false)
        .await
        .unwrap();
    assert_eq!(page.tracks[0].deletion_kind, CloudDeletionKind::Unknown);
    assert!(
        page.tracks[0].public_song_id.is_some(),
        "sharing is independent of missing type evidence"
    );
    assert_eq!(
        CloudDeletionKind::from_cached(&json!({"_cloud_delete_kind":"bogus"})),
        CloudDeletionKind::Unknown
    );
}
