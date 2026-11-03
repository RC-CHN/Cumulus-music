use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
};

const ASSIGN: &str = "/api/rtrs/abt/front/expinfo/list";
const V1: &str = "/api/v1/discovery/recommend/songs";
const V3: &str = "/api/v3/discovery/recommend/songs";

type Reply = (&'static str, Result<Value>);
struct Script(Mutex<VecDeque<Reply>>);

impl Api for Script {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let (expected, reply) = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request");
            assert_eq!(path, expected);
            assert!(!cache);
            assert_eq!(
                args,
                if path == ASSIGN {
                    json!({"expNames":"PH-PC-Blacklist"})
                } else {
                    json!({"limit":30})
                }
            );
            reply
        })
    }
}

fn client(
    root: &Path,
    owner: u64,
    requests: impl IntoIterator<Item = Reply>,
) -> (MusicClient, Arc<Script>) {
    let registry = ProfileRegistry::open(root, ApiOrigin::Pc).unwrap();
    let api = Arc::new(Script(Mutex::new(requests.into_iter().collect())));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(
            api.clone(),
            json!({"userId":owner}),
            &session.token().unwrap(),
        )
        .unwrap();
    (
        MusicClient::with_session(session, registry).with_official_daily_experiment(),
        api,
    )
}

#[tokio::test]
async fn daily_cache_survives_reopen_but_never_replaces_empty_fresh_results_or_permission_failures()
{
    let root = tempfile::tempdir().unwrap();
    let (first, wire) = client(
        root.path(),
        1,
        [
            (ASSIGN, Ok(json!({"code":200,"data":[]}))),
            (
                V1,
                Ok(
                    json!({"code":200,"recommend":[{"id":19,"name":"first","url":"private-token"},{"id":7,"name":"second"}],"data":{"showFreeTag":false}}),
                ),
            ),
            (V1, Ok(json!({"code":200,"recommend":[]}))),
        ],
    );
    let fresh = first.daily_songs().await.unwrap().value;
    assert_eq!(
        fresh
            .tracks
            .iter()
            .map(|track| track.id.as_str())
            .collect::<Vec<_>>(),
        ["19", "7"]
    );
    assert!(!fresh.daily.unwrap().cached);
    let empty = first.daily_songs().await.unwrap().value;
    assert!(empty.tracks.is_empty());
    assert!(!empty.daily.unwrap().cached);
    assert!(wire.0.lock().unwrap().is_empty());
    drop(first);
    let stored = ProfileRegistry::open(root.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("1", "daily-recommendations", "tracks-v1")
        .unwrap()
        .unwrap();
    assert!(!stored.to_string().contains("private-token"));
    let (reopened, wire) = client(
        root.path(),
        1,
        [
            (V1, Err(Error::Transport("offline".into()))),
            (V1, Ok(json!({"code":403}))),
            (V1, Err(Error::CredentialChanged)),
            (
                V1,
                Err(Error::HttpStatus {
                    status: 503,
                    endpoint: Some(V1.into()),
                    body: Some(Box::new(json!({"code":301}))),
                }),
            ),
        ],
    );
    let cached = reopened.daily_songs().await.unwrap().value;
    assert_eq!(
        cached
            .tracks
            .iter()
            .map(|track| track.id.as_str())
            .collect::<Vec<_>>(),
        ["19", "7"]
    );
    assert!(cached.revision.is_some());
    let state = cached.daily.unwrap();
    assert!(state.cached && state.show_free_tag);
    assert!(matches!(reopened.daily_songs().await, Err(error) if error.is_service_code(403)));
    assert!(matches!(
        reopened.daily_songs().await,
        Err(Error::CredentialChanged)
    ));
    assert!(matches!(reopened.daily_songs().await, Err(error) if error.is_service_code(301)));
    assert!(wire.0.lock().unwrap().is_empty());
    let (other, other_wire) = client(
        root.path(),
        2,
        [
            (
                ASSIGN,
                Ok(json!({"code":200,"data":[{"expName":"PH-PC-Blacklist","expGroupName":"t1"}]})),
            ),
            (V3, Err(Error::Transport("offline".into()))),
        ],
    );
    assert!(matches!(
        other.daily_songs().await,
        Err(Error::Transport(_))
    ));
    assert!(other_wire.0.lock().unwrap().is_empty());
}
