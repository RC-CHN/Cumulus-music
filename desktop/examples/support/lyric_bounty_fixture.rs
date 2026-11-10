//! Closed GUI backend for typed bounty intent and journal state, not a live API.
use cumulus_core::{
    Error, Result, network_diagnostics::DefaultRoute, protocol::api::LyricBountyPermission,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

const OWNER: &str = "2";
const JOURNAL: &str = "lyric-bounty-v1";
pub const REVISION: &str = "synthetic-bounty-scope";

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_LYRIC_BOUNTY_PROOF").is_some()
}

pub struct Proof {
    store: Arc<StateStore>,
    calls: Mutex<Vec<String>>,
    route: AtomicU8,
}

impl Proof {
    pub fn new(store: Arc<StateStore>) -> Self {
        Self {
            store,
            calls: Mutex::default(),
            route: AtomicU8::new(0),
        }
    }

    pub fn set_route(&self, value: &str) {
        let route = match value {
            "present" => 0,
            "missing" => 1,
            "unavailable" => 2,
            _ => panic!("unexpected synthetic route status"),
        };
        self.route.store(route, Ordering::Release);
    }

    pub fn route(&self) -> DefaultRoute {
        match self.route.load(Ordering::Acquire) {
            0 => DefaultRoute::Present,
            1 => DefaultRoute::Missing,
            _ => DefaultRoute::Unavailable,
        }
    }

    pub fn submit(&self, revision: &str, permission: &LyricBountyPermission<'_>) -> Result<Value> {
        assert_eq!(revision, REVISION);
        permission().map_err(|error| Error::RequestNotSent(Box::new(error)))?;
        assert_eq!(self.route(), DefaultRoute::Present);
        let row = self
            .store
            .values(OWNER, JOURNAL)?
            .into_iter()
            .filter_map(|(key, value)| (value == "sending").then_some(key))
            .collect::<Vec<_>>();
        assert_eq!(row.len(), 1);
        let key = &row[0];
        self.calls.lock().unwrap().push(key.clone());
        match key.as_str() {
            "1:6" | "2:7" => Ok(json!({"code":200})),
            "2:8" => Err(Error::Response(
                "synthetic acknowledgement lost after dispatch",
            )),
            _ => panic!("unexpected synthetic bounty intent: {key}"),
        }
    }

    pub fn report(&self) -> Value {
        json!({"calls": *self.calls.lock().unwrap(), "journal": self.store.values(OWNER, JOURNAL).unwrap()})
    }
}
