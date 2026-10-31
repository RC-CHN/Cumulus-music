use cumulus_core::{
    Error,
    account::AccountSession,
    catalog::intelligence::*,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

struct Backend {
    response: Value,
    calls: Mutex<Vec<Value>>,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
    fail: bool,
}
impl Backend {
    fn new(response: Value) -> Arc<Self> {
        Arc::new(Self {
            response,
            calls: Mutex::new(vec![]),
            hold: AtomicBool::new(false),
            started: Notify::new(),
            release: Notify::new(),
            fail: false,
        })
    }
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push(json!({"path":path,"payload":payload,"cache":cache}));
            if self.hold.load(Ordering::Acquire) {
                self.started.notify_one();
                self.release.notified().await;
            }
            if self.fail {
                return Err(Error::Transport("synthetic lost read".into()));
            }
            Ok(self.response.clone())
        })
    }
}
fn request(input: &Value) -> IntelligenceRequest {
    IntelligenceRequest {
        playlist_id: input["playlistId"].as_str().unwrap().into(),
        song_id: input["songId"].as_str().unwrap().into(),
        start_music_id: input["startMusicId"].as_str().unwrap().into(),
        start: serde_json::from_value(input["type"].clone()).unwrap(),
        count: input["count"].as_u64().unwrap() as usize,
    }
}
fn ordinary() -> IntelligenceRequest {
    request(
        &json!({"playlistId":"31","songId":"7","startMusicId":"7","type":"fromPlayAll","count":2}),
    )
}

#[tokio::test]
async fn frozen_executed_official_wrapper_vectors_match_wire_and_formatter_branches() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/official-intelligence.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    for case in cases {
        let api = Backend::new(case["response"].clone());
        let liked = case["input"]["starPlaylist"].as_array().unwrap().clone();
        let saved = liked.clone();
        let batch = IntelligenceApi::new(api.clone())
            .recommend(&request(&case["input"]), &liked)
            .await
            .unwrap();
        assert_eq!(
            api.calls.lock().unwrap().as_slice(),
            &[case["expected"]["wire"].clone()]
        );
        assert_eq!(
            json!(
                batch
                    .entries
                    .iter()
                    .map(|v| v.song.clone())
                    .collect::<Vec<_>>()
            ),
            case["expected"]["result"]["aiTracks"],
            "{}",
            case["name"]
        );
        assert_eq!(
            serde_json::to_value(&batch.recommendations).unwrap(),
            case["expected"]["result"]["aiTracksRecommendInfo"]
        );
        assert_eq!(
            liked, saved,
            "native formatting must not mutate the caller's liked cache"
        );
    }
}

#[tokio::test]
async fn missing_liked_metadata_is_an_explicit_unresolved_row_without_an_extra_lookup() {
    let api =
        Backend::new(json!({"code":200,"data":[{"id":"999","recommended":false,"alg":"missing"}]}));
    let batch = IntelligenceApi::new(api.clone())
        .recommend(&ordinary(), &[])
        .await
        .unwrap();
    assert_eq!(batch.entries.len(), 1);
    assert_eq!(batch.entries[0].source, IntelligenceSongSource::Unresolved);
    assert!(batch.entries[0].song.is_none());
    assert_eq!(batch.recommendations["999"].recommended, Some(json!(false)));
    assert_eq!(api.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn malformed_request_or_liked_inputs_are_rejected_before_transport() {
    let api = Backend::new(json!({"code":200,"data":[]}));
    let service = IntelligenceApi::new(api.clone());
    for id in ["0", "-1", "7.0", "", "cookie=private"] {
        let mut bad = ordinary();
        bad.song_id = id.into();
        assert!(matches!(
            service.recommend(&bad, &[]).await,
            Err(Error::InvalidInput(_))
        ));
    }
    for count in [0, MAX_LIKED_TRACKS + 1] {
        let mut bad = ordinary();
        bad.count = count;
        assert!(service.recommend(&bad, &[]).await.is_err());
    }
    assert!(
        service
            .recommend(&ordinary(), &[json!({"name":"no ID"})])
            .await
            .is_err()
    );
    assert!(api.calls.lock().unwrap().is_empty());
    let mut bound = ordinary();
    bound.count = MAX_LIKED_TRACKS;
    assert_eq!(bound.payload().unwrap()["count"], MAX_LIKED_TRACKS);
    assert!(serde_json::from_value::<IntelligenceStart>(json!("random")).is_err());
}

#[tokio::test]
async fn rejected_or_malformed_responses_are_not_retried_or_replaced_with_random_songs() {
    for response in [
        json!({"code":403}),
        json!({"code":301}),
        json!({"code":200}),
        json!({"code":200,"data":null}),
        json!({"code":200,"data":{}}),
        json!({"code":200,"data":[{}]}),
        json!({"code":200,"data":[{"id":"7","songInfo":true}]}),
        json!({"code":200,"data":[{"id":"7","songInfo":{"name":"missing ID"}}]}),
    ] {
        let api = Backend::new(response);
        assert!(
            IntelligenceApi::new(api.clone())
                .recommend(&ordinary(), &[])
                .await
                .is_err()
        );
        assert_eq!(api.calls.lock().unwrap().len(), 1);
    }
    let api = Arc::new(Backend {
        response: Value::Null,
        calls: Mutex::new(vec![]),
        hold: AtomicBool::new(false),
        started: Notify::new(),
        release: Notify::new(),
        fail: true,
    });
    assert!(matches!(
        IntelligenceApi::new(api.clone())
            .recommend(&ordinary(), &[])
            .await,
        Err(Error::Transport(_))
    ));
    assert_eq!(api.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_account_change_discards_the_pending_recommendation_read() {
    let api = Backend::new(
        json!({"code":200,"data":[{"id":"7","songInfo":{"id":"7","name":"old account song"}}]}),
    );
    let session = Arc::new(AccountSession::new(api.clone(), None));
    session
        .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let token = session.token().unwrap();
    api.hold.store(true, Ordering::Release);
    let owner = session.clone();
    let old = token.clone();
    let pending = tokio::spawn(async move {
        owner
            .run_for(&old, |api, _| async move {
                IntelligenceApi::new(api).recommend(&ordinary(), &[]).await
            })
            .await
    });
    api.started.notified().await;
    session
        .adopt(
            Backend::new(json!({"code":200})),
            json!({"userId":2}),
            &token,
        )
        .unwrap();
    api.release.notify_one();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(api.calls.lock().unwrap().len(), 1);
}
