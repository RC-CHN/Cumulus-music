use super::*;
use crate::protocol::api::{Api, ApiFuture};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

#[derive(Default)]
struct Gate {
    entered: Notify,
    release: Notify,
}

#[derive(Default)]
struct Backend {
    target_calls: AtomicUsize,
    other_calls: AtomicUsize,
    old_target: Gate,
    other: Gate,
}

impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/song/enhance/player/url/v1");
            let ids: Vec<Value> = serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
            let id = identifier(&ids[0]).unwrap();
            let (gate, level) = match id.as_str() {
                "1" if self.target_calls.fetch_add(1, Ordering::SeqCst) == 0 => {
                    (Some(&self.old_target), "standard")
                }
                "1" => (None, "higher"),
                "2" => {
                    self.other_calls.fetch_add(1, Ordering::SeqCst);
                    (Some(&self.other), "standard")
                }
                _ => panic!("unexpected synthetic song"),
            };
            if let Some(gate) = gate {
                gate.entered.notify_one();
                gate.release.notified().await;
            }
            Ok(json!({"code":200,"data":[{"id":id,"code":200,
                "url":"https://m1.music.126.net/audio?fixture=1","md5":"fixture",
                "level":level,"expi":120}]}))
        })
    }
}

#[tokio::test]
async fn fresh_authorization_retires_only_the_exact_old_source_and_reuses_the_new_result() {
    let api = Arc::new(Backend::default());
    let session = Arc::new(AccountSession::new(api.clone(), None));
    session
        .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
        .unwrap();
    let token = session.token().unwrap();
    let resolver = Arc::new(SourceResolver::new(session, None));
    let request = |song: &'static str| {
        let (resolver, token) = (resolver.clone(), token.clone());
        tokio::spawn(async move { resolver.resolve(&json!({"id":song}), &token).await })
    };
    let old = request("1");
    let unrelated = request("2");
    for gate in [&api.old_target, &api.other] {
        tokio::time::timeout(Duration::from_secs(5), gate.entered.notified())
            .await
            .unwrap();
    }
    let current = resolver
        .resolve_fresh_online(&json!({"id":"1"}), &token)
        .await
        .unwrap();
    assert_eq!(current.actual_quality(), Some("higher"));
    api.old_target.release.notify_one();
    api.other.release.notify_one();
    assert!(matches!(
        old.await.unwrap().err().unwrap().root(),
        Error::StaleResponse
    ));
    let other = unrelated.await.unwrap().unwrap();
    for (song, expected) in [("1", current), ("2", other)] {
        let cached = resolver.resolve(&json!({"id":song}), &token).await.unwrap();
        assert!(Arc::ptr_eq(&cached, &expected));
    }
    assert_eq!(api.target_calls.load(Ordering::SeqCst), 2);
    assert_eq!(api.other_calls.load(Ordering::SeqCst), 1);
}
