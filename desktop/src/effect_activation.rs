//! Native-only state for a single explicit standalone PEQ selection.
use cumulus_core::{
    Error, Result,
    client::{ClientError, MusicClient, NativeParametricActivationAttempt},
    effects::{
        authorization::{
            LeasePhase, NativeEffectReleaseReceipt, PendingNativeEffectRegistration, ReleaseRetry,
        },
        catalog::EffectId,
        material::PeqCall,
        parametric::{ParametricHost, ParametricPlan},
        service::StandaloneParametricPreflight,
    },
    playback::controller::{ParametricApplication, ParametricBindingStatus},
};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub struct Host {
    library: Option<PathBuf>,
    state: tokio::sync::Mutex<State>,
}

impl Host {
    pub fn new(resources: &Path) -> Self {
        Self {
            library: super::effects::library(resources),
            state: Default::default(),
        }
    }

    fn check_adapter(&self) -> Result<()> {
        let path = self
            .library
            .as_deref()
            .ok_or(Error::State("PEQ adapter unavailable"))?;
        let plan = ParametricPlan::from_calls(&[PeqCall::On { value: true }])?;
        // Check the same trusted adapter the audio factory uses, without
        // attaching a node or changing playback.
        drop(ParametricHost::open(path, plan)?);
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Idle,
    Preparing,
    Prepared,
    Unsupported,
    Configured,
    Deferred,
    RecoveryRequired,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    id: EffectId,
    phase: Phase,
    attempt: Option<String>,
    registration: Option<LeasePhase>,
    release: Option<String>,
    can_stop: bool,
    can_resume: bool,
    stop_needs_retry: bool,
    active_id: Option<EffectId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issue: Option<&'static str>,
}

enum Work {
    Preparing,
    Unsupported,
    Prepared {
        ticket: String,
        attempt: NativeParametricActivationAttempt,
    },
    Submitted {
        attempt: NativeParametricActivationAttempt,
    },
}

struct StopTicket {
    ticket: String,
    receipt: NativeEffectReleaseReceipt,
}

#[derive(Default)]
struct State {
    epoch: Option<String>,
    work: Option<(EffectId, Work)>,
    release: Option<StopTicket>,
    issue: Option<(EffectId, &'static str)>,
    next: u64,
}

impl State {
    fn select_epoch(&mut self, epoch: &str) {
        if self.epoch.as_deref() != Some(epoch) {
            self.epoch = Some(epoch.to_owned());
            self.work = None;
            self.release = None;
            self.issue = None;
        }
    }

    fn ticket(&mut self) -> Result<String> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or(Error::State("PEQ ticket exhausted"))?;
        Ok(self.next.to_string())
    }

    fn discard_unstarted_submission(&mut self, client: &MusicClient, epoch: &str) -> Result<()> {
        if let Some((id, Work::Submitted { attempt, .. })) = &self.work
            && attempt.submitted_target().is_none()
            && !client
                .native_effect_registration_phase(epoch, *id)?
                .is_some_and(LeasePhase::needs_release)
        {
            // The IPC future may have been dropped before any write or actor
            // dispatch. A spent in-memory attempt alone is not a cleanup duty.
            self.work = None;
        }
        Ok(())
    }

    async fn view(
        &mut self,
        client: &MusicClient,
        epoch: &str,
        id: EffectId,
        refresh_ticket: bool,
    ) -> Result<View> {
        self.discard_unstarted_submission(client, epoch)?;
        let registration = client.native_effect_registration_phase(epoch, id)?;
        let can_release = registration.is_some_and(LeasePhase::needs_release);
        let stale_ticket = self.release.as_ref().is_some_and(|stop| {
            stop.receipt.effect() != id || Some(stop.receipt.phase()) != registration
        });
        if refresh_ticket || stale_ticket || !can_release {
            self.release = None;
        }
        if can_release
            && self.release.is_none()
            && let Some(receipt) = client.native_effect_release_receipt(epoch, id)?
        {
            self.release = Some(StopTicket {
                ticket: self.ticket()?,
                receipt,
            });
        }
        let release = self.release.as_ref().map(|stop| stop.ticket.clone());
        let mut phase = if can_release {
            Phase::RecoveryRequired
        } else {
            Phase::Idle
        };
        let mut attempt_ticket = None;
        let mut can_resume = can_release && registration == Some(LeasePhase::Registered);
        if let Some((_, work)) = self.work.as_ref().filter(|(owner, _)| *owner == id) {
            match work {
                Work::Preparing if !can_release => phase = Phase::Preparing,
                Work::Unsupported if !can_release => phase = Phase::Unsupported,
                Work::Prepared { ticket, .. } if !can_release => {
                    phase = Phase::Prepared;
                    attempt_ticket = Some(ticket.clone());
                }
                Work::Submitted { attempt, .. } if registration == Some(LeasePhase::Registered) => {
                    if let Some(target) = attempt.submitted_target() {
                        let status = client
                            .inspect_native_bound_parametric(epoch, target)
                            .await?;
                        can_resume = status == ParametricBindingStatus::Absent;
                        phase = match status {
                            ParametricBindingStatus::Configured => Phase::Configured,
                            ParametricBindingStatus::Deferred => Phase::Deferred,
                            ParametricBindingStatus::Absent
                            | ParametricBindingStatus::DetachRequired => Phase::RecoveryRequired,
                        };
                    }
                }
                _ => {}
            }
        }
        let active_id = match self.work.as_ref() {
            Some((id, Work::Submitted { .. })) => Some(*id),
            _ => client
                .native_effect_pending(epoch)?
                .first()
                .map(|entry| entry.id),
        };
        client.validate_player_epoch(epoch)?;
        Ok(View {
            id,
            phase,
            attempt: attempt_ticket,
            registration,
            can_stop: release.is_some(),
            can_resume: can_resume && release.is_some() && active_id == Some(id),
            release,
            stop_needs_retry: stop_needs_retry(registration),
            active_id,
            issue: self
                .issue
                .filter(|(owner, _)| *owner == id)
                .map(|(_, issue)| issue),
        })
    }

