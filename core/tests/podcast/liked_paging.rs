use super::*;
use cumulus_core::{
    account::profiles::ProfileRegistry,
    client::{
        MusicClient,
        dto::{
            PodcastLikedPage, PodcastLikedPageTarget, PodcastLikedProgramTarget, PodcastLikedQuery,
        },
    },
    protocol::http::ApiOrigin,
};

const LIST: &str = "/api/content/my/liked/voice";
const COUNT: &str = "/api/content/my/liked/voice/count";

fn setup(
    replies: impl IntoIterator<Item = Reply>,
) -> (
    tempfile::TempDir,
    Arc<ScriptApi>,
    Arc<AccountSession>,
    Arc<MusicClient>,
) {
    let dir = tempfile::tempdir().unwrap();
    let api = Arc::new(ScriptApi::new(replies));
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let session = world(api.clone(), registry.store());
    let client = Arc::new(MusicClient::with_session(session.clone(), registry));
    (dir, api, session, client)
}

fn row(id: u64, time: Value) -> Value {
    json!({
        "voiceId":id, "voiceName":format!("Episode {id}"), "likeTime":time,
        "mainTrackId":id+10000, "cookie":"not-for-the-renderer"
    })
}

fn response(rows: Vec<Value>, more: bool, count: Option<u64>) -> Value {
    json!({"code":200,"data":{"data":rows,"hasMore":more,"count":count}})
}

async fn page(client: &MusicClient, query: PodcastLikedQuery) -> PodcastLikedPage {
    client.podcast_liked_page("1", query).await.unwrap().value
}

fn next(page: &PodcastLikedPage) -> PodcastLikedQuery {
    PodcastLikedQuery::Next {
        visit: page.visit.clone(),
        revision: page.revision.clone(),
        continuation: page.continuation.clone().unwrap(),
    }
}

fn suspend(page: &PodcastLikedPage) -> PodcastLikedQuery {
    PodcastLikedQuery::Suspend {
        visit: page.visit.clone(),
        revision: page.revision.clone(),
    }
}

fn restore(page: &PodcastLikedPage) -> PodcastLikedQuery {
    PodcastLikedQuery::Restore {
        visit: page.visit.clone(),
        revision: page.revision.clone(),
    }
}

fn target(page: &PodcastLikedPage, voice_id: &str) -> PodcastLikedPageTarget {
    PodcastLikedPageTarget {
        visit: page.visit.clone(),
        revision: page.revision.clone(),
        voice_id: voice_id.to_owned(),
    }
}

fn trace(api: &ScriptApi) -> Vec<(String, Value)> {
    api.calls()
        .into_iter()
        .map(|call| {
            assert!(!call.cache);
            (call.path, call.payload)
        })
        .collect()
}

fn read(cursor: Value) -> (String, Value) {
    (LIST.into(), json!({"limit":100,"likeTime":cursor}))
}

