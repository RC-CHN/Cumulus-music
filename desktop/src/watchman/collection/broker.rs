use cumulus_core::{Error, Result, watchman::MAX_BYTES};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasurementFrame {
    pub epoch: u64,
    pub request: u64,
    pub value: Value,
}
#[derive(Clone, Copy)]
pub struct Ticket {
    pub epoch: u64,
    pub request: u64,
}
#[derive(Default)]
struct State {
    epoch: u64,
    serial: u64,
    ready: bool,
    owner: Option<u64>,
    pending: BTreeMap<u64, oneshot::Sender<Result<Value>>>,
}
#[derive(Clone, Default)]
pub struct MeasurementBridge(Arc<Mutex<State>>);
impl MeasurementBridge {
    pub(super) fn claim(&self) -> Result<u64> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.owner.is_some() {
            return Err(Error::State(
                "main window already owns a measurement source",
            ));
        }
        state.serial = state
            .serial
            .checked_add(1)
            .filter(|v| *v <= 9_007_199_254_740_991)
            .ok_or(Error::State("measurement owner sequence exhausted"))?;
        let owner = state.serial;
        state.owner = Some(owner);
        Ok(owner)
    }
    pub(super) fn release(&self, owner: u64) {
        if let Ok(mut state) = self.0.lock()
            && state.owner == Some(owner)
        {
            state.owner = None;
            for (_, send) in std::mem::take(&mut state.pending) {
                let _ = send.send(Err(Error::Cancelled));
            }
        }
    }
    pub fn page_started(&self) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state.ready = false;
        state.epoch = state
            .epoch
            .checked_add(1)
            .filter(|v| *v <= 9_007_199_254_740_991)
            .ok_or(Error::State("measurement page epoch exhausted"))?;
        for (_, sender) in std::mem::take(&mut state.pending) {
            let _ = sender.send(Err(Error::StaleResponse));
        }
        Ok(())
    }
    pub fn page_finished(&self) -> Result<()> {
        self.0.lock().map_err(|_| Error::LockPoisoned)?.ready = true;
        Ok(())
    }
    pub fn epoch(&self) -> Result<u64> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if !state.ready {
            return Err(Error::State("measurement page is not ready"));
        }
        Ok(state.epoch)
    }
    pub fn begin(&self) -> Result<(Ticket, oneshot::Receiver<Result<Value>>)> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if !state.ready {
            return Err(Error::State("measurement page is not ready"));
        }
        if state.pending.len() >= 8 {
            return Err(Error::State("too many pending browser measurements"));
        }
        state.serial = state
            .serial
            .checked_add(1)
            .filter(|v| *v <= 9_007_199_254_740_991)
            .ok_or(Error::State("measurement request sequence exhausted"))?;
        let ticket = Ticket {
            epoch: state.epoch,
            request: state.serial,
        };
        let (send, receive) = oneshot::channel();
        state.pending.insert(ticket.request, send);
        Ok((ticket, receive))
    }
    pub fn cancel(&self, ticket: Ticket) {
        if let Ok(mut state) = self.0.lock()
            && state.epoch == ticket.epoch
        {
            state.pending.remove(&ticket.request);
        }
    }
    pub fn receive(&self, frame: MeasurementFrame) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if !state.ready || state.epoch != frame.epoch {
            return Err(Error::StaleResponse);
        }
        let sender = state
            .pending
            .remove(&frame.request)
            .ok_or(Error::StaleResponse)?;
        let result = if serde_json::to_vec(&frame.value)?.len() > MAX_BYTES * 2 {
            Err(Error::InvalidInput(
                "browser measurement frame exceeds limit",
            ))
        } else {
            Ok(frame.value)
        };
        sender.send(result).map_err(|_| Error::Cancelled)
    }
}
pub(super) struct Pending {
    pub bridge: MeasurementBridge,
    pub ticket: Ticket,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.bridge.cancel(self.ticket);
    }
}

#[tauri::command]
pub(crate) fn watchman_measurement<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    bridge: tauri::State<'_, MeasurementBridge>,
    frame: MeasurementFrame,
) -> std::result::Result<(), String> {
    if window.label() != "main" {
        return Err("watchman_measurement_rejected".into());
    }
    bridge
        .receive(frame)
        .map_err(|_| "watchman_measurement_rejected".into())
}
