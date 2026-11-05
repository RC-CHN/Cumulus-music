//! M147 native lifetime: initial/user/credential changes and a fixed 30m timer.
use super::{MusicClient, REFRESH, ScopeContext};
use crate::protocol::pc_experiments::PcAnonymousState;
use crate::{Error, Result};
use std::{
    future::pending,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{Instant, MissedTickBehavior},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcPresentationRuntimeStatus {
    WaitingForAccount,
    AnonymousPrerequisite,
    AnonymousUncertain,
    UnsupportedTransport,
    Refreshing,
    Ready,
    RefreshFailed,
    CacheWriteFailed,
    CredentialChurn,
    Stopped,
}

struct Active(Arc<AtomicBool>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// One native host owns this lease. No frontend command creates a scheduler.
pub struct PcPresentationRuntimeLease {
    stop: watch::Sender<bool>,
    status: watch::Receiver<PcPresentationRuntimeStatus>,
    actor: Option<JoinHandle<()>>,
}

impl PcPresentationRuntimeLease {
    pub fn subscribe_status(&self) -> watch::Receiver<PcPresentationRuntimeStatus> {
        self.status.clone()
    }
    pub async fn shutdown(mut self) -> Result<()> {
        self.stop.send_replace(true);
        if let Some(actor) = self.actor.take() {
            actor
                .await
                .map_err(|_| Error::State("PC presentation runtime stopped unexpectedly"))?;
        }
        Ok(())
    }
}
impl Drop for PcPresentationRuntimeLease {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}

impl MusicClient {
    pub fn start_pc_presentation_runtime(&self) -> Result<PcPresentationRuntimeLease> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| Error::State("PC presentation requires an async runtime"))?;
        let active = self.pc_experiments.presentation.running.clone();
        if active.swap(true, Ordering::AcqRel) {
            return Err(Error::MutationBusy);
        }
        let active = Active(active);
        self.pc_presentation()?;
        let scope = self.pc_presentation_context();
        let accounts = self.session.subscribe()?;
        let (stop, cancelled) = watch::channel(false);
        let (publish, status) = watch::channel(PcPresentationRuntimeStatus::WaitingForAccount);
        let actor = runtime.spawn(async move {
            let _active = active;
            run(scope, accounts, cancelled, &publish).await;
            publish.send_replace(PcPresentationRuntimeStatus::Stopped);
        });
        Ok(PcPresentationRuntimeLease {
            stop,
            status,
            actor: Some(actor),
        })
    }
}

async fn credentials_changed(receiver: &mut Option<watch::Receiver<String>>) -> bool {
    match receiver {
        Some(receiver) => receiver.changed().await.is_ok(),
        None => pending().await,
    }
}