#[tokio::test]
async fn liked_paging_reads_only_the_requested_page_and_pop_reuses_the_native_cursor() {
    let mut first = (1..=100)
        .map(|id| row(id, json!(1200 - id)))
        .collect::<Vec<_>>();
    first[4]["displayStatus"] = json!("UNDER_SHELF");
    first[99]["likeTime"] = json!("1100");
    let first = response(first, true, Some(102));
    let second = response(
        vec![
            row(100, json!(1100)),
            row(101, json!(1099)),
            row(102, json!(1098)),
        ],
        false,
        None,
    );
    let (_dir, api, _, client) = setup([
        first.clone().into(),
        json!({"code":200,"data":102}).into(),
        second.clone().into(),
        first.into(),
        second.into(),
    ]);
    let visit = page(&client, PodcastLikedQuery::Open {}).await;
    assert!(visit.episodes.is_empty());
    assert!(api.calls().is_empty());

    let first = page(&client, next(&visit)).await;
    assert_eq!(trace(&api), [read(json!(0))]);
    assert_eq!(first.loaded_count, 100);
    assert_eq!(first.count, Some(102));
    assert!(first.episodes.iter().all(|row| row.voice.liked));
    assert_eq!(first.episodes[4].voice.withdrawn, Some(true));
    assert!(first.episodes[4].voice.unavailable);
    let wire = serde_json::to_string(&first).unwrap();
    assert!(!wire.contains("not-for-the-renderer") && !wire.contains("likeTime"));
    assert!(
        client
            .podcast_state("1", false)
            .await
            .unwrap()
            .value
            .liked_ids
            .is_none()
    );
    assert!(matches!(
        client.podcast_liked_page_program("1", &target(&first, "102")),
        Err(Error::QueueChanged)
    ));
    let legacy = PodcastLikedProgramTarget {
        revision: first.revision.clone(),
        voice_id: "1".into(),
    };
    assert!(matches!(
        client.podcast_liked_program("1", &legacy),
        Err(Error::QueueChanged)
    ));
    assert_eq!(client.podcast_liked_count("1").await.unwrap().value, 102);

    let suspended = page(&client, suspend(&first)).await;
    assert!(suspended.continuation.is_none() && suspended.episodes.is_empty());
    assert!(matches!(
        client.podcast_liked_page_program("1", &target(&first, "1")),
        Err(Error::QueueChanged)
    ));
    let restored = page(&client, restore(&first)).await;
    assert_eq!(restored.visit, first.visit);
    assert_ne!(restored.revision, first.revision);
    assert_eq!(restored.episodes[99].voice.id, "100");
    assert_eq!(restored.count, Some(102));
    assert_eq!(trace(&api), [read(json!(0)), (COUNT.into(), json!({}))]);
    assert!(matches!(
        client.podcast_liked_page("1", next(&first)).await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        client.podcast_liked_page("1", suspend(&first)).await,
        Err(Error::QueueChanged)
    ));
    let suspended_again = page(&client, suspend(&restored)).await;
    assert!(matches!(
        client.podcast_liked_page("1", restore(&first)).await,
        Err(Error::QueueChanged)
    ));
    let restored = page(&client, restore(&suspended_again)).await;
    let terminal = page(&client, next(&restored)).await;
    assert_eq!(terminal.loaded_count, 102);
    assert_eq!(
        terminal
            .episodes
            .iter()
            .map(|row| row.voice.id.as_str())
            .collect::<Vec<_>>(),
        ["101", "102"]
    );
    assert!(terminal.continuation.is_none());
    assert_eq!(terminal.count, Some(102));
    assert_eq!(
        client
            .podcast_liked_page_program("1", &target(&terminal, "102"))
            .unwrap()
            .value
            .id,
        "102"
    );
    assert_eq!(
        trace(&api),
        [
            read(json!(0)),
            (COUNT.into(), json!({})),
            read(json!("1100"))
        ]
    );
    assert!(
        client
            .podcast_state("1", false)
            .await
            .unwrap()
            .value
            .liked_ids
            .is_none()
    );
    let full = client
        .podcast_state("1", true)
        .await
        .unwrap()
        .value
        .liked_ids
        .unwrap();
    assert_eq!(full.len(), 102);
    assert_eq!(full.last().unwrap(), "102");
    assert_eq!(&trace(&api)[3..], [read(json!(0)), read(json!("1100"))]);
}

