#[path = "support/lyric_wave.rs"]
mod lyric_wave;
mod support;
use cumulus_core::{Error, account::AccountSession, lyrics::*, storage::StateStore};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

async fn setup(
    store: Arc<StateStore>,
    replies: impl IntoIterator<Item = Reply>,
) -> (Arc<AccountSession>, Arc<ScriptApi>, Arc<LyricsService>) {
    let mut all = vec![json!({"code":200,"profile":{"userId":1}}).into()];
    all.extend(replies);
    let api = Arc::new(ScriptApi::new(all));
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session.refresh().await.unwrap();
    let service = Arc::new(LyricsService::new(session.clone()).unwrap());
    (session, api, service)
}
fn request(target: LyricTarget) -> LyricRequest {
    LyricRequest {
        target,
        demand: LyricDemand {
            force: true,
            ..Default::default()
        },
        downloaded_audio: None,
        track_mv: false,
    }
}
fn song(id: &str) -> LyricRequest {
    request(LyricTarget::Song(id.into()))
}
fn response(text: &str) -> Value {
    json!({"code":200,"lrc":{"version":2,"offset":500,"lyric":text},"sgc":false})
}

#[tokio::test]
async fn normalization_versions_cache_merge_offset_and_preferences_survive_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (session, api, service) = setup(
        store.clone(),
        [
            response("[00:01]one\n[00:02]two").into(),
            json!({"code":200,"tlyric":{"version":4,"lyric":"[00:01]一"}}).into(),
            Reply::Ready(Err(Error::Transport("lost".into()))),
        ],
    )
    .await;
    let first = service.fetch(song("1")).await.unwrap();
    assert_eq!(first.lyric.lyric_lines[0].time, Some(0.5));
    assert_eq!(
        api.calls()[1].payload,
        json!({"id":"1","lv":-1,"tv":-1,"rv":-1,"yv":-1})
    );
    let adjusted = service.adjust_offset(1.0).unwrap();
    assert_eq!(adjusted.lyric.offset, 1.0);
    assert_eq!(adjusted.lyric.lyric_lines[0].time, Some(0.0));
    service.fetch(song("1")).await.unwrap();
    assert_eq!(api.calls().len(), 2);
    let prefs = LyricPreferences {
        translation_enable: false,
        roma_enable: true,
    };
    service.set_preferences(prefs).unwrap();
    let reopened = LyricsService::new(session.clone()).unwrap();
    assert_eq!(reopened.snapshot().unwrap().preferences, prefs);
    let merged = reopened.fetch(song("1")).await.unwrap();
    assert_eq!(api.calls()[2].payload["lv"], 2);
    assert_eq!(merged.lyric.offset, 1.0);
    assert_eq!(merged.lyric.tlyric_lines.len(), 2);
    assert_eq!(merged.lyric.tlyric_lines[0].time, Some(1.0));
    reopened.clear().unwrap();
    let fallback = reopened.fetch(song("1")).await.unwrap();
    assert!(fallback.network_failed && fallback.from_cache && !fallback.is_lyric_fetch_failed);
    assert_eq!(fallback.lyric.lyric_lines[0].time, Some(0.0));
    assert_eq!(
        store.get("1", "lyrics_v1", "song:1").unwrap().unwrap()["response"]["lrc"]["offset"],
        1.0
    );
}

