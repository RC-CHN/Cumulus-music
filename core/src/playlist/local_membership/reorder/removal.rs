//! M/3160 unfav: selected device rows never enter the remote delete request.
use super::*;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Removal {
    /// Selection order, as submitted to the service; never device IDs.
    pub(super) online: Vec<String>,
    pub(super) local: Vec<PlaylistLocalRef>,
}

impl Removal {
    pub(super) fn valid(&self, intent: &Intent) -> bool {
        let Some(before) = online_set(&intent.before_online) else {
            return false;
        };
        let Some(removed) = online_set(&self.online) else {
            return false;
        };
        let local_count = intent.desired_local.len().saturating_add(self.local.len());
        !removed.is_empty()
            && removed.is_subset(&before)
            && (1..=MAX_LOCAL).contains(&local_count)
            && unique_refs(
                intent
                    .desired_local
                    .iter()
                    .map(|row| &row.reference)
                    .chain(&self.local),
            )
            .is_some()
            && intent
                .before_online
                .iter()
                .filter(|id| !removed.contains(id.as_str()))
                .eq(&intent.desired_online)
    }
}

pub(super) fn regular_owned(owner: &str, detail: &Value) -> bool {
    if detail["specialType"].as_i64() != Some(0) {
        return false;
    }
    let owners = [&detail["userId"], &detail["creator"]["userId"]];
    owners.iter().any(|id| !id.is_null())
        && owners
            .into_iter()
            .filter(|id| !id.is_null())
            .all(|id| identifier(id).is_ok_and(|id| id == owner))
}

pub(super) fn project_removal(
    rows: Vec<PlaylistLocalRow>,
    online: &[String],
    selected: Vec<PlaylistMixedOrderRow>,
) -> Result<(Vec<String>, Vec<LocatedLocal>, Removal)> {
    let mut picked: HashSet<_> = selected.iter().collect();
    if picked.len() != selected.len() || picked.len() > rows.len() {
        return Err(Error::InvalidInput(
            "invalid mixed playlist removal selection",
        ));
    }
    let mut remaining = Vec::with_capacity(rows.len() - picked.len());
    for row in rows {
        let row = identity(row, online)?;
        if !picked.remove(&row) {
            remaining.push(row);
        }
    }
    if !picked.is_empty() {
        return Err(Error::InvalidInput("mixed removal contains an unknown row"));
    }
    drop(picked);
    let mut removal = Removal {
        online: Vec::new(),
        local: Vec::new(),
    };
    for row in selected {
        match row {
            PlaylistMixedOrderRow::Online { id } => removal.online.push(id),
            PlaylistMixedOrderRow::Local(reference) => removal.local.push(reference),
        }
    }
    if removal.online.is_empty() {
        return Err(Error::InvalidInput(
            "use local playlist removal for a device-only selection",
        ));
    }
    let (online, local) = split(remaining, false)?;
    Ok((online, local, removal))
}
