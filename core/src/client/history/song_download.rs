//! Bind the history Download button to its native, ordered SONG selection.
use super::{HistoryCollectTarget, HistoryRowMenuTarget, HistorySelectionScope, row_menu};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    history::{HistoryKind, HistoryRecord, HistorySnapshot},
    library::LocalLibrary,
    protocol::{api::identifier, wire::serialization::js_truthy},
    storage::PlaybackDownloadKind,
};
use serde_json::Value;
use std::collections::BTreeSet;

const MAX_ROWS: usize = 600;

/// No renderer metadata or separately supplied anchor is accepted.
pub type HistorySongDownloadTarget = HistorySelectionScope;

pub(in crate::client) use super::song_collection::download_picker_collectable;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client) enum HistorySongDownloadSource<'a> {
    /// Only catalogue identity was observed. Download rights are independent.
    Catalog,
    /// An exact ledger candidate, not evidence of task ownership or status.
    DownloadTask(&'a str),
    Local,
    Cloud,
    Unsupported,
}

pub(in crate::client) struct HistorySongDownloadRow<'a> {
    pub(in crate::client) record: &'a HistoryRecord,
    pub(in crate::client) source: HistorySongDownloadSource<'a>,
}

pub(in crate::client) struct HistorySongDownloadProjection<'a> {
    pub(in crate::client) anchor: &'a HistoryRecord,
    pub(in crate::client) rows: Vec<HistorySongDownloadRow<'a>>,
    snapshot: &'a HistorySnapshot,
}

impl<'a> HistorySongDownloadProjection<'a> {
    pub(in crate::client) fn project_row(
        target: &HistoryRowMenuTarget,
        snapshot: &'a HistorySnapshot,
    ) -> Result<Self> {
        if target.history_kind != HistoryKind::Song || snapshot.kind != HistoryKind::Song {
            return Err(Error::InvalidInput("history song download requires songs"));
        }
        if target.revision != snapshot.revision {
            return Err(Error::QueueChanged);
        }
        if target.selected_ids.len() > MAX_ROWS {
            return Err(Error::InvalidInput("invalid history song download size"));
        }
        let selection = row_menu::resolve(snapshot, &target.clicked_id, &target.selected_ids)?;
        Self::project_resolved_row(selection, snapshot)
    }

    pub(super) fn project_resolved_row(
        selection: row_menu::ResolvedRows<'a>,
        snapshot: &'a HistorySnapshot,
    ) -> Result<Self> {
        if selection.selected.len() > MAX_ROWS {
            return Err(Error::InvalidInput("invalid history song download size"));
        }
        let anchor = selection.clicked;
        let selected = if selection.selected.is_empty() {
            vec![anchor]
        } else {
            selection.selected
        };
        Ok(Self {
            anchor,
            rows: selected
                .into_iter()
                .map(|record| HistorySongDownloadRow {
                    record,
                    source: row_source(record),
                })
                .collect(),
            snapshot,
        })
    }