#[tokio::test]
async fn liked_paging_refuses_duplicate_reads_and_discards_held_results_after_reset_or_account_change()
 {
    let (first_held, first_send) = gate();
    let (second_held, second_send) = gate();
    let (third_held, third_send) = gate();
    let (_dir, api, session, client) = setup([
        first_held,
        response(vec![row(82, json!(500))], true, None).into(),
        second_held,
        third_held,
    ]);
    let first = page(&client, PodcastLikedQuery::Open {}).await;
    let other = client.clone();
    let query = next(&first);
    let first_pending = tokio::spawn(async move { other.podcast_liked_page("1", query).await });
    api.wait_for_calls(1).await;
    assert!(matches!(
        client.podcast_liked_page("1", next(&first)).await,
        Err(Error::MutationBusy)
    ));
    assert_eq!(api.calls().len(), 1);
    let second = page(&client, PodcastLikedQuery::Open {}).await;
    let second = page(&client, next(&second)).await;
    first_send
        .send(Ok(response(vec![row(81, json!(700))], false, None)))
        .unwrap();
    assert!(matches!(
        first_pending.await.unwrap(),
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        client.podcast_liked_page_program("1", &target(&second, "81")),
        Err(Error::QueueChanged)
    ));
    let other = client.clone();
    let query = next(&second);
    let second_pending = tokio::spawn(async move { other.podcast_liked_page("1", query).await });
    api.wait_for_calls(3).await;
    let suspended = page(&client, suspend(&second)).await;
    let restored = page(&client, restore(&suspended)).await;
    second_send
        .send(Ok(response(vec![row(83, json!(480))], false, None)))
        .unwrap();
    assert!(matches!(
        second_pending.await.unwrap(),
        Err(Error::QueueChanged)
    ));
    assert_eq!(restored.loaded_count, 1);

    let other = client.clone();
    let query = next(&restored);
    let third_pending = tokio::spawn(async move { other.podcast_liked_page("1", query).await });
    api.wait_for_calls(4).await;
    let foreign = Arc::new(ScriptApi::new([]));
    session
        .adopt(
            foreign.clone(),
            json!({"userId":2}),
            &session.token().unwrap(),
        )
        .unwrap();
    third_send
        .send(Ok(response(vec![row(84, json!(480))], false, None)))
        .unwrap();
    assert!(matches!(
        third_pending.await.unwrap(),
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client.podcast_liked_page("2", restore(&second)).await,
        Err(Error::QueueChanged)
    ));
    assert!(foreign.calls().is_empty());
    assert_eq!(
        trace(&api),
        [
            read(json!(0)),
            read(json!(0)),
            read(json!(500)),
            read(json!(500))
        ]
    );
}

#[tokio::test]
async fn liked_paging_keeps_the_prefix_and_exact_retry_cursor_after_invalid_or_failed_reads() {
    let (_dir, api, _, client) = setup([
        response(vec![row(81, json!(900))], true, Some(3)).into(),
        response(vec![row(82, json!("900"))], true, Some(999)).into(),
        json!({"code":500}).into(),
        response(vec![row(81, json!("850"))], true, None).into(),
        response(vec![row(83, json!(800))], false, Some(2)).into(),
    ]);
    let visit = page(&client, PodcastLikedQuery::Open {}).await;
    let first = page(&client, next(&visit)).await;
    assert!(matches!(
        client.podcast_liked_page("1", next(&first)).await,
        Err(Error::Response("liked voice cursor did not advance"))
    ));
    assert!(client.podcast_liked_page("1", next(&first)).await.is_err());
    assert!(matches!(
        client.podcast_liked_page_program("1", &target(&first, "82")),
        Err(Error::QueueChanged)
    ));
    let recovered = page(&client, next(&first)).await;
    assert_eq!(recovered.loaded_count, 1);
    assert!(recovered.episodes.is_empty());
    assert_eq!(recovered.count, Some(3));
    assert!(matches!(
        client.podcast_liked_page("1", next(&first)).await,
        Err(Error::QueueChanged)
    ));
    let last = page(&client, next(&recovered)).await;
    assert_eq!(last.episodes[0].voice.id, "83");
    assert_eq!(last.loaded_count, 2);
    assert_eq!(last.count, Some(2));
    assert_eq!(
        trace(&api),
        [
            read(json!(0)),
            read(json!(900)),
            read(json!(900)),
            read(json!(900)),
            read(json!("850"))
        ]
    );
}

