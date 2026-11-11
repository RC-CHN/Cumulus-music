//! M/308 → M/1384 → M/176 ordinary-song transport decisions. No URL/decoder
//! failures can enter this planner. Playback authorization remains separate.
use super::{
    PlaybackMode, QueueEntry, QueueSnapshot, Step,
    controller::PlaybackSnapshot,
    preflight::{OrdinaryCandidate, OrdinaryQueuePreflight},
};
use crate::{
    Error, Result,
    protocol::{api::identifier, wire::serialization::js_truthy},
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug)]
pub enum OrdinarySkipTrigger {
    MiniBar(Step),
    NativeEof,
    DirectQueue,
    ExplicitCatalog,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkipBoundary {
    ContextUnresolved,
    ExcludedTrigger,
    NaturalEndOrRepeat,
    InitialRightsNotProven,
    AmbiguousQueue,
    IncompletePreflight,
    AlternateOrLocalHandoff,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrdinarySkipDecision {
    Unchanged(SkipBoundary),
    KeepCurrentDenied {
        attempted: String,
    },
    PauseCurrent {
        attempted: String,
    },
    StopAtDeniedTail {
        attempted: String,
    },
    StopAtErrorLimit {
        attempted: String,
        denial_count: usize,
    },
    PlayExisting {
        attempted: String,
        target: String,
        denial_count: usize,
        /// M/308 only sets this flag in the ordered manual tail branch.
        at_ordered_tail: bool,
    },
}

pub struct OrdinarySkipOutcome {
    pub decision: OrdinarySkipDecision,
    pub applied: Option<PlaybackSnapshot>,
}

// Deliberately narrow M/308 checkPlayPrivilege + M/23.Z common branch:
// normalized negative copyright status (not area-only), unavailable online
// bitrate, no trial/download-only/purchase flags, no declared alternative.
// Missing or malformed facts cannot be used to skip a track.
pub(super) fn plain_copyright_denial(entry: &QueueEntry) -> bool {
    let resource = &entry.resource;
    let song = &resource["song"];
    if [
        "path",
        "local_id",
        "download_task_id",
        "download_ledger",
        "cloud_song_id",
        "voice_id",
        "playback_constraint",
    ]
    .into_iter()
    .any(|key| !resource[key].is_null())
        || !entry.visible_in_queue()
        || identifier(&song["id"])
            .ok()
            .zip(identifier(&resource["id"]).ok())
            .is_none_or(|(a, b)| a != b || a.len() > 32)
    {
        return false;
    }
    plain_copyright_metadata(song, &song["privilege"])
}

pub(super) fn plain_copyright_metadata(
    song: &serde_json::Value,
    privilege: &serde_json::Value,
) -> bool {
    let value = |raw, normalized| privilege.get(raw).unwrap_or(&privilege[normalized]);
    if ["voiceId", "voice_id", "fileId", "isOfflineDownOnline"]
        .into_iter()
        .any(|key| js_truthy(&song[key]))
        || !(song["noCopyrightRcmd"].is_null()
            || song["noCopyrightRcmd"]["type"].as_i64() == Some(0))
        || privilege["fee"].as_i64() != Some(0)
        || js_truthy(&privilege["payed"])
    {
        return false;
    }
    let Some((status, (maximum, flag))) = value("st", "status").as_i64().zip(
        value("pl", "maxPlayBr")
            .as_i64()
            .zip(privilege["flag"].as_u64()),
    ) else {
        return false;
    };
    status < 0
        && (status != -100 || js_truthy(&privilege["toast"]))
        && maximum <= 0
        && flag < (1 << 22)
        && flag & ((1 << 10) | (1 << 11)) == 0
}

struct Traversal<'a> {
    queue: &'a QueueSnapshot,
    display_index: HashMap<&'a str, usize>,
    transport: Vec<usize>,
    position: usize,
    direction: Step,
    native_end: bool,
}

impl<'a> Traversal<'a> {
    fn new(
        queue: &'a QueueSnapshot,
        trigger: OrdinarySkipTrigger,
    ) -> std::result::Result<Self, SkipBoundary> {
        let (direction, native_end) = match trigger {
            OrdinarySkipTrigger::MiniBar(step) => (step, false),
            OrdinarySkipTrigger::NativeEof => (Step::Next, true),
            OrdinarySkipTrigger::DirectQueue | OrdinarySkipTrigger::ExplicitCatalog => {
                return Err(SkipBoundary::ExcludedTrigger);
            }
        };
        let display_index: HashMap<_, _> = queue
            .items
            .iter()
            .enumerate()
            .map(|(index, row)| (row.entry_id.as_str(), index))
            .collect();
        let ids: HashSet<_> = queue
            .items
            .iter()
            .filter_map(|row| identifier(&row.resource["id"]).ok())
            .collect();
        if display_index.len() != queue.items.len() || ids.len() != queue.items.len() {
            // M/1384 keys its starting row by resourceId. Duplicate song IDs
            // in a Rust queue would not identify the same original row.
            return Err(SkipBoundary::AmbiguousQueue);
        }
        if queue.items.iter().any(|row| {
            !row.visible_in_queue()
                || matches!(row.source["kind"].as_str(), Some("listen_together" | "fm"))
                || [
                    "path",
                    "local_id",
                    "download_task_id",
                    "download_ledger",
                    "cloud_song_id",
                    "voice_id",
                    "playback_constraint",
                ]
                .into_iter()
                .any(|key| !row.resource[key].is_null())
                || ["voiceId", "voice_id", "fileId"]
                    .into_iter()
                    .any(|key| js_truthy(&row.resource["song"][key]))
        }) {
            return Err(SkipBoundary::ContextUnresolved);
        }
        let transport: Vec<_> = if queue.mode == PlaybackMode::Random {
            queue
                .order
                .iter()
                .map(|id| display_index.get(id.as_str()).copied())
                .collect::<Option<Vec<_>>>()
                .ok_or(SkipBoundary::AmbiguousQueue)?
        } else {
            (0..queue.items.len()).collect()
        };
        let current = queue
            .current
            .as_deref()
            .ok_or(SkipBoundary::AmbiguousQueue)?;
        let position = transport
            .iter()
            .position(|&index| queue.items[index].entry_id == current)
            .ok_or(SkipBoundary::AmbiguousQueue)?;
        if native_end
            && (queue.mode == PlaybackMode::SingleLoop
                || queue.mode == PlaybackMode::Sequential && position + 1 == transport.len())
        {
            return Err(SkipBoundary::NaturalEndOrRepeat);
        }
        Ok(Self {
            queue,
            display_index,
            transport,
            position,
            direction,
            native_end,
        })
    }

