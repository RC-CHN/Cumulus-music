//! Project the history snapshot into the ordinary PC favorite orders.
use super::{HistorySelectionScope, row_menu};
use crate::{
    Error, Result,
    history::{HistoryKind, HistoryRecord, HistorySnapshot},
    protocol::{
        api::identifier,
        wire::serialization::{js_truthy, trim_js},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

const MAX_HISTORY_COLLECTION: usize = 600;

/// Only row identities from the same native history snapshot are accepted.
/// The context menu keeps its clicked row separate from the table selection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoryCollectTarget {
    All {
        revision: String,
    },
    Selected {
        revision: String,
        ids: Vec<String>,
    },
    Row {
        revision: String,
        clicked_id: String,
        selected_ids: Vec<String>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Privilege {
    Undefined,
    Falsey,
    Positive,
    Denied,
    Unknown,
}

impl Privilege {
    fn preferred(self) -> bool {
        matches!(self, Self::Undefined | Self::Positive)
    }

    fn allowed(self) -> bool {
        self.preferred() || self == Self::Falsey
    }
}

fn privilege(track: &Value) -> Privilege {
    // The native history mapper can drop the entire privilege object, and
    // the remote join stores null when its result is missing. Neither means
    // a verified row intentionally omitted subp/subPriv.
    let Some(row) = track.get("privilege").and_then(Value::as_object) else {
        return Privilege::Unknown;
    };
    if row
        .get("id")
        .is_some_and(|id| !id.is_null() && identifier(id).ok() != identifier(&track["id"]).ok())
    {
        return Privilege::Unknown;
    }
    // M/21:362 maps subp into subPriv. Retain absence and explicit null/0
    // separately, and do not choose between conflicting stored aliases.
    let raw = row.get("subp");
    let normalized = row.get("subPriv");
    if raw.zip(normalized).is_some_and(|(a, b)| a != b) {
        return Privilege::Unknown;
    }
    let Some(value) = raw.or(normalized) else {
        return Privilege::Undefined;
    };
    if !js_truthy(value) {
        return Privilege::Falsey;
    }
    match value
        .as_f64()
        .or_else(|| trim_js(value.as_str()?).parse().ok())
        .filter(|number| number.is_finite())
    {
        Some(number) if number > 0.0 => Privilege::Positive,
        Some(_) => Privilege::Denied,
        None => Privilege::Unknown,
    }
}

fn catalog_identity(row: &HistoryRecord, row_menu: bool) -> Option<&str> {
    let track = &row.resource;
    // History local origin is device provenance. Only the typed local
    // marker identifies a path-based row. A download task ID alone is
    // ambiguous: history did not retain its native Song/Cloud origin.
    if ["local_id", "local_revision", "offlineId", "onlineTrackId"]
        .into_iter()
        .any(|key| track.get(key).is_some_and(|value| !value.is_null()))
        || [
            "isLocal",
            "cloudSong",
            "cloud_song_id",
            "cloudSongId",
            "cs",
            "_cloud_public_id",
            "_cloud_catalog_id",
            "voice",
            "voice_id",
            "voiceId",
            "djProgramId",
            "programId",
            "mainTrackId",
        ]
        .into_iter()
        .any(|key| js_truthy(&track[key]))
        || ["cloudSong", "cs"]
            .into_iter()
            .any(|key| js_truthy(&track["privilege"][key]))
        // M213:73 treats an explicit fileId as local, and M556:165
        // excludes the private uploaded song type. These checks are for
        // the row menu; established header and batch policy stays intact.
        || row_menu
            && (track["fileId"].as_str().is_some_and(|id| !id.is_empty())
                || track["fileId"].as_u64().is_some_and(|id| id != 0)
                || ["songType", "t"].into_iter().any(|key| {
                    track[key].as_i64() == Some(1) || track[key].as_str() == Some("1")
                }))
    {
        return None;
    }
    (identifier(&track["id"]).ok()?.as_str() == row.id.as_str()).then_some(row.id.as_str())
}

fn catalog_id(row: &HistoryRecord, row_menu: bool) -> Option<&str> {
    if !row.resource["download_task_id"].is_null() {
        return None;
    }
    catalog_identity(row, row_menu)
}

impl HistoryCollectTarget {
    fn is_row_menu(&self) -> bool {
        matches!(self, Self::Row { .. })
    }

    pub(in crate::client) fn revision(&self) -> &str {
        match self {
            Self::All { revision }
            | Self::Selected { revision, .. }
            | Self::Row { revision, .. } => revision,
        }
    }

    fn rows<'a>(&self, snapshot: &'a HistorySnapshot) -> Result<Vec<&'a HistoryRecord>> {
        if snapshot.kind != HistoryKind::Song {
            return Err(Error::InvalidInput("history collection requires songs"));
        }
        if snapshot.revision != self.revision() {
            return Err(Error::QueueChanged);
        }
        let rows = match self {
            Self::All { .. } => {
                if !(1..=MAX_HISTORY_COLLECTION).contains(&snapshot.entries.len()) {
                    return Err(Error::InvalidInput("invalid history collection size"));
                }
                snapshot.entries.iter().collect()
            }
            Self::Selected { revision, ids } => HistorySelectionScope {
                revision: revision.clone(),
                ids: ids.clone(),
            }
            .ordered_rows(snapshot, MAX_HISTORY_COLLECTION)?,
            Self::Row {
                clicked_id,
                selected_ids,
                ..
            } => {
                if selected_ids.len() > MAX_HISTORY_COLLECTION {
                    return Err(Error::InvalidInput("invalid history collection size"));
                }
                let row = row_menu::resolve(snapshot, clicked_id, selected_ids)?;
                if row.selected.is_empty() {
                    vec![row.clicked]
                } else {
                    // M3085 uses the table's Set insertion order. The
                    // clicked row can be outside a retained modifier group;
                    // it is checked but must not enter that group's write.
                    row.selected
                }
            }
        };
        if rows
            .iter()
            .map(|row| &row.id)
            .collect::<BTreeSet<_>>()
            .len()
            != rows.len()
        {
            return Err(Error::QueueChanged);
        }
        Ok(rows)
    }

    pub(in crate::client) fn unobserved_privilege_ids(
        &self,
        snapshot: &HistorySnapshot,
    ) -> Result<Vec<String>> {
        Ok(self
            .rows(snapshot)?
            .into_iter()
            .filter_map(|row| {
                (row.resource["privilege"].is_null())
                    .then(|| catalog_id(row, self.is_row_menu()))
                    .flatten()
                    .map(str::to_owned)
            })
            .collect())
    }

    pub(in crate::client) fn download_candidates(
        &self,
        snapshot: &HistorySnapshot,
    ) -> Result<Vec<(String, String)>> {
        Ok(self
            .rows(snapshot)?
            .into_iter()
            .filter_map(|row| {
                let song = catalog_identity(row, self.is_row_menu())?;
                let task = row.resource["download_task_id"].as_str()?;
                (task.len() == 32 && task.bytes().all(|digit| digit.is_ascii_hexdigit()))
                    .then(|| (song.to_owned(), task.to_owned()))
            })
            .collect())
    }

    pub(in crate::client) fn project(
        &self,
        snapshot: &HistorySnapshot,
    ) -> Result<(usize, Vec<String>)> {
        let rows = self.rows(snapshot)?;
        let count = rows.len();
        let rows: Vec<_> = rows
            .into_iter()
            .map(|row| {
                (
                    catalog_id(row, self.is_row_menu()),
                    privilege(&row.resource),
                )
            })
            .collect();
        let eligible = |(id, access): &(Option<&str>, Privilege)| {
            id.filter(|_| access.allowed()).map(str::to_owned)
        };
        let ids = match self {
            // Header Favorite passes the complete array as resource (M/3102
            // :1645–1653). The popup's native add uses the reversed array and
            // only the falsey-or-positive test; it has no selectedData fallback.
            Self::All { .. } => rows.iter().rev().filter_map(eligible).collect(),
            // M3085:767–775 has no selectedData to filter for a single row.
            // Only the second subPriv test is applied to the clicked row.
            Self::Row { selected_ids, .. } if selected_ids.is_empty() => {
                rows.iter().filter_map(eligible).collect()
            }
            Self::Selected { .. } | Self::Row { .. } => {
                // M/3085:768–792 first checks undefined or positive. Unsupported
                // rows still occupy their original position. A known preferred
                // row or unknown privilege must not manufacture a fallback.
                if rows
                    .iter()
                    .any(|(_, access)| access.preferred() || *access == Privilege::Unknown)
                {
                    rows.iter()
                        .filter(|(_, access)| access.preferred())
                        .filter_map(eligible)
                        .collect()
                } else {
                    // Only original first, never the first eligible substitute.
                    rows.first().and_then(eligible).into_iter().collect()
                }
            }
        };
        Ok((count, ids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song(id: &str, raw: Value) -> HistoryRecord {
        let mut resource = raw;
        resource["id"] = json!(id);
        HistoryRecord::local(HistoryKind::Song, resource, 100).unwrap()
    }

    fn snapshot(entries: Vec<HistoryRecord>) -> HistorySnapshot {
        HistorySnapshot {
            kind: HistoryKind::Song,
            revision: "visible".into(),
            sync_enabled: false,
            local: Vec::new(),
            remote: None,
            entries,
        }
    }

    fn all() -> HistoryCollectTarget {
        HistoryCollectTarget::All {
            revision: "visible".into(),
        }
    }

    fn selected(ids: &[&str]) -> HistoryCollectTarget {
        HistoryCollectTarget::Selected {
            revision: "visible".into(),
            ids: ids.iter().map(|id| (*id).into()).collect(),
        }
    }

    fn row(clicked: &str, ids: &[&str]) -> HistoryCollectTarget {
        HistoryCollectTarget::Row {
            revision: "visible".into(),
            clicked_id: clicked.into(),
            selected_ids: ids.iter().map(|id| (*id).into()).collect(),
        }
    }

    #[test]
    fn header_reverses_full_order_and_selected_keeps_only_first_filter_in_order() {
        let rows = snapshot(vec![
            song("1", json!({"privilege":{"id":1}})),
            song("2", json!({"privilege":{"subp":null}})),
            song("3", json!({"privilege":{"subPriv":0}})),
            song("4", json!({"privilege":{"subp":-1}})),
            song("5", json!({"privilege":{"subPriv":"0"}})),
            song("6", json!({"privilege":{"subp":2,"subPriv":2}})),
        ]);
        assert_eq!(
            all().project(&rows).unwrap(),
            (6, ["6", "3", "2", "1"].map(str::to_owned).to_vec())
        );
        assert_eq!(
            selected(&["1", "2", "3", "4", "5", "6"])
                .project(&rows)
                .unwrap(),
            (6, ["1", "6"].map(str::to_owned).to_vec()),
        );
    }

    #[test]
    fn selected_fallback_uses_only_the_original_first_row() {
        let rows = snapshot(vec![
            song("1", json!({"privilege":{"subp":null}})),
            song("2", json!({"privilege":{"subp":-1}})),
            song("3", json!({"privilege":{"subp":0}})),
        ]);
        assert_eq!(
            selected(&["1", "2", "3"]).project(&rows).unwrap(),
            (3, vec!["1".to_owned()])
        );
        assert_eq!(
            selected(&["2", "3"]).project(&rows).unwrap(),
            (2, Vec::<String>::new())
        );
    }

    #[test]
    fn history_row_collect_singleton_only_applies_the_second_privilege_test() {
        let rows = snapshot(vec![
            song("1", json!({"privilege":{"st":-100,"subp":0}})),
            song("2", json!({"privilege":{"subPriv":-1}})),
            song("3", json!({"privilege":{"subp":null}})),
            song("4", json!({"privilege":{"subp":"0"}})),
            song("5", json!({"privilege":null})),
            song("6", json!({"songType":1,"privilege":{"subp":2}})),
            song("7", json!({"local_id":"file","privilege":{"subp":2}})),
        ]);
        for id in ["1", "3"] {
            assert_eq!(
                row(id, &[]).project(&rows).unwrap(),
                (1, vec![id.to_owned()])
            );
        }
        for id in ["2", "4", "5", "6", "local:file"] {
            assert_eq!(
                row(id, &[]).project(&rows).unwrap(),
                (1, Vec::<String>::new())
            );
        }
    }

    #[test]
    fn history_row_collect_keeps_gesture_order_and_does_not_promote_clicked() {
        let rows = snapshot(vec![
            song("1", json!({"privilege":{"subp":2}})),
            song("2", json!({"privilege":{"subp":0}})),
            song("3", json!({"privilege":{"subp":-1}})),
            song("4", json!({"privilege":{}})),
            song("5", json!({"privilege":{"subp":null}})),
            song("6", json!({"fileId":"local-match","privilege":{"subp":2}})),
        ]);
        assert_eq!(
            row("2", &["4", "2", "1"]).project(&rows).unwrap(),
            (3, ["4", "1"].map(str::to_owned).to_vec())
        );
        assert_eq!(
            row("1", &["5", "2", "3"]).project(&rows).unwrap(),
            (3, vec!["5".to_owned()])
        );
        assert_eq!(
            row("1", &["3", "2"]).project(&rows).unwrap(),
            (2, Vec::<String>::new())
        );
        assert_eq!(
            row("1", &["5", "6"]).project(&rows).unwrap(),
            (2, Vec::<String>::new())
        );
        assert!(matches!(
            row("99", &["5", "2"]).project(&rows),
            Err(Error::QueueChanged)
        ));
        for ids in [&["2"][..], &["2", "2"]] {
            assert!(matches!(
                row("1", ids).project(&rows),
                Err(Error::InvalidInput(_))
            ));
        }
    }

    #[test]
    fn unsupported_preferred_rows_cannot_promote_a_catalog_fallback() {
        let local = song("42", json!({"local_id":"42","privilege":{"subp":2}}));
        assert_eq!(local.id, "local:42");
        let rows = snapshot(vec![
            song("1", json!({"privilege":{"subp":0}})),
            local,
            song("3", json!({"download_task_id":"ambiguous","privilege":{}})),
            song("4", json!({"privilege":{"cs":true,"subp":2}})),
            song("5", json!({"privilege":{"subp":2,"subPriv":0}})),
            song("6", json!({"privilege":null})),
            song("7", json!({})),
        ]);
        assert_eq!(all().project(&rows).unwrap(), (7, vec!["1".to_owned()]));
        for id in ["local:42", "3", "4", "5", "6", "7"] {
            assert_eq!(
                selected(&["1", id]).project(&rows).unwrap(),
                (2, Vec::<String>::new())
            );
        }
    }

    #[test]
    fn projection_rejects_stale_non_song_invalid_order_and_unbounded_selection() {
        let mut rows = snapshot(vec![
            song("1", json!({"privilege":{}})),
            song("2", json!({"privilege":{}})),
        ]);
        for ids in [["2", "1"], ["1", "1"], ["1", "99"]] {
            assert!(matches!(
                selected(&ids).project(&rows),
                Err(Error::QueueChanged)
            ));
        }
        assert!(matches!(
            selected(&[]).project(&rows),
            Err(Error::InvalidInput(_))
        ));
        rows.revision = "changed".into();
        assert!(matches!(all().project(&rows), Err(Error::QueueChanged)));
        rows.kind = HistoryKind::Voice;
        assert!(matches!(all().project(&rows), Err(Error::InvalidInput(_))));
        let oversized = snapshot(
            (1..=601)
                .map(|id| song(&id.to_string(), json!({"privilege":{}})))
                .collect(),
        );
        assert!(matches!(
            all().project(&oversized),
            Err(Error::InvalidInput(_))
        ));
    }
}
