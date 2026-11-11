//! Record only facts observed by the actual native engine behind the actor.
use cumulus_core::{
    Result,
    effects::{
        equalizer::{EqualizerSettings, abi::EqualizerStats},
        parametric::ParametricPlan,
    },
    playback::engine::{
        AudioEngine, Cancellation, DecodedAudioFormat, EndReason, EngineEvent, EngineSnapshot,
        LoadRequest, LoadedMedia, MediaInput, MpvEngine,
    },
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque, btree_map::Entry},
    sync::{Arc, Mutex},
};

#[derive(Serialize)]
pub struct Load {
    pub native_file: bool,
    pub rooted_windows: bool,
    pub indexed_file: bool,
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
#[derive(Serialize)]
pub struct Yield {
    pub old_entry: i64,
    pub replacement: i64,
    pub graph_entry: Option<i64>,
}
#[derive(Serialize)]
pub struct Poll {
    pub sequence: u64,
    pub entry: Option<i64>,
    pub position: Option<f64>,
    pub paused: bool,
    pub seeking: bool,
    pub buffering: bool,
    pub ended: Vec<(i64, String, i32)>,
}
#[derive(Default, Serialize)]
pub struct Observations {
    pub ordinary_loads: usize,
    pub pinned_stops: Vec<&'static str>,
    pub stock_codecs: Vec<Option<String>>,
    pub stock_rooted_windows: Vec<bool>,
    pub stock_entries: Vec<i64>,
    pub decoded_formats: BTreeMap<i64, [u32; 2]>,
    pub loads: Vec<Load>,
    pub seeks: Vec<Seek>,
    pub processed_entries: BTreeSet<i64>,
    pub local_updates: Vec<EqualizerSettings>,
    pub eof_entries: Vec<i64>,
    pub yields: Vec<Yield>,
    pub stock_paused_position: Option<f64>,
    pub peq_processed_entries: BTreeSet<i64>,
    pub last_polls: VecDeque<Poll>,
    poll_sequence: u64,
}
impl Observations {
    /// Require an actual new native entry even if the old paused clock was
    /// already within the requested slider's numerical tolerance.
    pub(super) fn settled_spatial_seek(&self, rate_48k: bool) -> Option<f64> {
        let [load] = self.loads.as_slice() else {
            return None;
        };
        let [seek] = self.seeks.as_slice() else {
            return None;
        };
        let replacement = seek.replacement?;
        let tolerance = if rate_48k { 0.02 } else { 0.06 };
        (seek.old_entry == load.entry
            && replacement != load.entry
            && seek.graph_entry == Some(replacement)
            && self.last_polls.iter().any(|poll| {
                poll.entry == Some(replacement)
                    && poll.paused
                    && !poll.seeking
                    && (!rate_48k || !poll.buffering)
                    && poll
                        .position
                        .is_some_and(|position| (position - seek.seconds).abs() < tolerance)
            }))
        .then_some(seek.seconds)
    }
}

pub type Evidence = Arc<Mutex<Observations>>;

pub struct Recorded {
    pub engine: MpvEngine,
    pub evidence: Evidence,
    pub record_rate: bool,
    pub observe_stops: bool,
}

fn rooted_windows_source(source: &MediaInput) -> bool {
    #[cfg(windows)]
    {
        matches!(source, MediaInput::WindowsNativeFile(_))
    }
    #[cfg(not(windows))]
    {
        let _ = source;
        false
    }
}

impl AudioEngine for Recorded {
    fn reopen_retired_local_sink(&mut self, cancel: &Cancellation) -> Result<bool> {
        self.engine.reopen_retired_local_sink(cancel)
    }
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
        let rooted_windows = rooted_windows_source(&request.source);
        let loaded = self.engine.load(request, cancel)?;
        let mut evidence = self.evidence.lock().unwrap();
        evidence.stock_codecs.push(loaded.codec.clone());
        evidence.stock_rooted_windows.push(rooted_windows);
        evidence.stock_entries.push(loaded.entry_id);
        Ok(loaded)
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
        let rooted_windows = rooted_windows_source(&request.source);
        let native_file =
            rooted_windows || matches!(&request.source, MediaInput::NativeFile { .. });
        let start = request.start;
        let loaded = self
            .engine
            .load_with_local_equalizer(request, settings, cancel)?;
        self.evidence.lock().unwrap().loads.push(Load {
            native_file,
            rooted_windows,
            indexed_file: false,
            start,
            entry: loaded.entry_id,
            graph_entry: self.engine.local_equalizer_entry(),
            codec: loaded.codec.clone(),
        });
        Ok(loaded)
    }
    fn supports_indexed_audio_equalizer(
        &self,
        format: cumulus_core::library::IndexedAudioFormat,
    ) -> bool {
        self.engine.supports_indexed_audio_equalizer(format)
    }
    fn load_indexed_audio_equalizer(
        &mut self,
        receipt: cumulus_core::library::IndexedAudioReceipt,
        selected: cumulus_core::library::LocalResource,
        settings: EqualizerSettings,
        cancel: &Cancellation,
    ) -> Result<LoadedMedia> {
        let loaded = self
            .engine
            .load_indexed_audio_equalizer(receipt, selected, settings, cancel)?;
        self.evidence.lock().unwrap().loads.push(Load {
            native_file: false,
            rooted_windows: false,
            indexed_file: true,
            start: 0.0,
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
    fn yield_local_equalizer(&mut self, entry: i64, cancel: &Cancellation) -> Result<LoadedMedia> {
        let media = self.engine.yield_local_equalizer(entry, cancel)?;
        self.evidence.lock().unwrap().yields.push(Yield {
            old_entry: entry,
            replacement: media.entry_id,
            graph_entry: self.engine.local_equalizer_entry(),
        });
        Ok(media)
    }
    fn apply_parametric_component(&mut self, plan: ParametricPlan) -> Result<bool> {
        self.engine.apply_parametric_component(plan)
    }
    fn bypass_parametric_component(&mut self) -> Result<()> {
        self.engine.bypass_parametric_component()
    }
    fn restore_parametric_component(&mut self, plan: ParametricPlan) -> Result<()> {
        self.engine.restore_parametric_component(plan)
    }
    fn clear_parametric_component(&mut self) -> Result<()> {
        self.engine.clear_parametric_component()
    }
    fn pause(&mut self, paused: bool) -> Result<()> {
        self.engine.pause(paused).inspect_err(|error| {
            eprintln!("ordinary local EQ native pause={paused}: {error}");
        })
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
        let pinned = self
            .observe_stops
            .then(|| self.engine.local_equalizer_entry())
            .flatten();
        self.engine.stop()?;
        if let Some(entry) = pinned {
            let observation = match self.engine.poll() {
                Ok(value)
                    if value.entry_id.is_none()
                        && value.events.iter().any(|event| {
                            matches!(event, EngineEvent::Ended {
                        entry_id, reason: EndReason::Stop, error: 0,
                    } if *entry_id == entry)
                        }) =>
                {
                    "exact_stop"
                }
                Err(cumulus_core::Error::Playback {
                    operation: "native sink retired",
                    code: -1,
                }) => "native_destroyed",
                _ => "unexpected",
            };
            self.evidence.lock().unwrap().pinned_stops.push(observation);
        }
        Ok(())
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        let value = self.engine.poll().inspect_err(|error| {
            eprintln!("ordinary local EQ native poll: {error}");
        })?;
        let ended = value.events.iter().filter_map(|event| match *event {
            EngineEvent::Ended {
                entry_id,
                reason: EndReason::Eof,
                error: 0,
            } => Some(entry_id),
            _ => None,
        });
        let mut evidence = self.evidence.lock().unwrap();
        if self.record_rate
            && !value.seeking
            && !value.buffering
            && let Some(entry) = value.entry_id
            && let Entry::Vacant(slot) = evidence.decoded_formats.entry(entry)
            && let Ok(Some(format)) = self.engine.decoded_audio_format(entry)
        {
            assert_eq!(format.entry_id, entry);
            slot.insert([format.sample_rate, format.channels]);
        }
        evidence.eof_entries.extend(ended);
        if evidence.last_polls.len() == 8 {
            evidence.last_polls.pop_front();
        }
        evidence.poll_sequence += 1;
        let sequence = evidence.poll_sequence;
        evidence.last_polls.push_back(Poll {
            sequence,
            entry: value.entry_id,
            position: value.position,
            paused: value.paused,
            seeking: value.seeking,
            buffering: value.buffering,
            ended: value
                .events
                .iter()
                .filter_map(|event| match event {
                    EngineEvent::Ended {
                        entry_id,
                        reason,
                        error,
                    } => Some((*entry_id, format!("{reason:?}"), *error)),
                    _ => None,
                })
                .collect(),
        });
        if let Some(entry) = value.entry_id {
            if value.paused
                && !value.seeking
                && evidence
                    .yields
                    .last()
                    .is_some_and(|handoff| handoff.replacement == entry)
                && let Some(position) = value.position
            {
                evidence.stock_paused_position = Some(position);
            }
            if !value.paused
                && !value.seeking
                && !value.buffering
                && value.parametric_samples.is_some_and(|samples| samples > 0)
            {
                evidence.peq_processed_entries.insert(entry);
            }
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_seek_receipt_waits_for_replacement_even_with_close_old_clock() {
        let target = 0.500039682539683;
        let mut observed = Observations::default();
        observed.loads.push(Load {
            native_file: true,
            rooted_windows: false,
            indexed_file: false,
            start: 0.0,
            entry: 2,
            graph_entry: Some(2),
            codec: Some("pcm_f32le".into()),
        });
        let poll = |entry, position, seeking| Poll {
            sequence: 1,
            entry: Some(entry),
            position: Some(position),
            paused: true,
            seeking,
            buffering: false,
            ended: Vec::new(),
        };
        let old_clock = 0.4414965986394558;
        assert!(target - old_clock < 0.06, "exact CI false-positive vector");
        observed.last_polls.push_back(poll(2, old_clock, false));
        assert_eq!(observed.settled_spatial_seek(false), None);
        observed.seeks.push(Seek {
            seconds: target,
            old_entry: 2,
            replacement: Some(3),
            graph_entry: Some(3),
        });
        assert_eq!(observed.settled_spatial_seek(false), None);
        observed.last_polls.push_back(poll(3, target, true));
        assert_eq!(observed.settled_spatial_seek(false), None);
        observed.last_polls.push_back(poll(3, target, false));
        assert_eq!(observed.settled_spatial_seek(false), Some(target));
    }
}
