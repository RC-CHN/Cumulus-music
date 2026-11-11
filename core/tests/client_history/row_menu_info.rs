use super::*;
use cumulus_core::library::{AudioMetadata, LocalLibrary, probe::AudioProbe};
use std::{fs, path::Path};

const MEDIA: &[u8] = include_bytes!("../fixtures/tag_audio/seed.mp3");

struct HistoryAudioProbe;

impl AudioProbe for HistoryAudioProbe {
    fn inspect(&self, path: &Path) -> cumulus_core::Result<AudioMetadata> {
        Ok(AudioMetadata {
            format: None,
            duration: 120.0,
            title: path.file_stem().unwrap().to_string_lossy().into_owned(),
            album: "合成专辑".into(),
            artists: vec!["合成歌手".into()],
            bitrate: None,
            sample_rate: None,
            channels: None,
            container: path.extension().unwrap().to_string_lossy().into_owned(),
            cover_bytes: None,
        })
    }
}

fn history_library(f: &Fixture) -> LocalLibrary {
    LocalLibrary::with_probe(f.store.clone(), Arc::new(HistoryAudioProbe)).unwrap()
}

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

fn record_indexed(f: &Fixture, item: &cumulus_core::library::LibraryItem, position: u64) -> String {
    let kind = HistoryKind::Song;
    let record = HistoryRecord::local(
        kind,
        json!({"local_id":item.id,"local_revision":item.revision,"metadata":item.metadata}),
        position,
    )
    .unwrap();
    let id = record.id.clone();
    f.history
        .record_for(&f.session.token().unwrap(), kind, record)
        .unwrap();
    id
}

async fn reveal_offer(f: &Fixture, row: HistoryRowMenuTarget, path: &Path) -> String {
    let menu = f
        .client
        .history_row_menu_info(&f.epoch(), row)
        .await
        .unwrap()
        .value;
    let public = serde_json::to_value(menu).unwrap();
    assert!(
        !public
            .to_string()
            .contains(path.file_name().unwrap().to_str().unwrap())
    );
    assert_eq!(public["songFile"]["kind"], "reveal");
    public["songFile"]["receipt"].as_str().unwrap().into()
}

