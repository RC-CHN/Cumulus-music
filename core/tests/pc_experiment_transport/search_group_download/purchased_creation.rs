//! M/3085:3475–3488, 3969–4017 preserves the purchased tier through FavouriteModal.
//! Its create row is shared with ordinary downloads (FavouriteModal:419–436,
//! 529–540); the ACK fences below verify Cumulus's guarded recovery.
use super::super::{ScriptHttp, gate, reply};
use super::{
    AUTH, ApiOrigin, C, COOKIE, D, DownloadCommand, DownloadService, Error, HttpResponse, LIKED,
    MusicClient, PlaylistCollectSort, Quality, R, Snapshot, StateStore, client, decision,
    directory, prepare, purchased_search_page, search, vinyl_http,
};
use cumulus_core::{
    account::AccountSession,
    playlist::{PlaylistMutation, PlaylistPhase, PlaylistService},
};
use serde_json::{Value, json};
use std::sync::Arc;

const LIST: &str = "/api/user/playlist/v2";
const OWNED: &str = "/api/user/playlist";
const DETAIL: &str = "/api/v6/playlist/detail";
const CREATE: &str = "/api/playlist/create";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";
const NAME: &str = "合成已购新歌单";
const CHOSEN: Quality = Quality::HiRes;
const OTHER: Quality = Quality::Sky;

fn playlist() -> Value {
    json!({"id":91,"userId":71,"name":NAME,"privacy":10,"specialType":0,"tags":[]})
}
fn owned(present: bool) -> HttpResponse {
    json!({"code":200,"playlist":if present {vec![playlist()]} else {vec![]},"more":false}).into()
}
fn detail(ids: &[u64]) -> HttpResponse {
    let mut row = playlist();
    row["trackIds"] = json!(ids.iter().map(|id| json!({"id":id})).collect::<Vec<_>>());
    json!({"code":200,"playlist":row}).into()
}
fn lost(operation: &str) -> HttpResponse {
    HttpResponse::Ready(Err(Error::Transport(format!("synthetic lost {operation}"))))
}

struct Fixture {
    client: MusicClient,
    session: Arc<AccountSession>,
    http: Arc<ScriptHttp>,
    epoch: String,
    receipt: String,
    _root: tempfile::TempDir,
}

impl Fixture {
    async fn open(create: HttpResponse, tail: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let (api, http) = vinyl_http(
            root.path(),
            &[COOKIE],
            [
                purchased_search_page(),
                json!({"code":200,"ids":[43]}).into(),
                owned(false), // verified ordinary destination list
                owned(false), // independent system-liked lookup
                owned(false), // fresh preflight list before create
                create,
                owned(false), // initially absent despite dispatch
                owned(true),  // explicit readback sees the new owned playlist
            ]
            .into_iter()
            .chain(tail),
        );
        StateStore::open(root.path().join("config"))
            .unwrap()
            .put(
                "transport",
                "pc-mconfig-v1",
                "pc",
                &serde_json::to_value(Snapshot {
                    origin: ApiOrigin::Pc,
                    version: 1,
                    bucket: String::new(),
                    generation: 1,
                    source_url: Some("https://p1.music.126.net/obj/fixture.data".into()),
                    data: Some(json!({})),
                })
                .unwrap(),
            )
            .unwrap();
        let (client, session, epoch) = client(root.path(), api);
        let rows = search(&client, &epoch).await;
        assert_eq!(rows[2]["groupDownload"]["right"], "purchased");
        assert_eq!(rows[0]["groupDownload"]["right"], "ordinary");
        client.liked_ids().await.unwrap();
        directory(&client, &epoch, root.path()).await;
        let receipt = decision(
            &client,
            &epoch,
            prepare(&rows, 1, &[2, 1, 0, 3]),
            D::PurchasedQualityRequired,
        )
        .await
        .unwrap();
        let f = Self {
            client,
            session,
            http,
            epoch,
            receipt,
            _root: root,
        };
        assert!(
            f.run(f.create()).await.is_err(),
            "cannot create before selecting the purchased tier"
        );
        assert!(matches!(
            f.run(f.confirm(CHOSEN)).await.unwrap(),
            R::QualityConfirmed {decision:D::PlaylistRequired, receipt:Some(grant), quality:CHOSEN, ..}
            if grant == f.receipt
        ));
        f.trace(&[LIKED]);
        assert!(matches!(
            f.run(C::Destinations {
                receipt: f.receipt.clone(), sort: PlaylistCollectSort::Default, page: 0,
            }).await.unwrap(),
            R::Destinations { can_create:true, system_liked_receipt:None, page }
            if page.value.playlists.is_empty()
        ));
        f.trace(&[LIKED, LIST, OWNED]);
        f
    }