#[tokio::test]
async fn fresh_song_credit_omission_replaces_cache_but_transport_failure_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let with_credits = json!({
        "code":200, "lrc":{"version":7,"lyric":"[00:01]same version"},
        "lyricUser":{"id":11,"userid":21,"nickname":"原词贡献"},
        "transUser":{"id":12,"userid":22,"nickname":"译词贡献"}
    });
    let (_, api, service) = setup(
        store.clone(),
        [
            with_credits.into(),
            Reply::Ready(Err(Error::Transport("lost".into()))),
            json!({"code":200,"transUser":null}).into(),
            Reply::Ready(Err(Error::Transport("lost again".into()))),
            json!({"code":200,"lrc":{"lyric":"[00:01]other song"}}).into(),
        ],
    )
    .await;
    let first = service.fetch(song("1")).await.unwrap();
    assert_eq!(first.lyric.metadata["lyricContributor"]["userid"], "21");
    assert_eq!(
        first.lyric.metadata["translationContributor"]["userid"],
        "22"
    );

    service.clear().unwrap();
    let fallback = service.fetch(song("1")).await.unwrap();
    assert!(fallback.from_cache && fallback.network_failed);
    assert_eq!(fallback.lyric.metadata, first.lyric.metadata);

    service.clear().unwrap();
    let refreshed = service.fetch(song("1")).await.unwrap();
    assert!(!refreshed.network_failed);
    assert_eq!(refreshed.lyric.lyric_lines, first.lyric.lyric_lines);
    assert_eq!(refreshed.lyric.metadata["lyricContributor"], Value::Null);
    assert_eq!(
        refreshed.lyric.metadata["translationContributor"],
        Value::Null
    );
    assert_eq!(api.calls()[3].payload["lv"], 7);
    let cached = store.get("1", "lyrics_v1", "song:1").unwrap().unwrap();
    assert_eq!(cached["response"]["lyricContributor"], Value::Null);
    assert_eq!(cached["response"]["lrc"]["version"], 7);

    service.clear().unwrap();
    let fallback = service.fetch(song("1")).await.unwrap();
    assert!(fallback.from_cache && fallback.network_failed);
    assert_eq!(fallback.lyric.metadata, refreshed.lyric.metadata);
    let other = service.fetch(song("2")).await.unwrap();
    assert_eq!(other.lyric.lyric_lines[0].lyric, "other song");
    assert_eq!(other.lyric.metadata["lyricContributor"], Value::Null);
    assert_eq!(other.lyric.metadata["translationContributor"], Value::Null);
}

#[tokio::test]
async fn old_selection_cancellation_and_account_switch_cannot_publish_late_lyrics() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (pending, send) = gate();
    let (pending2, send2) = gate();
    let (session, api, service) = setup(
        store.clone(),
        [pending, response("[00:00]new").into(), pending2],
    )
    .await;
    let running = tokio::spawn({
        let s = service.clone();
        async move { s.fetch(song("1")).await }
    });
    api.wait_for_calls(2).await;
    service.fetch(song("2")).await.unwrap();
    send.send(Ok(response("[00:00]old"))).unwrap();
    assert!(matches!(running.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(
        service.snapshot().unwrap().resource_key.as_deref(),
        Some("song:2")
    );
    assert!(store.get("1", "lyrics_v1", "song:1").unwrap().is_none());
    let running = tokio::spawn({
        let s = service.clone();
        async move { s.fetch(song("3")).await }
    });
    api.wait_for_calls(4).await;
    session.detach().unwrap();
    send2.send(Ok(response("[00:00]too late"))).unwrap();
    assert!(matches!(running.await.unwrap(), Err(Error::StaleResponse)));
    assert!(matches!(service.snapshot(), Err(Error::StaleResponse)));
    assert!(store.get("1", "lyrics_v1", "song:3").unwrap().is_none());
}

#[tokio::test]
async fn old_window_adjustment_cannot_change_a_new_lyric_with_the_same_numeric_id() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (pending, send) = gate();
    let (_, api, service) = setup(
        store.clone(),
        [response("[00:01]ordinary song").into(), pending],
    )
    .await;
    let old_target = LyricTarget::Song("1".into());
    let new_target = LyricTarget::Cloud("1".into());
    service.fetch(request(old_target.clone())).await.unwrap();
    let new_read = tokio::spawn({
        let (service, target) = (service.clone(), new_target.clone());
        async move { service.fetch(request(target)).await }
    });
    api.wait_for_calls(3).await;
    let preferences = LyricPreferences {
        translation_enable: false,
        roma_enable: false,
    };
    let reject_old_window = || {
        assert!(matches!(
            service.adjust_target_offset(&old_target, 1.0),
            Err(Error::StaleResponse)
        ));
        assert!(matches!(
            service.set_target_preferences(&old_target, preferences),
            Err(Error::StaleResponse)
        ));
    };
    reject_old_window();
    send.send(Ok(json!({"code":200,"lrc":"[00:01]cloud song"})))
        .unwrap();
    new_read.await.unwrap().unwrap();
    reject_old_window();
    assert!(
        store
            .get("1", "lyric_preferences", "state")
            .unwrap()
            .is_none()
    );
    let adjusted = service.adjust_target_offset(&new_target, -1.0).unwrap();
    assert_eq!(adjusted.lyric.offset, -0.5);
    assert_eq!(adjusted.lyric.lyric_lines[0].time, Some(1.5));
    assert_eq!(
        store.get("1", "lyrics_v1", "song:1").unwrap().unwrap()["response"]["lrc"]["offset"],
        0.5
    );
    assert_eq!(
        service
            .set_target_preferences(&new_target, preferences)
            .unwrap()
            .preferences,
        preferences
    );
    assert_eq!(api.calls().len(), 3);
}

