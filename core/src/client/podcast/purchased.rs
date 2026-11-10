use super::{MusicClient, dto::*, view};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    podcast::{Episode, PodcastLibrary, PodcastScope, PurchasedItem, PurchasedKind},
    protocol::api::identifier,
    storage::OperationLock,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn play_count(value: &Value) -> Option<PodcastPurchasedPlayCount> {
    // Only the bought radio's M21.Z playCount supplies this column. Keep
    // numeric precision at the renderer boundary; M32.f also accepts an
    // already compact, numeric Chinese-unit string from the server.
    if let Some(number) = value
        .as_u64()
        .or_else(|| value.as_str()?.parse::<u64>().ok())
    {
        return (number < (1_u64 << 53)).then_some(PodcastPurchasedPlayCount::Number(number));
    }
    let compact = value.as_str().filter(|value| value.len() <= 40)?;
    let units_start =
        compact.find(|character: char| !character.is_ascii_digit() && character != '.')?;
    let (digits, units) = compact.split_at(units_start);
    let (whole, fraction) = digits
        .split_once('.')
        .map_or((digits, None), |(whole, fraction)| (whole, Some(fraction)));
    let digits_only =
        |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if digits_only(whole)
        && fraction.is_none_or(digits_only)
        && units
            .chars()
            .all(|unit| matches!(unit, '十' | '百' | '千' | '万' | '亿'))
    {
        Some(PodcastPurchasedPlayCount::Compact(compact.into()))
    } else {
        None
    }
}

pub(super) fn row(item: &PurchasedItem, liked: &[String]) -> PodcastPurchasedRow {
    match item {
        PurchasedItem::Radio(radio) => PodcastPurchasedRow::Radio(PodcastPurchasedRadio {
            card: view::card(radio),
            play_count: play_count(&radio.raw["playCount"]),
        }),
        PurchasedItem::Episode(episode) => {
            let mut result = view::episode(episode, liked);
            result.unavailable |= episode.display_status_withdrawn();
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
    Ok(row(&cached_purchased_item(session, token, target)?, &[]))
}

pub(in crate::client) fn purchased_playback_episode(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    target: &PodcastPurchasedTarget,
) -> Result<Episode> {
    if target.kind != PurchasedKind::Episode {
        return Err(Error::InvalidInput(
            "purchased playback requires a program row",
        ));
    }
    let item = cached_purchased_item(session, token, target)?;
    match row(&item, &[]) {
        PodcastPurchasedRow::Episode(view) if !view.unavailable => match item {
            PurchasedItem::Episode(episode) => Ok(episode),
            PurchasedItem::Radio(_) => Err(Error::QueueChanged),
        },
        _ => Err(Error::QueueChanged),
    }
}

fn cached_purchased_item(
    session: &Arc<AccountSession>,
    token: &SessionToken,
    target: &PodcastPurchasedTarget,
) -> Result<PurchasedItem> {
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
    snapshot
        .purchases
        .and_then(|value| {
            value
                .items
                .into_iter()
                .find(|row| row.identity() == (target.kind, target.id.as_str()))
        })
        .ok_or(Error::QueueChanged)
}
