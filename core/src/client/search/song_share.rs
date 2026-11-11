//! A dedicated Single Song result row may authorize the local share chooser.
use super::{SearchCategory, SearchItem, SearchResultView, SearchSection};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::song_share_allowed,
    client::{
        MusicClient,
        dto::{
            AccountData, HistoryRowShareChoice, HistoryRowShareOutcome, HistoryRowSharePreparation,
            TrackView,
        },
        share::qr_modules,
    },
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

const PAGE_LIMIT: usize = 32;
const ROW_LIMIT: usize = 128;
const PAGE_LIFETIME: Duration = Duration::from_secs(30 * 60);
const OFFER_LIFETIME: Duration = Duration::from_secs(180);
const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchSongShareTarget {
    receipt: String,
    track_id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Song {
    id: String,
    album_required: bool,
}

fn canonical_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('0')
        && id.bytes().all(|byte| byte.is_ascii_digit())
}

impl Song {
    pub(super) fn observe(raw: &Value, track: &TrackView) -> Option<Self> {
        let id = identifier(&raw["id"]).ok()?;
        let privilege = &raw["privilege"];
        // M556's search scene uses the ordinary clicked Song. Refuse private
        // source markers and a different onlineTrackId rather than guess which
        // resource's permissions should authorize an outgoing public URL.
        if id != track.id
            || !canonical_id(&id)
            || track.local
            || track.missing
            || track.voice_id.is_some()
            || js_truthy(&privilege["cs"])
            || [
                "fileId",
                "localId",
                "local_id",
                "cloudSongId",
                "cloud_song_id",
                "programId",
                "voiceId",
            ]
            .iter()
            .any(|field| js_truthy(&raw[*field]))
            || ["songType", "t"]
                .iter()
                .any(|field| raw[*field].as_i64() == Some(1) || raw[*field].as_str() == Some("1"))
            || (js_truthy(&raw["onlineTrackId"])
                && identifier(&raw["onlineTrackId"]).ok().as_deref() != Some(id.as_str()))
            || ["st", "status"].iter().any(|field| {
                let status = &privilege[*field];
                status
                    .as_i64()
                    .or_else(|| status.as_str()?.parse().ok())
                    .is_some_and(|value| value < 0)
            })
        {
            return None;
        }
        Some(Self {
            id,
            album_required: privilege["fee"].as_u64().is_some_and(|fee| fee & 4 != 0),
        })
    }
}

fn candidates(view: &mut SearchResultView) -> Option<(&mut [SearchItem], Vec<Option<Song>>)> {
    let rows = std::mem::take(&mut view.song_share_rows);
    let [section] = view.sections.as_mut_slice() else {
        return None;
    };
    let SearchSection {
        id,
        category,
        items,
        ..
    } = section;
    (view.query.category == SearchCategory::Songs
        && *category == Some(SearchCategory::Songs)
        && id.as_str() == SearchCategory::Songs.title()
        && items.len() == rows.len())
    .then_some((items.as_mut_slice(), rows))
}

struct Page {
    owner: SessionToken,
    api: Weak<dyn Api>,
    revision: String,
    issued: Instant,
    rows: BTreeMap<String, Arc<Song>>,
}

impl Page {
    fn owns(&self, owner: &SessionToken, api: &Arc<dyn Api>, revision: &str) -> bool {
        self.owner == *owner
            && self.api.ptr_eq(&Arc::downgrade(api))
            && self.revision == revision
            && self.issued.elapsed() < PAGE_LIFETIME
    }
}

struct Offer {
    owner: SessionToken,
    target: SearchSongShareTarget,
    receipt: String,
    song: Arc<Song>,
    issued: Instant,
    choosing: bool,
}

#[derive(Default)]
struct State {
    pages: VecDeque<Page>,
    generation: u64,
    offer: Option<Offer>,
}

#[derive(Default)]
pub(super) struct SearchSongShares(Mutex<State>);

struct PendingChoice<'a> {
    shares: &'a SearchSongShares,
    generation: u64,
}

