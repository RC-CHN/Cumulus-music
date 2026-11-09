//! Bounded native scheduling. No renderer timer and no network on the actor.
use super::{Cache, Scope};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    client::together::RoomDetachment,
    playback::controller::{ChorusTrigger, ChorusTriggers, PlaybackController},
    protocol::api::Api,
};
use std::{
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle, time::Instant};

const STARTUP_DELAY: Duration = Duration::from_secs(1);

struct StartupHold {
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: String,
    due: Instant,
}

impl StartupHold {
    fn matches(&self, scope: &Scope) -> bool {
        self.token == scope.token
            && self.api.ptr_eq(&Arc::downgrade(&scope.api))
            && self.credentials == scope.credentials
    }
}

#[derive(Default)]
struct Schedule {
    controller: Weak<PlaybackController>,
    startup_consumed: bool,
    startup: Option<JoinHandle<()>>,
    next: Option<JoinHandle<()>>,
}

impl Drop for Schedule {
    fn drop(&mut self) {
        for task in self.startup.iter().chain(&self.next) {
            task.abort();
        }
    }
}

pub(in crate::client::player) struct Host {
    pub(super) cache: Arc<Cache>,
    schedule: Mutex<Schedule>,
    startup_hold: watch::Sender<Option<StartupHold>>,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            cache: Default::default(),
            schedule: Default::default(),
            startup_hold: watch::channel(None).0,
        }
    }
}

impl Host {
    pub(in crate::client::player) fn bind(
        &self,
        controller: &Arc<PlaybackController>,
    ) -> Result<()> {
        self.schedule
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .controller = Arc::downgrade(controller);
        Ok(())
    }

    pub(in crate::client::player) fn triggers(
        self: &Arc<Self>,
        session: Arc<AccountSession>,
        room: RoomDetachment,
    ) -> ChorusTriggers {
        let host = Arc::downgrade(self);
        Arc::new(move |token, event| {
            let Some(host) = host.upgrade() else { return };
            let Ok(api) = session.native_api(token) else {
                return;
            };
            let Some(scope) = Scope::capture(token, api, room.authority(token)) else {
                return;
            };
            let Ok(mut schedule) = host.schedule.lock() else {
                return;
            };
            if matches!(event, ChorusTrigger::Startup) && schedule.startup_consumed {
                return;
            }
            let startup = matches!(event, ChorusTrigger::Startup);
            let due = Instant::now()
                + if startup {
                    STARTUP_DELAY
                } else {
                    Duration::ZERO
                };
            if startup {
                schedule.startup_consumed = true;
                host.startup_hold.send_replace(Some(StartupHold {
                    token: scope.token.clone(),
                    api: Arc::downgrade(&scope.api),
                    credentials: scope.credentials.clone(),
                    due,
                }));
            } else {
                // An independently accepted insertion can read the new queue
                // before the startup timeout. The timeout itself still runs.
                host.startup_hold.send_if_modified(|hold| {
                    if hold.as_ref().is_some_and(|hold| hold.matches(&scope)) {
                        *hold = None;
                        true
                    } else {
                        false
                    }
                });
            }
            let task = tokio::spawn(run(
                schedule.controller.clone(),
                host.cache.clone(),
                session.clone(),
                scope,
                event,
                due,
            ));
            let slot = if startup {
                &mut schedule.startup
            } else {
                &mut schedule.next
            };
            if let Some(old) = slot.replace(task) {
                old.abort();
            }
        })
    }

    pub(super) async fn wait_for_startup(
        &self,
        session: &AccountSession,
        scope: &Scope,
    ) -> Result<bool> {
        let mut hold = self.startup_hold.subscribe();
        let due = hold
            .borrow_and_update()
            .as_ref()
            .filter(|hold| hold.matches(scope) && hold.due > Instant::now())
            .map(|hold| hold.due);
        let Some(due) = due else { return Ok(false) };
        let mut account = session.subscribe()?;
        tokio::select! {
            biased;
            _ = account.wait_for(|current| current != &scope.token) => return Err(Error::StaleResponse),
            _ = hold.changed() => {}
            _ = tokio::time::sleep_until(due) => {}
        }
        scope.check(session)?;
        Ok(true)
    }
}

async fn run(
    controller: Weak<PlaybackController>,
    cache: Arc<Cache>,
    session: Arc<AccountSession>,
    scope: Scope,
    event: ChorusTrigger,
    due: Instant,
) {
    let Ok(mut account) = session.subscribe() else {
        return;
    };
    tokio::select! {
        biased;
        _ = account.wait_for(|current| current != &scope.token) => {}
        _ = async {
            // InsertNext runs as soon as the actor yields, after queue commit.
            if matches!(event, ChorusTrigger::Startup) {
                tokio::time::sleep_until(due).await;
            }
            if scope.check(&session).is_err() {
                return;
            }
            let Some(controller) = controller.upgrade() else { return };
            let Ok((current, queue)) = controller.queue_with_state(&scope.token).await else { return };
            if let ChorusTrigger::InsertNext(revision) = event
                && current.queue_revision != revision
            {
                return;
            }
            let Some(plan) = scope.plan(&current, &queue) else { return };
            let _ = cache.read(&session, &controller, &scope, &current, plan, None).await;
        } => {}
    }
}
