//! Official M/92 configuration center. Web responses never become PC policy.
mod http;
mod model;
use crate::{Error, Result, storage::StateStore};
pub use http::{ConfigBackend, NativeConfigBackend};
pub use model::{APPLICATION_KEY, ConfigUrl, Snapshot, Update};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::{Mutex as AsyncMutex, watch};

pub struct PcConfig {
    store: Arc<StateStore>,
    backend: Arc<dyn ConfigBackend>,
    gate: AsyncMutex<()>,
    closed: Mutex<bool>,
    cancel: watch::Sender<bool>,
}
impl PcConfig {
    pub fn new(store: Arc<StateStore>, backend: Arc<dyn ConfigBackend>) -> Result<Arc<Self>> {
        let (cancel, _) = watch::channel(false);
        let service = Arc::new(Self {
            store,
            backend,
            gate: AsyncMutex::new(()),
            closed: Mutex::new(false),
            cancel,
        });
        service.snapshot()?;
        Ok(service)
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let snapshot = self
            .store
            .get(model::OWNER, model::KIND, "pc")?
            .map(serde_json::from_value::<Snapshot>)
            .transpose()?
            .unwrap_or_default();
        snapshot.validate()?;
        Ok(snapshot)
    }
    pub fn close(&self) -> Result<()> {
        *self.closed.lock().map_err(|_| Error::LockPoisoned)? = true;
        self.cancel.send_replace(true);
        Ok(())
    }
    fn open(&self) -> Result<std::sync::MutexGuard<'_, bool>> {
        let guard = self.closed.lock().map_err(|_| Error::LockPoisoned)?;
        if *guard {
            return Err(Error::Cancelled);
        }
        Ok(guard)
    }
    /// Fire-and-forget as M/92 u(): an ancillary configuration failure cannot
    /// turn a successful business write into an unknown result or replay it.
    pub fn observe(self: &Arc<Self>, metadata: BTreeMap<String, String>) {
        let owned = self.clone();
        tokio::spawn(async move {
            let _ = owned.resolve(&metadata).await;
        });
    }
    /// Explicitly awaitable for native startup/tools and verification. One
    /// update is in flight; subsequent responses still update the bucket.
    pub async fn resolve(&self, metadata: &BTreeMap<String, String>) -> Result<Update> {
        {
            let _open = self.open()?;
            if let Some(bucket) =
                model::header(metadata, "mconfig-bucket").filter(|s| !s.is_empty())
            {
                if bucket.len() > 4096 || bucket.contains(['\r', '\n', '\0']) {
                    return Err(Error::Response("invalid MConfig bucket"));
                }
                self.store.transaction(|tx| {
                    let mut current = tx
                        .get(model::OWNER, model::KIND, "pc")?
                        .map(serde_json::from_value::<Snapshot>)
                        .transpose()?
                        .unwrap_or_default();
                    current.validate()?;
                    if current.bucket != bucket {
                        current.bucket = bucket.into();
                        tx.put(
                            model::OWNER,
                            model::KIND,
                            "pc",
                            &serde_json::to_value(current)?,
                        )?;
                    }
                    Ok(())
                })?;
            }
        }
        let info = model::information(metadata)?;
        if !info.is_object() {
            return Ok(Update::Ignored);
        }
        if info["os"] != "pc" {
            return Ok(Update::NonPc);
        }
        let Some(version) = info["version"]
            .as_u64()
            .filter(|v| *v > 0 && *v <= 9_007_199_254_740_991)
        else {
            return Ok(Update::Ignored);
        };
        let before = self.snapshot()?;
        if version == before.version {
            return Ok(Update::Current);
        }
        let Some(url) = info["fullUrl"].as_str().filter(|s| !s.is_empty()) else {
            return Ok(Update::Ignored);
        };
        let url = ConfigUrl::parse(url)?;
        let Ok(_gate) = self.gate.try_lock() else {
            return Ok(Update::InProgress);
        };
        let mut cancel = self.cancel.subscribe();
        drop(self.open()?);
        let data = tokio::select! {biased; _=cancelled(&mut cancel)=>return Err(Error::Cancelled),r=self.backend.get(&url)=>r?};
        if !data.is_object() || serde_json::to_vec(&data)?.len() > model::MAX_BYTES {
            return Err(Error::Response("invalid MConfig object"));
        }
        // Parse protected groups before publishing. A malformed fetched policy
        // must not erase the last usable one or block every ordinary request.
        super::context::validate_configuration(&data)?;
        let _open = self.open()?;
        self.store.transaction(|tx| {
            let mut current = tx
                .get(model::OWNER, model::KIND, "pc")?
                .map(serde_json::from_value::<Snapshot>)
                .transpose()?
                .unwrap_or_default();
            current.validate()?;
            if current.generation != before.generation {
                return Err(Error::StaleResponse);
            }
            current.version = version;
            current.source_url = Some(url.as_str().into());
            current.data = Some(data);
            current.generation = current
                .generation
                .checked_add(1)
                .ok_or(Error::State("MConfig generation exhausted"))?;
            tx.put(
                model::OWNER,
                model::KIND,
                "pc",
                &serde_json::to_value(current)?,
            )
        })?;
        Ok(Update::Updated)
    }
}
async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() {
            return;
        }
        if cancel.changed().await.is_err() {
            return;
        }
    }
}
