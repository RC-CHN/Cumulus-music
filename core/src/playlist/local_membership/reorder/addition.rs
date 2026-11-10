//! Ordinary catalogue favourite into an already mixed device playlist.
use super::*;

pub(super) fn validate_selection(ids: &[String]) -> Result<()> {
    if !(1..=GuardedPlaylistAdd::MAX_SONGS).contains(&ids.len()) || online_set(ids).is_none() {
        return Err(Error::InvalidInput(
            "invalid mixed playlist addition selection",
        ));
    }
    Ok(())
}

pub(super) fn valid(intent: &Intent, selected: &[String]) -> bool {
    let Some(before) = online_set(&intent.before_online) else {
        return false;
    };
    !intent.desired_local.is_empty()
        && validate_selection(selected).is_ok()
        && selected.iter().all(|id| !before.contains(id.as_str()))
        && intent.desired_online.len() <= MAX_REMOTE
        && selected
            .iter()
            .chain(&intent.before_online)
            .eq(&intent.desired_online)
        && intent
            .desired_local
            .iter()
            .all(|row| row.position >= selected.len())
}

pub(super) fn prepend(
    rows: Vec<PlaylistLocalRow>,
    online: &[String],
    selected: &[String],
) -> Result<(Vec<String>, Vec<LocatedLocal>)> {
    let existing = online.iter().map(String::as_str).collect::<HashSet<_>>();
    if selected.iter().any(|id| existing.contains(id.as_str())) {
        return Err(Error::InvalidInput(
            "mixed playlist addition requires new catalog songs",
        ));
    }
    // M/3160's success listener prepends selected rows to the previous full
    // playlist. Preserve every old device row after that entire new prefix.
    split(
        selected
            .iter()
            .cloned()
            .map(|id| PlaylistMixedOrderRow::Online { id })
            .chain(
                rows.into_iter()
                    .map(|row| identity(row, online))
                    .collect::<Result<Vec<_>>>()?,
            )
            .collect(),
        true,
    )
}

impl PlaylistLocalStore {
    pub(crate) fn has_device_members(&self, token: &SessionToken, id: &str) -> Result<bool> {
        identifier(&json!(id))?;
        self.session
            .with_token(token, |owner| has_entries(&self.store, owner, id))
    }

    pub(crate) async fn mixed_add_target(
        &self,
        token: &SessionToken,
        id: &str,
        selection: &[String],
    ) -> Result<Option<PlaylistMixedAddTarget>> {
        identifier(&json!(id))?;
        validate_selection(selection)?;
        let (owner, has_members) = self.session.with_token(token, |owner| {
            Ok((owner.to_owned(), has_entries(&self.store, owner, id)?))
        })?;
        if !has_members {
            return Ok(None);
        }
        let pending = self.session.with_token(token, |owner| {
            self.store
                .get(owner, ADDITIONS, id)?
                .map(|value| Intent::parse(owner, id, EditKind::Add, value))
                .transpose()
        })?;
        if let Some(intent) = pending.filter(|row| row.phase.pending()) {
            let current_selection = intent.addition.as_deref() == Some(selection);
            return Ok(Some(PlaylistMixedAddTarget::Pending {
                receipt: intent.receipt(),
                current_selection,
            }));
        }
        let (detail, _) = self.fresh_mixed_detail(token, id, EditKind::Add).await?;
        let remote = Remote::read(&owner, id, &detail)?;
        require_owner(&owner, &remote, &detail, EditKind::Add)?;
        let ids = online_ids(&detail, EditKind::Add)?;
        if online_set(&ids).is_none() {
            return Err(Error::Response("invalid complete mixed playlist IDs"));
        }
        let membership = self.project(token, id, &detail)?;
        if !membership
            .rows
            .iter()
            .any(|row| matches!(row, PlaylistLocalRow::Local(_)))
        {
            return Err(Error::QueueChanged);
        }
        Ok(Some(PlaylistMixedAddTarget::Ready {
            revision: membership.revision,
            operation_id: uuid::Uuid::new_v4().simple().to_string(),
        }))
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaylistMixedAddTarget {
    Ready {
        revision: String,
        #[serde(rename = "operationId")]
        operation_id: String,
    },
    Pending {
        receipt: PlaylistMixedAddReceipt,
        #[serde(rename = "currentSelection")]
        current_selection: bool,
    },
}
