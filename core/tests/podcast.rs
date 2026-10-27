mod support;
use cumulus_core::{
    Error,
    account::{AccountSession, resources::MutationPhase},
    catalog::PlayOptions,
    playback::source::SourceResolver,
    podcast::{Episode, LibraryKind, PodcastAction, PodcastApi, PodcastService, Radio},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::{Reply, ScriptApi, gate};
fn episode(id: u64) -> Value {
    json!({"id":id,"name":"Episode","mainSong":{"id":91,"duration":120000},"radio":{"id":71,"name":"Radio"},"dj":{"userId":7,"nickname":"Host"}})
}
fn detail(id: u64) -> Reply {
    json!({"code":200,"program":episode(id)}).into()
}
fn permission(id: u64, can_play: bool) -> Reply {
    json!({"code":200,"data":{"voiceId":id,"songId":91,"canPlay":can_play}}).into()
}
fn media() -> Reply {
    json!({"code":200,"data":[{"id":91,"code":200,"url":"https://m.test.music.126.net/program","md5":"abc","expi":300,"level":"standard"}]}).into()
}
fn world(api: Arc<ScriptApi>, store: Arc<StateStore>) -> Arc<AccountSession> {
    let s = Arc::new(AccountSession::new(api.clone(), Some(store)));
    s.adopt(api, json!({"userId":1}), &s.token().unwrap())
        .unwrap();
    s
}
fn radio_page(ids: Vec<Value>, cards: Vec<Value>) -> Reply {
    json!({"code":200,"data":{"radioIdList":ids,"myPodcastVOList":cards}}).into()
}
fn liked(ids: &[u64], more: bool, time: u64) -> Reply {
    json!({"code":200,"data":{"data":ids.iter().map(|id|json!({"voiceId":id,"voiceName":"Card","likeTime":time})).collect::<Vec<_>>(),"hasMore":more}}).into()
}

#[test]
fn official_radio_and_voice_identity_fallbacks_match_executed_javascript() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/podcast-official.json")).unwrap();
    for row in fixture["cases"].as_array().unwrap() {
        let output = if row["kind"] == "radio" {
            serde_json::to_value(Radio::from_official(&row["input"]).unwrap()).unwrap()
        } else {
            serde_json::to_value(Episode::from_official(&row["input"]).unwrap()).unwrap()
        };
        for (k, v) in row["expected"].as_object().unwrap() {
            assert_eq!(&output[k], v, "{} {k}", row["input"]);
        }
    }
    let e = Episode::from_official(&episode(81)).unwrap();
    assert_eq!(e.playback_resource().unwrap()["id"], "91");
    assert_eq!(e.playback_resource().unwrap()["voice_id"], "81");
    assert_eq!(e.normalized()["audio"]["id"], "71");
    assert_eq!(e.normalized()["dj"]["id"], 7);
    assert!(
        Episode::from_official(&json!({"voiceId":81}))
            .unwrap()
            .playback_resource()
            .is_err()
    );
}
#[tokio::test]
async fn my_library_preserves_first_id_snapshot_and_official_twenty_id_batches() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/podcast-official.json")).unwrap();
    let api = Arc::new(ScriptApi::new([
        radio_page(
            (1..=22).map(|i| json!(i)).collect(),
            (1..=20).map(|i| json!({"voiceListId":i})).collect(),
        ),
        radio_page(
            vec![],
            vec![json!({"id":20}), json!({"id":21}), json!({"id":22})],
        ),
    ]));
    let library = PodcastApi::new(api.clone())
        .my_library(LibraryKind::Podcast)
        .await
        .unwrap();
    assert_eq!(
        library.ids,
        fixture["pagination"]["first"]["radioIdList"]
            .as_array()
            .unwrap()
            .clone()
    );
    assert_eq!(
        library
            .items
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        (1..=22).map(|i| i.to_string()).collect::<Vec<_>>()
    );
    assert!(!library.more());
    assert_eq!(library.next_index, 22);
    for (call, expected) in api
        .calls()
        .iter()
        .zip(fixture["pagination"]["calls"].as_array().unwrap())
    {
        assert_eq!(&call.payload, expected);
        assert!(!call.cache);
    }
    for reply in [
        radio_page(vec![json!(1), json!(1)], vec![]),
        radio_page(vec![json!(1)], vec![json!({"id":2})]),
    ] {
        assert!(
            PodcastApi::new(Arc::new(ScriptApi::new([reply])))
                .my_library(LibraryKind::Podcast)
                .await
                .is_err()
        );
    }
}
#[tokio::test]
async fn legacy_cursor_is_time_tuple_and_created_id_not_offset() {
    for created in [false, true] {
        let api=Arc::new(ScriptApi::new([
            json!({"code":200,"data":{"data":[{"id":71,"createTime":100,"lastProgramCreateTime":200}],"hasMore":true}}).into(),
            json!({"code":200,"data":{"data":[{"id":72}],"hasMore":false}}).into(),
        ]));
        let items = PodcastApi::new(api.clone())
            .legacy_library(created)
            .await
            .unwrap();
        assert_eq!(items.len(), 2);
        let payload = &api.calls()[1].payload;
        assert_eq!(payload["createTime"], 100);
        assert_eq!(payload["lastProgramCreateTime"], 200);
        assert!(payload.get("offset").is_none());
        assert_eq!(
            payload["lastPodcastId"],
            if created { json!("71") } else { Value::Null }
        );
    }
    let repeating = json!({"code":200,"data":{"data":[{"id":71,"createTime":100,"lastProgramCreateTime":200}],"hasMore":true}});
    assert!(
        PodcastApi::new(Arc::new(ScriptApi::new([
            repeating.clone().into(),
            repeating.into()
        ])))
        .legacy_library(false)
        .await
        .is_err()
    );
}
#[tokio::test]
async fn liked_voice_cursor_preserves_types_and_filters_duplicate_overlap() {
    let api = Arc::new(ScriptApi::new([
        liked(&[81, 82], true, 900),
        liked(&[82, 83], false, 800),
    ]));
    let rows = PodcastApi::new(api.clone()).liked_voices().await.unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|v| v.liked == Some(true)));
    assert_eq!(api.calls()[1].payload, json!({"limit":100,"likeTime":900}));
    let api = Arc::new(ScriptApi::new([
        liked(&[81], true, 900),
        liked(&[81], true, 900),
    ]));
    assert!(PodcastApi::new(api).liked_voices().await.is_err());
}
#[tokio::test]
async fn discovery_retains_overlap_and_accepts_server_page_size_above_requested_limit() {
    let api=Arc::new(ScriptApi::new([
        json!({"code":200,"data":{"radioList":(1..=175).map(|i|json!({"id":i})).collect::<Vec<_>>(),"page":{"size":175,"cursor":"5","more":true}}}).into(),
        json!({"code":200,"data":{"blocks":[{"blockId":1,"items":[1]},{"blockId":1,"items":[2]}],"cursor":"next","hasMore":true}}).into(),
        json!({"code":200,"data":false}).into(),
    ]));
    let service = PodcastApi::new(api.clone());
    let page = service.category("1", "000", "", 5).await.unwrap();
    assert_eq!(api.calls()[0].payload["subId"], "000");
    assert_eq!(page.records.len(), 175);
    assert_eq!(page.cursor, "5");
    assert_eq!(service.home("").await.unwrap().records.len(), 2);
    assert!(!service.skip_enabled(&json!(71)).await.unwrap());
    assert_eq!(api.calls()[2].payload, json!({"radioId":71}));
}
#[tokio::test]
async fn episode_page_checks_order_and_preserves_cross_radio_collection_members() {
    for (more, asc, parent, success) in [
        (true, true, 71, true),
        (false, false, 71, false),
        (false, true, 72, true),
    ] {
        let mut item = episode(81);
        item["radio"]["id"] = json!(parent);
        let api = Arc::new(ScriptApi::new([
            json!({"code":200,"data":{"programs":[item],"count":2,"more":more,"asc":asc}}).into(),
        ]));
        let result = PodcastApi::new(api.clone())
            .programs(&json!(71), 1, 0, true)
            .await;
        assert_eq!(result.is_ok(), success);
        if success {
            assert_eq!(
                result.as_ref().unwrap().items[0].radio.as_ref().unwrap().id,
                parent.to_string()
            );
        }
        assert_eq!(
            api.calls()[0].payload,
            json!({"radioId":"71","limit":1,"offset":0,"asc":true})
        );
    }
}
#[tokio::test]
async fn curated_radio_snapshot_retains_each_voices_own_resume_and_permission_identity() {
    use cumulus_core::podcast::{PodcastLibrary, PodcastScope};
    let mut other = episode(82);
    other["radio"]["id"] = json!(72);
    other["mainSong"]["id"] = json!(92);
    let api=Arc::new(ScriptApi::new([
        json!({"code":200,"data":{"id":71,"name":"Collection"}}).into(),
        json!({"code":200,"data":{"programs":[episode(81),other],"count":2,"more":false,"asc":true}}).into(),
    ]));
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let session = world(api.clone(), store);
    let token = session.token().unwrap();
    let library = PodcastLibrary::new(session.clone()).unwrap();
    let scope = PodcastScope::Radio {
        id: "71".into(),
        ascending: true,
    };
    let snapshot = library.read_for(&token, &scope, true).await.unwrap();
    assert_eq!(snapshot.source_ids, vec!["81", "82"]);
    let resource = snapshot.voices[1].playback_resource().unwrap();
    assert_eq!(resource["voice_id"], "82");
    assert_eq!(resource["id"], "92");
    assert_eq!(resource["voice"]["audio"]["id"], "72");
    let cached = PodcastLibrary::new(session)
        .unwrap()
        .cached_for(&token, &scope)
        .unwrap()
        .unwrap();
    assert_eq!(cached.revision, snapshot.revision);
    assert_eq!(cached.voices[1].radio.as_ref().unwrap().id, "72");
    assert_eq!(api.calls().len(), 2);
}
#[tokio::test]
async fn denied_or_mismatched_program_never_requests_media() {
    for check in [
        permission(81, false),
        permission(82, true),
        json!({"code":200,"data":{"voiceId":81,"songId":92,"canPlay":true}}).into(),
    ] {
        let api = Arc::new(ScriptApi::new([check]));
        assert!(
            PodcastApi::new(api.clone())
                .play_url(
                    &Episode::from_official(&episode(81)).unwrap(),
                    PlayOptions::default()
                )
                .await
                .is_err()
        );
        assert_eq!(api.calls().len(), 1);
    }
    let api = Arc::new(ScriptApi::new([permission(81, true), media()]));
    PodcastApi::new(api.clone())
        .play_url(
            &Episode::from_official(&episode(81)).unwrap(),
            PlayOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(api.calls()[0].payload, json!({"voiceId":"81"}));
    assert_eq!(api.calls()[1].payload["ids"], "[\"91\"]");
    for data in [
        json!({"allowDownloadVoiceIds":[81],"forbidDownloadVoiceIds":[81],"downloadCheckResultList":[]}),
        json!({"allowDownloadVoiceIds":[82],"forbidDownloadVoiceIds":[],"downloadCheckResultList":[]}),
        json!({"allowDownloadVoiceIds":[81],"forbidDownloadVoiceIds":[],"downloadCheckResultList":[{"voiceId":81,"canDownload":false}]}),
    ] {
        let api = Arc::new(ScriptApi::new([json!({"code":200,"data":data}).into()]));
        assert!(
            PodcastApi::new(api.clone())
                .download_permission(&[json!(81)])
                .await
                .is_err()
        );
        assert!(api.calls()[0].payload["voiceIdsStr"].is_array());
    }
}
#[tokio::test]
async fn resolver_cache_cannot_turn_song_or_other_program_authorization_into_program_access() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new([
        media(),
        detail(81),
        permission(81, true),
        media(),
        detail(82),
        permission(82, false),
    ]));
    let session = world(
        api.clone(),
        Arc::new(StateStore::open(root.path()).unwrap()),
    );
    let token = session.token().unwrap();
    let resolver = SourceResolver::new(session, None);
    resolver.resolve(&json!({"id":91}), &token).await.unwrap();
    let program = json!({"id":91,"voice_id":81});
    resolver.resolve(&program, &token).await.unwrap();
    resolver.resolve(&program, &token).await.unwrap();
    assert_eq!(api.calls().len(), 4);
    assert!(
        resolver
            .resolve(&json!({"id":91,"voice_id":82}), &token)
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 6);
}
fn state(action: PodcastAction, value: bool) -> Reply {
    match action {
        PodcastAction::Subscribe => json!({"code":200,"data":{"id":71,"subed":value}}).into(),
        PodcastAction::Like => liked(if value { &[71] } else { &[] }, false, 10),
        PodcastAction::Resume => {
            json!({"code":200,"data":{"resumePlayValue":u8::from(value)}}).into()
        }
    }
}
#[tokio::test]
async fn program_download_uses_actual_bitrate_when_level_is_null_and_enforces_new_user_limit() {
    let episode = Episode::from_official(&episode(81)).unwrap();
    let permission = || {
        json!({"code":200,"data":{"allowDownloadVoiceIds":[81],"forbidDownloadVoiceIds":[],"downloadCheckResultList":[{"voiceId":81,"canDownload":true}]}}).into()
    };
    for (br, expected) in [
        (128000, Some("standard")),
        (192000, Some("higher")),
        (320000, Some("exhigh")),
        (999000, Some("lossless")),
        (111111, None),
    ] {
        let api = Arc::new(ScriptApi::new([
            permission(),
            json!({"code":200,"data":{"code":200,"id":91,"br":br,"level":null}}).into(),
        ]));
        let result = PodcastApi::new(api).download_url(&episode, "hires").await;
        if let Some(expected) = expected {
            let value = result.unwrap();
            assert_eq!(value["level"], expected);
            assert_eq!(value["official_level"], Value::Null);
            assert_eq!(value["level_origin"], "official_bitrate");
        } else {
            assert!(result.is_err());
        }
    }
    let mut limited = episode;
    limited.raw["icon"] = json!({"type":"NEWUSERLIMITFREE"});
    let api = Arc::new(ScriptApi::new([
        permission(),
        json!({"code":200,"data":{"code":200,"id":91,"br":128000,"level":null}}).into(),
    ]));
    let error = PodcastApi::new(api)
        .download_url(&limited, "standard")
        .await
        .unwrap_err();
    assert!(error.is_service_code(-144));
}
#[tokio::test]
async fn all_mutations_use_exact_official_payloads_and_wait_for_two_consistent_reads() {
    for action in [
        PodcastAction::Subscribe,
        PodcastAction::Like,
        PodcastAction::Resume,
    ] {
        let root = tempfile::tempdir().unwrap();
        let api = Arc::new(ScriptApi::new([
            state(action, false),
            json!({"code":200}).into(),
            state(action, false),
            state(action, true),
            state(action, false),
            state(action, true),
            state(action, true),
        ]));
        let session = world(
            api.clone(),
            Arc::new(StateStore::open(root.path()).unwrap()),
        );
        let token = session.token().unwrap();
        let service = PodcastService::new(session)
            .unwrap()
            .with_settle_delay(Duration::ZERO);
        let record = service
            .change_for(&token, action, "71", true)
            .await
            .unwrap();
        assert!(record.acknowledged);
        assert_eq!(record.phase, MutationPhase::Confirmed);
        assert_eq!(api.calls().len(), 7);
        let expected = match action {
            PodcastAction::Subscribe => ("/api/djradio/sub", json!({"id":"71"})),
            PodcastAction::Like => ("/api/resource/like", json!({"threadId":"A_DJ_1_71"})),
            PodcastAction::Resume => (
                "/api/dj/user/setting/resumeplay/update",
                json!({"radioId":"71","resumePlayValue":1}),
            ),
        };
        assert_eq!(api.calls()[1].path, expected.0);
        assert_eq!(api.calls()[1].payload, expected.1);
    }
}
#[tokio::test]
async fn lost_write_reply_survives_reopen_and_reconciliation_does_not_resend() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let api = Arc::new(ScriptApi::new([
        state(PodcastAction::Resume, false),
        Reply::Ready(Err(Error::Transport("lost response".into()))),
        state(PodcastAction::Resume, true),
        state(PodcastAction::Resume, true),
    ]));
    let session = world(api.clone(), store);
    let token = session.token().unwrap();
    let service = PodcastService::new(session.clone()).unwrap();
    assert!(
        service
            .change_for(&token, PodcastAction::Resume, "71", true)
            .await
            .is_err()
    );
    drop(service);
    let reopened = PodcastService::new(session)
        .unwrap()
        .with_settle_delay(Duration::ZERO);
    assert!(
        reopened
            .change_for(&token, PodcastAction::Resume, "71", false)
            .await
            .is_err()
    );
    let record = reopened
        .reconcile_for(&token, PodcastAction::Resume, "71")
        .await
        .unwrap();
    assert!(!record.acknowledged);
    assert_eq!(record.phase, MutationPhase::Confirmed);
    assert_eq!(
        api.calls()
            .iter()
            .filter(|v| v.path.ends_with("/update"))
            .count(),
        1
    );
}
#[tokio::test]
async fn account_switch_during_write_retains_original_pending_journal_and_rejects_late_data() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let (pending, reply) = gate();
    let api = Arc::new(ScriptApi::new([
        state(PodcastAction::Resume, false),
        pending,
    ]));
    let session = world(api.clone(), store.clone());
    let token = session.token().unwrap();
    let service = Arc::new(PodcastService::new(session.clone()).unwrap());
    let task = tokio::spawn({
        let service = service.clone();
        let token = token.clone();
        async move {
            service
                .change_for(&token, PodcastAction::Resume, "71", true)
                .await
        }
    });
    api.wait_for_calls(2).await;
    session
        .adopt(Arc::new(ScriptApi::new([])), json!({"userId":2}), &token)
        .unwrap();
    reply.send(Ok(json!({"code":200}))).unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        store
            .values("2", "podcast-mutations-v1")
            .unwrap()
            .is_empty()
    );
    let values = store.values("1", "podcast-mutations-v1").unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values.values().next().unwrap()["phase"], "uncertain");
    assert!(
        service
            .operation_for(&token, PodcastAction::Resume, "71")
            .is_err()
    );
}