#[tokio::test]
async fn liked_paging_discards_a_held_page_when_a_native_mutation_invalidates_the_prefix() {
    let (held, send) = gate();
    let (_dir, api, session, client) = setup([
        response(vec![row(81, json!(900))], true, None).into(),
        held,
        json!({"code":200,"data":{"resumePlayValue":0}}).into(),
    ]);
    let visit = page(&client, PodcastLikedQuery::Open {}).await;
    let first = page(&client, next(&visit)).await;
    let other = client.clone();
    let query = next(&first);
    let pending = tokio::spawn(async move { other.podcast_liked_page("1", query).await });
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
    send.send(Ok(response(vec![row(82, json!(800))], false, None)))
        .unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::QueueChanged)));
    assert!(matches!(
        client.podcast_liked_page_program("1", &target(&first, "81")),
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        client.podcast_liked_page("1", restore(&first)).await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(api.calls().len(), 3);
}

#[tokio::test(start_paused = true)]
async fn liked_paging_does_not_restore_a_suspended_prefix_past_its_retention_window() {
    let (_dir, api, _, client) = setup([response(vec![row(81, json!(900))], false, None).into()]);
    let visit = page(&client, PodcastLikedQuery::Open {}).await;
    let first = page(&client, next(&visit)).await;
    let suspended = page(&client, suspend(&first)).await;
    tokio::time::advance(Duration::from_secs(3_599)).await;
    let restored = page(&client, restore(&suspended)).await;
    assert_eq!(restored.episodes[0].voice.id, "81");
    let suspended = page(&client, suspend(&restored)).await;
    tokio::time::advance(Duration::from_secs(3_600)).await;
    assert!(matches!(
        client.podcast_liked_page("1", restore(&suspended)).await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(trace(&api), [read(json!(0))]);
}

#[test]
fn liked_paging_never_accepts_raw_wire_cursors_or_extra_account_keys() {
    assert!(
        serde_json::from_value::<PodcastLikedQuery>(json!({
            "kind":"next","visit":"a","revision":"b","continuation":"c","likeTime":900
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<PodcastLikedQuery>(json!({
            "kind":"open","uid":"1"
        }))
        .is_err()
    );
}

#[tokio::test]
async fn paged_liked_menu_drops_a_held_count_after_leaving_and_rejects_withdrawn_rows() {
    use cumulus_core::network_diagnostics::DefaultRoute;

    let mut withdrawn = row(81, json!(450));
    withdrawn["displayStatus"] = json!("UNDER_SHELF");
    let mut normal = row(82, json!(440));
    normal["listenerCount"] = json!("12082");
    let (held, deliver) = gate();
    let (_root, api, _, client) = setup([
        response(vec![withdrawn, normal], false, Some(2)).into(),
        held,
    ]);
    let opened = page(&client, PodcastLikedQuery::Open {}).await;
    let visible = page(&client, next(&opened)).await;
    let rendered = serde_json::to_value(&visible).unwrap();
    assert_eq!(rendered["episodes"][0]["listenerCount"], Value::Null);
    assert_eq!(rendered["episodes"][1]["listenerCount"], json!(12082));
    assert_eq!(rendered["episodes"][1]["id"], "82");

    assert!(matches!(
        client
            .podcast_liked_page_program_menu("1", &target(&visible, "81"), DefaultRoute::Present)
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(api.calls().len(), 1);

    let action = target(&visible, "82");
    let pending = tokio::spawn({
        let client = client.clone();
        let action = action.clone();
        async move {
            client
                .podcast_liked_page_program_menu("1", &action, DefaultRoute::Present)
                .await
        }
    });
    api.wait_for_calls(2).await;
    let suspended = page(&client, suspend(&visible)).await;
    deliver.send(Ok(json!({"code":200,"data":[]}))).unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::QueueChanged)));
    assert!(matches!(
        client
            .podcast_liked_page_program_directory_path("1", &action)
            .await,
        Err(Error::QueueChanged)
    ));
    let restored = page(&client, restore(&suspended)).await;
    let menu = client
        .podcast_liked_page_program_menu("1", &target(&restored, "82"), DefaultRoute::Missing)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(menu).unwrap()["value"],
        json!({"commentCount":null,"downloadComplete":false})
    );
    assert_eq!(
        trace(&api),
        [
            read(json!(0)),
            (
                "/api/resource/commentInfo/list".into(),
                json!({"resourceType":1,"resourceIds":"[\"82\"]"})
            ),
        ]
    );
    assert!(
        client
            .podcast_state("1", false)
            .await
            .unwrap()
            .value
            .liked_ids
            .is_none()
    );
}

const PREFIX: &str = "podcast-liked-prefix-v1";
const SAVED: &str = "prefix";

fn reopen(
    dir: &std::path::Path,
    uid: u64,
    replies: impl IntoIterator<Item = Reply>,
) -> (Arc<ScriptApi>, Arc<AccountSession>, MusicClient) {
    let api = Arc::new(ScriptApi::new(replies));
    let registry = ProfileRegistry::open(dir, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(
            api.clone(),
            json!({"userId":uid}),
            &session.token().unwrap(),
        )
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry);
    (api, session, client)
}

#[tokio::test]
async fn durable_liked_pop_consumes_scoped_display_cache_and_rotates_active_receipts() {
    let mut first = row(81, json!(950));
    first["displayStatus"] = json!("UNDER_SHELF");
    first["mainSong"] = json!({"id":10081,"url":"https://m.music.126.net/secret?token=private"});
    first["coverUrl"] = json!("https://p1.music.126.net/public.jpg?token=art-secret");
    first["dj"] = json!({"userId":5,"nickname":"Host","accessToken":"private-dj"});
    first["listenerCount"] = json!("61");
    let (dir, api, _, client) = setup([
        response(vec![first, row(82, json!("900"))], true, Some(5)).into(),
        json!({"code":200,"data":6}).into(),
        response(vec![row(83, json!(800))], false, None).into(),
    ]);
    let open = page(&client, PodcastLikedQuery::Open {}).await;
    let active = page(&client, next(&open)).await;
    assert_eq!(client.podcast_liked_count("1").await.unwrap().value, 6);
    let suspended = page(&client, suspend(&active)).await;
    assert!(suspended.episodes.is_empty() && suspended.continuation.is_none());
    assert!(json!(suspended).get("resume").is_none());

    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let record = store.get("1", PREFIX, SAVED).unwrap().unwrap();
    let serialized = record.to_string();
    for hidden in [
        active.visit.as_str(),
        active.revision.as_str(),
        active.continuation.as_deref().unwrap(),
        "not-for-the-renderer",
        "art-secret",
        "private-dj",
        "https://m.music.126.net",
        "mainSong",
    ] {
        assert!(!serialized.contains(hidden), "{hidden}");
    }
    assert!(serialized.contains("https://p1.music.126.net/public.jpg"));
    assert_eq!(record["count"], 6);
    assert_eq!(record["cursor"], json!("900"));
    assert!(
        client
            .podcast_state("1", false)
            .await
            .unwrap()
            .value
            .liked_ids
            .is_none()
    );

    let new = page(&client, restore(&suspended)).await;
    assert!(
        store.get("1", PREFIX, SAVED).unwrap().is_none(),
        "matching display cache was consumed"
    );
    assert_eq!(new.visit, active.visit);
    assert_ne!(new.revision, active.revision);
    assert_ne!(new.continuation, active.continuation);
    assert_eq!(new.loaded_count, 2);
    assert_eq!(new.count, Some(6));
    assert_eq!(new.episodes[0].voice.creator, "Host");
    assert_eq!(new.episodes[0].listener_count, Some(61));
    assert_eq!(new.episodes[0].voice.withdrawn, Some(true));
    assert_eq!(
        new.episodes[0].voice.cover_url.as_deref(),
        Some("https://p1.music.126.net/public.jpg?param=320y320")
    );
    assert!(matches!(
        client.podcast_liked_page_program("1", &target(&active, "82")),
        Err(Error::QueueChanged)
    ));
    assert_eq!(
        client
            .podcast_liked_page_program("1", &target(&new, "82"))
            .unwrap()
            .value
            .id,
        "82"
    );
    assert_eq!(
        trace(&api),
        [read(json!(0)), (COUNT.into(), json!({}))],
        "POP performs no wire read"
    );
    assert_eq!(page(&client, next(&new)).await.episodes[0].voice.id, "83");
    assert_eq!(
        trace(&api),
        [
            read(json!(0)),
            (COUNT.into(), json!({})),
            read(json!("900"))
        ]
    );
}

#[tokio::test]
async fn optional_liked_cache_failure_does_not_break_an_authorized_in_process_pop() {
    for failure in [
        "wrong-generation",
        "wrong-episode",
        "expired",
        "clock-reversed",
        "unwritable",
    ] {
        let (dir, api, _, client) =
            setup([response(vec![row(81, json!(900))], false, None).into()]);
        let open = page(&client, PodcastLikedQuery::Open {}).await;
        let active = page(&client, next(&open)).await;
        let suspended = page(&client, suspend(&active)).await;
        let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let mut record = store.get("1", PREFIX, SAVED).unwrap().unwrap();
        let saved = record["savedAtMs"].as_u64().unwrap();
        if failure == "unwritable" {
            let db = rusqlite::Connection::open(dir.path().join("core.sqlite3")).unwrap();
            db.execute_batch(
                "CREATE TRIGGER forbid_cache_consumption BEFORE DELETE ON state
                 WHEN OLD.kind='podcast-liked-prefix-v1'
                 BEGIN SELECT RAISE(FAIL, 'synthetic local write failure'); END;",
            )
            .unwrap();
        } else {
            record["rows"][0]["name"] = json!("Wrong cached name");
            match failure {
                "wrong-generation" => record["generation"] = json!(9000),
                "wrong-episode" => record["rows"][0]["id"] = json!("82"),
                "expired" => record["savedAtMs"] = json!(saved - 3_600_000),
                "clock-reversed" => record["savedAtMs"] = json!(saved + 3_600_000),
                _ => unreachable!(),
            }
            store.put("1", PREFIX, SAVED, &record).unwrap();
        }

        let restored = page(&client, restore(&suspended)).await;
        assert_eq!(restored.episodes[0].voice.id, "81", "{failure}");
        assert_eq!(restored.episodes[0].voice.name, "Episode 81", "{failure}");
        assert!(
            store.get("1", PREFIX, SAVED).unwrap().is_some(),
            "{failure}"
        );
        assert_eq!(trace(&api), [read(json!(0))], "{failure}");
    }
}

#[tokio::test]
async fn liked_checkpoints_never_authorize_a_route_in_another_native_instance() {
    let (dir, _, _, client) = setup([response(vec![row(81, json!(900))], false, None).into()]);
    let open = page(&client, PodcastLikedQuery::Open {}).await;
    let active = page(&client, next(&open)).await;
    let suspended = page(&client, suspend(&active)).await;
    assert!(
        serde_json::from_value::<PodcastLikedQuery>(
            json!({"kind":"restore_saved","resume":"ignored"})
        )
        .is_err()
    );
    for owner in [1, 2] {
        let (api, _, reopened) = reopen(
            dir.path(),
            owner,
            [response(vec![row(91, json!(1000))], false, None).into()],
        );
        assert!(matches!(
            reopened.podcast_liked_page("1", restore(&suspended)).await,
            Err(Error::QueueChanged)
        ));
        let empty = page(&reopened, PodcastLikedQuery::Open {}).await;
        assert_eq!(empty.loaded_count, 0);
        assert!(empty.continuation.is_some());
        assert!(api.calls().is_empty());
        assert_eq!(
            page(&reopened, next(&empty)).await.episodes[0].voice.id,
            "91"
        );
        assert_eq!(trace(&api), [read(json!(0))]);
    }
}

#[tokio::test]
async fn liked_back_keeps_the_exact_display_when_a_row_exceeds_the_local_cache_bound() {
    let title = "单".repeat(180);
    let mut first = row(81, json!(900));
    first["voiceName"] = json!(title);
    let (dir, api, _, client) = setup([response(vec![first], false, None).into()]);
    let open = page(&client, PodcastLikedQuery::Open {}).await;
    let active = page(&client, next(&open)).await;
    assert_eq!(active.episodes[0].voice.name, title);
    let suspended = page(&client, suspend(&active)).await;
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    assert!(registry.store().get("1", PREFIX, SAVED).unwrap().is_none());
    assert_eq!(
        page(&client, restore(&suspended)).await.episodes[0]
            .voice
            .name,
        title
    );
    assert_eq!(trace(&api), [read(json!(0))]);
}

#[tokio::test]
async fn liked_exit_saves_the_current_active_model_then_prunes_only_seven_day_prefixes() {
    let (dir, api, _, client) = setup([response(vec![row(81, json!(900))], false, None).into()]);
    let open = page(&client, PodcastLikedQuery::Open {}).await;
    let active = page(&client, next(&open)).await;
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let day = 24 * 60 * 60 * 1000;
    let expired = json!({"savedAtMs":now - 7 * day});
    let recent = json!({"savedAtMs":now - day});
    let future = json!({"savedAtMs":now + day});
    store.put("1", PREFIX, SAVED, &expired).unwrap();
    store.put("2", PREFIX, SAVED, &expired).unwrap();
    store.put("3", PREFIX, SAVED, &recent).unwrap();
    store.put("4", PREFIX, SAVED, &future).unwrap();
    store.put("2", PREFIX, "unrelated-key", &expired).unwrap();
    store.put("2", "unrelated-kind", SAVED, &expired).unwrap();

    assert_eq!(client.finish_podcast_liked_checkpoints().unwrap(), 1);
    let saved = store.get("1", PREFIX, SAVED).unwrap().unwrap();
    assert!(saved["savedAtMs"].as_u64().unwrap() >= now);
    assert_eq!(saved["rows"][0]["id"], "81");
    assert!(store.get("2", PREFIX, SAVED).unwrap().is_none());
    assert_eq!(store.get("3", PREFIX, SAVED).unwrap(), Some(recent));
    assert_eq!(store.get("4", PREFIX, SAVED).unwrap(), Some(future));
    assert_eq!(
        store.get("2", PREFIX, "unrelated-key").unwrap(),
        Some(expired.clone())
    );
    assert_eq!(
        store.get("2", "unrelated-kind", SAVED).unwrap(),
        Some(expired)
    );
    assert_eq!(
        client
            .podcast_liked_page_program("1", &target(&active, "81"))
            .unwrap()
            .value
            .id,
        "81"
    );

    let suspended = page(&client, suspend(&active)).await;
    let mut saved = store.get("1", PREFIX, SAVED).unwrap().unwrap();
    saved["savedAtMs"] = json!(now - day);
    store.put("1", PREFIX, SAVED, &saved).unwrap();
    assert_eq!(client.finish_podcast_liked_checkpoints().unwrap(), 0);
    assert_eq!(
        store.get("1", PREFIX, SAVED).unwrap(),
        Some(saved),
        "exit does not renew a departed route"
    );
    assert!(matches!(
        client.podcast_liked_page_program("1", &target(&suspended, "81")),
        Err(Error::QueueChanged)
    ));
    assert_eq!(trace(&api), [read(json!(0))]);
}
