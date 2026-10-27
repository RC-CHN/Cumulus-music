use super::*;
use crate::{Error, Result};
use std::collections::BTreeSet;
impl FmQueue {
    pub(super) fn empty(source: FmSource, rights: FmRights) -> Self {
        Self {
            source,
            revision: 0,
            tracks: vec![],
            index: 0,
            repeat: FmRepeat::Infinite,
            fetched_at: 0,
            rights,
            last_step_at: None,
        }
    }
    pub(super) fn append(&mut self, tracks: impl IntoIterator<Item = FmTrack>) {
        let mut ids = self
            .tracks
            .iter()
            .map(|t| t.id.clone())
            .collect::<BTreeSet<_>>();
        self.tracks
            .extend(tracks.into_iter().filter(|t| ids.insert(t.id.clone())));
    }
    pub(super) fn select(&mut self, index: usize) {
        self.index = index;
        if self.index > 10 {
            self.tracks.drain(..self.index - 10);
            self.index = 10;
        }
    }
    pub(super) fn bump(&mut self) -> Result<()> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::State("FM revision exhausted"))?;
        self.validate()
    }
    pub(super) fn find(
        &self,
        start: usize,
        previous: bool,
        blacklist: &FmBlacklist,
    ) -> Option<usize> {
        if previous {
            (0..=start.min(self.tracks.len().saturating_sub(1)))
                .rev()
                .find(|&i| self.tracks.get(i).is_some_and(|t| !blacklist.blocks(t)))
        } else {
            (start..self.tracks.len()).find(|&i| !blacklist.blocks(&self.tracks[i]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn tracks(values: &Value) -> Vec<FmTrack> {
        values
            .as_array()
            .unwrap()
            .iter()
            .map(|v| FmTrack {
                id: v["id"].as_str().unwrap().into(),
                entry_id: uuid::Uuid::new_v4().simple().to_string(),
                algorithm: String::new(),
                song: v.clone(),
            })
            .collect()
    }
    #[test]
    fn recovered_js_queue_helpers_match() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/fm-official.json")).unwrap();
        for case in fixture["refill"].as_array().unwrap() {
            let mut q = FmQueue::empty(FmSource::Personal, FmRights::default());
            let len = case["len"].as_u64().unwrap() as usize;
            q.tracks = (0..len)
                .map(|i| FmTrack {
                    id: i.to_string(),
                    entry_id: i.to_string(),
                    algorithm: String::new(),
                    song: serde_json::json!({"id":i}),
                })
                .collect();
            q.index = case["index"].as_u64().unwrap() as usize;
            assert_eq!(q.needs_refill(), case["expected"].as_bool().unwrap());
        }
        for case in fixture["merge"].as_array().unwrap() {
            let mut q = FmQueue::empty(FmSource::Personal, FmRights::default());
            q.tracks = tracks(&case["old"]);
            for batch in case["batches"].as_array().unwrap() {
                q.append(tracks(batch));
            }
            assert_eq!(
                serde_json::to_value(q.tracks.iter().map(|t| &t.song).collect::<Vec<_>>()).unwrap(),
                case["expected"]
            );
        }
        for case in fixture["selection"].as_array().unwrap() {
            let mut q = FmQueue::empty(FmSource::Personal, FmRights::default());
            q.tracks = tracks(&case["tracks"]);
            let ids = |key: &str| {
                case["blacklist"][key]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().into())
                    .collect()
            };
            let actual = q.find(
                case["start"].as_u64().unwrap() as usize,
                case["previous"].as_bool().unwrap(),
                &FmBlacklist {
                    songs: ids("songIdList"),
                    artists: ids("artistIdList"),
                },
            );
            assert_eq!(
                actual.map(|i| i as i64).unwrap_or(-1),
                case["expected"].as_i64().unwrap()
            );
        }
    }
}
