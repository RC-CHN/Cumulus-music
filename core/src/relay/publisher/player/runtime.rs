use super::super::{PublishOutcome, RelayPublisher};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, watch};

#[derive(Clone, Debug)]
pub struct RuntimeStatus {
    pub account: SessionToken,
    pub sending: bool,
    pub record_failed: bool,
    pub preflight_failed: bool,
}
pub struct Runtime {
    pub(super) publisher: Arc<RelayPublisher>,
    health: Arc<Mutex<RuntimeStatus>>,
    notify: Arc<Notify>,
    paused: watch::Sender<bool>,
    task: tokio::task::AbortHandle,
}
impl Runtime {
    pub fn start(session: Arc<AccountSession>) -> Result<Arc<Self>> {
        let publisher = Arc::new(RelayPublisher::new(session.clone())?);
        let health = Arc::new(Mutex::new(RuntimeStatus {
            account: session.token()?,
            sending: false,
            record_failed: false,
            preflight_failed: false,
        }));
        let notify = Arc::new(Notify::new());
        let (paused, receiver) = watch::channel(false);
        let accounts = session.subscribe()?;
        let task = tokio::spawn(worker(
            session,
            publisher.clone(),
            health.clone(),
            notify.clone(),
            receiver,
            accounts,
        ));
        Ok(Arc::new(Self {
            publisher,
            health,
            notify,
            paused,
            task: task.abort_handle(),
        }))
    }
    pub fn status(&self) -> Result<RuntimeStatus> {
        self.health
            .lock()
            .map(|h| h.clone())
            .map_err(|_| Error::LockPoisoned)
    }
    pub fn wake(&self) {
        self.notify.notify_one();
    }
    pub(super) fn recording(&self, token: &SessionToken, failed: bool) -> Result<()> {
        let mut health = self.health.lock().map_err(|_| Error::LockPoisoned)?;
        if health.account != *token {
            *health = RuntimeStatus {
                account: token.clone(),
                sending: false,
                record_failed: false,
                preflight_failed: false,
            };
        }
        health.record_failed = failed;
        self.paused.send_if_modified(|value| {
            if *value == failed {
                false
            } else {
                *value = failed;
                true
            }
        });
        Ok(())
    }
    pub(super) fn stop(&self) {
        self.task.abort();
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop();
    }
}
fn update(health: &Mutex<RuntimeStatus>, token: &SessionToken, sending: bool, failed: bool) {
    if let Ok(mut health) = health.lock()
        && health.account == *token
    {
        health.sending = sending;
        health.preflight_failed = failed;
    }
}
async fn worker(
    session: Arc<AccountSession>,
    publisher: Arc<RelayPublisher>,
    health: Arc<Mutex<RuntimeStatus>>,
    notify: Arc<Notify>,
    mut paused: watch::Receiver<bool>,
    mut accounts: watch::Receiver<SessionToken>,
) {
    'wait: loop {
        tokio::select! {
            _=notify.notified()=>(),
            value=accounts.changed()=>{if value.is_err(){return;}continue;},
            value=paused.changed()=>{if value.is_err(){return;}continue;},
        }
        if *paused.borrow_and_update() {
            continue;
        }
        let Ok(token) = session.token() else {
            continue;
        };
        if *accounts.borrow_and_update() != token {
            continue;
        }
        if token.uid.is_none() {
            continue;
        }
        let mut delay = Duration::from_millis(250);
        loop {
            if *paused.borrow() {
                continue 'wait;
            }
            update(&health, &token, true, false);
            let result = tokio::select! {
                result=publisher.publish_next_for(&token)=>Some(result),
                _=accounts.changed()=>None,
                _=paused.changed()=>None,
            };
            update(&health, &token, false, false);
            let Some(result) = result else {
                continue 'wait;
            };
            match result {
                Ok(PublishOutcome::Confirmed(_) | PublishOutcome::Recovered { .. }) => {
                    delay = Duration::from_millis(250)
                }
                Ok(PublishOutcome::Idle | PublishOutcome::Blocked(_) | PublishOutcome::Skipped) => {
                    continue 'wait;
                }
                Err(Error::StaleResponse | Error::Cancelled) => continue 'wait,
                Err(error) => {
                    // Dispatch errors are durable Blocked outcomes. Only local
                    // lock/preflight failures reach this retry path; Sending is
                    // recovered as unknown before any subsequent HTTP attempt.
                    update(
                        &health,
                        &token,
                        false,
                        !matches!(error, Error::MutationBusy),
                    );
                    tokio::select! {
                        _=tokio::time::sleep(delay)=>(),
                        _=accounts.changed()=>continue 'wait,
                        _=paused.changed()=>continue 'wait,
                    }
                    delay = (delay * 2).min(Duration::from_secs(60));
                }
            }
        }
    }
}
