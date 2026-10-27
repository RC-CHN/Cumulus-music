use super::*;
use std::sync::atomic::Ordering;
impl WatchmanRuntime {
    pub(super) async fn ensure_device(
        self: &Arc<Self>,
        config: ConfigSnapshot,
        force: bool,
    ) -> Result<()> {
        if !config.config().device_enabled() {
            return Ok(());
        }
        loop {
            let mut gate = self.flight.lock().await;
            let state = DeviceStore::new(self.store.clone(), config.config().clone()).snapshot()?;
            if !force && state.ready(config.config(), self.clock.now_ms()?) {
                return Ok(());
            }
            let (mut receiver, flight_build) = if let Some(flight) = gate.as_ref() {
                (flight.receiver.clone(), flight.build)
            } else {
                let id = self.next_flight.fetch_add(1, Ordering::Relaxed);
                let (sender, receiver) = watch::channel(None);
                *gate = Some(Flight {
                    id,
                    build: config.config().build(),
                    receiver: receiver.clone(),
                });
                let owned = self.clone();
                let snapshot = config.clone();
                tokio::spawn(async move {
                    let result = owned
                        .cancellable(owned.device_reports(&snapshot))
                        .await
                        .map_err(Arc::new);
                    let mut gate = owned.flight.lock().await;
                    if gate.as_ref().is_some_and(|f| f.id == id) {
                        *gate = None;
                    }
                    // Clear the slot before notifying waiters. An incompatible SDK
                    // waiter may immediately start its own initialization.
                    sender.send_replace(Some(result));
                });
                (receiver, config.config().build())
            };
            drop(gate);
            loop {
                if let Some(result) = receiver.borrow_and_update().clone() {
                    result.map_err(Error::Shared)?;
                    break;
                }
                receiver
                    .changed()
                    .await
                    .map_err(|_| Error::State("Watchman device worker ended without result"))?;
            }
            // Only a compatible completed flight can satisfy this snapshot. A
            // configuration update must not silently reuse an excluded old DIV.
            if flight_build == config.config().build()
                || DeviceStore::new(self.store.clone(), config.config().clone())
                    .snapshot()?
                    .ready(config.config(), self.clock.now_ms()?)
            {
                return Ok(());
            }
        }
    }
    async fn device_reports(&self, config: &ConfigSnapshot) -> Result<()> {
        let _gate = self.report_gate.lock().await;
        let mut attempt = DeviceAttempt::default();
        loop {
            let common = self.capture_common(config)?;
            let observations = self
                .collection
                .collect(CollectionMode::Device, config.config(), 0)
                .await?;
            let prepared = self.prepare_report(common, ReportKind::Device, &observations)?;
            let response = self.report(config, ReportKind::Device, &prepared).await?;
            let update = match response {
                Some(200) => Ok(ReportUpdate::Accepted),
                Some(420) => Ok(ReportUpdate::ClockCorrection),
                _ => Err(Error::Response("Watchman device report rejected")),
            };
            match attempt.response(update) {
                DeviceStep::Resubmit => {}
                DeviceStep::Complete => return Ok(()),
                _ => return Err(Error::Response("Watchman device initialization failed")),
            }
        }
    }
    /// Official five-minute device-refresh cadence. A weak reference and an
    /// task guard stop future refresh cycles. Call close() at host teardown to
    /// cancel shared initialization used by other callers as well.
    pub fn start_refresh(self: &Arc<Self>) -> RefreshTask {
        let runtime = Arc::downgrade(self);
        let (stop, mut stopped) = watch::channel(false);
        let worker = tokio::spawn(async move {
            loop {
                let Some(owned) = runtime.upgrade() else {
                    break;
                };
                let mut cancelled = owned.cancel.subscribe();
                tokio::select! {biased; _=super::cancelled(&mut stopped)=>break,_=super::cancelled(&mut cancelled)=>break,_=owned.refresh_device_if_due()=>{}}
                drop(owned);
                tokio::select! {biased; _=super::cancelled(&mut stopped)=>break,_=super::cancelled(&mut cancelled)=>break,_=tokio::time::sleep(Duration::from_secs(300))=>{}}
            }
        });
        RefreshTask {
            stop,
            worker: Some(worker),
        }
    }
}
pub struct RefreshTask {
    stop: watch::Sender<bool>,
    worker: Option<tokio::task::JoinHandle<()>>,
}
impl RefreshTask {
    pub async fn stop(mut self) -> Result<()> {
        self.stop.send_replace(true);
        if let Some(worker) = self.worker.take() {
            worker
                .await
                .map_err(|_| Error::State("Watchman refresh task failed"))?;
        }
        Ok(())
    }
}
impl Drop for RefreshTask {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}
