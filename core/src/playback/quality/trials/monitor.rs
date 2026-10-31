//! Background read-only expiry reconciliation survives a closed menu and an
//! idle/paused player. It never issues auth/set or resumes/reloads audio.
use super::{QualityTrialPhase, QualityTrials};
use crate::{account::AccountSession, playback::quality_preferences::QualityPreferences};
use std::{sync::Arc, time::Duration};

pub(crate) fn spawn(session: Arc<AccountSession>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let Ok(mut changes) = session.subscribe() else {
            return;
        };
        loop {
            changes.borrow_and_update();
            let delay = tokio::select! {
                changed = changes.changed() => { if changed.is_err() { return; } continue; },
                result = check(&session) => result.unwrap_or(Duration::from_secs(15)),
            };
            tokio::select! {
                changed = changes.changed() => { if changed.is_err() { return; } },
                _ = tokio::time::sleep(delay) => (),
            }
        }
    })
}
async fn check(session: &Arc<AccountSession>) -> crate::Result<Duration> {
    let token = session.token()?;
    let store = session.persistent_store()?;
    let preference = session.with_token(&token, |owner| QualityPreferences::read(&store, owner))?;
    let Some(binding) = &preference.trial else {
        return Ok(Duration::from_secs(1));
    };
    let trials = QualityTrials::new(session.clone())?;
    let record = trials.record_for(&token, binding.right)?;
    if record.is_none_or(|r| r.operation != binding.operation) {
        // Missing/corrupt records are not proof of an expiry.
        return Ok(Duration::from_secs(15));
    }
    let record = trials.observe_for(&token, binding.right).await?;
    if record.phase == QualityTrialPhase::Expired
        && !record.read_failed
        && record.observed.as_ref().is_some_and(|r| !r.can_use)
    {
        session.with_token(&token, |owner| {
            QualityPreferences::restore_trial(&store, owner, &preference)
        })?;
        return Ok(Duration::from_secs(1));
    }
    let milliseconds = record
        .observed
        .filter(|_| !record.read_failed)
        .and_then(|r| r.remaining_ms)
        .unwrap_or(15000)
        .clamp(1000, 15000);
    Ok(Duration::from_millis(milliseconds))
}
