//! Account-owned M/198 membership reads. This is independent of token renewal.
use super::{MembershipCommit, MembershipManager, manager::expiries};
use crate::{
    Result,
    account::{AccountSession, SessionStatus, SessionToken},
};
use std::{future::pending, sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinHandle, time::Instant};

const TIMEOUT: Duration = Duration::from_secs(20);
const OFFICIAL_TIMER_MAX_MS: f64 = 2_147_483_647.0;

/// Dropping the native owner cancels the read or deadline; no renderer command
/// or focus handler can start a new cycle.
pub struct MembershipLifecycle {
    task: JoinHandle<()>,
}

impl Drop for MembershipLifecycle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MembershipLifecycle {
    pub(crate) fn start(
        session: Arc<AccountSession>,
        manager: Arc<MembershipManager>,
    ) -> Result<Self> {
        let accounts = session.subscribe()?;
        let commits = session.subscribe_membership_commits()?;
        let task = tokio::spawn(run(session, manager, accounts, commits));
        Ok(Self { task })
    }
}

struct Schedule {
    expiries: [f64; 5],
    deadline: Option<Instant>,
}

impl Schedule {
    fn observe(&mut self, commit: &MembershipCommit) {
        if self.expiries == commit.expiries {
            return;
        }
        self.expiries = commit.expiries;
        self.deadline = commit
            .expiries
            .iter()
            .map(|date| *date - commit.server_now)
            .filter(|delay| *delay > 0.0)
            .reduce(f64::min)
            // M/198 Ie caps the JS timeout and still reads when that cap fires.
            .map(|delay| {
                commit.at
                    + Duration::from_millis((delay + 1_000.0).min(OFFICIAL_TIMER_MAX_MS) as u64)
            });
    }
}

fn initial_expiries(
    session: &AccountSession,
    manager: &MembershipManager,
    current: &SessionToken,
) -> [f64; 5] {
    let provisional = SessionToken {
        generation: 0,
        uid: None,
    };
    if session
        .is_initial_restore(&provisional, current)
        .unwrap_or(false)
        && let Ok(snapshot) = manager.snapshot()
        && current.uid.as_deref() == Some(snapshot.state.user_id.as_str())
    {
        // M/198 Ke restores four expiry fields from the same user's cache;
        // voiceBookVipExpireTime starts at zero until the fresh read.
        let mut cached = expiries(&snapshot.state);
        cached[4] = 0.0;
        return cached;
    }
    // Interactive selection starts with no previous account's deadlines.
    [0.0; 5]
}

async fn refresh(
    manager: &MembershipManager,
    token: &SessionToken,
    accounts: &mut watch::Receiver<SessionToken>,
) -> bool {
    tokio::select! {
        biased;
        _ = accounts.changed() => false,
        _ = tokio::time::timeout(TIMEOUT, manager.refresh_for(token.clone())) => true,
    }
}

async fn wait_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => pending().await,
    }
}

async fn run(
    session: Arc<AccountSession>,
    manager: Arc<MembershipManager>,
    mut accounts: watch::Receiver<SessionToken>,
    mut commits: watch::Receiver<Option<MembershipCommit>>,
) {
    'activation: loop {
        let token = accounts.borrow_and_update().clone();
        if !session
            .snapshot()
            .is_ok_and(|state| state.status == SessionStatus::Authenticated && state.token == token)
        {
            if accounts.changed().await.is_err() {
                return;
            }
            continue;
        }
        let mut schedule = Schedule {
            expiries: initial_expiries(&session, &manager, &token),
            deadline: None,
        };
        // An already verified read by the shell/member UI in this activation
        // can satisfy startup, but persisted cache alone never does.
        let read_in_activation = commits
            .borrow_and_update()
            .as_ref()
            .is_some_and(|commit| commit.token == token);
        if !read_in_activation && !refresh(&manager, &token, &mut accounts).await {
            continue;
        }
        loop {
            if let Some(commit) = commits
                .borrow_and_update()
                .as_ref()
                .filter(|commit| commit.token == token)
            {
                schedule.observe(commit);
            }
            tokio::select! {
                biased;
                changed = accounts.changed() => {
                    if changed.is_err() { return; }
                    continue 'activation;
                }
                changed = commits.changed() => {
                    if changed.is_err() { return; }
                }
                () = wait_deadline(schedule.deadline) => {
                    schedule.deadline = None;
                    if !refresh(&manager, &token, &mut accounts).await {
                        continue 'activation;
                    }
                }
            }
        }
    }
}
