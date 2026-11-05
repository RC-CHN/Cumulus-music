//! Native MGC playlist visit. The numeric trial is never renderer input.
use super::{Binding, MusicClient, SessionToken, ZoneAuthority};
use crate::{
    Error, Result,
    client::{
        content::{playlist_track_page, playlist_track_revision},
        dto::{AccountData, PageRequest, TrackPage, TrackView},
        mapping,
    },
    playlist::{PlaylistApi, ZonePlaylistMembership},
    protocol::api::{Api, identifier},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Arc, Mutex},
};

const MAX_VISITS: usize = 24;
const MAX_TRACKS: usize = 50_000;
const MAX_CACHED_DETAIL_ROWS: usize = 4096;

mod batch;

#[derive(Clone, Debug, Serialize)]
pub struct ZonePlaylistOrigin {
    pub receipt: String,
}

#[derive(Default)]
pub(in crate::client) struct DetailHost {
    visits: Arc<Mutex<VecDeque<Visit>>>,
    membership: Mutex<Option<HostMembership>>,
}

struct HostMembership {
    binding: Binding,
    rows: HashMap<String, ZonePlaylistMembership>,
}

fn host_membership(uid: &str, rows: &[Value]) -> HashMap<String, ZonePlaylistMembership> {
    use ZonePlaylistMembership::{Created, Liked, Unverified};

    // M/232.q partitions by the user's native list row's userId, and
    // async:hostResource picks the first specialType=5 from the same list.
    let star = rows
        .iter()
        .find(|row| row["specialType"].as_i64() == Some(5))
        .and_then(|row| identifier(&row["id"]).ok());
    let mut memberships = HashMap::new();
    for row in rows {
        let Ok(id) = identifier(&row["id"]) else {
            continue;
        };
        let entry = if identifier(&row["userId"]).ok().as_deref() == Some(uid)
            && !identifier(&row["creator"]["userId"]).is_ok_and(|creator| creator != uid)
        {
            if star.as_deref() == Some(&id) && row["specialType"].as_i64() == Some(5) {
                Liked
            } else {
                Created
            }
        } else {
            Unverified
        };
        memberships
            .entry(id)
            .and_modify(|known| {
                if *known != entry {
                    *known = Unverified;
                }
            })
            .or_insert(entry);
    }
    memberships.retain(|_, entry| *entry != Unverified);
    memberships
}

struct Visit {
    binding: Binding,
    page_code: String,
    playlist_id: String,
    receipt: String,
    membership: ZonePlaylistMembership,
    card_special: bool,
    read_sequence: u64,
    snapshot: Option<Arc<ZonePlaylistSnapshot>>,
}

pub(in crate::client) struct ZonePlaylistSnapshot {
    pub detail: Value,
    pub revision: String,
    pub trial_mode: i32,
    read_sequence: u64,
    loaded_detail: Mutex<BTreeMap<usize, TrackView>>,
}

impl ZonePlaylistSnapshot {
    fn remember_detail_page(&self, page: &TrackPage) -> Result<()> {
        let mut loaded = self.loaded_detail.lock().map_err(|_| Error::LockPoisoned)?;
        for (position, row) in (page.offset..).zip(&page.tracks) {
            if position >= MAX_CACHED_DETAIL_ROWS {
                break;
            }
            if row.local || row.missing {
                loaded.remove(&position);
            } else {
                loaded.insert(position, row.clone());
            }
        }
        Ok(())
    }

    fn batch_loaded_prefix(&self, page: &PageRequest) -> Result<BTreeMap<usize, TrackView>> {
        let loaded = self.loaded_detail.lock().map_err(|_| Error::LockPoisoned)?;
        // M/2997 starts the full hydration at the first unfilled row. Do
        // not skip a later cached row on the other side of that gap.
        Ok((0..)
            .zip(loaded.iter())
            .take_while(|(index, (position, _))| index == *position)
            .skip(page.offset)
            .take(page.limit)
            .map(|(_, (position, row))| (*position, row.clone()))
            .collect())
    }
}