#[tokio::test]
async fn mutation_invalidates_an_inflight_list_before_it_can_commit_stale_data() {
    use cumulus_core::podcast::{PodcastLibrary, PodcastScope};
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let (held, send) = gate();
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"data":{"id":71}}).into(),
        held,
        json!({"code":200,"data":{"resumePlayValue":0}}).into(),
    ]));
    let session = world(api.clone(), store);
    let scope = PodcastScope::Radio {
        id: "71".into(),
        ascending: true,
    };
    let capture = session.clone();
    let s = scope.clone();
    let reading = tokio::spawn(async move {
        PodcastLibrary::new(capture.clone())
            .unwrap()
            .read_for(&capture.token().unwrap(), &s, true)
            .await
    });
    api.wait_for_calls(2).await;
    PodcastService::new(session.clone())
        .unwrap()
        .change_for(
            &session.token().unwrap(),
            PodcastAction::Resume,
            "71",
            false,
        )
        .await
        .unwrap();
    send.send(Ok(
        json!({"code":200,"data":{"programs":[episode(81)],"count":1,"more":false,"asc":true}}),
    ))
    .unwrap();
    assert!(matches!(reading.await.unwrap(), Err(Error::StaleResponse)));
    assert!(
        PodcastLibrary::new(session.clone())
            .unwrap()
            .cached_for(&session.token().unwrap(), &scope)
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn complete_snapshots_reopen_without_network_and_failed_refresh_keeps_last_complete_list() {
    use cumulus_core::podcast::{PodcastLibrary, PodcastScope};
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(dir.path()).unwrap());
    let radio = || json!({"code":200,"data":{"id":71}}).into();
    let page = |ids: &[u64], more: bool| {
        json!({"code":200,"data":{"programs":ids.iter().map(|id|episode(*id)).collect::<Vec<_>>(),"count":3,"more":more,"asc":true}}).into()
    };
    let api = Arc::new(ScriptApi::new([
        radio(),
        page(&[81, 82], true),
        page(&[83], false),
        radio(),
        page(&[81, 82], true),
        page(&[82], false),
    ]));
    let session = world(api.clone(), store);
    let scope = PodcastScope::Radio {
        id: "71".into(),
        ascending: true,
    };
    let library = PodcastLibrary::new(session.clone()).unwrap();
    let first = library
        .read_for(&session.token().unwrap(), &scope, true)
        .await
        .unwrap();
    assert_eq!(first.voices.len(), 3);
    assert!(matches!(
        library
            .read_for(&session.token().unwrap(), &scope, true)
            .await,
        Err(Error::QueueChanged)
    ));
    let silent = Arc::new(ScriptApi::new([]));
    let reopened = world(
        silent.clone(),
        Arc::new(StateStore::open(dir.path()).unwrap()),
    );
    let cached = PodcastLibrary::new(reopened.clone())
        .unwrap()
        .read_for(&reopened.token().unwrap(), &scope, false)
        .await
        .unwrap();
    assert_eq!(cached.revision, first.revision);
    assert_eq!(cached.voices.len(), 3);
    assert!(silent.calls().is_empty());
    assert_eq!(api.calls()[2].payload["offset"], 2);
}