#[tokio::test]
async fn song_history_guest_reveals_only_the_indexed_original_audio_and_retires_cue_changes() {
    let f = fixture(false);
    let media = tempfile::tempdir().unwrap();
    let standalone = media.path().join("直接本机.mp3");
    let source = media.path().join("专辑原始音频.wav");
    let sheet = media.path().join("合成分轨.cue");
    fs::write(&standalone, MEDIA).unwrap();
    fs::write(&source, b"synthetic audio for CUE").unwrap();
    fs::write(
        &sheet,
        "FILE \"专辑原始音频.wav\" WAVE\nTRACK 01 AUDIO\nTITLE \"合成分轨\"\nINDEX 01 00:00:00\n",
    )
    .unwrap();
    let library = history_library(&f);
    library.scan(media.path()).unwrap();
    let files = library.list(Default::default()).unwrap();
    let ordinary = files
        .iter()
        .find(|item| item.path.ends_with("直接本机.mp3"))
        .unwrap();
    let cue = files
        .iter()
        .find(|item| {
            matches!(
                item.revision,
                cumulus_core::library::LocalRevision::Cue { .. }
            )
        })
        .unwrap();
    let standalone_id = record_indexed(&f, ordinary, 100);
    let cue_id = record_indexed(&f, cue, 200);
    let explicit = target(&f, HistoryKind::Song, &standalone_id, &[]).await;
    let receipt = reveal_offer(&f, explicit.clone(), &standalone).await;
    let lease = f
        .client
        .history_row_song_file_reveal(&f.epoch(), explicit.clone(), &receipt)
        .await
        .unwrap();
    assert_eq!(lease.path(), standalone.canonicalize().unwrap());
    assert!(
        f.client
            .history_row_song_file_reveal(&f.epoch(), explicit, &receipt)
            .await
            .is_err()
    );

    let cue_row = target(&f, HistoryKind::Song, &cue_id, &[]).await;
    let receipt = reveal_offer(&f, cue_row.clone(), &source).await;
    let lease = f
        .client
        .history_row_song_file_reveal(&f.epoch(), cue_row.clone(), &receipt)
        .await
        .unwrap();
    assert_eq!(lease.path(), source.canonicalize().unwrap());
    let receipt = reveal_offer(&f, cue_row.clone(), &source).await;
    fs::write(&sheet, "REM changed sheet").unwrap();
    assert!(
        f.client
            .history_row_song_file_reveal(&f.epoch(), cue_row, &receipt)
            .await
            .is_err()
    );
    assert!(f.api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn song_history_mapping_offer_is_bound_to_clicked_scope_and_refuses_new_distinct_matches() {
    let f = fixture(true);
    let media = tempfile::tempdir().unwrap();
    let first = media.path().join("旧本机映射.mp3");
    let second = media.path().join("新本机映射.mp3");
    fs::write(&first, MEDIA).unwrap();
    fs::write(&second, MEDIA).unwrap();
    let library = history_library(&f);
    library.scan(media.path()).unwrap();
    let files = library.list(Default::default()).unwrap();
    let at = |path: &Path| {
        files
            .iter()
            .find(|item| item.path.ends_with(path.file_name().unwrap()))
            .unwrap()
    };
    library
        .map_online(&f.session, &at(&first).id, &json!(10))
        .unwrap();
    seed_history_batch(&f, HistoryKind::Song, &[(10, 300), (11, 200), (12, 100)]);
    let row = target(&f, HistoryKind::Song, "10", &["11", "12"]).await;
    let offer = reveal_offer(&f, row.clone(), &first).await;
    let changed = HistoryRowMenuTarget {
        selected_ids: vec!["12".into(), "11".into()],
        ..row.clone()
    };
    assert!(
        f.client
            .history_row_song_file_reveal(&f.epoch(), changed, &offer)
            .await
            .is_err()
    );
    let lease = f
        .client
        .history_row_song_file_reveal(&f.epoch(), row.clone(), &offer)
        .await
        .unwrap();
    assert_eq!(lease.path(), first.canonicalize().unwrap());

    let offer = reveal_offer(&f, row.clone(), &first).await;
    library
        .map_online(&f.session, &at(&second).id, &json!(10))
        .unwrap();
    assert!(
        f.client
            .history_row_song_file_reveal(&f.epoch(), row.clone(), &offer)
            .await
            .is_err()
    );
    let ambiguous = f
        .client
        .history_row_menu_info(&f.epoch(), row.clone())
        .await
        .unwrap()
        .value;
    assert!(serde_json::to_value(ambiguous).unwrap()["songFile"].is_null());
    library
        .map_online(&f.session, &at(&second).id, &json!(12))
        .unwrap();
    let offer = reveal_offer(&f, row.clone(), &first).await;
    let other = target(&f, HistoryKind::Song, "12", &[]).await;
    reveal_offer(&f, other, &second).await;
    assert!(
        f.client
            .history_row_song_file_reveal(&f.epoch(), row, &offer)
            .await
            .is_err()
    );

    // A finished ledger branch outranks the same mapped file even when the
    // published file is unavailable; another owner's ledger has no effect.
    let row = target(&f, HistoryKind::Song, "10", &[]).await;
    let finished = json!({"song_id":"10","status":"complete","origin":{"kind":"song"}});
    f.store
        .put("other", "download_tasks_v1", "old-other", &finished)
        .unwrap();
    let offer = reveal_offer(&f, row.clone(), &first).await;
    let lease = f
        .client
        .history_row_song_file_reveal(&f.epoch(), row.clone(), &offer)
        .await
        .unwrap();
    let pending = reveal_offer(&f, row.clone(), &first).await;
    f.store
        .put("9", "download_tasks_v1", "new-finished", &finished)
        .unwrap();
    assert!(lease.check_current(&f.client).is_err());
    assert!(
        f.client
            .history_row_song_file_reveal(&f.epoch(), row.clone(), &pending)
            .await
            .is_err()
    );
    let terminal = f
        .client
        .history_row_menu_info(&f.epoch(), row)
        .await
        .unwrap()
        .value;
    let terminal = serde_json::to_value(terminal).unwrap();
    assert!(terminal["songFile"].is_null());
    assert_eq!(terminal["songDownload"]["kind"], "unavailable");
}
