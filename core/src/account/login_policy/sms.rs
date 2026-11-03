//! M/363's independent `massagechange` experiment, cached like M/147.
use crate::{Error, Result, protocol::api::Api, storage::StateStore};
use serde_json::{Value, json};
use std::{
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub(crate) struct SmsLoginPolicy {
    gate: tokio::sync::Mutex<()>,
    cache: Mutex<Cache>,
}
#[derive(Default)]
struct Cache {
    loaded: bool,
    attempted: Option<u64>,
    middle: bool,
}
fn assignment(value: &Value) -> Result<bool> {
    if value["code"] != 200 {
        return Err(Error::Response("SMS experiment rejected"));
    }
    let rows = value["data"]
        .as_array()
        .filter(|rows| rows.len() <= 4096)
        .ok_or(Error::Response("SMS experiment list missing"))?;
    Ok(rows
        .iter()
        .find(|row| row["expName"] == "massagechange")
        .is_some_and(|row| row["expGroupName"] == "t1"))
}
impl SmsLoginPolicy {
    pub(crate) async fn resolve(
        &self,
        api: &dyn Api,
        owner: &str,
        store: Option<&StateStore>,
    ) -> Result<bool> {
        let _gate = self.gate.lock().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::State("system clock precedes UNIX epoch"))?
            .as_secs();
        {
            let mut cache = self.cache.lock().map_err(|_| Error::LockPoisoned)?;
            if !cache.loaded {
                if let Some(store) = store {
                    cache.middle = store
                        .get(owner, "login-policy-v1", "pc-sms")?
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false);
                }
                cache.loaded = true;
            }
            if cache
                .attempted
                .is_some_and(|at| now < at.saturating_add(1800))
            {
                return Ok(cache.middle);
            }
        }
        let result = api
            .request(super::AB_PATH, json!({"expNames":"massagechange"}), false)
            .await
            .and_then(|value| assignment(&value));
        let mut cache = self.cache.lock().map_err(|_| Error::LockPoisoned)?;
        if let Ok(middle) = result {
            if let Some(store) = store {
                store.put(owner, "login-policy-v1", "pc-sms", &json!(middle))?;
            }
            cache.middle = middle;
        }
        // Failed lookup keeps the last known group or the original empty default.
        cache.attempted = Some(now);
        Ok(cache.middle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::api::ApiFuture;
    use std::{
        collections::VecDeque,
        sync::atomic::{AtomicUsize, Ordering},
    };
    struct Probe {
        calls: AtomicUsize,
        replies: Mutex<VecDeque<Result<Value>>>,
    }
    impl Probe {
        fn new(replies: impl IntoIterator<Item = Result<Value>>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                replies: Mutex::new(replies.into_iter().collect()),
            }
        }
    }
    impl Api for Probe {
        fn request<'a>(
            &'a self,
            path: &'a str,
            payload: Value,
            cache: bool,
        ) -> ApiFuture<'a, Value> {
            Box::pin(async move {
                assert_eq!(path, super::super::AB_PATH);
                assert_eq!(payload, json!({"expNames":"massagechange"}));
                assert!(!cache);
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.replies
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("unexpected experiment repeat")
            })
        }
    }
    fn assigned(group: &str) -> Value {
        json!({"code":200,"data":[{"expName":"massagechange","expGroupName":group}]})
    }
    #[tokio::test]
    async fn caches_are_owner_bound_and_reopen_failure_keeps_only_that_owners_last_group() {
        let root = tempfile::tempdir().unwrap();
        let store = StateStore::open(root.path()).unwrap();
        let first = SmsLoginPolicy::default();
        let backend = Probe::new([Ok(assigned("t1"))]);
        let (a, b) = tokio::join!(
            first.resolve(&backend, "one", Some(&store)),
            first.resolve(&backend, "one", Some(&store))
        );
        assert!(a.unwrap() && b.unwrap());
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
        let broken = Probe::new([Err(Error::Transport("synthetic lookup lost".into()))]);
        assert!(
            SmsLoginPolicy::default()
                .resolve(&broken, "one", Some(&store))
                .await
                .unwrap()
        );
        let broken = Probe::new([Err(Error::Transport("synthetic lookup lost".into()))]);
        assert!(
            !SmsLoginPolicy::default()
                .resolve(&broken, "two", Some(&store))
                .await
                .unwrap()
        );
        assert!(
            store
                .get("two", "login-policy-v1", "pc-sms")
                .unwrap()
                .is_none()
        );
        let reset = Probe::new([Ok(assigned(""))]);
        assert!(
            !SmsLoginPolicy::default()
                .resolve(&reset, "one", Some(&store))
                .await
                .unwrap()
        );
        assert_eq!(
            store.get("one", "login-policy-v1", "pc-sms").unwrap(),
            Some(json!(false))
        );
    }
    #[tokio::test]
    async fn failed_refresh_retains_group_and_clock_rollback_does_not_repeat_lookup() {
        let policy = SmsLoginPolicy::default();
        let backend = Probe::new([Ok(assigned("t1")), Ok(json!({"code":503,"data":[]}))]);
        assert!(policy.resolve(&backend, "one", None).await.unwrap());
        policy.cache.lock().unwrap().attempted = Some(0);
        assert!(policy.resolve(&backend, "one", None).await.unwrap());
        policy.cache.lock().unwrap().attempted = Some(u64::MAX);
        assert!(policy.resolve(&backend, "one", None).await.unwrap());
        assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn only_the_first_exact_official_group_enables_the_middle_route() {
        assert!(assignment(&assigned("t1")).unwrap());
        for group in ["T1", "t1 ", "t2", ""] {
            assert!(!assignment(&assigned(group)).unwrap());
        }
        assert!(!assignment(&json!({"code":200,"data":[{"expName":"massagechange","expGroupName":""},{"expName":"massagechange","expGroupName":"t1"}]})).unwrap());
        assert!(assignment(&json!({"code":200,"data":{}})).is_err());
        assert!(assignment(&json!({"code":503,"data":[]})).is_err());
        assert!(assignment(&json!({"code":200,"data":vec![json!({});4097]})).is_err());
    }
}