    async fn run(&self, action: C) -> cumulus_core::Result<R> {
        self.client.search_group_download(&self.epoch, action).await
    }
    fn create(&self) -> C {
        C::CreatePlaylist {
            receipt: self.receipt.clone(),
            name: NAME.into(),
            private_playlist: true,
        }
    }
    fn confirm(&self, quality: Quality) -> C {
        C::ConfirmQuality {
            receipt: self.receipt.clone(),
            quality,
        }
    }
    fn status(&self, reconcile: bool) -> C {
        C::CreationStatus {
            receipt: self.receipt.clone(),
            reconcile,
        }
    }
    fn add(&self) -> C {
        C::AddCreated {
            receipt: self.receipt.clone(),
        }
    }
    fn start(&self, continuation: &str) -> C {
        C::StartFromPlaylist {
            receipt: self.receipt.clone(),
            continuation: continuation.into(),
        }
    }
    fn tasks(&self) -> DownloadService {
        DownloadService::new(self.session.clone()).unwrap()
    }
    fn records(&self) -> Vec<PlaylistMutation> {
        PlaylistService::new(self.session.clone())
            .unwrap()
            .operations(&self.session.token().unwrap())
            .unwrap()
    }
    fn trace(&self, expected: &[&str]) {
        let calls = self.http.calls();
        assert_eq!(
            calls[1..]
                .iter()
                .map(|call| call.path.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
    async fn reject_reselection(&self) {
        assert!(matches!(
            self.run(self.confirm(OTHER)).await,
            Err(Error::InvalidInput(_))
        ));
    }
}

fn creation(result: R) -> Value {
    let R::Creation { result } = result else {
        panic!("missing creation result");
    };
    serde_json::to_value(result).unwrap()
}

#[tokio::test]
async fn search_group_download_lost_purchased_create_ack_cannot_authorize_add_or_download() {
    let f = Fixture::open(lost("purchased create ACK"), []).await;
    let first = creation(f.run(f.create()).await.unwrap());
    assert_eq!(first["createPhase"], "uncertain");
    assert_eq!(first["canAdd"], false);
    assert!(first["add"].is_null());
    assert_eq!(creation(f.run(f.create()).await.unwrap()), first);
    f.trace(&[LIKED, LIST, OWNED, OWNED, CREATE, OWNED]);

    let inferred = creation(f.run(f.status(true)).await.unwrap());
    assert_eq!(
        inferred["createPhase"], "confirmed",
        "the owned-list delta can settle the journal"
    );
    assert_eq!(inferred["canAdd"], false);
    assert!(inferred["playlistId"].is_null() && inferred["add"].is_null());
    f.reject_reselection().await;
    assert!(matches!(f.run(f.add()).await, Err(Error::StaleResponse)));
    assert!(matches!(
        f.run(f.start("91")).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(creation(f.run(f.create()).await.unwrap()), inferred);
    assert_eq!(creation(f.run(f.status(true)).await.unwrap()), inferred);
    assert!(f.tasks().list().unwrap().is_empty());
    let records = f.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].command.kind(), "create");
    assert_eq!(records[0].playlist_id.as_deref(), Some("91"));
    assert_eq!(records[0].phase, PlaylistPhase::Confirmed);
    assert_eq!(records[0].error_class.as_deref(), Some("transport"));
    assert!(!records[0].codes.contains_key("main"));
    f.trace(&[LIKED, LIST, OWNED, OWNED, CREATE, OWNED, OWNED]);
}

#[tokio::test]
async fn search_group_download_late_purchased_create_preserves_the_tier_through_explicit_recovery()
{
    let (response, acknowledge) = gate();
    let denied = || json!({"code":200,"data":{"code":403}}).into();
    let f = Fixture::open(
        response,
        [
            detail(&[]),
            lost("purchased add ACK"),
            detail(&[]),
            detail(&[43, 41, 44]),
            denied(),
            denied(),
            denied(),
        ],
    )
    .await;
    let creating = f.run(f.create());
    tokio::pin!(creating);
    tokio::select! {
        result = &mut creating => panic!("create finished before the controlled acknowledgement: {:?}", result.err()),
        () = f.http.wait(6) => {}
    }
    assert!(matches!(
        f.run(C::Status {
            receipt: f.receipt.clone()
        })
        .await
        .unwrap(),
        R::Pending
    ));
    assert!(matches!(f.run(f.create()).await, Err(Error::MutationBusy)));
    assert!(matches!(f.run(f.add()).await, Err(Error::MutationBusy)));
    f.trace(&[LIKED, LIST, OWNED, OWNED, CREATE]);
    assert!(
        acknowledge
            .send(Ok(reply(json!({"code":200,"id":91}))))
            .is_ok()
    );
    let first = creation(creating.await.unwrap());
    assert_eq!(first["canAdd"], false);
    assert!(first["playlistId"].is_null() && first["add"].is_null());
    f.reject_reselection().await;
    assert!(matches!(f.run(f.add()).await, Err(Error::StaleResponse)));
    assert_eq!(creation(f.run(f.status(false)).await.unwrap()), first);
    f.trace(&[LIKED, LIST, OWNED, OWNED, CREATE, OWNED]);

    let recovered = creation(f.run(f.status(true)).await.unwrap());
    assert_eq!(recovered["createPhase"], "confirmed");
    assert_eq!(recovered["playlistId"], "91");
    assert_eq!(recovered["canAdd"], true);
    assert!(recovered["add"].is_null() && f.tasks().list().unwrap().is_empty());
    assert_eq!(creation(f.run(f.create()).await.unwrap()), recovered);
    f.reject_reselection().await;
    f.trace(&[LIKED, LIST, OWNED, OWNED, CREATE, OWNED, OWNED]);

    let settings = f.client.download_quality_preferences(&f.epoch).unwrap();
    let mut options = settings.options;
    options.quality = Quality::ExHigh;
    options.cover = false;
    f.client
        .download_command(
            &f.epoch,
            DownloadCommand::Preferences {
                revision: settings.revision,
                options,
            },
        )
        .await
        .unwrap();

    let pending_add = creation(f.run(f.add()).await.unwrap());
    assert_eq!(pending_add["add"]["phase"], "uncertain");
    assert!(pending_add["add"]["continuation"].is_null());
    assert_eq!(creation(f.run(f.add()).await.unwrap()), pending_add);
    assert!(f.tasks().list().unwrap().is_empty());
    f.trace(&[
        LIKED, LIST, OWNED, OWNED, CREATE, OWNED, OWNED, DETAIL, ADD, DETAIL,
    ]);

    let verified = creation(f.run(f.status(true)).await.unwrap());
    assert_eq!(verified["add"]["phase"], "confirmed");
    let continuation = verified["add"]["continuation"].as_str().unwrap();
    assert_eq!(creation(f.run(f.status(true)).await.unwrap()), verified);
    assert!(matches!(
        f.run(f.start("forged")).await,
        Err(Error::StaleResponse)
    ));
    f.reject_reselection().await;
    assert!(f.tasks().list().unwrap().is_empty());
    f.trace(&[
        LIKED, LIST, OWNED, OWNED, CREATE, OWNED, OWNED, DETAIL, ADD, DETAIL, DETAIL,
    ]);

    let R::Accepted { result } = f.run(f.start(continuation)).await.unwrap() else {
        panic!("verified creation and collection must accept the selected download");
    };
    assert_eq!(result.accepted.len(), 3);
    assert!(matches!(
        f.run(f.start(continuation)).await.unwrap(),
        R::Accepted {result: repeated} if repeated.accepted == result.accepted
    ));
    let tasks = f.tasks().list().unwrap();
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.song_id.as_str())
            .collect::<Vec<_>>(),
        ["44", "41", "43"]
    );
    assert!(
        tasks
            .iter()
            .all(|task| task.options.quality == CHOSEN && !task.options.cover)
    );
    assert_eq!(
        f.client
            .download_quality_preferences(&f.epoch)
            .unwrap()
            .options
            .quality,
        Quality::ExHigh
    );
    let mut operations = f
        .records()
        .into_iter()
        .map(|record| {
            assert_eq!(record.phase, PlaylistPhase::Confirmed);
            assert_eq!(record.playlist_id.as_deref(), Some("91"));
            record.command.kind()
        })
        .collect::<Vec<_>>();
    operations.sort_unstable();
    assert_eq!(operations, ["create", "tracks"]);
    f.http.wait(15).await;
    f.trace(&[
        LIKED, LIST, OWNED, OWNED, CREATE, OWNED, OWNED, DETAIL, ADD, DETAIL, DETAIL, AUTH, AUTH,
        AUTH,
    ]);
    let calls = f.http.calls();
    assert_eq!(calls[5].payload["privacy"], "10");
    assert_eq!(calls[9].payload["pid"], "91");
    assert_eq!(calls[9].payload["trackIds"], r#"["43","41","44"]"#);
    assert!(
        calls[12..]
            .iter()
            .all(|call| call.payload["level"] == "hires")
    );
}