async fn run(
    scope: ScopeContext,
    mut accounts: watch::Receiver<crate::account::SessionToken>,
    mut cancelled: watch::Receiver<bool>,
    publish: &watch::Sender<PcPresentationRuntimeStatus>,
) {
    let mut timer = tokio::time::interval_at(Instant::now() + REFRESH, REFRESH);
    timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut current = accounts.borrow_and_update().clone();
    let mut credentials = None;
    let mut read_due = true;
    let mut credential_restarts = 0u8;
    loop {
        if *cancelled.borrow() {
            return;
        }
        if read_due {
            read_due = false;
            let api = match scope.session.native_api(&current) {
                Ok(api) => api,
                Err(Error::StaleResponse) => {
                    // The generation can advance after the watch value was
                    // sampled. Rebind instead of permanently stopping a live
                    // lease during ordinary account restoration or switching.
                    current = accounts.borrow_and_update().clone();
                    credentials = None;
                    credential_restarts = 0;
                    read_due = true;
                    continue;
                }
                Err(_) => return,
            };
            let Ok(session) = scope.session.snapshot() else {
                return;
            };
            let guest_ready = scope
                .session
                .guest_ready_for(&current, &api)
                .unwrap_or(false);
            if guest_ready && current.uid.is_none() {
                match scope.isolate_guest(&current, &api) {
                    Ok(Some(_)) => {
                        current = accounts.borrow_and_update().clone();
                        credentials = None;
                        read_due = true;
                        continue;
                    }
                    Ok(None) => (),
                    Err(_) => {
                        publish.send_replace(PcPresentationRuntimeStatus::AnonymousPrerequisite);
                        credentials = api.pc_presentation_credential_changes().ok().flatten();
                        if let Some(receiver) = credentials.as_mut() {
                            receiver.borrow_and_update();
                        }
                        let _ = scope.snapshot(&current, &api);
                        continue_wait(&mut cancelled, &mut accounts, &mut credentials, &mut timer)
                            .await;
                        if *cancelled.borrow() {
                            return;
                        }
                        current = accounts.borrow_and_update().clone();
                        read_due = true;
                        continue;
                    }
                }
            }
            credentials = api.pc_presentation_credential_changes().ok().flatten();
            if let Some(receiver) = credentials.as_mut() {
                receiver.borrow_and_update();
            }
            let _ = scope.snapshot(&current, &api);
            let status = if current.uid.is_none() && !guest_ready {
                Some(PcPresentationRuntimeStatus::WaitingForAccount)
            } else if api
                .pc_presentation_credential_revision()
                .ok()
                .flatten()
                .is_none()
            {
                Some(PcPresentationRuntimeStatus::UnsupportedTransport)
            } else if current.uid.is_none()
                && matches!(api.pc_anonymous_state(), Ok(PcAnonymousState::Uncertain))
            {
                Some(PcPresentationRuntimeStatus::AnonymousUncertain)
            } else {
                None
            };
            if let Some(status) = status {
                publish.send_replace(status);
            } else {
                publish.send_replace(PcPresentationRuntimeStatus::Refreshing);
                let allowed = || Ok(true);
                let anonymous = current.uid.is_none();
                let before = api.pc_presentation_credential_revision().ok().flatten();
                if anonymous {
                    credentials = None;
                }
                let read_scope = scope.clone();
                let read_token = current.clone();
                let read_api = api.clone();
                let refresh = async move {
                    if anonymous {
                        read_scope
                            .live_anonymous_groups_for(&read_token, &read_api, true)
                            .await
                    } else {
                        read_scope
                            .live_pc_experiment_groups_for(&read_token, &read_api, true, &allowed)
                            .await
                    }
                };
                tokio::select! {
                    biased;
                    _ = cancelled.changed() => return,
                    changed = accounts.changed() => {
                        if changed.is_err() { return; }
                        current = accounts.borrow_and_update().clone();
                        credential_restarts = 0;
                        read_due = true;
                        continue;
                    },
                    changed = credentials_changed(&mut credentials) => {
                        if !changed { credentials = None; }
                        let _ = scope.snapshot(&current, &api);
                        if changed && credential_restarts == 0 {
                            credential_restarts = 1;
                            read_due = true;
                        } else {
                            credential_restarts = 0;
                            publish.send_replace(PcPresentationRuntimeStatus::CredentialChurn);
                        }
                        continue;
                    },
                    result = refresh => {
                        if anonymous {
                            credentials = api.pc_presentation_credential_changes().ok().flatten();
                            if let Some(receiver) = credentials.as_mut() { receiver.borrow_and_update(); }
                            if matches!(&result, Err(Error::StaleResponse | Error::CredentialChanged))
                                && api.pc_presentation_credential_revision().ok().flatten() != before
                                && credential_restarts == 0
                            {
                                credential_restarts = 1;
                                read_due = true;
                                continue;
                            }
                        }
                        publish.send_replace(match result {
                            Ok(_) if scope.host.presentation.cache_write_failed.load(Ordering::Acquire) => PcPresentationRuntimeStatus::CacheWriteFailed,
                            Ok(_) => PcPresentationRuntimeStatus::Ready,
                            Err(_) if anonymous && matches!(api.pc_anonymous_state(), Ok(PcAnonymousState::Uncertain)) => PcPresentationRuntimeStatus::AnonymousUncertain,
                            Err(_) => PcPresentationRuntimeStatus::RefreshFailed,
                        });
                    },
                }
            }
        }
        tokio::select! {
            biased;
            _ = cancelled.changed() => return,
            changed = accounts.changed() => {
                if changed.is_err() { return; }
                current = accounts.borrow_and_update().clone();
                credentials = None;
                credential_restarts = 0;
                read_due = true;
            },
            changed = credentials_changed(&mut credentials) => {
                if changed { read_due = true; } else { credentials = None; }
                credential_restarts = 0;
            },
            _ = timer.tick() => { read_due = true; credential_restarts = 0; },
        }
    }
}

async fn continue_wait(
    cancelled: &mut watch::Receiver<bool>,
    accounts: &mut watch::Receiver<crate::account::SessionToken>,
    credentials: &mut Option<watch::Receiver<String>>,
    timer: &mut tokio::time::Interval,
) {
    tokio::select! {
        biased;
        _ = cancelled.changed() => (),
        _ = accounts.changed() => (),
        changed = credentials_changed(credentials) => { if !changed { *credentials = None; } },
        _ = timer.tick() => (),
    }
}