#[tokio::test]
async fn abort_releases_loading_and_cross_instance_resource_lock_without_replay() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (pending, _send) = gate();
    let (session, api, service) = setup(store, [pending, response("[00:00]retry").into()]).await;
    let running = tokio::spawn({
        let s = service.clone();
        async move { s.fetch(song("1")).await }
    });
    api.wait_for_calls(2).await;
    let other = LyricsService::new(session).unwrap();
    assert!(matches!(
        other.fetch(song("1")).await,
        Err(Error::MutationBusy)
    ));
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());
    assert!(!service.snapshot().unwrap().is_loading);
    assert!(service.snapshot().unwrap().is_lyric_fetch_failed);
    other.fetch(song("1")).await.unwrap();
    assert_eq!(api.calls().len(), 3);
}

#[tokio::test]
async fn demand_cloud_and_legacy_cache_use_official_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (_, api, service) = setup(
        store,
        [
            json!({"code":200,"lrc":"[00:01]cloud"}).into(),
            json!({"code":200}).into(),
        ],
    )
    .await;
    let mut input = song("1");
    input.demand = LyricDemand::default();
    service.fetch(input).await.unwrap();
    let mut input = song("1");
    input.track_mv = true;
    service.fetch(input).await.unwrap();
    assert_eq!(api.calls().len(), 1);
    let cloud = service
        .fetch(request(LyricTarget::Cloud("9".into())))
        .await
        .unwrap();
    assert_eq!(api.calls()[1].path, "/api/cloud/lyric/get");
    assert_eq!(
        api.calls()[1].payload,
        json!({"songId":"9","userId":"1","lv":-1,"kv":-1})
    );
    assert_eq!(cloud.lyric.metadata["isCloudLyric"], true);
    assert!(
        !service
            .import_official_cache(
                &LyricTarget::Song("1".into()),
                json!({"yrc":{"lyric":"[0,1]bad old cache"}})
            )
            .unwrap()
    );
    let cached = json!({"code":200,"lrc":{"version":0,"lyric":"[00:01]cached","offset":1000}});
    assert!(
        service
            .import_official_cache(&LyricTarget::Song("1".into()), cached.clone())
            .unwrap()
    );
    assert!(
        service
            .import_official_cache(&LyricTarget::Song("1".into()), cached)
            .is_err()
    );
    let restored = service.fetch(song("1")).await.unwrap();
    assert_eq!(api.calls()[2].payload["lv"], -1);
    assert_eq!(restored.lyric.offset, 1.0);
    assert_eq!(restored.lyric.lyric_lines[0].time, Some(0.0));
}

