use super::{
    OutputIssue, OutputLease, OutputObservation, OutputPhase,
    runtime::{Key, Policy},
};
use crate::{Error, Result, playback::engine::*};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub struct ManagedOutput<E: AudioEngine + ?Sized> {
    // Release the native device before dropping the registration.
    inner: Box<E>,
    lease: OutputLease,
    checked: Option<Instant>,
    version: u64,
    policy: Option<Policy>,
    failed_selection: Option<(Key, String)>,
}
impl OutputLease {
    pub fn wrap<E: AudioEngine + ?Sized>(self, inner: Box<E>) -> ManagedOutput<E> {
        ManagedOutput {
            inner,
            lease: self,
            checked: None,
            version: 0,
            policy: None,
            failed_selection: None,
        }
    }
}
impl<E: AudioEngine + ?Sized> ManagedOutput<E> {
    fn observation(
        &self,
        policy: &Policy,
        phase: OutputPhase,
        issue: Option<OutputIssue>,
        devices: Option<&OutputDevices>,
    ) -> Result<()> {
        self.lease.observe(
            &policy.key,
            OutputObservation {
                kind: self.lease.kind,
                revision: policy.key.revision,
                phase,
                issue,
                configured: devices.map(|d| d.configured.clone()),
                driver: devices.and_then(|d| d.active_driver.clone()),
            },
        )
    }
    fn synchronize(&mut self, force: bool) -> Result<()> {
        if !self.lease.valid() {
            return Err(Error::Cancelled);
        }
        let version = self.lease.runtime.version();
        if !force
            && self.version == version
            && self
                .checked
                .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
        {
            return Ok(());
        }
        self.checked = Some(Instant::now());
        self.version = version;
        let preferences = self.lease.runtime.preferences()?;
        let mut policy = self.lease.policy(&preferences)?;
        let changed = self.policy.as_ref().is_some_and(|p| p.key != policy.key);
        if changed {
            self.failed_selection = None;
        }
        self.policy = Some(policy.clone());
        let mut devices = match self.inner.output_devices() {
            Ok(Some(value)) => value,
            Ok(None) => return self.observation(&policy, OutputPhase::Unsupported, None, None),
            // Enumeration failure must not destroy a healthy decoder or be
            // confused with a music/network failure. The UI can retry discovery.
            Err(_) => {
                return self.observation(
                    &policy,
                    OutputPhase::Failed,
                    Some(OutputIssue::Enumeration),
                    None,
                );
            }
        };
        self.lease.devices(&devices.devices)?;
        if policy.effective() != "auto"
            && !devices.devices.iter().any(|d| d.id == policy.effective())
        {
            policy = self
                .lease
                .fallback(&policy.key, OutputIssue::MissingDevice, false)?;
        }
        let candidate = policy.effective().to_owned();
        let attempted = (policy.key.clone(), candidate.clone());
        if devices.configured != candidate || changed {
            if self.failed_selection.as_ref() == Some(&attempted) {
                self.policy = Some(policy.clone());
                return self.observation(
                    &policy,
                    OutputPhase::Failed,
                    Some(OutputIssue::Selection),
                    Some(&devices),
                );
            }
            if self.inner.set_output_device(&candidate).is_ok() {
                devices.configured = candidate;
                self.failed_selection = None;
            } else if candidate != "auto" {
                policy = self
                    .lease
                    .fallback(&policy.key, OutputIssue::Selection, false)?;
                if self.inner.set_output_device("auto").is_ok() {
                    devices.configured = "auto".into();
                    self.failed_selection = None;
                } else {
                    self.failed_selection = Some((policy.key.clone(), "auto".into()));
                }
            } else {
                self.failed_selection = Some(attempted);
            }
        } else {
            // A timed-out native write may complete later. Readback settles
            // configuration only; it is still not physical endpoint evidence.
            self.failed_selection = None;
        }
        self.policy = Some(policy.clone());
        self.observation(
            &policy,
            if self.failed_selection.is_some() || policy.initialization_failed {
                OutputPhase::Failed
            } else if policy.fallback.is_some() {
                OutputPhase::Fallback
            } else {
                OutputPhase::Configured
            },
            if self.failed_selection.is_some() {
                Some(OutputIssue::Selection)
            } else if policy.initialization_failed {
                Some(OutputIssue::Initialization)
            } else {
                policy.fallback
            },
            Some(&devices),
        )
    }
    fn recover_output(&mut self) -> Result<()> {
        let policy = self
            .policy
            .as_ref()
            .ok_or(Error::State("audio output has not been inspected"))?;
        let policy = self
            .lease
            .fallback(&policy.key, OutputIssue::Initialization, true)?;
        self.policy = Some(policy.clone());
        let applied = self.inner.set_output_device("auto");
        if applied.is_err() {
            self.observation(
                &policy,
                OutputPhase::Failed,
                Some(OutputIssue::Initialization),
                None,
            )?;
        } else {
            self.lease.observe(
                &policy.key,
                OutputObservation {
                    kind: self.lease.kind,
                    revision: policy.key.revision,
                    phase: OutputPhase::Fallback,
                    issue: Some(OutputIssue::Initialization),
                    configured: Some("auto".into()),
                    // A successful configuration command does not prove the
                    // replacement output has initialized or chosen an endpoint.
                    driver: None,
                },
            )?;
        }
        applied
    }
    fn record_load_failure(&mut self) {
        if let Some(policy) = self
            .policy
            .as_ref()
            .and_then(|policy| self.lease.initialization(&policy.key, true).ok())
        {
            let _ = self.observation(
                &policy,
                OutputPhase::Failed,
                Some(OutputIssue::Initialization),
                None,
            );
            self.policy = Some(policy);
        }
    }
}
fn output_failure(error: &Error) -> bool {
    // Pinned mpv client ABI MPV_ERROR_AO_INIT_FAILED. Never retry an arbitrary
    // decoder, URL, entitlement or network failure as an audio-device problem.
    matches!(error.root(), Error::Playback { code: -14, .. })
}
fn remaining(request: &LoadRequest, deadline: Instant) -> Result<LoadRequest> {
    let timeout = deadline
        .checked_duration_since(Instant::now())
        .filter(|t| !t.is_zero())
        .ok_or(Error::Playback {
            operation: "output preparation timeout",
            code: -1,
        })?;
    Ok(LoadRequest {
        timeout,
        ..request.clone()
    })
}
impl<E: AudioEngine + ?Sized> AudioEngine for ManagedOutput<E> {
    fn maintain_output(&mut self) -> Result<()> {
        self.synchronize(false)
    }
    fn output_devices(&mut self) -> Result<Option<OutputDevices>> {
        self.inner.output_devices()
    }
    fn set_output_device(&mut self, _: &str) -> Result<()> {
        Err(Error::State(
            "managed output must use persisted preferences",
        ))
    }
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        request.validate()?;
        let cancel = self.lease.cancellation(cancel);
        cancel.check()?;
        let deadline = Instant::now() + request.timeout;
        self.synchronize(true)?;
        cancel.check()?;
        let result = match self.inner.load(remaining(&request, deadline)?, &cancel) {
            Err(error) if output_failure(&error) && !cancel.is_cancelled() => {
                if self.recover_output().is_err() {
                    self.record_load_failure();
                    return Err(error);
                }
                cancel.check()?;
                let result = self.inner.load(remaining(&request, deadline)?, &cancel);
                if result.as_ref().is_err_and(output_failure) {
                    self.record_load_failure();
                }
                result
            }
            result => result,
        };
        if cancel.is_cancelled() {
            let _ = self.inner.stop();
            return Err(Error::Cancelled);
        }
        if result.is_ok()
            && self
                .policy
                .as_ref()
                .is_some_and(|p| p.initialization_failed)
        {
            let key = self.policy.as_ref().expect("inspected policy").key.clone();
            let policy = self.lease.initialization(&key, false)?;
            self.observation(&policy, OutputPhase::Fallback, policy.fallback, None)?;
            self.policy = Some(policy);
        }
        result
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        if !paused {
            self.synchronize(false)?;
        }
        self.inner.pause(paused)
    }
    fn seek(&mut self, seconds: f64) -> Result<()> {
        self.inner.seek(seconds)
    }
    fn volume(&mut self, value: f64) -> Result<()> {
        self.inner.volume(value)
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        self.inner.speed(value)
    }
    fn stop(&mut self) -> Result<()> {
        self.inner.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.synchronize(false)?;
        let mut value = self.inner.poll()?;
        if let Some(entry) = value.entry_id {
            let failed = value.events.iter().any(|e| matches!(e, EngineEvent::Ended { entry_id, error: -14, reason: EndReason::Error } if *entry_id == entry));
            if failed && self.recover_output().is_ok() {
                for event in &mut value.events {
                    if matches!(event, EngineEvent::Ended { entry_id, error: -14, reason: EndReason::Error } if *entry_id == entry)
                    {
                        *event = EngineEvent::OutputReload { entry_id: entry };
                    }
                }
            } else if failed {
                self.record_load_failure();
            }
        }
        Ok(value)
    }
    fn apply_equalizer(
        &mut self,
        value: crate::effects::equalizer::EqualizerSettings,
    ) -> Result<bool> {
        self.inner.apply_equalizer(value)
    }
    fn clear_equalizer(&mut self) -> Result<()> {
        self.inner.clear_equalizer()
    }
    fn observe_equalizer(
        &mut self,
    ) -> Result<Option<crate::effects::equalizer::abi::EqualizerStats>> {
        self.inner.observe_equalizer()
    }
}
impl<E: VideoEngine + ?Sized> VideoEngine for ManagedOutput<E> {
    fn resize(&self, width: u32, height: u32) -> Result<()> {
        self.inner.resize(width, height)
    }
    fn frame_after(&self, sequence: u64) -> Result<Option<Arc<VideoFrame>>> {
        self.inner.frame_after(sequence)
    }
}
