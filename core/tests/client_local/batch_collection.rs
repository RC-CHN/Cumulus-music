use super::*;
use cumulus_core::{
    playlist::PlaylistPhase,
    protocol::api::{GuardedPlaylistAdd, PlaylistAddPermission},
};
use std::sync::Mutex;
use tokio::sync::Notify;

#[derive(Default)]
struct Collector {
    writes: Mutex<Vec<Value>>,
    reads: Mutex<Vec<(String, Value)>>,
    tracks: Mutex<Vec<String>>,
    duplicate: AtomicBool,
    lose_reply: AtomicBool,
    hold_detail: AtomicBool,
    detail_started: Notify,
    detail_release: Notify,
}

fn target(id: u32, owner: u32, special: Value) -> Value {
    json!({"id":id, "userId":owner, "creator":{"userId":owner}, "name":format!("list {id}"),
        "privacy":0, "specialType":special, "trackCount":0, "tags":[]})
}

impl Api for Collector {
    fn local_collection_scope_revision(&self) -> Result<Option<String>> {
        Ok(Some("synthetic-local-collector".into()))
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, cached: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cached);
            self.reads.lock().unwrap().push((path.into(), args.clone()));
            match path {
                "/api/v3/song/detail" => {
                    let rows: Value = serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    let id = &rows[0]["id"];
                    // Local rows keep privilege nested in onlineTrack in M170.
                    // The toolbar's filter never promotes this nested subp.
                    Ok(
                        json!({"code":200,"songs":[{"id":id,"name":"sealed","privilege":{"id":id,"subp":-1}}]}),
                    )
                }
                "/api/user/playlist/v2" => {
                    assert_eq!(
                        args,
                        json!({"uid":"9","offset":0,"limit":10,"sortType":"default"})
                    );
                    let mut mismatch = target(75, 9, json!(0));
                    mismatch["creator"]["userId"] = json!(4);
                    Ok(json!({"code":200,"more":false,"playlist":[
                        target(81, 9, json!(0)), target(82, 9, json!(0)), target(77, 4, json!(0)),
                        target(98, 9, json!(5)), target(71, 9, json!("0")), mismatch]}))
                }
                "/api/v6/playlist/detail" => {
                    if self.hold_detail.swap(false, Ordering::SeqCst) {
                        self.detail_started.notify_one();
                        self.detail_release.notified().await;
                    }
                    if self.lose_reply.load(Ordering::SeqCst)
                        && !self.writes.lock().unwrap().is_empty()
                    {
                        return Err(Error::State("synthetic observation unavailable"));
                    }
                    let id = args["id"].as_str().unwrap().parse().unwrap();
                    let tracks = self.tracks.lock().unwrap().clone();
                    let mut row = target(id, 9, json!(0));
                    row["trackCount"] = json!(tracks.len());
                    row["trackIds"] =
                        json!(tracks.iter().map(|id| json!({"id":id})).collect::<Vec<_>>());
                    Ok(json!({"code":200,"playlist":row}))
                }
                _ => panic!("unexpected sealed request {path}"),
            }
        })
    }

    fn request_guarded_playlist_add<'a>(
        &'a self,
        write: GuardedPlaylistAdd,
        permission: &'a PlaylistAddPermission<'a>,
        dispatched: &'a AtomicBool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            permission()?;
            dispatched.store(true, Ordering::SeqCst);
            let (path, args) = write.into_request()?;
            assert_eq!(path, "/api/v1/playlist/manipulate/tracks");
            let ids: Vec<String> =
                serde_json::from_str(args["trackIds"].as_str().unwrap()).unwrap();
            self.writes.lock().unwrap().push(args);
            if self.duplicate.load(Ordering::SeqCst) {
                return Ok(json!({"code":502}));
            }
            let mut tracks = self.tracks.lock().unwrap();
            for id in ids {
                if !tracks.contains(&id) {
                    tracks.push(id);
                }
            }
            if self.lose_reply.load(Ordering::SeqCst) {
                return Err(Error::State("synthetic lost write reply"));
            }
            Ok(json!({"code":200}))
        })
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    media: std::path::PathBuf,
    api: Arc<Collector>,
    session: Arc<AccountSession>,
    client: MusicClient,
    library: LocalLibrary,
    query: LocalQuery,
    page: LocalPageView,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("media");
        std::fs::create_dir(&media).unwrap();
        for name in ["A", "B", "C"] {
            std::fs::write(media.join(format!("{name}.wav")), b"synthetic fingerprint").unwrap();
        }
        let (session, client, library) =
            client(&root.path().join("state"), Arc::new(Probe::default()));
        library.scan(&media).unwrap();
        let api = Arc::<Collector>::default();
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let query = LocalQuery {
            descending: false,
            ..Default::default()
        };
        let page = client.local_page(query.clone()).await.unwrap();
        Self {
            _root: root,
            media,
            api,
            session,
            client,
            library,
            query,
            page,
        }
    }

    fn selection(&self, indices: &[usize]) -> LocalCollectSelection {
        let tracks = indices
            .iter()
            .map(|&index| {
                let row = &self.page.tracks[index];
                json!({"id":row.track.id,"revision":row.revision})
            })
            .collect::<Vec<_>>();
        serde_json::from_value(
            json!({"query":self.query,"revision":self.page.revision,"tracks":tracks}),
        )
        .unwrap()
    }

    async fn bind(&self, index: usize, song: &str) {
        let row = &self.page.tracks[index];
        let item = self.library.item(&row.track.id).unwrap();
        cumulus_core::library::matching::LocalMatching::new(
            self.session.clone(),
            self.library.clone(),
        )
        .unwrap()
        .bind(&row.track.id, &item.revision, &json!(song))
        .await
        .unwrap();
    }

    async fn prepare(&self) -> String {
        let plan = self
            .client
            .local_collect_prepare("1", self.selection(&[1, 0]))
            .await
            .unwrap();
        assert_eq!(plan.value.count, 2);
        plan.value.receipt
    }

    async fn destinations(&self, receipt: &str) {
        let reply = self
            .client
            .local_collect_destinations("1", receipt, PlaylistCollectSort::Default, 0)
            .await
            .unwrap();
        assert_eq!(
            reply
                .value
                .playlists
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["81", "82"]
        );
        assert_eq!(reply.value.next_page, None);
    }

    async fn choose(&self, receipt: &str, id: &str, reconcile: bool) -> Result<LocalCollectResult> {
        Ok(self
            .client
            .local_collect_existing("1", receipt, id, reconcile)
            .await?
            .value)
    }
}

