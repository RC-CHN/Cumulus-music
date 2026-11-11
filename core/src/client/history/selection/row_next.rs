//! M106/M195 pass the table's gesture order to M3085 separately from its anchor.
use crate::{
    Error, Result,
    history::{HistoryKind, HistoryRecord, HistorySnapshot},
};
use std::collections::{HashMap, HashSet};

pub(super) fn project(
    snapshot: HistorySnapshot,
    clicked: &str,
    selected: &[String],
) -> Result<(Vec<HistoryRecord>, usize)> {
    if !matches!(snapshot.kind, HistoryKind::Song | HistoryKind::Voice)
        || clicked.is_empty()
        || selected.len() == 1
        || selected.len() > 50_300
    {
        return Err(Error::InvalidInput("invalid history row next selection"));
    }
    let mut available: HashMap<_, _> = snapshot
        .entries
        .into_iter()
        .map(|row| (row.id.clone(), row))
        .collect();
    let (rows, anchor) = if selected.is_empty() {
        (
            vec![available.remove(clicked).ok_or(Error::QueueChanged)?],
            0,
        )
    } else {
        let anchor = selected
            .iter()
            .position(|id| id == clicked)
            .ok_or(Error::InvalidInput(
                "history row next anchor is outside selection",
            ))?;
        let mut seen = HashSet::with_capacity(selected.len());
        let rows = selected
            .iter()
            .map(|id| {
                if !seen.insert(id) {
                    return Err(Error::InvalidInput("duplicate history row next selection"));
                }
                available.remove(id).ok_or(Error::QueueChanged)
            })
            .collect::<Result<Vec<_>>>()?;
        (rows, anchor)
    };
    // M556:134–139 uses the displayed rights for menu enablement; it
    // disables the group only when every chosen track is copyright-down.
    if snapshot.kind == HistoryKind::Song && rows.iter().all(copyright_down) {
        return Err(Error::State(
            "history row next selection is copyright blocked",
        ));
    }
    Ok((rows, anchor))
}

fn copyright_down(row: &HistoryRecord) -> bool {
    if row.resource["local_id"].is_string() {
        return false;
    }
    let rights = &row.resource["privilege"];
    let status = rights.get("status").unwrap_or(&rights["st"]);
    status
        .as_i64()
        .or_else(|| status.as_str()?.parse().ok())
        .is_some_and(|status| status < 0)
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
    fn next_uses_only_clicked_or_the_gesture_order_and_keeps_the_anchor() {
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
    fn next_rejects_unknown_duplicate_or_unbound_ids() {
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
