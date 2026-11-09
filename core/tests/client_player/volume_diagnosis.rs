use super::*;
use cumulus_core::client::OutputDiagnosticRead::{Ready, Unavailable};

#[tokio::test]
async fn volume_diagnosis_drops_endpoint_results_when_player_or_account_changes() {
    let (_dir, backend, session, client, _audio_opens) = fixture();
    client
        .player_action("1", PlayerAction::Volume { value: 84.0 })
        .await
        .unwrap();

    let changed_during_master = client
        .check_volume_diagnosis(
            "1",
            80.0,
            84.0,
            async {
                client
                    .player_action("1", PlayerAction::Volume { value: 85.0 })
                    .await
                    .unwrap();
                Ready(0.29)
            },
            async { panic!("stale software volume must not sample the second endpoint") },
        )
        .await
        .unwrap();
    assert!(changed_during_master.is_none());

    let changed_during_spatial = client
        .check_volume_diagnosis("1", 84.0, 85.0, std::future::ready(Unavailable), async {
            client
                .player_action("1", PlayerAction::Volume { value: 86.0 })
                .await
                .unwrap();
            Ready(true)
        })
        .await
        .unwrap();
    assert!(changed_during_spatial.is_none());

    let switched_during_master = client
        .check_volume_diagnosis(
            "1",
            85.0,
            86.0,
            async {
                session
                    .adopt(backend, json!({"userId":10}), &session.token().unwrap())
                    .unwrap();
                Ready(0.29)
            },
            async { panic!("an old account must not sample the second endpoint") },
        )
        .await;
    assert!(matches!(switched_during_master, Err(Error::StaleResponse)));
}