impl MusicClient {
    /// Reuse a successful live host playlist read. Neither a caller-supplied
    /// owner nor an old persisted presentation list can grant the V6 route.
    pub(in crate::client) fn remember_zone_playlist_membership(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        revision: &str,
        rows: &[Value],
    ) -> Result<()> {
        self.session.with_verified_profile(token, api, |uid, _| {
            if api
                .verified_pc_experiment_revision()
                .ok()
                .flatten()
                .as_deref()
                == Some(revision)
            {
                *self
                    .vertical_zone_detail
                    .membership
                    .lock()
                    .map_err(|_| Error::LockPoisoned)? = Some(HostMembership {
                    binding: Binding::new(token, api, revision),
                    rows: host_membership(uid, rows),
                });
            }
            Ok(())
        })
    }

    /// An opaque visit is minted only from a current, playable horizontal
    /// MGC card. Different cards can expose the same playlist without sharing
    /// an origin, and a later ordinary playlist route needs no receipt.
    pub async fn vertical_zone_playlist_origin(
        &self,
        epoch: &str,
        view_id: &str,
        card_key: &str,
        id: &str,
    ) -> Result<AccountData<ZonePlaylistOrigin>> {
        let id = identifier(&json!(id))?;
        let authority = self
            .vertical_zone_authority(epoch)
            .await?
            .ok_or(Error::StaleResponse)?;
        let registry = self.vertical_zone.registry.lock().await;
        let value =
            self.session
                .with_verified_profile(&authority.token, &authority.api, |_, _| {
                    authority.check_revision()?;
                    let current = registry
                        .as_ref()
                        .filter(|entry| authority.matches(&entry.owner))
                        .ok_or(Error::StaleResponse)?;
                    let mut page_code = None;
                    let (is_mgc, card_special) = self.vertical_zone.playlist_context(
                        &current.owner,
                        view_id,
                        card_key,
                        &id,
                        |page| {
                            page_code = Some(page.to_owned());
                            current.menu.contains(page)
                        },
                    )?;
                    if !is_mgc {
                        return Err(Error::InvalidInput(
                            "playlist card has no MGC detail origin",
                        ));
                    }
                    let page_code = page_code.ok_or(Error::StaleResponse)?;
                    let membership = self
                        .vertical_zone_detail
                        .membership
                        .lock()
                        .map_err(|_| Error::LockPoisoned)?
                        .as_ref()
                        .filter(|known| authority.matches(&known.binding))
                        .and_then(|known| known.rows.get(&id))
                        .copied()
                        .unwrap_or_default();
                    let mut visits = self
                        .vertical_zone_detail
                        .visits
                        .lock()
                        .map_err(|_| Error::LockPoisoned)?;
                    visits.retain(|visit| authority.matches(&visit.binding));
                    let receipt = uuid::Uuid::new_v4().simple().to_string();
                    visits.push_back(Visit {
                        binding: Binding::new(
                            &authority.token,
                            &authority.api,
                            &authority.revision,
                        ),
                        page_code,
                        playlist_id: id,
                        receipt: receipt.clone(),
                        membership,
                        card_special,
                        read_sequence: 0,
                        snapshot: None,
                    });
                    while visits.len() > MAX_VISITS {
                        visits.pop_front();
                    }
                    Ok(ZonePlaylistOrigin { receipt })
                })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    async fn zone_playlist_visit<T>(
        &self,
        authority: &ZoneAuthority,
        receipt: &str,
        id: &str,
        operation: impl FnOnce(&mut Visit) -> Result<T>,
    ) -> Result<T> {
        if receipt.len() != 32
            || !receipt
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(Error::InvalidInput("invalid MGC playlist receipt"));
        }
        let registry = self.vertical_zone.registry.lock().await;
        self.session
            .with_verified_profile(&authority.token, &authority.api, |_, _| {
                authority.check_revision()?;
                let mut visits = self
                    .vertical_zone_detail
                    .visits
                    .lock()
                    .map_err(|_| Error::LockPoisoned)?;
                let visit = visits
                    .iter_mut()
                    .find(|visit| {
                        visit.receipt == receipt
                            && visit.playlist_id == id
                            && authority.matches(&visit.binding)
                            && authority.permits(registry.as_ref(), &visit.page_code)
                    })
                    .ok_or(Error::StaleResponse)?;
                operation(visit)
            })
    }