    fn step(&self, position: usize) -> usize {
        let size = self.transport.len();
        match self.direction {
            Step::Next => (position + 1) % size,
            Step::Previous => (position + size - 1) % size,
        }
    }

    fn attempted(&self) -> &'a QueueEntry {
        &self.queue.items[self.transport[self.step(self.position)]]
    }
}

pub(crate) fn initial_row(
    queue: &QueueSnapshot,
    trigger: OrdinarySkipTrigger,
) -> std::result::Result<&QueueEntry, SkipBoundary> {
    Ok(Traversal::new(queue, trigger)?.attempted())
}

pub(crate) fn after_initial_denial(
    queue: &QueueSnapshot,
    scan: &OrdinaryQueuePreflight,
    trigger: OrdinarySkipTrigger,
) -> Result<OrdinarySkipDecision> {
    after_initial_denial_with_download(queue, scan, trigger, false)
}

/// Only the strict EOF loader can turn an already authorized publication
/// into a candidate. Existing directional callers stay online-only.
pub(crate) fn after_initial_denial_with_download(
    queue: &QueueSnapshot,
    scan: &OrdinaryQueuePreflight,
    trigger: OrdinarySkipTrigger,
    allow_download: bool,
) -> Result<OrdinarySkipDecision> {
    use OrdinaryCandidate::*;
    use OrdinarySkipDecision as Decision;
    let t = match Traversal::new(queue, trigger) {
        Ok(t) => t,
        Err(reason) => return Ok(Decision::Unchanged(reason)),
    };
    if scan.rows.len() != queue.items.len()
        || scan.rows.iter().zip(&queue.items).any(|(fact, item)| {
            fact.entry_id != item.entry_id
                || identifier(&item.resource["id"]).as_deref().ok()
                    != Some(fact.original_song_id.as_str())
        })
    {
        return Err(Error::StaleResponse);
    }
    if !scan.fully_classified() {
        return Ok(Decision::Unchanged(SkipBoundary::IncompletePreflight));
    }
    let attempted = t.attempted().entry_id.clone();
    let display = t.display_index[attempted.as_str()];
    let facts = &scan.rows[display];
    if !plain_copyright_denial(t.attempted())
        || !facts.no_download_publication
        || facts.candidate != Unavailable
    {
        return Ok(Decision::Unchanged(SkipBoundary::InitialRightsNotProven));
    }
    let Some(last) = scan
        .candidate_order
        .last()
        .and_then(|&index| scan.rows.get(index))
    else {
        return Ok(Decision::PauseCurrent { attempted });
    };
    // M/308 searches the original displayed trackId for the final M/59
    // candidate's ID; it does not inherit the substituted row's display slot.
    let ordered_tail = if queue.mode == PlaybackMode::Sequential {
        let last_id = match &last.candidate {
            SameRecording { id } => id,
            _ => &last.original_song_id,
        };
        let tail = scan
            .rows
            .iter()
            .position(|row| &row.original_song_id == last_id)
            .unwrap_or(0);
        display >= tail
    } else {
        false
    };
    if ordered_tail && t.native_end {
        return Ok(Decision::StopAtDeniedTail { attempted });
    }

    // M/308's first jump does not supply assignedResourceId. Thus the
    // playlist-scene call attempts the same row from the old current again.
    // Each independently denied row then becomes M/1384's assigned error
    // step origin. Do not treat any URL read as this metadata rejection.
    let mut position = t.step(t.position);
    for count in 0..queue.items.len().min(100) {
        let row = &scan.rows[t.transport[position]];
        match &row.candidate {
            Online | AccountDownload if allow_download || row.candidate == Online => {
                return Ok(Decision::PlayExisting {
                    attempted,
                    target: row.entry_id.clone(),
                    denial_count: count,
                    at_ordered_tail: ordered_tail,
                });
            }
            AccountDownload | SameRecording { .. } | OriginalRetained { .. } => {
                return Ok(Decision::Unchanged(SkipBoundary::AlternateOrLocalHandoff));
            }
            Unavailable
                if row.no_download_publication
                    && plain_copyright_denial(&queue.items[t.transport[position]]) =>
            {
                position = t.step(position);
            }
            _ => return Ok(Decision::Unchanged(SkipBoundary::IncompletePreflight)),
        }
    }
    Ok(Decision::StopAtErrorLimit {
        attempted,
        denial_count: queue.items.len().min(100),
    })
}

#[cfg(test)]
mod tests;
