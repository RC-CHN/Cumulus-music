use super::*;

async fn target(
    f: &Fixture,
    kind: HistoryKind,
    id: &str,
    selected: &[&str],
) -> HistoryRowMenuTarget {
    let page = f
        .client
        .history_page(&f.epoch(), kind, false)
        .await
        .unwrap();
    HistoryRowMenuTarget {
        history_kind: kind,
        revision: page.value.revision,
        clicked_id: id.into(),
        selected_ids: selected.iter().map(|id| (*id).into()).collect(),
    }
}

async fn inspect(f: &Fixture, kind: HistoryKind, id: &str, selected: &[&str]) -> Value {
    let menu = f
        .client
        .history_row_menu_info(&f.epoch(), target(f, kind, id, selected).await)
        .await
        .unwrap()
        .value;
    let public = serde_json::to_value(menu).unwrap();
    let bytes = public.to_string();
    assert!(!bytes.contains("PRIVATE") && !bytes.contains("cookie"));
    public
}

#[tokio::test]
async fn row_menu_info_distinguishes_song_and_voice_groups_and_exposes_only_public_ids() {
    let f = fixture(true);
    let song = HistoryKind::Song;
    let voice = HistoryKind::Voice;
    seed_history_batch(&f, song, &[(10, 200), (11, 100)]);
    seed_history_batch(&f, voice, &[(9, 200), (10, 100)]);

    let single = inspect(&f, song, "10", &[]).await;
    assert_eq!(single["comment"]["resource"]["kind"], "track");
    assert_eq!(single["comment"]["resource"]["id"], "10");
    assert_eq!(single["comment"]["count"], 36);
    assert_eq!(single["copyLink"], "https://music.163.com/song?id=10");
    let group = inspect(&f, song, "10", &["11", "10"]).await;
    assert!(group["comment"].is_null() && group["copyLink"].is_null());

    let episodes = inspect(&f, voice, "10", &["9", "10"]).await;
    assert_eq!(episodes["comment"]["resource"]["kind"], "voice");
    assert_eq!(episodes["comment"]["resource"]["id"], "10");
    assert_eq!(episodes["comment"]["count"], 47);
    assert_eq!(episodes["copyLink"], "https://music.163.com/dj?id=10");
    assert_eq!(
        *f.api.row_comment_calls.lock().unwrap(),
        [
            json!({"resourceType":4,"resourceIds":r#"["10"]"#}),
            json!({"resourceType":4,"resourceIds":r#"["10"]"#}),
            json!({"resourceType":1,"resourceIds":r#"["10"]"#})
        ],
        "Song group still reads clicked count before hiding; Voice uses episode identity"
    );
}

#[tokio::test]
async fn row_menu_info_keeps_comments_for_copyright_down_and_degrades_count_only() {
    let f = fixture(true);
    let kind = HistoryKind::Song;
    let mut raw = song(&json!(10));
    raw["privilege"] = json!({"status":-100});
    f.history
        .record_for(
            &f.session.token().unwrap(),
            kind,
            HistoryRecord::local(kind, raw, 100).unwrap(),
        )
        .unwrap();
    for mode in [1, 2] {
        f.api.row_comment_failure.store(mode, Ordering::SeqCst);
        let row = inspect(&f, kind, "10", &[]).await;
        assert_eq!(row["comment"]["resource"]["id"], "10");
        assert!(row["comment"]["count"].is_null());
        assert!(row["copyLink"].is_null());
    }
    assert_eq!(f.api.row_comment_calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn row_menu_info_rejects_unknown_rows_and_retired_snapshots_even_if_count_arrives() {
    let kind = HistoryKind::Song;
    for retire_account in [false, true] {
        let f = fixture(true);
        let epoch = f.epoch();
        seed_history_batch(&f, kind, &[(10, 200), (11, 100)]);
        for (id, group) in [
            ("99", &[][..]),
            ("10", &["10", "99"][..]),
            ("10", &["10", "10"][..]),
        ] {
            assert!(
                f.client
                    .history_row_menu_info(&epoch, target(&f, kind, id, group).await)
                    .await
                    .is_err()
            );
        }
        assert!(f.api.row_comment_calls.lock().unwrap().is_empty());

        let input = target(&f, kind, "10", &[]).await;
        f.api.hold.store(true, Ordering::SeqCst);
        let client = f.client.clone();
        let response =
            tokio::spawn(async move { client.history_row_menu_info(&epoch, input).await });
        let deadline = std::time::Duration::from_secs(5);
        tokio::time::timeout(deadline, f.api.started.notified())
            .await
            .unwrap();
        if retire_account {
            f.session
                .adopt(
                    f.api.clone(),
                    json!({"userId":10}),
                    &f.session.token().unwrap(),
                )
                .unwrap();
        } else {
            seed_history_batch(&f, kind, &[(12, 300)]);
        }
        f.api.release.notify_one();
        assert!(matches!(
            tokio::time::timeout(deadline, response)
                .await
                .unwrap()
                .unwrap(),
            Err(Error::QueueChanged | Error::StaleResponse)
        ));
        assert_eq!(f.api.row_comment_calls.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn row_menu_info_never_uses_a_local_file_identity_or_reads_for_a_guest() {
    let f = fixture(false);
    let kind = HistoryKind::Song;
    seed_history_batch(&f, kind, &[(10, 200)]);
    let public = inspect(&f, kind, "10", &[]).await;
    assert_eq!(public["copyLink"], "https://music.163.com/song?id=10");
    assert!(public["comment"]["count"].is_null());

    f.history.record_for(&f.session.token().unwrap(), kind, HistoryRecord::local(kind,
        json!({"local_id":"private-disk-file","path":"/private/path","id":12,"name":"本地"}), 300).unwrap()).unwrap();
    let revision = f
        .history
        .snapshot_for(&f.session.token().unwrap(), kind)
        .unwrap()
        .revision;
    let local = serde_json::to_value(
        f.client
            .history_row_menu_info(
                &f.epoch(),
                HistoryRowMenuTarget {
                    history_kind: kind,
                    revision,
                    clicked_id: "local:private-disk-file".into(),
                    selected_ids: vec![],
                },
            )
            .await
            .unwrap()
            .value,
    )
    .unwrap();
    assert!(local["comment"].is_null() && local["copyLink"].is_null());
    assert!(!local.to_string().contains("private"));
    assert!(f.api.row_comment_calls.lock().unwrap().is_empty());
}
