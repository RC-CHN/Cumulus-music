//! Synthetic WebView proof: hold the same file lock as a background sender.
//! No requests or playback actions are emulated by this fixture.
use cumulus_core::{
    Error, Result,
    storage::{OperationLock, StateStore},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub struct Fixture {
    store: Arc<StateStore>,
    held: Mutex<Option<OperationLock>>,
}
impl Fixture {
    pub fn new(store: Arc<StateStore>) -> Self {
        Self {
            store,
            held: Mutex::new(None),
        }
    }
    pub async fn command(&self, owner: &str, action: &str) -> Result<Value> {
        match action {
            "hold" => {
                let mut acquired = None;
                for _ in 0..50 {
                    match OperationLock::acquire(self.store.directory(), "relay-v1", owner) {
                        Ok(lock) => {
                            acquired = Some(lock);
                            break;
                        }
                        Err(Error::MutationBusy) => {
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await
                        }
                        Err(error) => return Err(error),
                    }
                }
                let lock = acquired.ok_or(Error::MutationBusy)?;
                let mut held = self.held.lock().map_err(|_| Error::LockPoisoned)?;
                if held.is_some() {
                    return Err(Error::State("synthetic relay lock already held"));
                }
                *held = Some(lock);
            }
            "release" => {
                self.held.lock().map_err(|_| Error::LockPoisoned)?.take();
            }
            "inspect" => (),
            _ => return Err(Error::InvalidInput("synthetic relay lock action")),
        }
        let preparing = self
            .store
            .values(owner, "relay-playback-intents-v1")?
            .into_iter()
            .filter(|(_, value)| value["phase"] == "preparing")
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        Ok(
            json!({"held":self.held.lock().map_err(|_| Error::LockPoisoned)?.is_some(),"preparing":preparing}),
        )
    }
}