    fn note(&mut self, id: EffectId, error: Error) {
        self.issue = Some((id, ClientError::from(error).message));
    }
}

fn stop_needs_retry(phase: Option<LeasePhase>) -> bool {
    matches!(
        phase,
        Some(LeasePhase::Releasing | LeasePhase::StopUncertain)
    )
}

fn authorize<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    client: &MusicClient,
    epoch: &str,
) -> Result<()> {
    if window.label() != "main" {
        return Err(Error::InvalidInput(
            "PEQ activation is restricted to the main window",
        ));
    }
    client.validate_player_epoch(epoch)
}

#[tauri::command]
pub async fn effect_peq_pending<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
) -> std::result::Result<Vec<PendingNativeEffectRegistration>, ClientError> {
    authorize(&window, &client, &epoch)?;
    client.native_effect_pending(&epoch).map_err(Into::into)
}

#[tauri::command]
pub async fn effect_peq_status<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    host: tauri::State<'_, Host>,
    epoch: String,
    id: EffectId,
) -> std::result::Result<View, ClientError> {
    authorize(&window, &client, &epoch)?;
    let mut state = host.state.try_lock().map_err(|_| Error::MutationBusy)?;
    state.select_epoch(&epoch);
    state
        .view(&client, &epoch, id, false)
        .await
        .map_err(Into::into)
}

/// Reissue a cleanup receipt from local durable state; never contacts the
/// server or promotes a deferred application to configured.
#[tauri::command]
pub async fn effect_peq_reconcile<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    host: tauri::State<'_, Host>,
    epoch: String,
    id: EffectId,
) -> std::result::Result<View, ClientError> {
    authorize(&window, &client, &epoch)?;
    let mut state = host.state.try_lock().map_err(|_| Error::MutationBusy)?;
    state.select_epoch(&epoch);
    state
        .view(&client, &epoch, id, true)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn effect_peq_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    host: tauri::State<'_, Host>,
    epoch: String,
    id: EffectId,
) -> std::result::Result<View, ClientError> {
    authorize(&window, &client, &epoch)?;
    let mut state = host.state.try_lock().map_err(|_| Error::MutationBusy)?;
    state.select_epoch(&epoch);
    state.discard_unstarted_submission(&client, &epoch)?;
    if state
        .work
        .as_ref()
        .is_some_and(|(_, work)| matches!(work, Work::Submitted { .. }))
        || client
            .native_effect_registration_phase(&epoch, id)?
            .is_some_and(LeasePhase::needs_release)
        || !client.native_effect_pending(&epoch)?.is_empty()
    {
        return Err(Error::MutationBusy.into());
    }
    host.check_adapter()?;
    state.issue = None;
    state.work = Some((id, Work::Preparing));
    let result = client
        .preflight_native_effect_parametric(&epoch, id, Default::default())
        .await;
    client.validate_player_epoch(&epoch)?;
    state.work = Some((
        id,
        match result? {
            StandaloneParametricPreflight::Pending { .. } => Work::Preparing,
            StandaloneParametricPreflight::Unsupported => Work::Unsupported,
            StandaloneParametricPreflight::Ready(receipt) => Work::Prepared {
                ticket: state.ticket()?,
                attempt: NativeParametricActivationAttempt::new(receipt),
            },
        },
    ));
    state
        .view(&client, &epoch, id, false)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn effect_peq_activate<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    host: tauri::State<'_, Host>,
    epoch: String,
    id: EffectId,
    attempt: String,
    resume_existing: Option<bool>,
) -> std::result::Result<View, ClientError> {
    authorize(&window, &client, &epoch)?;
    let mut state = host.state.try_lock().map_err(|_| Error::MutationBusy)?;
    state.select_epoch(&epoch);
    if resume_existing.unwrap_or(false) {
        return resume_registered(&host, &client, &mut state, &epoch, id, &attempt)
            .await
            .map_err(Into::into);
    }
    if !matches!(&state.work, Some((owner, Work::Prepared { ticket, .. })) if *owner == id && ticket == &attempt)
    {
        return Err(Error::StaleResponse.into());
    }
    if client
        .native_effect_pending(&epoch)?
        .iter()
        .any(|entry| entry.id != id)
    {
        return Err(Error::MutationBusy.into());
    }
    host.check_adapter()?;
    let Some((_, Work::Prepared { attempt, .. })) = state.work.take() else {
        unreachable!("exact prepared state was just checked");
    };
    state.issue = None;
    state.release = None;
    let work = state.work.insert((id, Work::Submitted { attempt }));
    let Work::Submitted { attempt } = &mut work.1 else {
        unreachable!("submitted state was just installed");
    };
    let result = client
        .activate_native_effect_parametric(&epoch, attempt)
        .await;
    client.validate_player_epoch(&epoch)?;
    match result {
        Ok(_) => (),
        Err(error) => {
            if attempt.submitted_target().is_none()
                && !client
                    .native_effect_registration_phase(&epoch, id)?
                    .is_some_and(LeasePhase::needs_release)
            {
                state.work = None;
                return Err(error.into());
            }
            state.note(id, error);
        }
    }
    state
        .view(&client, &epoch, id, true)
        .await
        .map_err(Into::into)
}