    pub(in crate::client) fn project(
        target: &HistorySongDownloadTarget,
        snapshot: &'a HistorySnapshot,
    ) -> Result<Self> {
        if snapshot.kind != HistoryKind::Song {
            return Err(Error::InvalidInput("history song download requires songs"));
        }
        let rows: Vec<_> = target
            .ordered_rows(snapshot, MAX_ROWS)?
            .into_iter()
            .map(|record| HistorySongDownloadRow {
                record,
                source: source(record),
            })
            .collect();
        if rows
            .iter()
            .map(|row| row.record.id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != rows.len()
        {
            return Err(Error::QueueChanged);
        }
        // M3102:1876–1908 takes the first selected row as resource. M3085
        // still uses that original row after removing completed/local/blocked
        // rows from the download list; a surviving row cannot replace it.
        let anchor = rows.first().ok_or(Error::QueueChanged)?.record;
        Ok(Self {
            anchor,
            rows,
            snapshot,
        })
    }

    /// The history button puts selectedData in its own from. M3085:3475–3488
    /// keeps it in the picker even though resource contains the download
    /// survivors. M3085:764–783 then filters the original selectedData, and
    /// M3160:1061 restores the collection order. This does not reorder the
    /// separate download list or promote an omitted row into its anchor.
    pub(in crate::client) fn playlist_song_ids(&self) -> Result<Vec<String>> {
        let target = HistoryCollectTarget::Selected {
            revision: self.snapshot.revision.clone(),
            ids: self.rows.iter().map(|row| row.record.id.clone()).collect(),
        };
        let (_, mut ids) = target.project(self.snapshot)?;
        // An unsupported row can still affect the official preferred-filter
        // decision, but its identity must never enter a write. A downloaded
        // row needs an explicit native ledger proof before any normalization.
        let ordinary: BTreeSet<_> = self
            .rows
            .iter()
            .filter(|row| row.source == HistorySongDownloadSource::Catalog)
            .map(|row| row.record.id.as_str())
            .collect();
        ids.retain(|id| ordinary.contains(id.as_str()));
        Ok(ids)
    }

    /// The row picker receives only the surviving resource array. A row
    /// wrapped around an exact completed Song task may be filtered but must
    /// never be rewritten based solely on the renderer or on a file name.
    pub(in crate::client) fn row_download_candidates(&self) -> Vec<(String, String)> {
        self.rows
            .iter()
            .map(|row| row.record)
            .chain(std::iter::once(self.anchor))
            .filter_map(|row| match row_source(row) {
                HistorySongDownloadSource::DownloadTask(task) => {
                    Some((row.id.clone(), task.to_owned()))
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(in crate::client) fn row_anchor_source(&self) -> HistorySongDownloadSource<'a> {
        row_source(self.anchor)
    }

    pub(super) fn row_copyright_blocked(&self) -> bool {
        self.rows.iter().all(|row| {
            matches!(
                row.source,
                HistorySongDownloadSource::Catalog | HistorySongDownloadSource::DownloadTask(_)
            ) && row_menu::song_copyright_blocked(row.record) == Some(true)
        })
    }
}

pub(in crate::client) fn row_hidden(
    library: &LocalLibrary,
    session: &AccountSession,
    owner: &SessionToken,
    song: &str,
) -> Result<bool> {
    let store = session.persistent_store()?;
    let complete = session.with_token(owner, |uid| {
        store.read_snapshot(|tx| {
            Ok(tx
                .playback_download_rows(uid, song, PlaybackDownloadKind::Song)?
                .iter()
                .any(|task| task.status == "complete"))
        })
    })?;
    Ok(complete
        || !library
            .matched_online_song_ids(session, owner, &[song].into_iter().collect())?
            .is_empty())
}

pub(super) fn row_source(row: &HistoryRecord) -> HistorySongDownloadSource<'_> {
    let song = &row.resource;
    if song["fileId"].as_str().is_some_and(|id| !id.is_empty())
        || song["fileId"].as_u64().is_some_and(|id| id != 0)
    {
        return HistorySongDownloadSource::Local;
    }
    if ["songType", "t"]
        .into_iter()
        .any(|key| song[key].as_i64() == Some(1) || song[key].as_str() == Some("1"))
    {
        return HistorySongDownloadSource::Cloud;
    }
    source(row)
}

fn any_truthy(resource: &Value, keys: &[&str]) -> bool {
    keys.iter().any(|key| js_truthy(&resource[*key]))
}

pub(super) fn source(row: &HistoryRecord) -> HistorySongDownloadSource<'_> {
    use HistorySongDownloadSource as Source;
    let song = &row.resource;
    if row.id.starts_with("local:")
        || !song["local_id"].is_null()
        || !song["local_revision"].is_null()
        || any_truthy(song, &["isLocal", "localTrack", "audioFile"])
    {
        return Source::Local;
    }
    if any_truthy(
        song,
        &[
            "cloudSong",
            "cloud_song_id",
            "cloudSongId",
            "cs",
            "_cloud_public_id",
            "_cloud_catalog_id",
        ],
    ) || any_truthy(&song["privilege"], &["cloudSong", "cs"])
    {
        return Source::Cloud;
    }
    if identifier(&song["id"]).ok().as_deref() != Some(row.id.as_str())
        || !song["offlineId"].is_null()
        || !song["onlineTrackId"].is_null()
        || any_truthy(
            song,
            &[
                "download_voice_id",
                "voice",
                "voice_id",
                "voiceId",
                "djProgramId",
                "programId",
                "radio",
                "audio",
                "mainTrackId",
                "filename",
                "onlineTrack",
                "isOfflineDownOnline",
            ],
        )
    {
        return Source::Unsupported;
    }
    match &song["download_task_id"] {
        Value::Null => Source::Catalog,
        Value::String(task) if task.len() == 32 && task.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Source::DownloadTask(task)
        }
        _ => Source::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song(id: &str, mut raw: Value) -> HistoryRecord {
        raw["id"] = json!(id);
        HistoryRecord::local(HistoryKind::Song, raw, 100).unwrap()
    }

    fn snapshot(entries: Vec<HistoryRecord>) -> HistorySnapshot {
        HistorySnapshot {
            kind: HistoryKind::Song,
            revision: "displayed".into(),
            sync_enabled: false,
            local: Vec::new(),
            remote: None,
            entries,
        }
    }

    fn selected(ids: &[&str]) -> HistorySongDownloadTarget {
        HistorySelectionScope {
            revision: "displayed".into(),
            ids: ids.iter().map(|id| (*id).into()).collect(),
        }
    }

    #[test]
    fn history_download_row_does_not_promote_or_append_its_independent_clicked_anchor() {
        let history = snapshot(vec![
            song("9", json!({})),
            song("7", json!({"privilege":{"st":-1}})),
            song("5", json!({"privilege":{"status":-100}})),
        ]);
        let mut target = HistoryRowMenuTarget {
            history_kind: HistoryKind::Song,
            revision: history.revision.clone(),
            clicked_id: "9".into(),
            selected_ids: vec!["5".into(), "7".into()],
        };
        let projected = HistorySongDownloadProjection::project_row(&target, &history).unwrap();
        assert_eq!(projected.anchor.id, "9");
        assert!(
            projected
                .rows
                .iter()
                .map(|row| row.record.id.as_str())
                .eq(["5", "7"])
        );
        assert!(projected.row_copyright_blocked());
        target.selected_ids.clear();
        let projected = HistorySongDownloadProjection::project_row(&target, &history).unwrap();
        assert!(!projected.row_copyright_blocked());
        assert_eq!(projected.rows[0].record.id, "9");
    }

    #[test]
    fn history_download_keeps_the_original_anchor_and_snapshot_order() {
        let mut history = snapshot(vec![
            song("9", json!({})),
            song("7", json!({"privilege":{"st":-1}})),
            song("5", json!({})),
        ]);
        let projection =
            HistorySongDownloadProjection::project(&selected(&["7", "5"]), &history).unwrap();
        assert_eq!(projection.anchor.id, "7");
        assert!(
            projection
                .rows
                .iter()
                .map(|row| row.record.id.as_str())
                .eq(["7", "5"])
        );
        assert!(
            projection
                .rows
                .iter()
                .all(|row| row.source == HistorySongDownloadSource::Catalog)
        );
        let only = HistorySongDownloadProjection::project(&selected(&["5"]), &history).unwrap();
        assert_eq!(only.anchor.id, "5");
        for ids in [&["5", "7"][..], &["7", "7"], &["7", "6"]] {
            assert!(matches!(
                HistorySongDownloadProjection::project(&selected(ids), &history),
                Err(Error::QueueChanged)
            ));
        }
        history.revision = "newer".into();
        assert!(matches!(
            HistorySongDownloadProjection::project(&selected(&["7"]), &history),
            Err(Error::QueueChanged)
        ));
        history.kind = HistoryKind::Voice;
        assert!(matches!(
            HistorySongDownloadProjection::project(&selected(&["7"]), &history),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn history_download_distinguishes_sources_without_granting_any_capability() {
        use HistorySongDownloadSource as Source;
        const TASK: &str = "0123456789abcdef0123456789abcdef";
        let history = snapshot(vec![
            song("9", json!({"local_id":"file","local_revision":"seen"})),
            song(
                "8",
                json!({"privilege":{"cs":true},"_cloud_public_id":"70"}),
            ),
            song("7", json!({"download_task_id":TASK})),
            song("6", json!({"download_task_id":"wrong"})),
            song("5", json!({"offlineId":"90","onlineTrackId":"91"})),
            song("4", json!({"radio":{"id":"92"}})),
            song("0003", json!({})),
        ]);
        let target = selected(&["local:file", "8", "7", "6", "5", "4", "0003"]);
        let projection = HistorySongDownloadProjection::project(&target, &history).unwrap();
        assert_eq!(projection.anchor.id, "local:file");
        assert!(projection.rows.iter().map(|row| row.source).eq([
            Source::Local,
            Source::Cloud,
            Source::DownloadTask(TASK),
            Source::Unsupported,
            Source::Unsupported,
            Source::Unsupported,
            Source::Catalog,
        ]));
    }

    #[test]
    fn history_download_picker_uses_original_selection_and_two_favorite_filters() {
        let history = snapshot(vec![
            song("9", json!({"privilege":{"subp":0}})),
            // Negative download status can remove this from media admission;
            // the independent favorite filter still sees its original subp.
            song("7", json!({"privilege":{"st":-1,"subp":2}})),
            song("5", json!({"privilege":{}})),
            song("3", json!({"audioFile":true,"privilege":{"subp":2}})),
        ]);
        let projection =
            HistorySongDownloadProjection::project(&selected(&["9", "7", "5", "3"]), &history)
                .unwrap();
        assert_eq!(projection.playlist_song_ids().unwrap(), ["7", "5"]);
    }

    #[test]
    fn history_download_picker_does_not_replace_an_unsupported_fallback() {
        let history = snapshot(vec![
            song("9", json!({"privilege":{"subp":-1}})),
            song("7", json!({"privilege":{"subp":0}})),
            song("5", json!({"privilege":{"subp":null}})),
        ]);
        let projection =
            HistorySongDownloadProjection::project(&selected(&["9", "7", "5"]), &history).unwrap();
        assert!(projection.playlist_song_ids().unwrap().is_empty());
        let projection =
            HistorySongDownloadProjection::project(&selected(&["7", "5"]), &history).unwrap();
        assert_eq!(projection.playlist_song_ids().unwrap(), ["7"]);
    }
}
