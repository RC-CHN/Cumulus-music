#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    history::{HistoryKind, HistoryRecord, HistoryService},
    membership::Quality,
    podcast::PodcastScope,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;
#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<String>>,
    chapter_reads: Mutex<Vec<Value>>,
    deny: AtomicBool,
    hold: AtomicBool,
    resume_books: AtomicBool,
    around_missing: AtomicBool,
    started: Notify,
    release: Notify,
}
fn song(id: &Value) -> Value {
    json!({"id":id,"name":"歌曲","dt":120000,"ar":[{"id":12,"name":"创作者"}],"al":{"id":7,"name":"专辑","picUrl":"https://p1.music.126.net/image.jpg?token=PRIVATE"},"url":"https://example.invalid/PRIVATE","cookie":"PRIVATE"})
}
fn radio(id: &Value) -> Value {
    json!({"id":id,"name":"播客或书","programCount":2,"categoryId":if id=="10" {10001}else{1},"picUrl":"https://p1.music.126.net/image.jpg?token=PRIVATE"})
}
fn voice(id: u64, radio_id: &Value) -> Value {
    json!({"id":id,"name":"声音","mainSong":{"id":id+1000,"duration":120000},"duration":120000,"radio":radio(radio_id),"url":"https://example.invalid/PRIVATE"})
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(path.into());
            Ok(match path {
                "/api/v3/song/detail" => {
                    if self.hold.load(Ordering::SeqCst) {
                        self.started.notify_one();
                        self.release.notified().await;
                    }
                    let ids: Vec<Value> = serde_json::from_str(p["c"].as_str().unwrap()).unwrap();
                    json!({"code":200,"songs":ids.iter().map(|v|song(&v["id"])).collect::<Vec<_>>()})
                }
                "/api/song/enhance/privilege" => {
                    if self.deny.load(Ordering::SeqCst) {
                        return Err(Error::Transport("privilege read failed".into()));
                    }
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":ids.into_iter().map(|id|json!({"id":id,"pl":320000})).collect::<Vec<_>>()})
                }
                "/api/album/privilege" => {
                    json!({"code":200,"data":[{"id":1,"pl":320000},{"id":2,"pl":320000}]})
                }
                "/api/resource-exposure/config" => {
                    json!({"code":200,"data":{"resourceType":"other"}})
                }
                "/api/album/v3/detail" => {
                    json!({"code":200,"album":{"id":7,"name":"专辑"},"songs":[song(&json!(1)),song(&json!(2))]})
                }
                "/api/v6/playlist/detail" => {
                    json!({"code":200,"playlist":{"trackIds":[{"id":1},{"id":3,"v":-1},{"id":2}]}})
                }
                "/api/djradio/v3/get" => json!({"code":200,"data":radio(&p["id"])}),
                "/api/voice/book/recent" => {
                    assert_eq!(p, json!({}));
                    let mut chapter = voice(6, &json!(22));
                    chapter["radio"]["subCount"] = json!(154);
                    chapter["djPlayRecordVo"] =
                        json!({"id":"6","listenLocation":2500,"isListened":false});
                    let next = (30..50).map(|id| voice(5, &json!(id))).collect::<Vec<_>>();
                    json!({"code":200,"data":{"currentList":[chapter],"yesterdayList":next[..19],"passingList":next[19..]}})
                }
                "/api/v6/dj/program/byradio" => {
                    if p["radioId"] == "22" {
                        let limit = p["limit"].as_u64().unwrap();
                        assert!(matches!(limit, 200 | 250));
                        let mut expected = json!({"radioId":"22","limit":limit,"offset":0});
                        if let Some(asc) = p["asc"].as_bool() {
                            expected["asc"] = json!(asc);
                        } else {
                            assert_eq!(limit, 250);
                        }
                        assert_eq!(p, expected);
                        self.chapter_reads.lock().unwrap().push(p.clone());
                    }
                    let mut second = voice(6, &p["radioId"]);
                    if p["radioId"] == "22" {
                        second["name"] = json!("本机保存的章节名");
                    }
                    let mut rows = vec![voice(5, &p["radioId"]), second];
                    if p["asc"] == false {
                        rows.reverse();
                    }
                    json!({"code":200,"data":{"programs":rows,"count":2,"more":false,"asc":p["asc"]}})
                }
                "/api/content/my/liked/voice" => {
                    json!({"code":200,"data":{"data":[],"hasMore":false}})
                }
                "/api/dj/around/program/v1" => {
                    assert_eq!(p, json!({"programId":"6","roundNum":250}));
                    let ids = if self.around_missing.load(Ordering::SeqCst) {
                        vec![5, 7]
                    } else {
                        vec![5, 6, 7]
                    };
                    json!({"code":200,"data":{"list":ids.into_iter().map(|id|voice(id,&json!(22))).collect::<Vec<_>>(),"asc":true}})
                }
                "/api/dj/program/detail" => {
                    let (id, radio_id) = match p["id"].as_str() {
                        Some("5") => (5, 8),
                        Some("6") => (6, 22),
                        Some("7") => (7, 22),
                        _ => panic!("unexpected history program detail identity"),
                    };
                    json!({"code":200,"program":voice(id,&json!(radio_id))})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":u8::from(p["radioId"] == "22" && self.resume_books.load(Ordering::SeqCst))}})
                }
                "/api/dj/play/url/check" => {
                    let id = p["voiceId"]
                        .as_u64()
                        .or_else(|| p["voiceId"].as_str()?.parse().ok())
                        .unwrap();
                    json!({"code":200,"data":{"voiceId":p["voiceId"],"songId":1000 + id,"canPlay":true}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(p["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/synthetic.mp3?key=PRIVATE","md5":"abc","expi":120,"level":"standard"}]})
                }
                "/api/song/lyric/v1" => json!({"code":200,"nolyric":true}),
                _ => panic!("unexpected history test endpoint: {path}"),
            })
        })
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    api: Arc<Backend>,
    session: Arc<AccountSession>,
    client: Arc<MusicClient>,
    history: HistoryService,
    store: Arc<cumulus_core::storage::StateStore>,
}
fn fixture(authenticated: bool) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    if authenticated {
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
    }
    let history = HistoryService::new(session.clone()).unwrap();
    let client = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default()))));
    Fixture {
        _root: root,
        api,
        session,
        client: Arc::new(client),
        history,
        store,
    }
}
impl Fixture {
    fn seed(&self, kind: HistoryKind) -> String {
        let raw = match kind {
            HistoryKind::Song => song(&json!("90071992547409931234")),
            HistoryKind::Album => {
                json!({"id":7,"name":"专辑","artists":[{"id":12,"name":"创作者"}],"picUrl":"https://p1.music.126.net/image.jpg?auth=PRIVATE"})
            }
            HistoryKind::Playlist => {
                json!({"id":6,"name":"歌单","creator":{"nickname":"创作者"},"coverImgUrl":"https://p1.music.126.net/image.jpg?auth=PRIVATE"})
            }
            HistoryKind::Voice => voice(5, &json!(8)),
            HistoryKind::Podcast => radio(&json!(8)),
            HistoryKind::Audiobook => radio(&json!("10")),
        };
        let row = HistoryRecord::local(kind, raw, 100).unwrap();
        let id = row.id.clone();
        self.history
            .record_for(&self.session.token().unwrap(), kind, row)
            .unwrap();
        id
    }
    fn epoch(&self) -> String {
        self.session.token().unwrap().generation.to_string()
    }
    async fn selection(&self, kind: HistoryKind, ids: Vec<String>) -> PlaySelection {
        let page = self
            .client
            .history_page(&self.epoch(), kind, false)
            .await
            .unwrap()
            .value;
        PlaySelection::History {
            history_kind: kind,
            revision: page.revision,
            ids,
            index: 0,
            quality: Quality::Standard,
        }
    }
}
#[tokio::test]
async fn six_typed_pages_redact_native_secrets_preserve_large_ids_and_isolate_accounts() {
    let f = fixture(true);
    for kind in HistoryKind::ALL {
        let id = f.seed(kind);
        let page = f
            .client
            .history_page(&f.epoch(), kind, false)
            .await
            .unwrap();
        let data = serde_json::to_value(page.value).unwrap();
        assert_eq!(data["entries"][0]["id"], id);
        let encoded = data.to_string();
        assert!(
            !encoded.contains("PRIVATE")
                && !encoded.contains("example.invalid")
                && !encoded.contains("cookie")
        );
        assert_eq!(data["entries"][0]["device"], "本机");
    }
    assert!(f.api.calls.lock().unwrap().is_empty());
    let old = f.epoch();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client.history_page(&old, HistoryKind::Song, false).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        f.client
            .history_page(&f.epoch(), HistoryKind::Song, false)
            .await
            .unwrap()
            .value
            .entries
            .is_empty()
    );
}
#[tokio::test]
async fn six_history_routes_expand_real_native_queues_and_refresh_rights() {
    for kind in HistoryKind::ALL {
        let f = fixture(true);
        let id = f.seed(kind);
        let selected = f.selection(kind, vec![id]).await;
        let state = f.client.play_selection(&f.epoch(), selected).await.unwrap();
        assert_eq!(
            state.queue_len,
            if matches!(kind, HistoryKind::Song | HistoryKind::Voice) {
                1
            } else {
                2
            }
        );
        assert!(state.error.is_none());
        assert_eq!(serde_json::to_value(state.state).unwrap(), "playing");
        assert!(!serde_json::to_string(&state).unwrap().contains("PRIVATE"));
        let calls = f.api.calls.lock().unwrap();
        if matches!(
            kind,
            HistoryKind::Song | HistoryKind::Album | HistoryKind::Playlist
        ) {
            assert!(calls.iter().any(|c| c == "/api/song/enhance/privilege"));
        } else {
            assert!(calls.iter().any(|c| c == "/api/dj/play/url/check"));
        }
    }
}
#[tokio::test]
async fn stale_foreign_duplicate_and_reordered_history_selections_fail_before_metadata_reads() {
    for mode in ["stale", "foreign", "duplicate", "order"] {
        let f = fixture(true);
        let first = f.seed(HistoryKind::Song);
        f.history
            .record_for(
                &f.session.token().unwrap(),
                HistoryKind::Song,
                HistoryRecord::local(HistoryKind::Song, song(&json!(2)), 99).unwrap(),
            )
            .unwrap();
        let mut selected = f
            .selection(HistoryKind::Song, vec![first.clone(), "2".into()])
            .await;
        if let PlaySelection::History { revision, ids, .. } = &mut selected {
            match mode {
                "stale" => *revision = "stale".into(),
                "foreign" => *ids = vec!["404".into()],
                "duplicate" => *ids = vec![first.clone(), first],
                _ => ids.reverse(),
            }
        }
        assert!(f.client.play_selection(&f.epoch(), selected).await.is_err());
        assert!(f.api.calls.lock().unwrap().is_empty());
    }
}
#[tokio::test]
async fn failed_privilege_refresh_cannot_replace_a_playing_queue() {
    let f = fixture(true);
    let id = f.seed(HistoryKind::Song);
    let state = f
        .client
        .play_selection(
            &f.epoch(),
            f.selection(HistoryKind::Song, vec![id.clone()]).await,
        )
        .await
        .unwrap();
    f.api.deny.store(true, Ordering::SeqCst);
    assert!(
        f.client
            .play_selection(&f.epoch(), f.selection(HistoryKind::Song, vec![id]).await)
            .await
            .is_err()
    );
    let after = f
        .client
        .subscribe_playback(&f.epoch())
        .await
        .unwrap()
        .current()
        .unwrap();
    assert_eq!(state.entry_id, after.entry_id);
    assert_eq!(state.queue_len, after.queue_len);
}
#[tokio::test]
async fn account_switch_during_history_metadata_read_discards_late_queue() {
    let f = fixture(true);
    let id = f.seed(HistoryKind::Song);
    let selected = f.selection(HistoryKind::Song, vec![id]).await;
    f.api.hold.store(true, Ordering::SeqCst);
    let client = f.client.clone();
    let epoch = f.epoch();
    let task = tokio::spawn(async move { client.play_selection(&epoch, selected).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), f.api.started.notified())
        .await
        .unwrap();
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    f.api.release.notify_one();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
}
#[tokio::test]
async fn guest_history_does_not_request_remote_resources() {
    let f = fixture(false);
    let page = f
        .client
        .history_page(&f.epoch(), HistoryKind::Song, true)
        .await
        .unwrap()
        .value;
    assert!(!page.can_sync && page.entries.is_empty());
    assert!(f.api.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unusable_download_reference_falls_back_only_through_fresh_online_metadata_and_rights() {
    let f = fixture(true);
    let mut resource = song(&json!(1));
    resource["download_task_id"] = json!(uuid::Uuid::new_v4().to_string());
    f.history
        .record_for(
            &f.session.token().unwrap(),
            HistoryKind::Song,
            HistoryRecord::local(HistoryKind::Song, resource, 100).unwrap(),
        )
        .unwrap();
    let state = f
        .client
        .play_selection(
            &f.epoch(),
            f.selection(HistoryKind::Song, vec!["1".into()]).await,
        )
        .await
        .unwrap();
    assert_eq!(state.track.unwrap().id, "1");
    let calls = f.api.calls.lock().unwrap();
    let rights = calls
        .iter()
        .position(|p| p == "/api/song/enhance/privilege")
        .unwrap();
    let media = calls
        .iter()
        .position(|p| p == "/api/song/enhance/player/url/v1")
        .unwrap();
    assert!(rights < media);
}

#[tokio::test]
async fn listened_books_owns_local_pages_and_exact_chapter_selection() {
    let f = fixture(true);
    let epoch = f.epoch();
    let page = f
        .client
        .listened_books(&epoch, ListenedBooksQuery::Open)
        .await
        .unwrap();
    let value = serde_json::to_value(page.value).unwrap();
    let revision = value["revision"].as_str().unwrap().to_owned();
    assert_eq!(value["total"], 21);
    assert_eq!(value["pages"], 2);
    assert_eq!(value["number"], 1);
    assert_eq!(value["rows"].as_array().unwrap().len(), 20);
    assert_eq!(value["rows"][0]["radio"]["id"], "22");
    assert_eq!(value["rows"][0]["subscribers"], 154);
    assert_eq!(value["rows"][0]["chapter"]["percent"], 2);
    assert_eq!(value["rows"][19]["radio"]["id"], "48");
    assert!(!value.to_string().contains("PRIVATE"));
    let next = f
        .client
        .listened_books(
            &epoch,
            ListenedBooksQuery::Page {
                revision: revision.clone(),
                number: 2,
            },
        )
        .await
        .unwrap();
    let next = serde_json::to_value(next.value).unwrap();
    assert_eq!(next["rows"].as_array().unwrap().len(), 1);
    assert_eq!(next["rows"][0]["radio"]["id"], "49");
    assert_eq!(*f.api.calls.lock().unwrap(), ["/api/voice/book/recent"]);

    let playback = f
        .client
        .play_selection(
            &epoch,
            PlaySelection::ListenedBookChapter {
                revision: revision.clone(),
                key: 0,
            },
        )
        .await
        .unwrap();
    assert_eq!(playback.queue_len, 2);
    let track = serde_json::to_value(playback.track).unwrap();
    assert_eq!(track["voiceId"], "6");
    assert_eq!(track["id"], "1006");
    {
        let calls = f.api.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|path| *path == "/api/voice/book/recent")
                .count(),
            1
        );
        assert!(
            calls
                .windows(2)
                .any(|paths| { paths == ["/api/dj/program/detail", "/api/dj/play/url/check"] })
        );
        assert!(!calls.iter().any(|path| path.contains("/play-record/")));
    }

    f.client
        .listened_books(&epoch, ListenedBooksQuery::Open)
        .await
        .unwrap();
    assert!(matches!(
        f.client
            .play_selection(
                &epoch,
                PlaySelection::ListenedBookChapter {
                    revision: revision.clone(),
                    key: 0
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":10}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        f.client
            .listened_books(
                &f.epoch(),
                ListenedBooksQuery::Page {
                    revision,
                    number: 1
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
}

#[tokio::test]
async fn listened_books_replays_matching_departed_chapter_in_official_around_order() {
    let f = fixture(true);
    let epoch = f.epoch();
    let token = f.session.token().unwrap();
    f.history.set_sync_for(&token, false).unwrap();
    f.api.resume_books.store(true, Ordering::SeqCst);
    let open = |epoch: &str| {
        let client = f.client.clone();
        let epoch = epoch.to_owned();
        async move {
            let page = client
                .listened_books(&epoch, ListenedBooksQuery::Open)
                .await
                .unwrap();
            serde_json::to_value(page.value).unwrap()
        }
    };
    let select = |page: &Value| PlaySelection::ListenedBookChapter {
        revision: page["revision"].as_str().unwrap().to_owned(),
        key: 0,
    };
    let first = open(&epoch).await;
    assert_eq!(first["rows"][0]["chapter"]["recent"], false);
    let remote = f
        .client
        .play_selection(&epoch, select(&first))
        .await
        .unwrap();
    assert_eq!(remote.position, Some(2.5));
    assert_eq!(remote.queue_len, 2);
    let seek = f
        .client
        .player_action(&epoch, PlayerAction::Seek { seconds: 18.75 })
        .await
        .unwrap();
    assert_eq!(seek.position, Some(18.75));
    assert!(
        f.store
            .get("9", "podcast-latest-voice-v1", "22")
            .unwrap()
            .is_none()
    );
    f.client
        .player_action(&epoch, PlayerAction::Stop)
        .await
        .unwrap();
    let local = f
        .store
        .get("9", "podcast-latest-voice-v1", "22")
        .unwrap()
        .unwrap();
    assert_eq!(
        local,
        json!({"id":"6","mainTrackId":"1006","name":"本机保存的章节名","time":18,"duration":120})
    );

    let latest = open(&epoch).await;
    let chapter = &latest["rows"][0]["chapter"];
    assert_eq!(chapter["recent"], true);
    assert_eq!(chapter["name"], "本机保存的章节名");
    assert_eq!(chapter["percent"], 15);
    let around = f
        .client
        .play_selection(&epoch, select(&latest))
        .await
        .unwrap();
    assert_eq!(
        around.position,
        Some(18.0),
        "original whole-second checkpoint wins for this gesture"
    );
    assert_eq!(
        around.track.as_ref().unwrap().voice_id.as_deref(),
        Some("6")
    );
    let queue = f.client.player_queue(&epoch).await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|row| row.track.voice_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["7", "6", "5"]
    );

    f.api.around_missing.store(true, Ordering::SeqCst);
    assert!(matches!(
        f.client.play_selection(&epoch, select(&latest)).await,
        Err(Error::QueueChanged)
    ));
    let unchanged = f.client.player_snapshot(&epoch).await.unwrap();
    assert_eq!(unchanged.play_id, around.play_id);
    assert_eq!(unchanged.queue_revision, around.queue_revision);
    {
        let calls = f.api.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|path| *path == "/api/dj/around/program/v1")
                .count(),
            2
        );
        assert_eq!(
            calls
                .iter()
                .filter(|path| *path == "/api/v6/dj/program/byradio")
                .count(),
            1
        );
        assert!(!calls.iter().any(|path| path == "/api/dj/playrecord/upload"));
    }

    // A later local chapter for the same book must not change the old row's
    // assigned episode, visible name, or route.
    f.store
        .put(
            "9",
            "podcast-latest-voice-v1",
            "22",
            &json!({"id":"7","mainTrackId":"1007","name":"不同章节","time":30,"duration":120}),
        )
        .unwrap();
    let different_chapter = open(&epoch).await;
    assert_eq!(different_chapter["rows"][0]["chapter"]["recent"], false);
    let original_row = f
        .client
        .play_selection(&epoch, select(&different_chapter))
        .await
        .unwrap();
    assert_eq!(original_row.track.unwrap().voice_id.as_deref(), Some("6"));
    assert_eq!(original_row.position, Some(2.5));
    assert_eq!(original_row.queue_len, 2);
    assert_eq!(
        f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|path| *path == "/api/dj/around/program/v1")
            .count(),
        2
    );

    f.session
        .adopt(f.api.clone(), json!({"userId":10}), &token)
        .unwrap();
    let next_epoch = f.epoch();
    let other = open(&next_epoch).await;
    assert_eq!(other["rows"][0]["chapter"]["recent"], false);
    // Deliberately no player snapshot or warm-up on the new account: its
    // first source-bound action must take a fence from the new owner.
    let other_playback = f
        .client
        .play_selection(&next_epoch, select(&other))
        .await
        .unwrap();
    assert_eq!(other_playback.position, Some(2.5));
    assert_eq!(other_playback.queue_len, 2);
    assert!(
        f.store
            .get("10", "podcast-latest-voice-v1", "22")
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn listened_books_applies_saved_radio_order_without_reordering_the_active_queue() {
    let f = fixture(true);
    let epoch = f.epoch();
    let open = || async {
        let response = f
            .client
            .listened_books(&epoch, ListenedBooksQuery::Open)
            .await
            .unwrap();
        let page = serde_json::to_value(response.value).unwrap();
        PlaySelection::ListenedBookChapter {
            revision: page["revision"].as_str().unwrap().into(),
            key: 0,
        }
    };
    let queue = || async {
        let value = f.client.player_queue(&epoch).await.unwrap();
        value
            .tracks
            .into_iter()
            .map(|row| row.track.voice_id.unwrap())
            .collect::<Vec<_>>()
    };
    let scope = |ascending| PodcastScope::Radio {
        id: "22".into(),
        ascending,
    };
    let load_detail = |ascending| {
        let query = PodcastQuery {
            scope: scope(ascending),
            keyword: String::new(),
            limit: 50,
            offset: 0,
            revision: None,
        };
        let (client, epoch) = (&f.client, &epoch);
        async move { client.podcast_page(epoch, query, true).await.unwrap() }
    };
    let save_order = |page: PodcastPage, ascending| {
        f.client
            .podcast_radio_order(
                &epoch,
                PodcastRadioOrder::Set {
                    scope: page.scope,
                    revision: page.revision,
                    ascending,
                },
            )
            .unwrap()
    };

    let selected = f.client.play_selection(&epoch, open().await).await.unwrap();
    assert_eq!(queue().await, ["5", "6"]);
    let held = f
        .client
        .player_action(&epoch, PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(held.play_id, selected.play_id);
    let detail = load_detail(true).await;
    assert_eq!(save_order(detail, false).value, Some(false));
    let still_held = f.client.player_snapshot(&epoch).await.unwrap();
    assert_eq!(still_held.state, held.state);
    assert_eq!(still_held.position, held.position);
    assert_eq!(still_held.play_id, held.play_id);
    assert_eq!(still_held.queue_revision, held.queue_revision);
    assert_eq!(queue().await, ["5", "6"]);

    // Pause was not a departure, so the next request still uses byradio.
    let full = f.client.play_selection(&epoch, open().await).await.unwrap();
    assert_eq!(full.track.unwrap().voice_id.as_deref(), Some("6"));
    assert_eq!(queue().await, ["6", "5"]);
    f.client
        .player_action(&epoch, PlayerAction::Stop)
        .await
        .unwrap();
    let detail = load_detail(false).await;
    assert_eq!(save_order(detail, true).value, Some(true));
    let around = f.client.play_selection(&epoch, open().await).await.unwrap();
    assert_eq!(around.track.unwrap().voice_id.as_deref(), Some("6"));
    assert_eq!(queue().await, ["5", "6", "7"]);
    assert_eq!(
        *f.api.chapter_reads.lock().unwrap(),
        [
            json!({"radioId":"22","limit":250,"offset":0}),
            json!({"radioId":"22","limit":200,"offset":0,"asc":true}),
            json!({"radioId":"22","limit":250,"offset":0,"asc":false}),
            json!({"radioId":"22","limit":200,"offset":0,"asc":false}),
        ]
    );
    assert_eq!(
        f.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|path| *path == "/api/dj/around/program/v1")
            .count(),
        1
    );
}