    pub async fn vertical_zone_playlist_detail(
        &self,
        epoch: &str,
        receipt: &str,
        id: &str,
        page: PageRequest,
    ) -> Result<AccountData<TrackPage>> {
        let id = identifier(&json!(id))?;
        mapping::page(page.offset, page.limit)?;
        if page.offset > 0 && page.revision.is_none() {
            return Err(Error::InvalidInput(
                "MGC playlist continuation requires its detail revision",
            ));
        }
        let authority = self
            .vertical_zone_authority(epoch)
            .await?
            .ok_or(Error::StaleResponse)?;
        let (sequence, existing, membership, special) = self
            .zone_playlist_visit(&authority, receipt, &id, |visit| {
                if let Some(expected) = &page.revision {
                    let snapshot = visit
                        .snapshot
                        .as_ref()
                        .filter(|known| &known.revision == expected)
                        .cloned()
                        .ok_or(Error::QueueChanged)?;
                    return Ok((visit.read_sequence, Some(snapshot), visit.membership, false));
                }
                visit.read_sequence = visit
                    .read_sequence
                    .checked_add(1)
                    .ok_or(Error::State("MGC playlist read sequence exhausted"))?;
                let special = visit
                    .snapshot
                    .as_ref()
                    .map(|known| known.detail["specialType"].as_i64() == Some(100))
                    .unwrap_or(visit.card_special);
                Ok((visit.read_sequence, None, visit.membership, special))
            })
            .await?;
        let initial = existing.is_none();
        let (token, bound) = (&authority.token, &authority.api);
        let authorization = &authority;
        let mut changes = self.session.subscribe()?;
        let id_ref = id.as_str();
        let (value, snapshot) = self.session.run_for(token, |api, uid| async move {
            self.session.require_api(token, bound)?;
            self.session.require_api(token, &api)?;
            tokio::select! {
                biased;
                _ = changes.changed() => Err(Error::StaleResponse),
                result = async {
                    let snapshot = if let Some(existing) = existing {
                        existing
                    } else {
                        let detail = PlaylistApi::new(api.clone())
                            .zone_mgc_detail(id_ref, &uid, membership, special, || authorization.check_revision())
                            .await?;
                        let tracks = detail["trackIds"].as_array()
                            .ok_or(Error::Response("missing MGC playlist trackIds"))?;
                        if tracks.len() > MAX_TRACKS {
                            return Err(Error::Response("MGC playlist exceeds track bound"));
                        }
                        Arc::new(ZonePlaylistSnapshot {
                            revision: playlist_track_revision(id_ref, &detail)?,
                            detail,
                            trial_mode: membership.trial_mode(),
                            read_sequence: sequence,
                            loaded_detail: Default::default(),
                        })
                    };
                    authorization.check_revision()?;
                    let value = playlist_track_page(
                        api, &uid, id_ref, &snapshot.detail, &page, Some(snapshot.trial_mode), None,
                    ).await?;
                    Ok((value, snapshot))
                } => result,
            }
        }).await?;
        self.zone_playlist_visit(&authority, receipt, &id, |visit| {
            if visit.read_sequence != sequence {
                return Err(Error::StaleResponse);
            }
            snapshot.remember_detail_page(&value)?;
            if initial {
                visit.snapshot = Some(snapshot);
            } else if !visit
                .snapshot
                .as_ref()
                .is_some_and(|known| Arc::ptr_eq(known, &snapshot))
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        })
        .await?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub(in crate::client) async fn vertical_zone_playlist_snapshot(
        &self,
        token: &SessionToken,
        receipt: &str,
        id: &str,
        revision: &str,
    ) -> Result<Arc<ZonePlaylistSnapshot>> {
        let authority = self
            .vertical_zone_authority(&token.generation.to_string())
            .await?
            .filter(|authority| authority.token == *token)
            .ok_or(Error::StaleResponse)?;
        self.zone_playlist_visit(&authority, receipt, id, |visit| {
            visit
                .snapshot
                .as_ref()
                .filter(|snapshot| snapshot.revision == revision)
                .cloned()
                .ok_or(Error::QueueChanged)
        })
        .await
    }
}
