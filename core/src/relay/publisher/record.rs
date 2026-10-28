use super::*;
use crate::relay::{RelayMachine, RelayResourceKind};
impl RelayPublisher {
    /// Native-only committed event. Local machine + immutable request(s) commit
    /// together. The sender serializes HTTP separately and may be in flight.
    pub fn record_for(
        &self,
        token: &SessionToken,
        frame: PublicationFrame,
        event: PublicationEvent,
        config: &RelayConfig,
    ) -> Result<Vec<String>> {
        frame.validate()?;
        validate_config(config)?;
        let (owner, device) = self.identity(token)?;
        self.session.with_token(token, |_| {
            self.store.transaction(|tx| {
                let mut state = load(tx, &owner, &device)?;
                if state
                    .frame
                    .as_ref()
                    .is_some_and(|old| old.queue_revision > frame.queue_revision)
                {
                    return Err(Error::StaleResponse);
                }
                state.frame = Some(frame.clone());
                let uploads = apply(&mut state, &frame, event, config)?;
                let mut ids = Vec::new();
                for upload in uploads {
                    ids.push(append(tx, &owner, &mut state, upload, None)?.id);
                }
                // Backlog saturation creates a fresh authoritative snapshot. It
                // never replays or clears responsibility for an in-flight/unknown write.
                if records(tx, &owner, &device)?
                    .iter()
                    .filter(|r| r.phase == PublicationPhase::Prepared)
                    .count()
                    > MAX_PREPARED
                    && frame.can_publish()
                {
                    supersede(tx, &owner, &device, state.recovery.as_deref())?;
                    let upload = snapshot(&mut state, &frame, config)?;
                    ids = vec![append(tx, &owner, &mut state, upload, None)?.id];
                }
                save(tx, &owner, &state)?;
                Ok(ids)
            })
        })
    }
}
pub(super) fn validate_config(config: &RelayConfig) -> Result<()> {
    if config.snapshot_size < 100
        || config.snapshot_size > super::super::types::MAX_RESOURCES
        || !(3..=86_400).contains(&config.radio_progress_interval)
    {
        return Err(Error::InvalidInput("invalid relay publisher configuration"));
    }
    Ok(())
}
pub(super) fn supersede(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    device: &str,
    protected: Option<&str>,
) -> Result<()> {
    for mut row in records(tx, owner, device)? {
        if row.phase == PublicationPhase::Prepared && protected != Some(row.id.as_str()) {
            row.phase = PublicationPhase::Superseded;
            put(tx, owner, &row)?;
        }
    }
    Ok(())
}
pub(super) fn snapshot(
    state: &mut PublicationState,
    frame: &PublicationFrame,
    config: &RelayConfig,
) -> Result<RelayUpload> {
    if state.machine.is_none() {
        state.machine = Some(RelayMachine::start(
            frame
                .source
                .clone()
                .ok_or(Error::State("relay source missing"))?,
            frame
                .current
                .clone()
                .ok_or(Error::State("relay current missing"))?,
        )?);
    }
    state
        .machine
        .as_mut()
        .ok_or(Error::State("relay machine missing"))?
        .force_snapshot(frame.snapshot(config)?)
}
fn apply(
    state: &mut PublicationState,
    frame: &PublicationFrame,
    event: PublicationEvent,
    config: &RelayConfig,
) -> Result<Vec<RelayUpload>> {
    if !frame.can_publish()
        || matches!(
            event,
            PublicationEvent::Clear
                | PublicationEvent::Command {
                    command: crate::relay::RelayCommand::Clear
                }
        )
    {
        return state
            .machine
            .take()
            .map(|m| m.terminal_clear().map(|u| vec![u]))
            .unwrap_or(Ok(vec![]));
    }
    match event {
        PublicationEvent::Source => {
            let machine = RelayMachine::start(
                frame
                    .source
                    .clone()
                    .ok_or(Error::State("relay source missing"))?,
                frame
                    .current
                    .clone()
                    .ok_or(Error::State("relay current missing"))?,
            )?;
            let upload = machine.source_upload(frame.mode, false)?;
            state.machine = Some(machine);
            Ok(vec![upload])
        }
        PublicationEvent::Snapshot => Ok(vec![snapshot(state, frame, config)?]),
        PublicationEvent::ColdStart => state
            .machine
            .as_ref()
            .map(|m| m.source_upload(frame.mode, false).map(|u| vec![u]))
            .unwrap_or(Ok(vec![])),
        PublicationEvent::Mode { from_handoff: true } => Ok(vec![]),
        PublicationEvent::Mode {
            from_handoff: false,
        } => {
            if let Some(machine) = &state.machine {
                Ok(vec![machine.state_upload(frame.mode)?])
            } else {
                Ok(vec![snapshot(state, frame, config)?])
            }
        }
        PublicationEvent::Select => {
            let Some(machine) = &mut state.machine else {
                return Ok(vec![]);
            };
            let (upload, outside) = machine.select(
                frame
                    .current
                    .clone()
                    .ok_or(Error::State("relay current missing"))?,
                frame.mode,
            )?;
            let mut uploads = vec![upload];
            if outside {
                uploads.push(snapshot(state, frame, config)?);
            }
            Ok(uploads)
        }
        PublicationEvent::VoiceProgress { progress_ms } => {
            let Some(machine) = &mut state.machine else {
                return Ok(vec![]);
            };
            if machine.current().kind != RelayResourceKind::Voice
                || Some(machine.current()) != frame.current.as_ref()
            {
                return Ok(vec![]);
            }
            Ok(vec![machine.voice_progress(progress_ms, frame.mode)?])
        }
        PublicationEvent::Command { command } => {
            let Some(machine) = &mut state.machine else {
                return Ok(vec![]);
            };
            match machine.command(command) {
                Ok(upload) => Ok(vec![upload]),
                Err(Error::State("relay snapshot required")) => {
                    Ok(vec![snapshot(state, frame, config)?])
                }
                Err(error) => Err(error),
            }
        }
        PublicationEvent::Clear => unreachable!(),
    }
}
