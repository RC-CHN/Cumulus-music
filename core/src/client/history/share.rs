//! Native History share authorization (M556/M622, M3085:1310–1395).
use super::{
    AccountData, HistoryRowMenuTarget, HistoryRowShareChoice, HistoryRowShareOutcome,
    HistoryRowSharePreparation, MusicClient, row_menu, row_menu_info, voice_file_menu,
};
use crate::client::share::qr_modules;
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::{CatalogApi, song_share_allowed},
    history::{HistoryKind, HistoryRecord, HistoryService},
    protocol::api::Api,
};
use std::{
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

const LIFETIME: Duration = Duration::from_secs(180);
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ShareTarget {
    Song { id: String, album_required: bool },
    Voice { id: String },
}

impl ShareTarget {
    fn public_url(&self) -> String {
        let (path, id) = match self {
            Self::Song { id, .. } => ("song", id),
            Self::Voice { id } => ("dj", id),
        };
        format!("https://music.163.com/{path}?id={id}")
    }
}

pub(super) fn project(
    kind: HistoryKind,
    clicked: &HistoryRecord,
    has_group: bool,
) -> Option<ShareTarget> {
    // M556's common check hides Song Share for either own or parent
    // selection; M622 checks only its own from.selectedData, which the
    // History Voice row does not have. The Voice group never replaces click.
    if kind == HistoryKind::Song && has_group {
        return None;
    }
    let id = row_menu_info::public_id(kind, clicked)?;
    let raw = &clicked.resource;
    match kind {
        HistoryKind::Song => {
            let p = &raw["privilege"];
            if ["st", "status"].into_iter().any(|key| {
                p[key]
                    .as_i64()
                    .or_else(|| p[key].as_str()?.parse().ok())
                    .is_some_and(|v| v < 0)
            }) || ["songType", "t"]
                .into_iter()
                .any(|key| raw[key].as_i64() == Some(1) || raw[key].as_str() == Some("1"))
            {
                return None;
            }
            // M35.e(fee,"bb") has bit value 4. Surface the verified
            // prerequisite if rights are denied; sharing never opens payment.
            let album_required = p["fee"].as_u64().is_some_and(|fee| fee & 4 != 0);
            Some(ShareTarget::Song { id, album_required })
        }
        HistoryKind::Voice => {
            let episode = voice_file_menu::episode(clicked).ok()?;
            (episode.id == id).then_some(ShareTarget::Voice { id })
        }
        _ => None,
    }
}

fn scoped(
    history: &HistoryService,
    token: &SessionToken,
    target: &HistoryRowMenuTarget,
) -> Result<Option<ShareTarget>> {
    if !matches!(target.history_kind, HistoryKind::Song | HistoryKind::Voice) {
        return Err(Error::InvalidInput(
            "history sharing requires a song or program",
        ));
    }
    let snapshot = history.snapshot_for(token, target.history_kind)?;
    if snapshot.revision != target.revision {
        return Err(Error::QueueChanged);
    }
    let row = row_menu::resolve(&snapshot, &target.clicked_id, &target.selected_ids)?;
    Ok(project(
        snapshot.kind,
        row.clicked,
        !row.selected.is_empty(),
    ))
}

struct Offer {
    sequence: u64,
    issued: Instant,
    owner: SessionToken,
    api: Weak<dyn Api>,
    target: HistoryRowMenuTarget,
    resource: ShareTarget,
    receipt: String,
}

#[derive(Default)]
pub(in crate::client) struct HistoryRowShareHost(Mutex<(u64, Option<Offer>)>);

impl HistoryRowShareHost {
    fn begin(&self) -> Result<u64> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        state.0 = state
            .0
            .checked_add(1)
            .ok_or(Error::State("history share generation exhausted"))?;
        state.1 = None;
        Ok(state.0)
    }

    fn current(&self, sequence: u64) -> Result<()> {
        let state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.0 != sequence {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    fn publish(&self, offer: Offer) -> Result<String> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.0 != offer.sequence {
            return Err(Error::StaleResponse);
        }
        let receipt = offer.receipt.clone();
        state.1 = Some(offer);
        Ok(receipt)
    }

    fn take(
        &self,
        owner: &SessionToken,
        api: &Arc<dyn Api>,
        target: &HistoryRowMenuTarget,
        resource: &ShareTarget,
        receipt: &str,
    ) -> Result<Offer> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if !state.1.as_ref().is_some_and(|offer| {
            &offer.owner == owner
                && &offer.target == target
                && &offer.resource == resource
                && offer.receipt == receipt
                && offer.issued.elapsed() < LIFETIME
                && offer
                    .api
                    .upgrade()
                    .is_some_and(|held| Arc::ptr_eq(&held, api))
        }) {
            return Err(Error::QueueChanged);
        }
        state.1.take().ok_or(Error::QueueChanged)
    }

    fn cancel(
        &self,
        owner: &SessionToken,
        target: &HistoryRowMenuTarget,
        receipt: &str,
    ) -> Result<()> {
        let mut state = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if state.1.as_ref().is_some_and(|offer| {
            &offer.owner == owner && &offer.target == target && offer.receipt == receipt
        }) {
            state.1 = None;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Access {
    Allowed,
    Unavailable,
    AlbumRequired,
}

impl MusicClient {
    async fn history_share_access(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        resource: &ShareTarget,
    ) -> Result<Access> {
        let ShareTarget::Song { id, album_required } = resource else {
            return Ok(Access::Allowed);
        };
        // M15 Dg sends exactly ids = JSON.stringify([id]), without
        // trialMode. Both invocations must use fresh uncached rights.
        let rows = tokio::time::timeout(
            TIMEOUT,
            self.session.run_for(token, |current, _| async move {
                if !Arc::ptr_eq(&current, api) {
                    return Err(Error::StaleResponse);
                }
                CatalogApi::new(current)
                    .privileges(std::slice::from_ref(id), None)
                    .await
            }),
        )
        .await
        .map_err(|_| Error::Response("song sharing check timed out"))??;
        if song_share_allowed(&rows, id)? {
            Ok(Access::Allowed)
        } else if *album_required {
            Ok(Access::AlbumRequired)
        } else {
            Ok(Access::Unavailable)
        }
    }

    pub async fn history_row_share_prepare(
        &self,
        epoch: &str,
        target: HistoryRowMenuTarget,
    ) -> Result<AccountData<HistoryRowSharePreparation>> {
        use HistoryRowSharePreparation as P;
        let token = self.player_token(epoch)?;
        let history = HistoryService::new(self.session.clone())?;
        let resource = scoped(&history, &token, &target)?;
        let publish = |value| AccountData {
            epoch: epoch.into(),
            value,
        };
        let kind = target.history_kind;
        let revision = target.revision.clone();
        let Some(resource) = resource else {
            return history
                .with_matching_revision(&token, kind, &revision, |_| Ok(publish(P::Unavailable)));
        };
        // The official Song dispatcher can request guest rights before the
        // login gate. The native account transport rejects guest run_for;
        // resolve the normal login prompt without a guest privilege request.
        if token.uid.is_none() {
            return history.with_matching_revision(&token, kind, &revision, |_| {
                Ok(publish(P::LoginRequired))
            });
        }
        let login = self.auth.pending_guard();
        login()?;
        let api = self.session.native_api(&token)?;
        self.session
            .with_verified_profile(&token, &api, |_, _| Ok(()))?;
        let sequence = self.history_row_share.begin()?;
        let access = self.history_share_access(&token, &api, &resource).await?;
        login()?;
        self.session.require_api(&token, &api)?;
        history.with_matching_revision(&token, kind, &revision, |_| {
            self.history_row_share.current(sequence)?;
            let value = match access {
                Access::Allowed => P::Ready {
                    receipt: self.history_row_share.publish(Offer {
                        sequence,
                        issued: Instant::now(),
                        owner: token.clone(),
                        api: Arc::downgrade(&api),
                        target,
                        resource,
                        receipt: uuid::Uuid::new_v4().simple().to_string(),
                    })?,
                },
                Access::Unavailable => P::Unavailable,
                Access::AlbumRequired => P::AlbumRequired,
            };
            Ok(publish(value))
        })
    }

    pub async fn history_row_share_choose(
        &self,
        epoch: &str,
        target: HistoryRowMenuTarget,
        receipt: &str,
        choice: HistoryRowShareChoice,
    ) -> Result<AccountData<HistoryRowShareOutcome>> {
        use HistoryRowShareOutcome as O;
        let token = self.player_token(epoch)?;
        let login = self.auth.pending_guard();
        login()?;
        let history = HistoryService::new(self.session.clone())?;
        let resource = scoped(&history, &token, &target)?.ok_or(Error::QueueChanged)?;
        let api = self.session.native_api(&token)?;
        self.session
            .with_verified_profile(&token, &api, |_, _| Ok(()))?;
        let offer = self
            .history_row_share
            .take(&token, &api, &target, &resource, receipt)?;
        let access = self.history_share_access(&token, &api, &resource).await?;
        let value = match access {
            Access::Unavailable => O::Unavailable,
            Access::AlbumRequired => O::AlbumRequired,
            Access::Allowed => {
                let url = resource.public_url();
                match choice {
                    HistoryRowShareChoice::Copy => O::Copy { url },
                    HistoryRowShareChoice::WechatQr => O::WechatQr {
                        modules: qr_modules(&url)?,
                    },
                }
            }
        };
        login()?;
        self.session.require_api(&token, &api)?;
        history.with_matching_revision(&token, target.history_kind, &target.revision, |_| {
            self.history_row_share.current(offer.sequence)?;
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    /// Dismiss an unused offer. Repeated dismissals and stale targets cannot
    /// retire a different offer, and this never reads upstream privileges.
    pub fn history_row_share_cancel(
        &self,
        epoch: &str,
        target: HistoryRowMenuTarget,
        receipt: &str,
    ) -> Result<()> {
        let token = self.player_token(epoch)?;
        self.history_row_share.cancel(&token, &target, receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::{AccountSession, profiles::ProfileRegistry},
        protocol::{api::ApiFuture, http::ApiOrigin},
    };
    use serde_json::{Value, json};
    use std::sync::atomic::{AtomicI64, Ordering};

    #[derive(Default)]
    struct Wire {
        share_right: AtomicI64,
        reads: Mutex<Vec<Value>>,
    }

    impl Api for Wire {
        fn request<'a>(
            &'a self,
            path: &'a str,
            payload: Value,
            cache: bool,
        ) -> ApiFuture<'a, Value> {
            Box::pin(async move {
                assert_eq!(path, "/api/song/enhance/privilege");
                assert!(!cache);
                let ids: Vec<String> =
                    serde_json::from_str(payload["ids"].as_str().unwrap()).unwrap();
                let [id] = ids.as_slice() else {
                    panic!("only the clicked Song may be checked");
                };
                self.reads.lock().unwrap().push(payload);
                Ok(
                    json!({"code":200,"data":[{"id":id,"sp":self.share_right.load(Ordering::SeqCst)}]}),
                )
            })
        }
    }

    struct Fixture {
        _root: tempfile::TempDir,
        wire: Arc<Wire>,
        session: Arc<AccountSession>,
        client: MusicClient,
        history: HistoryService,
        token: SessionToken,
        epoch: String,
    }

    impl Fixture {
        fn new(signed_in: bool) -> Self {
            let root = tempfile::tempdir().unwrap();
            let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
            let wire = Arc::new(Wire::default());
            let session = Arc::new(AccountSession::new(wire.clone(), Some(registry.store())));
            if signed_in {
                session
                    .adopt(wire.clone(), json!({"userId":9}), &session.token().unwrap())
                    .unwrap();
            }
            let history = HistoryService::new(session.clone()).unwrap();
            let token = session.token().unwrap();
            let epoch = token.generation.to_string();
            let client = MusicClient::with_session(session.clone(), registry);
            Self {
                _root: root,
                wire,
                session,
                client,
                history,
                token,
                epoch,
            }
        }

        fn record(&self, kind: HistoryKind, raw: Value, at: u64) {
            self.history
                .record_for(
                    &self.token,
                    kind,
                    HistoryRecord::local(kind, raw, at).unwrap(),
                )
                .unwrap();
        }

        fn target(
            &self,
            kind: HistoryKind,
            clicked: &str,
            selected: &[&str],
        ) -> HistoryRowMenuTarget {
            let revision = self
                .history
                .snapshot_for(&self.token, kind)
                .unwrap()
                .revision;
            HistoryRowMenuTarget {
                history_kind: kind,
                revision,
                clicked_id: clicked.into(),
                selected_ids: selected.iter().map(|id| (*id).into()).collect(),
            }
        }

        async fn prepare(&self, target: HistoryRowMenuTarget) -> HistoryRowSharePreparation {
            self.client
                .history_row_share_prepare(&self.epoch, target)
                .await
                .unwrap()
                .value
        }
    }

    fn receipt(result: HistoryRowSharePreparation) -> String {
        match result {
            HistoryRowSharePreparation::Ready { receipt } => receipt,
            other => panic!("expected authorized share receipt, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn song_checks_fresh_exact_privilege_before_prepare_and_each_one_shot_choice() {
        use HistoryRowShareOutcome as O;
        use HistoryRowSharePreparation as P;
        let f = Fixture::new(true);
        let kind = HistoryKind::Song;
        f.record(
            kind,
            json!({"id":12,"name":"曲目","privilege":{"fee":4,"st":0}}),
            100,
        );
        f.record(
            kind,
            json!({"id":13,"name":"下架曲目","privilege":{"st":-100}}),
            200,
        );
        assert_eq!(f.prepare(f.target(kind, "13", &[])).await, P::Unavailable);
        assert_eq!(
            f.prepare(f.target(kind, "12", &["13", "12"])).await,
            P::Unavailable
        );
        assert!(f.wire.reads.lock().unwrap().is_empty());
        let target = f.target(kind, "12", &[]);
        assert_eq!(f.prepare(target.clone()).await, P::AlbumRequired);
        f.wire.share_right.store(1, Ordering::SeqCst);
        let first = receipt(f.prepare(target.clone()).await);
        f.wire.share_right.store(0, Ordering::SeqCst);
        let denied = f
            .client
            .history_row_share_choose(
                &f.epoch,
                target.clone(),
                &first,
                HistoryRowShareChoice::Copy,
            )
            .await
            .unwrap()
            .value;
        assert_eq!(denied, O::AlbumRequired);
        assert!(
            f.client
                .history_row_share_choose(
                    &f.epoch,
                    target.clone(),
                    &first,
                    HistoryRowShareChoice::Copy
                )
                .await
                .is_err()
        );
        f.wire.share_right.store(1, Ordering::SeqCst);
        let second = receipt(f.prepare(target.clone()).await);
        let approved = f
            .client
            .history_row_share_choose(&f.epoch, target, &second, HistoryRowShareChoice::Copy)
            .await
            .unwrap()
            .value;
        assert_eq!(
            approved,
            O::Copy {
                url: "https://music.163.com/song?id=12".into()
            }
        );
        let reads = f.wire.reads.lock().unwrap();
        assert_eq!(reads.len(), 5);
        assert!(reads.iter().all(|call| call == &json!({"ids":r#"["12"]"#})));
    }

    #[tokio::test]
    async fn voice_share_is_clicked_only_and_scoped_to_account_and_ordered_gesture_without_a_song_read()
     {
        let f = Fixture::new(true);
        let kind = HistoryKind::Voice;
        for id in [3_u64, 4, 5] {
            f.record(kind, json!({"id":id,"name":"节目","mainSong":{"id":id+100,"duration":5000},"duration":5000,"radio":{"id":60,"name":"播客"}}), id);
        }
        let row = f.target(kind, "3", &["5", "4"]);
        let first = receipt(f.prepare(row.clone()).await);
        let reordered = HistoryRowMenuTarget {
            selected_ids: vec!["4".into(), "5".into()],
            ..row.clone()
        };
        assert!(
            f.client
                .history_row_share_choose(
                    &f.epoch,
                    reordered,
                    &first,
                    HistoryRowShareChoice::WechatQr
                )
                .await
                .is_err()
        );
        let qr = f
            .client
            .history_row_share_choose(
                &f.epoch,
                row.clone(),
                &first,
                HistoryRowShareChoice::WechatQr,
            )
            .await
            .unwrap()
            .value;
        assert_eq!(
            qr,
            HistoryRowShareOutcome::WechatQr {
                modules: qr_modules("https://music.163.com/dj?id=3").unwrap()
            }
        );
        let second = receipt(f.prepare(row.clone()).await);
        f.session.detach().unwrap();
        assert!(
            f.client
                .history_row_share_choose(&f.epoch, row, &second, HistoryRowShareChoice::Copy)
                .await
                .is_err()
        );
        assert!(f.wire.reads.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn share_cancel_is_exact_idempotent_and_never_reads_or_replays() {
        let f = Fixture::new(true);
        let kind = HistoryKind::Song;
        f.record(kind, json!({"id":12,"name":"曲目"}), 100);
        f.record(kind, json!({"id":13,"name":"另一首曲目"}), 200);
        f.wire.share_right.store(1, Ordering::SeqCst);
        let target = f.target(kind, "12", &[]);
        let other = f.target(kind, "13", &[]);
        let obsolete = receipt(f.prepare(target.clone()).await);
        let current = receipt(f.prepare(target.clone()).await);
        f.client
            .history_row_share_cancel(&f.epoch, target.clone(), &obsolete)
            .unwrap();
        f.client
            .history_row_share_cancel(&f.epoch, other, &current)
            .unwrap();
        let result = f
            .client
            .history_row_share_choose(
                &f.epoch,
                target.clone(),
                &current,
                HistoryRowShareChoice::Copy,
            )
            .await
            .unwrap()
            .value;
        assert!(matches!(result, HistoryRowShareOutcome::Copy { .. }));
        let canceled = receipt(f.prepare(target.clone()).await);
        for _ in 0..2 {
            f.client
                .history_row_share_cancel(&f.epoch, target.clone(), &canceled)
                .unwrap();
        }
        assert!(matches!(
            f.client
                .history_row_share_choose(&f.epoch, target, &canceled, HistoryRowShareChoice::Copy)
                .await,
            Err(Error::QueueChanged)
        ));
        assert_eq!(f.wire.reads.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn guest_scope_prompts_login_without_a_share_receipt_or_privilege_request() {
        let f = Fixture::new(false);
        let kind = HistoryKind::Song;
        f.record(kind, json!({"id":12,"name":"曲目"}), 100);
        assert_eq!(
            f.prepare(f.target(kind, "12", &[])).await,
            HistoryRowSharePreparation::LoginRequired
        );
        assert!(f.wire.reads.lock().unwrap().is_empty());
    }
}