impl Drop for PendingChoice<'_> {
    fn drop(&mut self) {
        // A transport failure or dropped future cannot leave a consumed offer
        // behind, or clear a replacement share attempt.
        let _ = self.shares.finish_choice(self.generation);
    }
}

fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

impl SearchSongShares {
    pub(super) fn capture(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        revision: Option<String>,
        view: &mut SearchResultView,
    ) -> Result<()> {
        let Some((items, candidates)) = candidates(view) else {
            return Ok(());
        };
        let Some(revision) = revision else {
            return Ok(());
        };
        let mut rows = BTreeMap::new();
        for (item, row) in items.iter_mut().zip(candidates) {
            if rows.len() >= ROW_LIMIT {
                break;
            }
            let (
                SearchItem::Song {
                    track,
                    song_share_receipt,
                    ..
                },
                Some(row),
            ) = (item, row)
            else {
                continue;
            };
            if row.id != track.id {
                continue;
            }
            let receipt = nonce();
            rows.insert(receipt.clone(), Arc::new(row));
            *song_share_receipt = Some(receipt);
        }
        if !rows.is_empty() {
            let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
            state.pages.retain(|page| page.owns(token, api, &revision));
            if state.pages.len() >= PAGE_LIMIT {
                state.pages.pop_front();
            }
            state.pages.push_back(Page {
                owner: token.clone(),
                api: Arc::downgrade(api),
                revision,
                issued: Instant::now(),
                rows,
            });
        }
        Ok(())
    }

    fn source(
        &self,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        revision: &str,
        target: &SearchSongShareTarget,
    ) -> Result<Arc<Song>> {
        if target.receipt.len() != 32
            || !target.receipt.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !canonical_id(&target.track_id)
        {
            return Err(Error::InvalidInput("invalid search song share target"));
        }
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state
            .pages
            .iter()
            .rev()
            .find_map(|page| {
                page.owns(owner, api, revision)
                    .then(|| page.rows.get(&target.receipt))
                    .flatten()
                    .filter(|song| song.id == target.track_id)
            })
            .cloned()
            .ok_or(Error::StaleResponse)
    }

    fn begin(&self) -> Result<u64> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or(Error::State("search sharing generation exhausted"))?;
        state.offer = None;
        Ok(state.generation)
    }

    fn current(&self, generation: u64) -> Result<()> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        (state.generation == generation)
            .then_some(())
            .ok_or(Error::StaleResponse)
    }

    fn publish(&self, generation: u64, offer: Offer) -> Result<String> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.generation != generation {
            return Err(Error::StaleResponse);
        }
        let receipt = offer.receipt.clone();
        state.offer = Some(offer);
        Ok(receipt)
    }

    fn take(
        &self,
        owner: &SessionToken,
        target: &SearchSongShareTarget,
        receipt: &str,
        song: &Arc<Song>,
    ) -> Result<PendingChoice<'_>> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let generation = state.generation;
        let offer = state
            .offer
            .as_mut()
            .filter(|offer| {
                !offer.choosing
                    && &offer.owner == owner
                    && &offer.target == target
                    && offer.receipt == receipt
                    && Arc::ptr_eq(&offer.song, song)
                    && offer.issued.elapsed() < OFFER_LIFETIME
            })
            .ok_or(Error::StaleResponse)?;
        // Consume the capability immediately, retaining the exact identity
        // so that its owner can still cancel an in-flight channel check.
        offer.choosing = true;
        Ok(PendingChoice {
            shares: self,
            generation,
        })
    }

    fn finish_choice(&self, generation: u64) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.generation != generation || !state.offer.as_ref().is_some_and(|o| o.choosing) {
            return Err(Error::StaleResponse);
        }
        state.offer = None;
        Ok(())
    }

    fn cancel(
        &self,
        owner: &SessionToken,
        target: &SearchSongShareTarget,
        receipt: &str,
    ) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.offer.as_ref().is_some_and(|offer| {
            &offer.owner == owner && &offer.target == target && offer.receipt == receipt
        }) {
            state.generation = state
                .generation
                .checked_add(1)
                .ok_or(Error::State("search sharing generation exhausted"))?;
            state.offer = None;
        }
        Ok(())
    }
}

