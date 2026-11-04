use super::{ConfirmedCapture, Context, Deadline, PrivateServer, Rejected, Result};
use std::{
    collections::HashSet,
    ffi::{CStr, CString},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

const REVIEW_WINDOW: Duration = Duration::from_secs(30);
pub(super) const START_WINDOW: Duration = Duration::from_secs(3);
static NEXT_SNAPSHOT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
pub struct OutputChoice {
    snapshot: u64,
    row: usize,
}

pub enum Consent {
    Confirmed,
    Declined,
}

pub struct OutputOptions {
    server: PrivateServer,
    context: Context,
    topology: Topology,
    snapshot: u64,
    expires: Instant,
}

impl OutputOptions {
    pub(super) fn new(server: PrivateServer, context: Context, topology: Topology) -> Result<Self> {
        let snapshot = NEXT_SNAPSHOT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| Rejected("private output selection sequence exhausted"))?;
        Ok(Self {
            server,
            context,
            topology,
            snapshot,
            expires: Instant::now() + REVIEW_WINDOW,
        })
    }

    /// Candidate labels are local native names. The opaque key only applies to
    /// this discovery operation and is not appropriate for storage or IPC.
    pub fn choices(&self) -> impl Iterator<Item = (OutputChoice, &CStr, &CStr)> {
        self.topology
            .candidates
            .iter()
            .enumerate()
            .map(|(row, candidate)| {
                (
                    OutputChoice {
                        snapshot: self.snapshot,
                        row,
                    },
                    &*candidate.sink,
                    &*candidate.source,
                )
            })
    }

    /// The trusted host calls this only in response to a user decision.
    /// It consumes the menu even on rejection; no existing approval is reused.
    pub fn decide(
        self,
        choice: OutputChoice,
        consent: Consent,
        stop: &AtomicBool,
    ) -> Result<ConfirmedCapture> {
        if matches!(consent, Consent::Declined) {
            return Err(Rejected("private capture was not confirmed"));
        }
        if Instant::now() >= self.expires {
            return Err(Rejected("private output selection expired"));
        }
        let selected = choice
            .for_snapshot(self.snapshot, self.topology.candidates)
            .ok_or(Rejected(
                "private output choice belongs to a different selection",
            ))?;
        let deadline = Deadline::startup(stop);
        deadline.check()?;
        self.server.verify()?;
        if !self.context.topology(&deadline)?.contains(&selected) {
            return Err(Rejected("confirmed private output changed"));
        }
        if Instant::now() >= self.expires {
            return Err(Rejected("private output selection expired"));
        }
        Ok(ConfirmedCapture {
            server: self.server,
            context: self.context,
            selected,
            expires: Instant::now() + START_WINDOW,
        })
    }
}

impl OutputChoice {
    fn for_snapshot(self, snapshot: u64, candidates: Vec<Candidate>) -> Option<Candidate> {
        (self.snapshot == snapshot)
            .then(|| candidates.into_iter().nth(self.row))
            .flatten()
    }
}

pub(super) struct Device {
    pub index: u32,
    pub name: CString,
    pub relation: Option<(u32, CString)>,
}

#[derive(PartialEq, Eq)]
pub(super) struct Candidate {
    pub sink_index: u32,
    pub sink: CString,
    pub source_index: u32,
    pub source: CString,
}

pub(super) struct Topology {
    candidates: Vec<Candidate>,
}

