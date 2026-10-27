//! Native coordinator. Collection is an explicit host dependency; there is no
//! default empty/fabricated environment and no Python/JS business subprocess.
mod report;
mod worker;
use super::*;
use crate::{
    Error, Result,
    protocol::{api::ApiFuture, context::TokenProvider},
    storage::StateStore,
};
use serde_json::Value;
use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, watch};
pub use worker::RefreshTask;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionMode {
    Device,
    Behavior,
}
#[derive(Clone)]
pub struct Observation {
    pub key: String,
    pub value: Value,
    pub failed: bool,
}
#[derive(Clone, Default)]
pub struct Observations {
    pub device: Vec<Observation>,
    pub environment: Vec<Observation>,
    pub events: Vec<Observation>,
}
pub trait Collection: Send + Sync {
    /// Device facts, dynamic environment and drained event groups remain
    /// distinct. The host must return real measurements or explicit failure.
    fn collect<'a>(
        &'a self,
        mode: CollectionMode,
        config: &'a Config,
        elapsed_ms: u64,
    ) -> ApiFuture<'a, Observations>;
    fn collect_at<'a>(
        &'a self,
        mode: CollectionMode,
        config: &'a Config,
        elapsed_ms: u64,
        _reference_ms: u64,
    ) -> ApiFuture<'a, Observations> {
        self.collect(mode, config, elapsed_ms)
    }
    fn record_timing(&self, _slot: TimingSlot, _elapsed_ms: u64) -> Result<()> {
        Ok(())
    }
    /// Native collection already preserves the official cached device order
    /// and environment traversal order; generic injected collectors may not.
    fn preserves_field_order(&self) -> bool {
        false
    }
}
struct PublicState {
    sequence: u8,
    business: Option<String>,
    nonce: Option<String>,
    track: Option<String>,
    last_use_ms: u64,
}
type DeviceResult = std::result::Result<(), Arc<Error>>;
struct Flight {
    id: u64,
    build: SdkBuild,
    receiver: watch::Receiver<Option<DeviceResult>>,
}
pub struct WatchmanRuntime {
    store: Arc<StateStore>,
    configs: Arc<ConfigService>,
    transport: Arc<dyn Transport>,
    clock: Arc<dyn EntropyClock>,
    collection: Arc<dyn Collection>,
    public: Mutex<PublicState>,
    closed: Mutex<bool>,
    cancel: watch::Sender<bool>,
    flight: AsyncMutex<Option<Flight>>,
    next_flight: std::sync::atomic::AtomicU64,
    report_gate: AsyncMutex<()>,
}
impl WatchmanRuntime {
    pub fn new(
        store: Arc<StateStore>,
        transport: Arc<dyn Transport>,
        clock: Arc<dyn EntropyClock>,
        collection: Arc<dyn Collection>,
        product: &str,
    ) -> Result<Arc<Self>> {
        let configs = Arc::new(ConfigService::new(
            store.clone(),
            transport.clone(),
            clock.clone(),
            product,
        )?);
        let (cancel, _) = watch::channel(false);
        let now = clock.now_ms()?;
        Ok(Arc::new(Self {
            store,
            configs,
            transport,
            clock,
            collection,
            public: Mutex::new(PublicState {
                sequence: 0,
                business: None,
                nonce: None,
                track: None,
                last_use_ms: now,
            }),
            closed: Mutex::new(false),
            cancel,
            flight: AsyncMutex::new(None),
            next_flight: std::sync::atomic::AtomicU64::new(1),
            report_gate: AsyncMutex::new(()),
        }))
    }
    fn public(&self) -> Result<MutexGuard<'_, PublicState>> {
        self.public.lock().map_err(|_| Error::LockPoisoned)
    }
    fn open(&self) -> Result<MutexGuard<'_, bool>> {
        let guard = self.closed.lock().map_err(|_| Error::LockPoisoned)?;
        if *guard {
            return Err(Error::Cancelled);
        }
        Ok(guard)
    }
    pub fn close(&self) -> Result<()> {
        let mut closed = self.closed.lock().map_err(|_| Error::LockPoisoned)?;
        *closed = true;
        self.cancel.send_replace(true);
        Ok(())
    }
    pub fn set_track_native(&self, track: Option<String>) -> Result<()> {
        let _open = self.open()?;
        if track
            .as_ref()
            .is_some_and(|s| s.len() > 4096 || s.contains(['\r', '\n', '\0']))
        {
            return Err(Error::InvalidInput("invalid Watchman track ID"));
        }
        self.public()?.track = track;
        Ok(())
    }
    async fn cancellable<T>(
        &self,
        future: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        drop(self.open()?);
        let mut cancel = self.cancel.subscribe();
        tokio::select! {biased; _=cancelled(&mut cancel)=>Err(Error::Cancelled),value=future=>value}
    }
    pub async fn get_token(
        self: &Arc<Self>,
        business: &str,
        timeout_ms: Option<u64>,
    ) -> Result<TokenResult> {
        self.cancellable(self.token_inner(business, timeout_ms))
            .await
    }
    async fn token_inner(
        self: &Arc<Self>,
        business: &str,
        timeout_ms: Option<u64>,
    ) -> Result<TokenResult> {
        if business.is_empty() || business.len() > 128 || business.contains(['\r', '\n', '\0']) {
            return Err(Error::InvalidInput("invalid Watchman business"));
        }
        let snapshot = self.configs.load(false).await?;
        self.collection.record_timing(
            TimingSlot::Configuration,
            snapshot.config().decode_elapsed_ms(),
        )?;
        if snapshot.config().business_suppressed(business) {
            return Err(Error::State(
                "Watchman configuration suppresses this business",
            ));
        }
        self.ensure_device(snapshot.clone(), false).await?;
        let _gate = self.report_gate.lock().await;
        // A delayed continuation can resume after another build has replaced
        // DIV. Recheck under the report gate before changing public fields or
        // consuming events; never send that record in an incompatible packet.
        if !DeviceStore::new(self.store.clone(), snapshot.config().clone())
            .snapshot()?
            .version_compatible(snapshot.config())
        {
            return Err(Error::StaleResponse);
        }
        let now = self.clock.now_ms()?;
        let mut random = [0; 8];
        self.clock.fill(&mut random)?;
        let nonce = request_nonce(now, random);
        let elapsed = {
            let mut p = self.public()?;
            let elapsed = now.saturating_sub(p.last_use_ms);
            p.last_use_ms = now;
            p.business = Some(business.into());
            p.nonce = Some(nonce.clone());
            elapsed
        };
        let common = self.capture_common(&snapshot)?;
        let observed = self
            .collection
            .collect_at(CollectionMode::Behavior, snapshot.config(), elapsed, now)
            .await?;
        if !observed.device.is_empty() {
            return Err(Error::InvalidInput(
                "device facts supplied for behavior report",
            ));
        }
        let mut prepared = self.prepare_report(common, ReportKind::Behavior, &observed)?;
        let mut attempt = BehaviorAttempt::new(
            snapshot.config().clone(),
            business,
            nonce,
            self.clock.now_ms()?,
            timeout_ms,
        )?;
        let deadline = tokio::time::Instant::now()
            + Duration::from_millis(timeout_ms.unwrap_or(snapshot.config().token_timeout_ms()));
        loop {
            let state = DeviceStore::new(self.store.clone(), snapshot.config().clone());
            let step = match tokio::time::timeout_at(
                deadline,
                self.report(&snapshot, ReportKind::Behavior, &prepared),
            )
            .await
            {
                Ok(reply) => attempt.response(reply?, state.snapshot()?.did_native())?,
                Err(_) => attempt.timeout(attempt.deadline_ms(), state.snapshot()?.did_native())?,
            };
            match step {
                BehaviorStep::Token(result) => return Ok(result),
                BehaviorStep::IllegalBusiness => {
                    return Err(Error::State("Watchman business key is illegal"));
                }
                BehaviorStep::Resubmit => {
                    prepared = self.prepare_report(
                        self.capture_common(&snapshot)?,
                        ReportKind::Behavior,
                        &observed,
                    )?;
                }
                BehaviorStep::Ignored => {
                    return Err(Error::State("Watchman attempt ended without result"));
                }
            }
        }
    }
    pub async fn nd_info(self: &Arc<Self>) -> Result<String> {
        self.cancellable(async {
            let config = self.configs.load(false).await?;
            self.collection.record_timing(
                TimingSlot::Configuration,
                config.config().decode_elapsed_ms(),
            )?;
            self.ensure_device(config.clone(), false).await?;
            DeviceStore::new(self.store.clone(), config.config().clone())
                .snapshot()?
                .nd_info(config.config())
        })
        .await
    }
    pub async fn refresh_device_if_due(self: &Arc<Self>) -> Result<()> {
        self.cancellable(async {
            let config = self.configs.load(false).await?;
            self.collection.record_timing(
                TimingSlot::Configuration,
                config.config().decode_elapsed_ms(),
            )?;
            let state = DeviceStore::new(self.store.clone(), config.config().clone()).snapshot()?;
            let now = self.clock.now_ms()?;
            if !state.ready(config.config(), now) || state.refresh_due(config.config(), now) {
                self.ensure_device(config, true).await?;
            }
            Ok(())
        })
        .await
    }
}
/// Native RequestContext integration; the caller must still supply reviewed PC
/// MConfig and a real Collection. SDK fallback token shapes remain distinguishable
/// through get_token(), and never imply that a music endpoint accepted them.
impl TokenProvider for Arc<WatchmanRuntime> {
    fn token<'a>(&'a self, business: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async move {
            Ok(self
                .get_token(business, None)
                .await?
                .encoded_native()
                .into())
        })
    }
}
async fn cancelled(receiver: &mut watch::Receiver<bool>) {
    loop {
        if *receiver.borrow_and_update() {
            return;
        }
        if receiver.changed().await.is_err() {
            return;
        }
    }
}