async fn resume_registered(
    host: &Host,
    client: &MusicClient,
    state: &mut State,
    epoch: &str,
    id: EffectId,
    ticket: &str,
) -> Result<View> {
    if !state.release.as_ref().is_some_and(|entry| {
        entry.ticket == ticket
            && entry.receipt.effect() == id
            && entry.receipt.phase() == LeasePhase::Registered
    }) {
        return Err(Error::StaleResponse);
    }
    if client
        .native_effect_pending(epoch)?
        .iter()
        .any(|entry| entry.id != id)
    {
        return Err(Error::MutationBusy);
    }
    if let Some((owner, Work::Submitted { attempt })) = &state.work
        && *owner == id
        && let Some(target) = attempt.submitted_target()
        && client
            .inspect_native_bound_parametric(epoch, target)
            .await?
            != ParametricBindingStatus::Absent
    {
        return Err(Error::MutationBusy);
    }
    host.check_adapter()?;
    let entry = state.release.take().ok_or(Error::StaleResponse)?;
    state.issue = None;
    let work = state.work.insert((
        id,
        Work::Submitted {
            attempt: NativeParametricActivationAttempt::existing(id),
        },
    ));
    let Work::Submitted { attempt } = &mut work.1 else {
        unreachable!("resume state was just installed");
    };
    let result = client
        .resume_native_effect_parametric(epoch, attempt, &entry.receipt)
        .await;
    client.validate_player_epoch(epoch)?;
    if let Err(error) = result {
        state.note(id, error);
    }
    state.view(client, epoch, id, true).await
}

#[tauri::command]
pub async fn effect_peq_stop<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    host: tauri::State<'_, Host>,
    epoch: String,
    id: EffectId,
    release: String,
    retry_unknown: Option<bool>,
) -> std::result::Result<View, ClientError> {
    authorize(&window, &client, &epoch)?;
    let mut state = host.state.try_lock().map_err(|_| Error::MutationBusy)?;
    state.select_epoch(&epoch);
    let retry = retry_unknown.unwrap_or(false);
    if !state.release.as_ref().is_some_and(|stop| {
        stop.ticket == release
            && stop.receipt.effect() == id
            && retry == stop_needs_retry(Some(stop.receipt.phase()))
    }) {
        return Err(Error::StaleResponse.into());
    }
    let stop = state.release.take().ok_or(Error::StaleResponse)?;
    let target = state.work.as_ref().and_then(|(owner, work)| match work {
        Work::Submitted { attempt, .. } if *owner == id => attempt.submitted_target(),
        _ => None,
    });
    if let Some(target) = target {
        let detach = client.clear_native_bound_parametric(&epoch, target).await;
        client.validate_player_epoch(&epoch)?;
        let failure = match detach {
            Ok(ParametricApplication::Cleared) => None,
            Ok(_) => Some(Error::State("PEQ detach was not confirmed")),
            Err(Error::StaleResponse)
                if client
                    .inspect_native_bound_parametric(&epoch, target)
                    .await?
                    == ParametricBindingStatus::Absent =>
            {
                // A submission may have been rejected before mount. Only
                // proceed to the core's independent cross-process absence
                // fence; a stale target by itself proves nothing.
                None
            }
            Err(error) => Some(error),
        };
        if let Some(error) = failure {
            state.note(id, error);
            return state
                .view(&client, &epoch, id, true)
                .await
                .map_err(Into::into);
        }
    }
    if state.work.as_ref().is_some_and(|(owner, _)| *owner == id) {
        state.work = None;
    }
    state.issue = None;
    let policy = if retry {
        ReleaseRetry::ExplicitAfterUnknownResult
    } else {
        ReleaseRetry::Never
    };
    let result = client
        .release_native_effect(&epoch, &stop.receipt, policy)
        .await;
    client.validate_player_epoch(&epoch)?;
    if let Err(error) = result {
        state.note(id, error);
    }
    state
        .view(&client, &epoch, id, true)
        .await
        .map_err(Into::into)
}
