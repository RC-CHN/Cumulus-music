//! M106/M195 pass the table's gesture order to M3085 separately from its anchor.
use crate::{
    Error, Result,
    history::{HistoryKind, HistoryRecord, HistorySnapshot},
};
use std::collections::{HashMap, HashSet};

pub(super) struct ResolvedRows<'a> {
    pub clicked: &'a HistoryRecord,
    pub selected: Vec<&'a HistoryRecord>,
}

/// M106 keeps the existing table selection on modifier right-click. Bind
/// both identities here; playback additionally requires its clicked anchor
/// to be a member of a multi-selection.
pub(super) fn resolve<'a>(
    snapshot: &'a HistorySnapshot,
    clicked: &str,
    selected: &[String],
) -> Result<ResolvedRows<'a>> {
    if !matches!(snapshot.kind, HistoryKind::Song | HistoryKind::Voice)
        || clicked.is_empty()
        || selected.len() == 1
        || selected.len() > 50_300
    {
        return Err(Error::InvalidInput("invalid history row menu selection"));
    }
    let available: HashMap<_, _> = snapshot
        .entries
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect();
    let clicked = *available.get(clicked).ok_or(Error::QueueChanged)?;
    let mut seen = HashSet::with_capacity(selected.len());
    let selected = selected
        .iter()
        .map(|id| {
            if !seen.insert(id) {
                return Err(Error::InvalidInput("duplicate history row menu selection"));
            }
            available
                .get(id.as_str())
                .copied()
                .ok_or(Error::QueueChanged)
        })
        .collect::<Result<_>>()?;
    Ok(ResolvedRows { clicked, selected })
}

pub(super) fn project(
    snapshot: HistorySnapshot,
    clicked: &str,
    selected: &[String],
) -> Result<(Vec<HistoryRecord>, usize)> {
    let selection = resolve(&snapshot, clicked, selected)?;
    let (rows, anchor) = if selection.selected.is_empty() {
        (vec![selection.clicked], 0)
    } else {
        let anchor = selection
            .selected
            .iter()
            .position(|row| row.id == selection.clicked.id)
            .ok_or(Error::InvalidInput(
                "history row menu anchor is outside selection",
            ))?;
        (selection.selected, anchor)
    };
    // M556:96–100,134–139 uses the displayed rights for menu enablement; it
    // disables the group only when every chosen track is copyright-down.
    if snapshot.kind == HistoryKind::Song
        && rows
            .iter()
            .all(|row| song_copyright_blocked(row) == Some(true))
    {
        return Err(Error::State(
            "history row menu selection is copyright blocked",
        ));
    }
    Ok((rows.into_iter().cloned().collect(), anchor))
}

pub(super) fn song_copyright_blocked(row: &HistoryRecord) -> Option<bool> {
    if row.resource["local_id"].is_string() {
        return None;
    }
    let program = &row.resource["programId"];
    if program.as_u64().is_some_and(|id| id != 0)
        || program.as_str().is_some_and(|id| !id.is_empty())
    {
        return None;
    }
    let rights = &row.resource["privilege"];
    let status = rights.get("status").unwrap_or(&rights["st"]);
    status
        .as_i64()
        .or_else(|| status.as_str()?.parse().ok())
        .map(|status| status < 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snapshot() -> HistorySnapshot {
        HistorySnapshot {
            kind: HistoryKind::Song,
            revision: "revision".into(),
            sync_enabled: false,
            local: vec![],
            remote: None,
            entries: [("10", 0), ("20", -100), ("30", 0)]
                .into_iter()
                .map(|(id, status)| {
                    HistoryRecord::local(
                        HistoryKind::Song,
                        json!({"id":id,"privilege":{"st":status}}),
                        1,
                    )
                    .unwrap()
                })
                .collect(),
        }
    }

    #[test]
    fn row_menu_uses_only_clicked_or_the_gesture_order_and_keeps_the_anchor() {
        let (rows, anchor) = project(snapshot(), "10", &[]).unwrap();
        assert_eq!(rows[0].id, "10");
        assert_eq!(anchor, 0);
        let selected = ["30".into(), "20".into(), "10".into()];
        let (rows, anchor) = project(snapshot(), "20", &selected).unwrap();
        assert!(rows.iter().map(|row| &row.id).eq(&selected));
        assert_eq!(anchor, 1);
        // The mixed group reaches the separate native anchor check; a
        // single definitive copyright-down row never opens this action.
        assert!(matches!(
            project(snapshot(), "20", &[]),
            Err(Error::State(_))
        ));
    }

    #[test]
    fn row_menu_rejects_unknown_duplicate_or_unbound_ids() {
        assert!(matches!(
            project(snapshot(), "10", &["10".into(), "99".into()]),
            Err(Error::QueueChanged)
        ));
        for ids in [
            vec!["20".into(), "30".into()],
            vec!["10".into(), "10".into()],
            vec!["10".into()],
        ] {
            assert!(matches!(
                project(snapshot(), "10", &ids),
                Err(Error::InvalidInput(_))
            ));
        }
    }
}
