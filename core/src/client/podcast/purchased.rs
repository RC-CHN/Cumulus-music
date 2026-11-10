use super::{MusicClient, dto::*, view};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    podcast::{PodcastLibrary, PodcastScope, PurchasedItem, PurchasedKind},
    protocol::api::identifier,
    storage::OperationLock,
};
use serde_json::json;
use std::sync::Arc;

pub(super) fn row(item: &PurchasedItem, liked: &[String]) -> PodcastPurchasedRow {
    match item {
        PurchasedItem::Radio(radio) => PodcastPurchasedRow::Radio(view::card(radio)),
        PurchasedItem::Episode(episode) => {
            let mut result = view::episode(episode, liked);
            result.unavailable |= episode.raw["displayStatus"]
                .as_str()
                .is_some_and(|status| !status.is_empty() && status != "NORMAL");
            PodcastPurchasedRow::Episode(result)
        }
    }
}

impl MusicClient {
    pub fn podcast_purchased_target(
        &self,
        epoch: &str,
        target: &PodcastPurchasedTarget,
    ) -> Result<crate::client::dto::AccountData<PodcastPurchasedRow>> {
        let token = self.podcast_token(epoch)?;
        let row = cached_purchased_target(&self.session, &token, target)?;
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(crate::client::dto::AccountData {
            epoch: epoch.into(),
            value: row,
        })
    }
}

pub(in crate::client) fn purchased_download_voice(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    target: &PodcastPurchasedTarget,
) -> Result<String> {
    if target.kind != PurchasedKind::Episode {
        return Err(Error::InvalidInput(
            "purchased download requires a program row",
        ));
    }
    match cached_purchased_target(session, token, target)? {
        PodcastPurchasedRow::Episode(episode) if !episode.unavailable => Ok(episode.id),
        _ => Err(Error::QueueChanged),
    }
}

pub(in crate::client) fn lock_purchased_download_voice(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    target: &PodcastPurchasedTarget,
) -> Result<OperationLock> {
    let locked = PodcastLibrary::new(session.clone())?.lock_purchases_for(token)?;
    purchased_download_voice(session, token, target)?;
    Ok(locked)
}

fn cached_purchased_target(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    target: &PodcastPurchasedTarget,
) -> Result<PodcastPurchasedRow> {
    if target.id.len() > 32
        || identifier(&json!(target.id)).is_err()
        || target.revision.len() != 64
        || !target.revision.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::InvalidInput("invalid purchased podcast target"));
    }
    let snapshot = PodcastLibrary::new(session.clone())?
        .cached_for(token, &PodcastScope::Purchased)?
        .filter(|snapshot| snapshot.revision == target.revision)
        .ok_or(Error::QueueChanged)?;
    let item = snapshot
        .purchases
        .as_ref()
        .and_then(|value| {
            value
                .items
                .iter()
                .find(|row| row.identity() == (target.kind, target.id.as_str()))
        })
        .ok_or(Error::QueueChanged)?;
    Ok(row(item, &[]))
}