#[tokio::test]
async fn local_collection_sends_display_order_once_and_observes_official_duplicate_failure() {
    let f = Fixture::new().await;
    f.bind(0, "802").await;
    f.bind(1, "801").await;
    f.api.reads.lock().unwrap().clear();
    let receipt = f.prepare().await;
    assert!(
        f.api.reads.lock().unwrap().is_empty(),
        "prepare is native only"
    );
    assert!(f.choose(&receipt, "81", false).await.is_err());
    f.destinations(&receipt).await;
    assert!(f.choose(&receipt, "98", false).await.is_err());
    assert!(f.choose(&receipt, "77", false).await.is_err());
    assert!(f.choose(&receipt, "81", true).await.is_err());
    assert!(f.api.writes.lock().unwrap().is_empty());
    let result = f.choose(&receipt, "81", false).await.unwrap();
    assert_eq!(result.phase, PlaylistPhase::Confirmed);
    assert!(!result.duplicate);
    assert!(f.choose(&receipt, "81", false).await.is_err());
    assert_eq!(
        *f.api.writes.lock().unwrap(),
        [json!({"pid":"81","trackIds":"[\"802\",\"801\"]","op":"add"})]
    );
    assert_eq!(
        f.api
            .reads
            .lock()
            .unwrap()
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>(),
        [
            "/api/user/playlist/v2",
            "/api/v6/playlist/detail",
            "/api/v6/playlist/detail"
        ]
    );

    f.client.local_collect_cancel("1", &receipt).unwrap();
    f.api.duplicate.store(true, Ordering::SeqCst);
    let receipt = f.prepare().await;
    f.destinations(&receipt).await;
    let result = f.choose(&receipt, "81", false).await.unwrap();
    assert_eq!(result.phase, PlaylistPhase::Rejected);
    assert!(result.duplicate);
    assert_eq!(
        f.api.writes.lock().unwrap().len(),
        2,
        "do not synthesize a no-op when all IDs are present"
    );
    assert_eq!(
        f.choose(&receipt, "81", true).await.unwrap().phase,
        PlaylistPhase::Rejected
    );
    assert_eq!(f.api.writes.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn local_collection_rejects_unmapped_id_only_duplicate_and_late_file_or_epoch() {
    let f = Fixture::new().await;
    f.bind(0, "802").await;
    assert!(
        f.client
            .local_collect_prepare("1", f.selection(&[0, 1]))
            .await
            .is_err()
    );
    f.library
        .map_online(&f.session, &f.page.tracks[1].track.id, &json!(801))
        .unwrap();
    assert!(
        f.client
            .local_collect_prepare("1", f.selection(&[0, 1]))
            .await
            .is_err()
    );
    f.bind(1, "802").await;
    assert!(
        f.client
            .local_collect_prepare("1", f.selection(&[0, 1]))
            .await
            .is_err()
    );
    f.bind(1, "801").await;
    let receipt = f.prepare().await;
    f.destinations(&receipt).await;
    f.api.hold_detail.store(true, Ordering::SeqCst);
    let held = f.choose(&receipt, "81", false);
    let drift = async {
        f.api.detail_started.notified().await;
        std::fs::write(
            f.media.join("A.wav"),
            b"replacement changes native fingerprint",
        )
        .unwrap();
        f.api.detail_release.notify_one();
    };
    let (result, ()) =
        tokio::time::timeout(Duration::from_secs(4), async { tokio::join!(held, drift) })
            .await
            .unwrap();
    assert!(matches!(result, Err(Error::QueueChanged)));
    assert!(f.api.writes.lock().unwrap().is_empty());
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(f.choose(&receipt, "81", false).await.is_err());
    assert!(f.api.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn local_collection_lost_reply_has_only_explicit_readback_and_one_durable_write() {
    let f = Fixture::new().await;
    f.bind(0, "802").await;
    f.bind(1, "801").await;
    let receipt = f.prepare().await;
    f.destinations(&receipt).await;
    f.api.lose_reply.store(true, Ordering::SeqCst);
    let result = f.choose(&receipt, "81", false).await.unwrap();
    assert_eq!(result.phase, PlaylistPhase::Uncertain);
    assert!(f.choose(&receipt, "81", false).await.is_err());
    assert!(f.choose(&receipt, "82", true).await.is_err());
    let journal = f.client.playlist_operations("1").unwrap().value;
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].phase, PlaylistPhase::Uncertain);
    assert_eq!(f.api.writes.lock().unwrap().len(), 1);

    // Read-only recovery uses the stored write, even if the source disappears.
    std::fs::remove_file(f.media.join("A.wav")).unwrap();
    f.api.lose_reply.store(false, Ordering::SeqCst);
    assert_eq!(
        f.choose(&receipt, "81", true).await.unwrap().phase,
        PlaylistPhase::Confirmed
    );
    assert_eq!(f.api.writes.lock().unwrap().len(), 1);
    assert!(f.client.playlist_operations("1").unwrap().value.is_empty());
    let durable = cumulus_core::playlist::PlaylistService::new(f.session.clone())
        .unwrap()
        .operations(&f.session.token().unwrap())
        .unwrap();
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].operation_id, journal[0].operation_id);
    assert_eq!(durable[0].phase, PlaylistPhase::Confirmed);
}