struct Authority<'a> {
    owner: &'a SessionToken,
    api: Arc<dyn Api>,
    revision: String,
    target: &'a SearchSongShareTarget,
    song: Arc<Song>,
}

enum Access {
    Allowed,
    Unavailable,
    AlbumRequired,
}

impl MusicClient {
    fn search_song_share_source<'a>(
        &self,
        owner: &'a SessionToken,
        target: &'a SearchSongShareTarget,
    ) -> Result<Authority<'a>> {
        if owner.uid.is_none() || self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        let api = self.session.native_api(owner)?;
        let revision = api
            .search_song_share_credential_revision()?
            .ok_or(Error::StaleResponse)?;
        let song = self
            .search
            .song_shares
            .source(owner, &api, &revision, target)?;
        Ok(Authority {
            owner,
            api,
            revision,
            target,
            song,
        })
    }

    fn check_search_song_share(&self, source: &Authority<'_>, generation: u64) -> Result<()> {
        if self.login_pending()? {
            return Err(Error::StaleResponse);
        }
        self.session
            .with_verified_profile(source.owner, &source.api, |_, _| {
                self.check_search_song_share_source(source)?;
                self.search.song_shares.current(generation)
            })
    }

    fn check_search_song_share_source(&self, source: &Authority<'_>) -> Result<()> {
        if source
            .api
            .search_song_share_credential_revision()?
            .as_deref()
            != Some(source.revision.as_str())
            || !Arc::ptr_eq(
                &source.song,
                &self.search.song_shares.source(
                    source.owner,
                    &source.api,
                    &source.revision,
                    source.target,
                )?,
            )
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    async fn search_song_share_access(
        &self,
        source: &Authority<'_>,
        generation: u64,
    ) -> Result<Access> {
        let permission = || self.check_search_song_share(source, generation);
        permission()?;
        let permission = &permission;
        // M3085 calls this at chooser entry and for the selected channel.
        // The recovered helper can cache; this narrower authority always
        // obtains fresh results and retains the same checked wire revision.
        let response = tokio::time::timeout(
            READ_TIMEOUT,
            self.session.run_for(source.owner, |current, _| async move {
                if !Arc::ptr_eq(&current, &source.api) {
                    return Err(Error::StaleResponse);
                }
                current
                    .request_search_song_share_privilege(
                        &source.song.id,
                        &source.revision,
                        permission,
                    )
                    .await
            }),
        )
        .await
        .map_err(|_| Error::Response("song sharing check timed out"))??;
        permission()?;
        let rows = response["data"]
            .as_array()
            .ok_or(Error::Response("song privileges missing"))?;
        Ok(if song_share_allowed(rows, &source.song.id)? {
            Access::Allowed
        } else if source.song.album_required {
            Access::AlbumRequired
        } else {
            Access::Unavailable
        })
    }

    pub async fn search_song_share_prepare(
        &self,
        epoch: &str,
        target: SearchSongShareTarget,
    ) -> Result<AccountData<HistoryRowSharePreparation>> {
        use HistoryRowSharePreparation as P;
        let owner = self.player_token(epoch)?;
        let source = self.search_song_share_source(&owner, &target)?;
        let generation = self
            .session
            .with_verified_profile(&owner, &source.api, |_, _| {
                self.check_search_song_share_source(&source)?;
                self.search.song_shares.begin()
            })?;
        let access = self.search_song_share_access(&source, generation).await?;
        self.check_search_song_share(&source, generation)?;
        self.session
            .with_verified_profile(&owner, &source.api, |_, _| {
                self.check_search_song_share_source(&source)?;
                self.search.song_shares.current(generation)?;
                let value = match access {
                    Access::Allowed => P::Ready {
                        receipt: self.search.song_shares.publish(
                            generation,
                            Offer {
                                owner: owner.clone(),
                                target: target.clone(),
                                receipt: nonce(),
                                song: source.song.clone(),
                                issued: Instant::now(),
                                choosing: false,
                            },
                        )?,
                    },
                    Access::Unavailable => P::Unavailable,
                    Access::AlbumRequired => P::AlbumRequired,
                };
                Ok(AccountData {
                    epoch: epoch.into(),
                    value,
                })
            })
    }

    pub async fn search_song_share_choose(
        &self,
        epoch: &str,
        target: SearchSongShareTarget,
        receipt: &str,
        choice: HistoryRowShareChoice,
    ) -> Result<AccountData<HistoryRowShareOutcome>> {
        use HistoryRowShareOutcome as O;
        let owner = self.player_token(epoch)?;
        let source = self.search_song_share_source(&owner, &target)?;
        let pending = self
            .session
            .with_verified_profile(&owner, &source.api, |_, _| {
                self.check_search_song_share_source(&source)?;
                self.search
                    .song_shares
                    .take(&owner, &target, receipt, &source.song)
            })?;
        let generation = pending.generation;
        let value = match self.search_song_share_access(&source, generation).await? {
            Access::Allowed => {
                let url = format!("https://music.163.com/song?id={}", source.song.id);
                match choice {
                    HistoryRowShareChoice::Copy => O::Copy { url },
                    HistoryRowShareChoice::WechatQr => O::WechatQr {
                        modules: qr_modules(&url)?,
                    },
                }
            }
            Access::Unavailable => O::Unavailable,
            Access::AlbumRequired => O::AlbumRequired,
        };
        self.check_search_song_share(&source, generation)?;
        self.session
            .with_verified_profile(&owner, &source.api, |_, _| {
                self.check_search_song_share_source(&source)?;
                // Commit under the same lock used by exact cancellation; a
                // cancelled response cannot publish a URL or QR matrix.
                self.search.song_shares.finish_choice(generation)?;
                Ok(AccountData {
                    epoch: epoch.into(),
                    value,
                })
            })
    }

    pub fn search_song_share_cancel(
        &self,
        epoch: &str,
        target: SearchSongShareTarget,
        receipt: &str,
    ) -> Result<()> {
        let owner = self.player_token(epoch)?;
        self.search.song_shares.cancel(&owner, &target, receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::{AccountSession, profiles::ProfileRegistry},
        catalog::search::results::SearchResultQuery,
        protocol::{api::ApiFuture, http::ApiOrigin},
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

    #[derive(Default)]
    struct Wire {
        revision: AtomicU64,
        share_right: AtomicI64,
        rotate_during_read: AtomicBool,
        reads: Mutex<Vec<(String, String)>>,
    }

    fn search_reply() -> Value {
        let resource = |raw| json!({"baseInfo":{"simpleSongData":raw}});
        json!({"code":200,"data":{"totalCount":5,"hasMore":false,"resources":[
            resource(json!({"id":1,"name":"ordinary","privilege":{"st":0}})),
            resource(json!({"id":2,"name":"album","privilege":{"st":0,"fee":4}})),
            resource(json!({"id":3,"name":"copyright","privilege":{"st":-100}})),
            resource(json!({"id":4,"name":"private","privilege":{"cs":true}})),
            resource(json!({"id":5,"name":"alternate","onlineTrackId":6}))
        ]}})
    }

    impl Api for Wire {
        fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
            Box::pin(async move {
                assert_eq!(path, "/api/search/song/list/page");
                Ok(search_reply())
            })
        }
        fn search_song_share_credential_revision(&self) -> Result<Option<String>> {
            Ok(Some(self.revision.load(Ordering::SeqCst).to_string()))
        }
        fn request_search_song_share_privilege<'a>(
            &'a self,
            id: &'a str,
            revision: &'a str,
            permission: &'a (dyn Fn() -> Result<()> + Send + Sync),
        ) -> ApiFuture<'a, Value> {
            Box::pin(async move {
                permission()?;
                assert_eq!(
                    Some(revision),
                    self.search_song_share_credential_revision()?.as_deref()
                );
                self.reads
                    .lock()
                    .unwrap()
                    .push((id.into(), revision.into()));
                if self.rotate_during_read.load(Ordering::SeqCst) {
                    self.revision.fetch_add(1, Ordering::SeqCst);
                }
                Ok(
                    json!({"code":200,"data":[{"id":id,"sp":self.share_right.load(Ordering::SeqCst)}]}),
                )
            })
        }
    }

    struct Fixture {
        _root: tempfile::TempDir,
        wire: Arc<Wire>,
        client: MusicClient,
        epoch: String,
    }

    fn query() -> SearchResultQuery {
        SearchResultQuery {
            keyword: "合成".into(),
            category: SearchCategory::Songs,
            offset: 0,
            limit: 30,
            correct: true,
            channel: None,
        }
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
            let wire = Arc::new(Wire::default());
            wire.share_right.store(1, Ordering::SeqCst);
            let session = Arc::new(AccountSession::new(wire.clone(), Some(registry.store())));
            session
                .adopt(wire.clone(), json!({"userId":9}), &session.token().unwrap())
                .unwrap();
            let epoch = session.token().unwrap().generation.to_string();
            let client = MusicClient::with_session(session, registry);
            Self {
                _root: root,
                wire,
                client,
                epoch,
            }
        }

        async fn targets(&self) -> Vec<SearchSongShareTarget> {
            let result = self
                .client
                .search_results(&self.epoch, query())
                .await
                .unwrap();
            let serialized = serde_json::to_value(result.value).unwrap();
            assert_eq!(serialized["sections"][0]["id"], "单曲");
            let rows = serialized["sections"][0]["items"].as_array().unwrap();
            assert_eq!(rows.len(), 5);
            assert!(
                rows[..2]
                    .iter()
                    .all(|row| row["songPublicLinkAvailable"] == true)
            );
            assert!(
                rows[2..]
                    .iter()
                    .all(|row| row.get("songShareReceipt").is_none())
            );
            rows[..2]
                .iter()
                .map(|row| {
                    serde_json::from_value(json!({
                        "receipt":row["songShareReceipt"], "trackId":row["track"]["id"]
                    }))
                    .unwrap()
                })
                .collect()
        }
    }

    async fn prepare(fixture: &Fixture, target: SearchSongShareTarget) -> String {
        let HistoryRowSharePreparation::Ready { receipt } = fixture
            .client
            .search_song_share_prepare(&fixture.epoch, target)
            .await
            .unwrap()
            .value
        else {
            panic!("expected positive fresh privilege");
        };
        receipt
    }

    #[tokio::test]
    async fn binds_raw_song_to_its_own_offer_and_rechecks_right_before_copy() {
        let fixture = Fixture::new();
        let rows = fixture.targets().await;
        let wrong_id = SearchSongShareTarget {
            track_id: "2".into(),
            ..rows[0].clone()
        };
        assert!(
            fixture
                .client
                .search_song_share_prepare(&fixture.epoch, wrong_id)
                .await
                .is_err()
        );
        let fake_ticket = SearchSongShareTarget {
            receipt: nonce(),
            ..rows[0].clone()
        };
        assert!(
            fixture
                .client
                .search_song_share_prepare(&fixture.epoch, fake_ticket)
                .await
                .is_err()
        );
        assert!(fixture.wire.reads.lock().unwrap().is_empty());

        let offer = prepare(&fixture, rows[0].clone()).await;
        assert!(
            fixture
                .client
                .search_song_share_choose(
                    &fixture.epoch,
                    rows[1].clone(),
                    &offer,
                    HistoryRowShareChoice::Copy
                )
                .await
                .is_err()
        );
        assert_eq!(fixture.wire.reads.lock().unwrap().len(), 1);
        let value = fixture
            .client
            .search_song_share_choose(
                &fixture.epoch,
                rows[0].clone(),
                &offer,
                HistoryRowShareChoice::Copy,
            )
            .await
            .unwrap()
            .value;
        assert_eq!(
            value,
            HistoryRowShareOutcome::Copy {
                url: "https://music.163.com/song?id=1".into()
            }
        );
        assert!(
            fixture
                .client
                .search_song_share_choose(
                    &fixture.epoch,
                    rows[0].clone(),
                    &offer,
                    HistoryRowShareChoice::WechatQr
                )
                .await
                .is_err()
        );
        assert_eq!(
            *fixture.wire.reads.lock().unwrap(),
            [("1".into(), "0".into()), ("1".into(), "0".into())]
        );
    }

    #[tokio::test]
    async fn denial_at_choice_retires_offer_and_rotation_cannot_publish() {
        let fixture = Fixture::new();
        let rows = fixture.targets().await;
        let offer = prepare(&fixture, rows[1].clone()).await;
        fixture.wire.share_right.store(0, Ordering::SeqCst);
        let value = fixture
            .client
            .search_song_share_choose(
                &fixture.epoch,
                rows[1].clone(),
                &offer,
                HistoryRowShareChoice::WechatQr,
            )
            .await
            .unwrap()
            .value;
        assert_eq!(value, HistoryRowShareOutcome::AlbumRequired);
        assert!(
            fixture
                .client
                .search_song_share_choose(
                    &fixture.epoch,
                    rows[1].clone(),
                    &offer,
                    HistoryRowShareChoice::Copy
                )
                .await
                .is_err()
        );
        fixture
            .wire
            .rotate_during_read
            .store(true, Ordering::SeqCst);
        assert!(
            fixture
                .client
                .search_song_share_prepare(&fixture.epoch, rows[0].clone())
                .await
                .is_err()
        );
        assert!(
            fixture
                .client
                .search_song_share_prepare(&fixture.epoch, rows[0].clone())
                .await
                .is_err()
        );
        assert_eq!(fixture.wire.reads.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn cancellation_retires_only_exact_search_offer_without_another_read() {
        let fixture = Fixture::new();
        let rows = fixture.targets().await;
        let offer = prepare(&fixture, rows[0].clone()).await;
        fixture
            .client
            .search_song_share_cancel(&fixture.epoch, rows[1].clone(), &offer)
            .unwrap();
        let value = fixture
            .client
            .search_song_share_choose(
                &fixture.epoch,
                rows[0].clone(),
                &offer,
                HistoryRowShareChoice::WechatQr,
            )
            .await
            .unwrap()
            .value;
        assert_eq!(
            value,
            HistoryRowShareOutcome::WechatQr {
                modules: qr_modules("https://music.163.com/song?id=1").unwrap()
            }
        );
        let offer = prepare(&fixture, rows[0].clone()).await;
        fixture
            .client
            .search_song_share_cancel(&fixture.epoch, rows[0].clone(), &offer)
            .unwrap();
        fixture
            .client
            .search_song_share_cancel(&fixture.epoch, rows[0].clone(), &offer)
            .unwrap();
        assert!(
            fixture
                .client
                .search_song_share_choose(
                    &fixture.epoch,
                    rows[0].clone(),
                    &offer,
                    HistoryRowShareChoice::Copy
                )
                .await
                .is_err()
        );
        assert_eq!(fixture.wire.reads.lock().unwrap().len(), 3);
    }

    #[test]
    fn unsigned_projection_only_marks_exact_public_rows_and_never_issues_receipts() {
        let (view, ..) =
            super::super::results::project(search_reply(), query(), "", false).unwrap();
        let json = serde_json::to_value(view).unwrap();
        let rows = json["sections"][0]["items"].as_array().unwrap();
        assert!(
            rows[..2]
                .iter()
                .all(|row| row["songPublicLinkAvailable"] == true)
        );
        assert!(
            rows[2..]
                .iter()
                .all(|row| row.get("songPublicLinkAvailable").is_none())
        );
        assert!(rows.iter().all(|row| row.get("songShareReceipt").is_none()));
    }
}