#[tokio::test]
async fn local_utf16_sidecar_and_download_fallback_do_not_need_remote_write() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path().join("state")).unwrap());
    let (_, api, service) = setup(store, [json!({"code":200}).into()]).await;
    let audio = tmp.path().join("sample.flac");
    let text = "[00:01]本地\r\n[00:02]第二行";
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(audio.with_extension("lrc"), bytes).unwrap();
    let local = service
        .fetch(request(LyricTarget::Local(audio.clone())))
        .await
        .unwrap();
    assert!(local.from_sidecar);
    assert_eq!(local.lyric.lyric_lines.len(), 2);
    assert_eq!(api.calls().len(), 1);
    service.adjust_offset(1.0).unwrap();
    service.clear().unwrap();
    let reload = service
        .fetch(request(LyricTarget::Local(audio.clone())))
        .await
        .unwrap();
    assert_eq!(reload.lyric.offset, 0.5);
    let mut input = song("1");
    input.downloaded_audio = Some(audio.clone());
    let fallback = service.fetch(input).await.unwrap();
    assert!(fallback.from_sidecar);
    assert_eq!(fallback.lyric.lyric_lines[0].lyric, "本地");
    std::fs::write(audio.with_extension("lrc"), [0xff]).unwrap();
    service.clear().unwrap();
    assert!(
        service
            .fetch(request(LyricTarget::Local(audio)))
            .await
            .is_err()
    );
    assert!(!service.snapshot().unwrap().is_loading);
}

#[tokio::test]
async fn local_lyric_tags_are_read_without_network_and_never_used_as_download_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path().join("state")).unwrap());
    let (_, api, service) = setup(store, [json!({"code":200}).into()]).await;
    let fixtures: [(&str, &[u8]); 3] = [
        ("mp3", include_bytes!("fixtures/tag_audio/seed.mp3")),
        ("flac", include_bytes!("fixtures/tag_audio/seed.flac")),
        ("m4a", include_bytes!("fixtures/tag_audio/seed.m4a")),
    ];
    for (ext, bytes) in fixtures {
        let audio = tmp.path().join(format!("测试.{ext}"));
        std::fs::write(&audio, bytes).unwrap();
        let input = request(LyricTarget::Local(audio.clone()));
        let tag = service.fetch(input.clone()).await.unwrap();
        assert!(tag.from_embedded && !tag.from_sidecar, "{ext}");
        assert!(tag.lyric.scrollable);
        assert_eq!(tag.lyric.lyric_lines[0].lyric, "保留歌词");
        assert_eq!(tag.lyric.lyric_lines[0].time, Some(0.0));
        std::fs::write(audio.with_extension("lrc"), "[00:02]Sidecar").unwrap();
        service.clear().unwrap();
        let sidecar = service.fetch(input.clone()).await.unwrap();
        assert!(sidecar.from_sidecar && !sidecar.from_embedded);
        assert_eq!(sidecar.lyric.lyric_lines[0].lyric, "Sidecar");
        std::fs::write(audio.with_extension("lrc"), "  ").unwrap();
        service.clear().unwrap();
        assert!(service.fetch(input).await.unwrap().from_embedded);
        assert_eq!(std::fs::read(audio).unwrap(), bytes);
    }
    assert_eq!(api.calls().len(), 1);
    let audio = tmp.path().join("测试.flac");
    std::fs::remove_file(audio.with_extension("lrc")).unwrap();
    let mut download = song("1");
    download.downloaded_audio = Some(audio);
    let missing = service.fetch(download).await.unwrap();
    assert!(!missing.from_embedded && !missing.from_sidecar);
    assert!(missing.lyric.lyric_lines.is_empty());
    assert_eq!(api.calls().len(), 2);
}