#[tokio::test]
async fn local_collection_requires_the_exact_mapped_cue_segments_and_current_sheet() {
    let mut f = Fixture::new().await;
    let sheet = f.media.join("A.cue");
    std::fs::write(
        &sheet,
        "FILE \"A.wav\" WAVE\nTRACK 01 AUDIO\nTITLE \"Cue A\"\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nTITLE \"Cue B\"\nINDEX 01 00:30:00\n",
    )
    .unwrap();
    f.library.scan(&f.media).unwrap();
    f.query.keyword = "Cue".into();
    f.page = f.client.local_page(f.query.clone()).await.unwrap();
    assert_eq!(f.page.tracks.len(), 2);
    assert!(f.page.tracks.iter().all(|row| row.cue));
    f.bind(0, "802").await;
    f.bind(1, "801").await;
    let receipt = f.prepare().await;
    f.destinations(&receipt).await;
    // Only the physical sheet changes; the source audio and indexed CUE IDs
    // remain, but these previously mapped segment revisions are stale.
    std::fs::write(&sheet, b"changed without rescanning").unwrap();
    assert!(matches!(
        f.choose(&receipt, "81", false).await,
        Err(Error::QueueChanged)
    ));
    assert!(f.api.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn local_collection_unchanged_membership_cannot_settle_a_lost_duplicate_reply() {
    let f = Fixture::new().await;
    f.bind(0, "802").await;
    f.bind(1, "801").await;
    *f.api.tracks.lock().unwrap() = vec!["802".into(), "801".into()];
    let receipt = f.prepare().await;
    f.destinations(&receipt).await;
    f.api.lose_reply.store(true, Ordering::SeqCst);
    assert_eq!(
        f.choose(&receipt, "81", false).await.unwrap().phase,
        PlaylistPhase::Uncertain
    );
    f.api.lose_reply.store(false, Ordering::SeqCst);
    assert_eq!(
        f.choose(&receipt, "81", true).await.unwrap().phase,
        PlaylistPhase::Uncertain
    );
    let journal = f.client.playlist_operations("1").unwrap().value;
    assert_eq!(journal.len(), 1);
    f.client.local_collect_cancel("1", &receipt).unwrap();
    assert_eq!(
        f.client
            .reconcile_playlist("1", &journal[0].operation_id)
            .await
            .unwrap()
            .value
            .phase,
        PlaylistPhase::Uncertain,
        "standalone journal recovery must not bypass local duplicate uncertainty",
    );
    assert_eq!(f.api.writes.lock().unwrap().len(), 1);
}
