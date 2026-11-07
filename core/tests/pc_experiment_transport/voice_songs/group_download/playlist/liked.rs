use super::*;
use cumulus_core::playlist::PlaylistService;

fn star(id: u64) -> Value {
    json!({"id":id,"userId":25,"creator":{"userId":25},"privacy":10,
        "specialType":5,"name":"合成系统喜欢"})
}
fn observation(tracks: &[u64], likes: &[u64]) -> [HttpResponse; 3] {
    let mut row = star(83);
    row["tags"] = json!([]);
    row["trackIds"] = json!(tracks.iter().map(|id| json!({"id":id})).collect::<Vec<_>>());
    [
        owned_list(vec![star(83)]),
        json!({"code":200,"playlist":row}).into(),
        json!({"code":200,"ids":likes}).into(),
    ]
}
fn preview() -> [HttpResponse; 2] {
    let [list, detail, _] = observation(&[], &[]);
    [list, detail]
}

async fn observed(f: &Fixture, card: &str, selection: &str) -> String {
    let response = f
        .client
        .player_voice_songs_download_liked_destination(&f.epoch, &f.play_id, card, selection)
        .await
        .unwrap();
    assert_eq!(response.epoch, f.epoch);
    let row = response.value.unwrap();
    assert_eq!(row.playlist_id, "83");
    row.destination_receipt
}

#[tokio::test]
async fn system_liked_requires_coherent_readback_after_definite_false_or_lost_ack() {
    for known_false in [true, false] {
        let ack = if known_false {
            json!({"code":403}).into()
        } else {
            HttpResponse::Ready(Err(Error::Transport(
                "synthetic lost system liked reply".into(),
            )))
        };
        let checked = if known_false {
            observation(&[701, 301], &[701, 301])
        } else {
            observation(&[307, 701, 301], &[307, 301, 701])
        };
        let (f, card) = fixture(
            [likes()]
                .into_iter()
                .chain(preview())
                .chain(preview())
                .chain(observation(&[701, 301], &[701, 301]))
                .chain([ack])
                .chain(observation(&[307, 701, 301], &[701, 301]))
                .chain(checked)
                .chain([deny(), deny()]),
        )
        .await;
        configure(&f).await;
        let selection = prepare_picker(&f, &card).await;
        let choose = |receipt| {
            f.client.player_voice_songs_download_choose_liked(
                &f.epoch, &f.play_id, &card, &selection, receipt,
            )
        };
        assert!(matches!(choose("83").await, Err(Error::StaleResponse)));
        let old = observed(&f, &card, &selection).await;
        let destination = observed(&f, &card, &selection).await;
        assert_ne!(old, destination);
        assert!(matches!(choose(&old).await, Err(Error::StaleResponse)));
        assert!(matches!(
            f.client
                .player_voice_songs_download_choose_playlist(
                    &f.epoch, &f.play_id, &card, &selection, "83"
                )
                .await,
            Err(Error::StaleResponse)
        ));
        let first = choose(&destination).await.unwrap();
        assert_eq!(first.phase, PlaylistPhase::Uncertain);
        assert!(first.continuation.is_none());
        assert_eq!(
            choose(&destination).await.unwrap().operation_id,
            first.operation_id
        );
        assert!(f.store.values("25", TASKS).unwrap().is_empty());
        assert_eq!(
            f.store.get("25", "resources", "liked").unwrap(),
            Some(json!(["301"]))
        );
        let readback = f
            .client
            .player_voice_songs_download_playlist_status(
                &f.epoch, &f.play_id, &card, &selection, true,
            )
            .await
            .unwrap();
        assert_eq!(readback.operation_id, first.operation_id);
        assert_eq!(readback.add_failed, known_false);
        assert_eq!(
            readback.phase,
            if known_false {
                PlaylistPhase::Rejected
            } else {
                PlaylistPhase::Confirmed
            }
        );
        let permit = readback.continuation.unwrap();
        let start = || {
            f.client.player_voice_songs_download_start_from_playlist(
                &f.epoch, &f.play_id, &card, &selection, &permit,
            )
        };
        let result = start().await.unwrap();
        assert_eq!(start().await.unwrap().accepted, result.accepted);
        let default = f
            .client
            .download_quality_preferences(&f.epoch)
            .unwrap()
            .options
            .quality;
        ordered_tasks(&f, &result, &["301", "302"], default).await;
        let journal = PlaylistService::new(f.session.clone())
            .unwrap()
            .operations(&f.session.token().unwrap())
            .unwrap();
        assert_eq!(journal.len(), 1);
        assert!(journal[0].system_liked_group);
        let wire = f.http.calls();
        let add = wire.iter().find(|call| call.path == ADD).unwrap();
        assert_eq!(add.payload["pid"], "83");
        assert_eq!(add.payload["trackIds"], r#"["307","301"]"#);
        assert!(
            add.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-voice")
        );
        assert_eq!(
            trace(&f),
            [
                CARD, LIKED, OWNED, DETAIL, OWNED, DETAIL, OWNED, DETAIL, LIKED, ADD, OWNED,
                DETAIL, LIKED, OWNED, DETAIL, LIKED, AUTH, AUTH
            ]
        );
    }
}

#[tokio::test]
async fn a_new_system_star_cannot_replace_the_observed_download_target() {
    let (f, card) = fixture(
        [likes()]
            .into_iter()
            .chain(preview())
            .chain([owned_list(vec![star(84)])]),
    )
    .await;
    configure(&f).await;
    let selection = prepare_picker(&f, &card).await;
    let receipt = observed(&f, &card, &selection).await;
    assert!(matches!(
        f.client
            .player_voice_songs_download_choose_liked(
                &f.epoch, &f.play_id, &card, &selection, &receipt
            )
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(trace(&f), [CARD, LIKED, OWNED, DETAIL, OWNED]);
    assert!(f.store.values("25", TASKS).unwrap().is_empty());
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
}