#[tokio::test]
async fn local_lyrics_follow_the_official_path_order_without_expanding_download_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let store = Arc::new(StateStore::open(root.join("state")).unwrap());
    let (_, api, service) = setup(store, [json!({"code":200}).into()]).await;
    std::fs::create_dir_all(root.join("lyric/lrc")).unwrap();
    std::fs::create_dir(root.join("lrc")).unwrap();
    let audio = root.join("合成.session.mp3");
    let seed = include_bytes!("fixtures/tag_audio/seed.mp3");
    std::fs::write(&audio, seed).unwrap();
    let candidates = [
        ("合成.session.lrc", "adjacent LRC"),
        ("合成.session.krc", "adjacent text KRC"),
        ("lyric/合成.session.lrc", "first folder LRC"),
        ("lyric/合成.session.krc", "first folder text KRC"),
        ("lyric/lrc/合成.session.lrc", "nested LRC"),
        ("lyric/lrc/合成.session.krc", "nested text KRC"),
    ];
    for (name, text) in candidates {
        std::fs::write(root.join(name), format!("[00:01.250]{text}")).unwrap();
    }
    std::fs::write(root.join("lrc/合成.session.lrc"), "[00:01]wrong sibling").unwrap();
    let local = request(LyricTarget::Local(audio.clone()));
    for (name, expected) in candidates {
        service.clear().unwrap();
        let result = service.fetch(local.clone()).await.unwrap();
        assert!(result.from_sidecar && !result.from_embedded);
        assert_eq!(result.lyric.lyric_lines[0].lyric, expected);
        assert_eq!(result.lyric.lyric_lines[0].time, Some(1.25));
        std::fs::remove_file(root.join(name)).unwrap();
    }
    service.clear().unwrap();
    let result = service.fetch(local).await.unwrap();
    assert!(result.from_embedded && !result.from_sidecar);
    assert_eq!(result.lyric.lyric_lines[0].lyric, "保留歌词");
    assert_eq!(api.calls().len(), 1);
    assert_eq!(std::fs::read(&audio).unwrap(), seed);

    std::fs::write(root.join(candidates[4].0), "[00:01]local only").unwrap();
    let mut download = song("1");
    download.downloaded_audio = Some(audio);
    let result = service.fetch(download).await.unwrap();
    assert!(!result.from_embedded && !result.from_sidecar);
    assert!(result.lyric.lyric_lines.is_empty());
    assert_eq!(api.calls().len(), 2);
}

#[tokio::test]
async fn local_first_candidate_rejects_binary_or_directory_without_skipping_to_nested_text() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let store = Arc::new(StateStore::open(root.join("state")).unwrap());
    let (_, api, service) = setup(store, []).await;
    let audio = root.join("fixture.wav");
    std::fs::write(&audio, lyric_wave::wave(3)).unwrap();
    std::fs::create_dir_all(root.join("lyric/lrc")).unwrap();
    std::fs::write(root.join("lyric/lrc/fixture.lrc"), "[00:01]nested").unwrap();
    let local = request(LyricTarget::Local(audio.clone()));
    let bad = audio.with_extension("krc");
    std::fs::write(&bad, b"krc1\x00\xff\x12\x01").unwrap();
    assert!(matches!(
        service.fetch(local.clone()).await,
        Err(Error::InvalidInput("binary KRC lyrics are unsupported"))
    ));
    std::fs::remove_file(&bad).unwrap();
    std::fs::create_dir(&bad).unwrap();
    service.clear().unwrap();
    assert!(service.fetch(local.clone()).await.is_err());
    std::fs::remove_dir(bad).unwrap();
    service.clear().unwrap();
    let result = service.fetch(local).await.unwrap();
    assert!(result.from_sidecar);
    assert_eq!(result.lyric.lyric_lines[0].lyric, "nested");
    assert_eq!(api.calls().len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn local_lyrics_reject_redirected_new_folders_and_leaf() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let store = Arc::new(StateStore::open(root.join("state")).unwrap());
    let (_, api, service) = setup(store, []).await;
    let outside = root.join("outside");
    let library = root.join("library");
    std::fs::create_dir(&library).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let audio = library.join("fixture.wav");
    std::fs::write(&audio, lyric_wave::wave(3)).unwrap();
    let unrelated = outside.join("fixture.lrc");
    std::fs::write(&unrelated, "[00:01]must not read").unwrap();
    let local = request(LyricTarget::Local(audio));
    let first = library.join("lyric");
    symlink(&outside, &first).unwrap();
    assert!(service.fetch(local.clone()).await.is_err());
    std::fs::remove_file(&first).unwrap();
    std::fs::create_dir(&first).unwrap();
    let nested = first.join("lrc");
    symlink(&outside, &nested).unwrap();
    service.clear().unwrap();
    assert!(service.fetch(local.clone()).await.is_err());
    std::fs::remove_file(&nested).unwrap();
    std::fs::create_dir(&nested).unwrap();
    let leaf = nested.join("fixture.lrc");
    symlink(unrelated, &leaf).unwrap();
    service.clear().unwrap();
    assert!(service.fetch(local.clone()).await.is_err());
    std::fs::remove_file(&leaf).unwrap();
    std::fs::write(leaf, "[00:01]owned folder").unwrap();
    service.clear().unwrap();
    let result = service.fetch(local).await.unwrap();
    assert_eq!(result.lyric.lyric_lines[0].lyric, "owned folder");
    assert_eq!(api.calls().len(), 1);
}