impl Topology {
    pub fn new(sinks: Vec<Device>, sources: Vec<Device>) -> Result<Self> {
        if !unique_devices(&sinks) || !unique_devices(&sources) {
            return Err(Rejected("ambiguous private audio endpoint identity"));
        }
        let mut candidates = Vec::new();
        for sink in &sinks {
            let Some((source_index, source_name)) = &sink.relation else {
                continue;
            };
            if !valid_name(&sink.name) || !valid_name(source_name) {
                continue;
            }
            // A partial competing claim (same index or name) is ambiguous,
            // even if one entry also has a correctly formed backlink.
            let mut source_claims = sources
                .iter()
                .filter(|source| claims(source, sink.index, &sink.name));
            let Some(source) = source_claims.next() else {
                continue;
            };
            if source_claims.next().is_some()
                || source.index != *source_index
                || source.name != *source_name
                || !source
                    .relation
                    .as_ref()
                    .is_some_and(|(index, name)| *index == sink.index && *name == sink.name)
                || sinks
                    .iter()
                    .filter(|other| claims(other, source.index, &source.name))
                    .count()
                    != 1
            {
                continue;
            }
            candidates.push(Candidate {
                sink_index: sink.index,
                sink: sink.name.clone(),
                source_index: source.index,
                source: source.name.clone(),
            });
        }
        candidates.sort_by(|a, b| a.sink.cmp(&b.sink));
        Ok(Self { candidates })
    }

    pub fn contains(&self, candidate: &Candidate) -> bool {
        self.candidates.contains(candidate)
    }
}

fn claims(device: &Device, index: u32, name: &CStr) -> bool {
    device
        .relation
        .as_ref()
        .is_some_and(|(linked_index, linked_name)| {
            *linked_index == index || linked_name.as_c_str() == name
        })
}

fn unique_devices(devices: &[Device]) -> bool {
    let mut indices = HashSet::new();
    let mut names = HashSet::new();
    devices
        .iter()
        .all(|device| indices.insert(device.index) && names.insert(device.name.as_c_str()))
}

fn valid_name(name: &CStr) -> bool {
    let bytes = name.to_bytes();
    bytes.starts_with(b"cumulus_recognition_")
        && bytes.len() <= 96
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(index: u32, name: &CStr, relation: Option<(u32, &CStr)>) -> Device {
        Device {
            index,
            name: name.to_owned(),
            relation: relation.map(|(index, name)| (index, name.to_owned())),
        }
    }

    #[test]
    fn only_a_unique_reciprocal_output_monitor_is_selectable() {
        let output = c"cumulus_recognition_output";
        let monitor = c"cumulus_recognition_output.monitor";
        let fake = c"cumulus_recognition_fake.monitor";
        let sink = || device(31, output, Some((91, monitor)));
        let linked = || device(91, monitor, Some((31, output)));
        let source_only = || device(92, fake, None);
        let valid = Topology::new(vec![sink()], vec![linked(), source_only()]).unwrap();
        assert_eq!(valid.candidates.len(), 1);
        assert!(
            Topology::new(vec![sink()], vec![source_only()])
                .unwrap()
                .candidates
                .is_empty()
        );
        for conflicting in [
            device(92, fake, Some((31, c"wrong_name"))),
            device(92, fake, Some((33, output))),
        ] {
            let invalid = Topology::new(vec![sink()], vec![linked(), conflicting]).unwrap();
            assert!(invalid.candidates.is_empty());
        }
        assert!(Topology::new(vec![sink()], vec![linked(), linked()]).is_err());
        let other_sink = device(32, c"cumulus_recognition_other", Some((91, monitor)));
        assert!(
            Topology::new(vec![sink(), other_sink], vec![linked()])
                .unwrap()
                .candidates
                .is_empty()
        );
    }

    #[test]
    fn same_names_on_recreated_indices_and_another_snapshot_do_not_match() {
        let output = c"cumulus_recognition_output";
        let monitor = c"cumulus_recognition_output.monitor";
        let topology = |sink, source| {
            Topology::new(
                vec![device(sink, output, Some((source, monitor)))],
                vec![device(source, monitor, Some((sink, output)))],
            )
            .unwrap()
        };
        let initial = topology(3, 7);
        assert!(!topology(9, 17).contains(&initial.candidates[0]));
        let choice = OutputChoice {
            snapshot: 9,
            row: 0,
        };
        assert!(choice.for_snapshot(10, initial.candidates).is_none());
    }
}
