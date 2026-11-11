//! M225/M57 use the grabbed hit separately from the ordered selected Songs.
//! M3085 splice does not carry the search menu's trial mode. Use the same
//! verified Lyrics observations but separate grants for ordinary public Songs;
//! private, local, trial and same-recording fallback lanes are excluded.
use super::*;

#[derive(Serialize)]
pub struct LyricQueueDropRow {
    receipt: String,
    right: SearchNextRight,
}

#[derive(Default)]
pub(super) struct Grants {
    rows: HashMap<Uuid, Uuid>,
    first: HashMap<String, Uuid>,
}

impl Grants {
    pub(super) fn retain(&mut self, rows: &HashMap<Uuid, TrackView>) {
        self.rows.retain(|_, row| rows.contains_key(row));
        self.first
            .retain(|_, receipt| self.rows.contains_key(receipt));
    }

    pub(super) fn issue(
        &mut self,
        song: String,
        row: Uuid,
        right: SearchNextRight,
    ) -> LyricQueueDropRow {
        let receipt = Uuid::new_v4();
        self.rows.insert(receipt, row);
        self.first.entry(song).or_insert(receipt);
        LyricQueueDropRow {
            receipt: receipt.simple().to_string(),
            right,
        }
    }
}

pub(in crate::client) struct LyricDropSelection {
    episode: Uuid,
    receipts: Vec<Uuid>,
    pub(in crate::client) tracks: Vec<TrackView>,
}

impl LyricGroupPlayback {
    pub(in crate::client) fn resolve_drop(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &str,
        anchor: &str,
        rows: &[String],
    ) -> Result<LyricDropSelection> {
        const INVALID: Error = Error::InvalidInput("invalid lyric queue drop selection");
        let parse = |value: &str| {
            let receipt = Uuid::try_parse(value).map_err(|_| INVALID)?;
            if receipt.simple().to_string() != value {
                return Err(INVALID);
            }
            Ok(receipt)
        };
        if rows.is_empty() || rows.len() > MAX_GROUP || (rows.len() == 1 && rows[0] != anchor) {
            return Err(INVALID);
        }
        let anchor = parse(anchor)?;
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let episode = state
            .as_ref()
            .filter(|episode| episode.owns(token, &Arc::downgrade(api), credentials))
            .ok_or(Error::StaleResponse)?;
        let track_for = |receipt: &Uuid| {
            episode
                .drops
                .rows
                .get(receipt)
                .and_then(|row| episode.rows.get(row))
                .ok_or(Error::StaleResponse)
        };
        let grabbed = track_for(&anchor)?;
        let mut seen = HashSet::new();
        let mut receipts = Vec::with_capacity(rows.len() + 1);
        let mut tracks = Vec::with_capacity(rows.len());
        for receipt in rows {
            let receipt = parse(receipt)?;
            let track = track_for(&receipt)?;
            // M225 looks up each Song in selectedData using the first table
            // row; the physical grabbed hit is separate from those resources.
            if !seen.insert(track.id.as_str())
                || (rows.len() > 1 && episode.drops.first.get(&track.id) != Some(&receipt))
            {
                return Err(INVALID);
            }
            let right = episode
                .seen
                .get(&track.id)
                .and_then(Option::as_ref)
                .ok_or(Error::StaleResponse)?;
            receipts.push(receipt);
            if right.kind == SearchNextRight::Playable {
                tracks.push(track.clone());
            }
        }
        if !seen.contains(grabbed.id.as_str()) {
            return Err(INVALID);
        }
        if tracks.is_empty() {
            return Err(Error::State("selected lyric songs are unavailable online"));
        }
        if !receipts.contains(&anchor) {
            receipts.push(anchor);
        }
        Ok(LyricDropSelection {
            episode: episode.id,
            receipts,
            tracks,
        })
    }

    pub(in crate::client) fn drop_current(&self, selection: &LyricDropSelection) -> bool {
        self.0.lock().is_ok_and(|state| {
            state.as_ref().is_some_and(|episode| {
                episode.live()
                    && episode.id == selection.episode
                    && selection
                        .receipts
                        .iter()
                        .all(|receipt| episode.drops.rows.contains_key(receipt))
            })
        })
    }
}
