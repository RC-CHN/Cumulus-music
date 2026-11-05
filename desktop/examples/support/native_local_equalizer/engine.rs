//! Record only facts observed by the actual native engine behind the actor.
use cumulus_core::{
    Result,
    effects::equalizer::{EqualizerSettings, abi::EqualizerStats},
    playback::engine::{
        AudioEngine, Cancellation, DecodedAudioFormat, EndReason, EngineEvent, EngineSnapshot,
        LoadRequest, LoadedMedia, MediaInput, MpvEngine,
    },
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

#[derive(Serialize)]
pub struct Load {
    pub native_file: bool,
    pub start: f64,
    pub entry: i64,
    pub graph_entry: Option<i64>,
    pub codec: Option<String>,
}
#[derive(Serialize)]
pub struct Seek {
    pub seconds: f64,
    pub old_entry: i64,
    pub replacement: Option<i64>,
    pub graph_entry: Option<i64>,
}
#[derive(Default, Serialize)]
pub struct Observations {
    pub ordinary_loads: usize,
    pub loads: Vec<Load>,
    pub seeks: Vec<Seek>,
    pub processed_entries: BTreeSet<i64>,
    pub local_updates: Vec<EqualizerSettings>,
    pub eof_entries: Vec<i64>,
}
pub type Evidence = Arc<Mutex<Observations>>;

pub struct Recorded {
    pub engine: MpvEngine,
    pub evidence: Evidence,
}
impl AudioEngine for Recorded {
    fn decoded_audio_format(&mut self, entry: i64) -> Result<Option<DecodedAudioFormat>> {
        self.engine.decoded_audio_format(entry)
    }
    fn apply_equalizer(&mut self, settings: EqualizerSettings) -> Result<bool> {
        self.engine.apply_equalizer(settings)
    }
    fn bypass_equalizer_for_source(&mut self) -> Result<bool> {
        self.engine.bypass_equalizer_for_source()
    }
    fn clear_equalizer(&mut self) -> Result<()> {
        self.engine.clear_equalizer()
    }
    fn observe_equalizer(&mut self) -> Result<Option<EqualizerStats>> {
        self.engine.observe_equalizer()
    }
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        self.evidence.lock().unwrap().ordinary_loads += 1;
        self.engine.load(request, cancel)
    }
    fn supports_local_equalizer(&self) -> bool {
        self.engine.supports_local_equalizer()
    }
    fn load_with_local_equalizer(
        &mut self,
        request: LoadRequest,
        settings: EqualizerSettings,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        let native_file = matches!(&request.source, MediaInput::NativeFile { .. });
        let start = request.start;
        let loaded = self
            .engine
            .load_with_local_equalizer(request, settings, cancel)?;
        self.evidence.lock().unwrap().loads.push(Load {
            native_file,
            start,
            entry: loaded.entry_id,
            graph_entry: self.engine.local_equalizer_entry(),
            codec: loaded.codec.clone(),
        });
        Ok(loaded)
    }
    fn local_equalizer_entry(&self) -> Option<i64> {
        self.engine.local_equalizer_entry()
    }
    fn pump_local_equalizer(&mut self) -> Result<()> {
        self.engine.pump_local_equalizer()
    }
    fn apply_local_equalizer(&mut self, settings: EqualizerSettings) -> Result<()> {
        self.engine.apply_local_equalizer(settings)?;
        self.evidence.lock().unwrap().local_updates.push(settings);
        Ok(())
    }
    fn local_equalizer_processing(&mut self) -> Result<bool> {
        let processing = self.engine.local_equalizer_processing()?;
        if processing && let Some(entry) = self.engine.local_equalizer_entry() {
            self.evidence
                .lock()
                .unwrap()
                .processed_entries
                .insert(entry);
        }
        Ok(processing)
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        self.engine.pause(paused)
    }
    fn seek(&mut self, seconds: f64) -> Result<()> {
        self.engine.seek(seconds)
    }
    fn seek_with_receipt(
        &mut self,
        seconds: f64,
        entry: i64,
        cancel: &Cancellation,
    ) -> Result<Option<LoadedMedia>> {
        let result = self.engine.seek_with_receipt(seconds, entry, cancel)?;
        self.evidence.lock().unwrap().seeks.push(Seek {
            seconds,
            old_entry: entry,
            replacement: result.as_ref().map(|media| media.entry_id),
            graph_entry: self.engine.local_equalizer_entry(),
        });
        Ok(result)
    }
    fn volume(&mut self, value: f64) -> Result<()> {
        self.engine.volume(value)
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        self.engine.speed(value)
    }
    fn stop(&mut self) -> Result<()> {
        self.engine.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        let value = self.engine.poll()?;
        let ended = value.events.iter().filter_map(|event| match *event {
            EngineEvent::Ended {
                entry_id,
                reason: EndReason::Eof,
                error: 0,
            } => Some(entry_id),
            _ => None,
        });
        self.evidence.lock().unwrap().eof_entries.extend(ended);
        Ok(value)
    }
}