#[tokio::test]
async fn persistence_failure_and_auth_expiry_do_not_publish_success() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (_, _, service) = setup(
        store.clone(),
        [response("[00:01]one").into(), json!({"code":301}).into()],
    )
    .await;
    let db = rusqlite::Connection::open(tmp.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER deny_lyrics BEFORE INSERT ON state WHEN NEW.kind='lyrics_v1' BEGIN SELECT RAISE(FAIL,'test failure'); END;").unwrap();
    assert!(matches!(
        service.fetch(song("1")).await,
        Err(Error::Storage(_))
    ));
    assert!(service.snapshot().unwrap().resource_key.is_none());
    assert!(!service.snapshot().unwrap().is_loading);
    db.execute_batch("DROP TRIGGER deny_lyrics;").unwrap();
    assert!(
        service
            .fetch(song("2"))
            .await
            .unwrap_err()
            .is_service_code(301)
    );
    assert!(matches!(service.snapshot(), Err(Error::StaleResponse)));
}

#[tokio::test]
async fn versioned_views_reuse_unchanged_documents_and_report_empty_results_without_retry() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(tmp.path()).unwrap());
    let (session, api, service) = setup(
        store,
        [
            response("[00:01]one\n[00:02]two").into(),
            Reply::Ready(Err(Error::Transport("offline".into()))),
        ],
    )
    .await;
    let target = LyricTarget::Song("1".into());
    assert!(service.snapshot_if_changed(&target, None).is_err());
    service.fetch(song("1")).await.unwrap();
    let (first, _) = service.snapshot_if_changed(&target, None).unwrap().unwrap();
    let requests = api.calls().len();
    for _ in 0..100 {
        assert!(
            service
                .snapshot_if_changed(&target, Some(&first))
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(api.calls().len(), requests);
    service.adjust_offset(1.0).unwrap();
    let (offset, snapshot) = service
        .snapshot_if_changed(&target, Some(&first))
        .unwrap()
        .unwrap();
    assert_ne!(offset, first);
    assert_eq!(snapshot.lyric.offset, 1.0);
    service
        .set_preferences(LyricPreferences {
            translation_enable: false,
            roma_enable: true,
        })
        .unwrap();
    let (preferences, _) = service
        .snapshot_if_changed(&target, Some(&offset))
        .unwrap()
        .unwrap();
    assert_ne!(preferences, offset);
    service.clear().unwrap();
    assert!(
        service
            .snapshot_if_changed(&target, Some(&preferences))
            .is_err()
    );
    let missing = LyricTarget::Song("2".into());
    let empty = service.fetch(song("2")).await.unwrap();
    assert!(empty.network_failed);
    let (failed, value) = service
        .snapshot_if_changed(&missing, None)
        .unwrap()
        .unwrap();
    assert!(value.resource_key.is_none() && value.network_failed);
    for _ in 0..10 {
        assert!(
            service
                .snapshot_if_changed(&missing, Some(&failed))
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(api.calls().len(), requests + 1);
    assert!(service.snapshot_if_changed(&target, None).is_err());
    session.detach().unwrap();
    assert!(
        service
            .snapshot_if_changed(&missing, Some(&failed))
            .is_err()
    );
}
