//! The transport and media gates use only in-memory fakes and synthetic state.
#![cfg(target_os = "linux")]

#[path = "../examples/support/signed_window_read_gate.rs"]
mod signed_window_read_gate;

use cumulus_core::{
    Error, Result,
    account::{SessionStatus, credentials::Credentials},
    catalog::{CatalogApi, PlayOptions, search::results::SearchChannel},
    client::{
        MusicClient,
        dto::{SearchCategory, SearchResultQuery},
    },
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest, MediaUrl},
    playback::engine::Cancellation,
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{
            APP_VERSION, ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply,
            MusicHttpClient, OFFICIAL_WINDOWS_USER_AGENT, RequestInput, RequestOptions,
            RequestPlan, RoutingPolicy, WireRoute, build_request,
        },
        mconfig::{ConfigBackend, ConfigUrl, PcConfig},
        pc_experiments::{ASSIGNMENT_PATH, assignment_arguments},
        wire::{crypto, eapi},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use signed_window_read_gate::Gate;
use std::{
    collections::{BTreeMap, VecDeque},
    io::{Cursor, Read},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

const OWNER: &str = "83";
const SONG: &str = "41";
const QUERY: &str = "合成查询";
const ACCOUNT: &str = "/api/w/nuser/account/get";
const LIST: &str = "/api/user/playlist";
const LIKES: &str = "/api/song/like/get";
const TABS: &str = "/api/search/pc/result/tab";
const V1: &str = "/api/search/pc/complex/page";
const V3: &str = "/api/search/pc/complex/page/v3";
const SONGS: &str = "/api/search/song/list/page";
const DETAIL: &str = "/api/v3/song/detail";
const SOURCE: &str = "/api/song/enhance/player/url/v1";
const MEDIA: &str = "https://m1.music.126.net/synthetic-audio.m4a?only=1";
const BYTES: &[u8] = b"not a real audio asset";

fn reply(body: Value) -> HttpReply {
    HttpReply {
        status: 200,
        body: serde_json::to_vec(&body).unwrap(),
        body_status: BodyStatus::Complete,
        set_cookies: vec![],
        refresh_token: None,
        metadata: BTreeMap::new(),
    }
}

fn account(id: &str) -> HttpReply {
    reply(json!({"code":200,"profile":{"userId":id,"nickname":"合成账号"}}))
}

fn song() -> Value {
    json!({"id":SONG,"name":"合成歌曲","ar":[{"name":"合成歌手"}],"al":{"name":"合成专辑"},"dt":12000})
}

fn source() -> Value {
    json!({"id":SONG,"code":200,"url":MEDIA,"size":BYTES.len(),"md5":eapi::md5_hex(BYTES),"level":"standard","expi":60})
}

#[derive(Default)]
struct ClosedAux(AtomicUsize);
impl TokenProvider for ClosedAux {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Err(Error::State("synthetic SDK disabled")) })
    }
}
impl ConfigBackend for ClosedAux {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Err(Error::State("synthetic config download disabled")) })
    }
}

enum Response {
    Reply(HttpReply),
    Never,
}

#[derive(Default)]
struct Script {
    steps: Mutex<VecDeque<(&'static str, Response)>>,
    calls: AtomicUsize,
}
impl Script {
    fn push(&self, path: &'static str, reply: HttpReply) {
        self.steps
            .lock()
            .unwrap()
            .push_back((path, Response::Reply(reply)));
    }
}
impl HttpBackend for Script {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let (path, reply) = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected network dispatch");
            assert_eq!(
                plan.url,
                ApiOrigin::Pc.request_url(WireRoute::Eapi, path).unwrap()
            );
            match reply {
                Response::Reply(reply) => Ok(reply),
                Response::Never => std::future::pending().await,
            }
        })
    }
}

struct Fixture {
    root: tempfile::TempDir,
    profile: ClientProfile,
    credentials: Arc<Credentials>,
    config: Arc<PcConfig>,
    aux: Arc<ClosedAux>,
    context: Arc<RequestContext>,
    script: Arc<Script>,
    gate: Arc<Gate>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_extra_cookies(&[])
    }

    fn with_extra_cookies(extra: &[&str]) -> Self {
        Self::build(extra, false)
    }

    fn build(extra: &[&str], preview_only: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let credentials = Arc::new(
            Credentials::open(
                Arc::new(StateStore::open(root.path().join("profile")).unwrap()),
                ApiOrigin::Pc,
            )
            .unwrap(),
        );
        let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, ACCOUNT).unwrap();
        let revision = credentials.request_credentials(&target).unwrap().revision;
        let mut cookies =
            vec!["MUSIC_U=synthetic-window-read; Path=/eapi; Secure; HttpOnly".into()];
        cookies.extend(extra.iter().map(|cookie| (*cookie).to_owned()));
        credentials
            .record_response(&target, &cookies, None, &revision)
            .unwrap();
        let profile = ClientProfile {
            user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
            os_version: "synthetic Windows version".into(),
            client_sign: Some("synthetic-sign".into()),
        };
        let script = Arc::new(Script::default());
        let store = Arc::new(StateStore::open(root.path().join("config")).unwrap());
        store
            .put(
                "transport",
                "pc-mconfig-v1",
                "pc",
                &json!({
                    "origin":ApiOrigin::Pc,"version":3,"bucket":"","generation":1,
                    "source_url":"https://m1.music.126.net/obj/synthetic.data","data":{}
                }),
            )
            .unwrap();
        let aux = Arc::new(ClosedAux::default());
        let config = PcConfig::new(store, aux.clone()).unwrap();
        let context =
            Arc::new(RequestContext::from_pc(config.clone(), APP_VERSION, aux.clone()).unwrap());
        let gate = Gate::new(
            script.clone(),
            credentials.clone(),
            profile.clone(),
            &config.snapshot().unwrap(),
            OWNER.into(),
            QUERY.into(),
            SONG.into(),
        )
        .unwrap();
        let gate = Arc::new(if preview_only {
            gate.with_comprehensive_track()
        } else {
            gate
        });
        Self {
            root,
            profile,
            credentials,
            config,
            aux,
            context,
            script,
            gate,
        }
    }

    fn plan(&self, path: &str, payload: Value, encrypted: bool) -> RequestPlan {
        let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, path).unwrap();
        let mut plan = build_request(RequestInput {
            origin: ApiOrigin::Pc,
            path,
            payload,
            credentials: &self.credentials.request_credentials(&target).unwrap(),
            profile: &self.profile,
            extra_headers: &BTreeMap::new(),
            options: RequestOptions {
                encrypt_response: encrypted,
                routing: RoutingPolicy::OfficialEapi,
                ..Default::default()
            },
        })
        .unwrap();
        plan.headers.insert(
            "MConfig-Info".into(),
            self.config
                .snapshot()
                .unwrap()
                .request_header(APP_VERSION)
                .unwrap(),
        );
        plan
    }

    async fn send(&self, path: &str, payload: Value) -> Result<HttpReply> {
        self.gate.post(self.plan(path, payload, false)).await
    }

    async fn enter(&self) {
        self.script.push(ACCOUNT, account(OWNER));
        self.send(ACCOUNT, json!({})).await.unwrap();
    }

    fn client(&self) -> MusicClient {
        MusicClient::open_with_request_context(
            &self.root.path().join("host"),
            Some(&self.root.path().join("profile")),
            self.profile.clone(),
            Some(self.gate.clone()),
            Some(self.context.clone()),
        )
        .unwrap()
        .with_official_search_experiment()
    }

    fn catalog(&self) -> CatalogApi {
        CatalogApi::new(Arc::new(
            MusicHttpClient::with_backend(
                self.credentials.clone(),
                self.profile.clone(),
                RequestOptions::default(),
                Some(self.context.clone()),
                self.gate.clone(),
            )
            .unwrap(),
        ))
    }

    async fn search(&self, client: &MusicClient, epoch: &str, v3: bool) {
        self.script.push(
            ASSIGNMENT_PATH,
            reply(json!({"code":200,"data":[{
                "expName":"PH-PC-searchHot","expGroupName":if v3 { "t1" } else { "c" }
            }]})),
        );
        if v3 {
            self.script.push(
                TABS,
                reply(
                    json!({"code":200,"data":{"defaultTabs":[{"title":"综合"},{"title":"单曲"}]}}),
                ),
            );
        }
        self.script.push(
            if v3 { V3 } else { V1 },
            reply(json!({"code":200,"data":{"blocks":[]}})),
        );
        self.script.push(SONGS, reply(json!({"code":200,"data":{"totalCount":1,"resources":[{"type":"song","baseInfo":{"simpleSongData":song()}}],"hasMore":false}})));
        for (category, limit) in [(SearchCategory::All, 30), (SearchCategory::Songs, 10)] {
            client
                .search_results(
                    epoch,
                    SearchResultQuery {
                        keyword: QUERY.into(),
                        category,
                        offset: 0,
                        limit,
                        correct: true,
                        channel: Some(SearchChannel::Typing),
                    },
                )
                .await
                .unwrap();
        }
    }

    async fn authorize(&self, row: Value) -> Result<()> {
        self.script
            .push(DETAIL, reply(json!({"code":200,"songs":[song()]})));
        self.script
            .push(SOURCE, reply(json!({"code":200,"data":[row]})));
        let catalog = self.catalog();
        catalog.songs(&[json!(SONG)], 100).await?;
        catalog
            .play_url(
                &json!(SONG),
                PlayOptions {
                    level: "standard",
                    encode_type: "aac",
                    trial_mode: 42,
                },
            )
            .await?;
        Ok(())
    }
}

struct Blob(Cursor<&'static [u8]>);
impl MediaBody for Blob {
    fn read<'a>(&'a mut self, bytes: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move { Ok(self.0.read(bytes)?) })
    }
}

struct Source {
    status: u16,
    headers: BTreeMap<String, String>,
    bytes: &'static [u8],
    calls: AtomicUsize,
}
impl Source {
    fn new(status: u16, bytes: &'static [u8]) -> Self {
        Self {
            status,
            headers: BTreeMap::from([("content-length".into(), BYTES.len().to_string())]),
            bytes,
            calls: AtomicUsize::new(0),
        }
    }
}
impl MediaBackend for Source {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::Relaxed);
            assert_eq!(request.url.as_str(), MEDIA);
            assert_eq!(request.offset, 0);
            assert!(request.if_range.is_none());
            Ok(MediaReply {
                status: self.status,
                headers: self.headers.clone(),
                body: Box::new(Blob(Cursor::new(self.bytes))),
            })
        })
    }
}

#[cfg(feature = "signed-window-read-proof")]
struct TestArm<'a> {
    fixture: &'a Fixture,
    media: &'a Source,
    calls: AtomicUsize,
    mode: &'a str,
}

#[cfg(feature = "signed-window-read-proof")]
impl signed_window_read_gate::MediaArm for TestArm<'_> {
    fn arm<'a>(&'a self, url: &'a MediaUrl, _: &'a Cancellation) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::Relaxed);
            assert!(self.fixture.gate.facts().await.media_consumed);
            assert_eq!(self.media.calls.load(Ordering::Relaxed), 0);
            assert_eq!(url.as_str(), MEDIA);
            match self.mode {
                "acknowledged" => Ok(()),
                "expired" => std::future::pending().await,
                "rotated" => self.fixture.credentials.clear(),
                "refused" => Err(Error::State("synthetic host refused")),
                _ => unreachable!(),
            }
        })
    }
}

#[cfg(feature = "signed-window-read-proof")]
#[tokio::test]
async fn dynamic_cdn_arms_only_a_consumed_signed_receipt_and_fails_closed() {
    let url = MediaUrl::parse(MEDIA).unwrap();
    let forged = MediaUrl::parse("https://m1.music.126.net/synthetic-audio.m4a?only=2").unwrap();
    let cancel = Cancellation::default();
    for mode in ["acknowledged", "expired", "rotated", "refused"] {
        let f = Fixture::new();
        let media = Source::new(200, BYTES);
        let arm = TestArm {
            fixture: &f,
            media: &media,
            calls: AtomicUsize::new(0),
            mode,
        };
        let fetch = |requested| {
            f.gate
                .fetch_verified_armed(&media, requested, f.root.path(), &cancel, Some(&arm))
        };
        // A forged UI stage cannot mint an upstream signed response.
        assert!(fetch(&url).await.is_err());
        assert_eq!(arm.calls.load(Ordering::Relaxed), 0);
        f.script.push(ACCOUNT, account(OWNER));
        let client = f.client();
        let identity = client.restore_account().await.unwrap();
        f.search(&client, &identity.epoch, false).await;
        let mut row = source();
        if mode == "expired" {
            row["expi"] = json!(1);
        }
        f.authorize(row).await.unwrap();
        assert!(fetch(&forged).await.is_err());
        assert_eq!(arm.calls.load(Ordering::Relaxed), 0);
        let result = fetch(&url).await;
        let admitted = mode == "acknowledged";
        assert_eq!(result.is_ok(), admitted);
        assert!(fetch(&url).await.is_err());
        assert_eq!(arm.calls.load(Ordering::Relaxed), 1);
        assert_eq!(media.calls.load(Ordering::Relaxed), usize::from(admitted));
    }
}

#[tokio::test]
async fn scoped_comprehensive_preview_only_authorizes_the_exact_song_without_songs_tab() {
    for v3 in [false, true] {
        let f = Fixture::build(&[], true);
        f.script.push(ACCOUNT, account(OWNER));
        let client = f.client();
        let identity = client.restore_account().await.unwrap();
        f.script.push(
            ASSIGNMENT_PATH,
            reply(json!({"code":200,"data":[{
                "expName":"PH-PC-searchHot","expGroupName":if v3 { "t1" } else { "c" }
            }]})),
        );
        if v3 {
            f.script.push(
                TABS,
                reply(json!({"code":200,"data":{"defaultTabs":[{"title":"综合"}]}})),
            );
        }
        let base = if v3 {
            json!({"simpleSongData":song()})
        } else {
            song()
        };
        f.script.push(
            if v3 { V3 } else { V1 },
            reply(json!({"code":200,"data":{"blocks":[{
                "blockCode":"search_block_song","resources":[{"type":"song","baseInfo":base}]
            }]}})),
        );
        let query = SearchResultQuery {
            keyword: QUERY.into(),
            category: SearchCategory::All,
            offset: 0,
            limit: 30,
            correct: true,
            channel: Some(SearchChannel::Typing),
        };
        client.search_results(&identity.epoch, query).await.unwrap();
        let facts = f.gate.facts().await;
        assert!(
            facts.comprehensive_read && facts.selected_from_preview && facts.selected_song_found
        );
        assert!(
            f.send(
                SONGS,
                json!({"keyword":QUERY,"scene":"normal","needCorrect":true,
            "channel":"typing","limit":10,"offset":0})
            )
            .await
            .is_err()
        );
        assert!(
            f.send(DETAIL, json!({"c":"[{\"id\":\"999\",\"v\":0}]"}))
                .await
                .is_err()
        );
        assert_eq!(f.gate.facts().await.attempted_reads, facts.attempted_reads);
        f.authorize(source()).await.unwrap();
        assert!(f.gate.facts().await.media_authorized);
    }
}

#[tokio::test]
async fn actual_core_signs_both_search_branches_and_only_one_verified_media_can_be_taken() {
    use std::os::unix::fs::MetadataExt;
    for v3 in [false, true] {
        let f = Fixture::new();
        f.script.push(ACCOUNT, account(OWNER));
        let client = f.client();
        let identity = client.restore_account().await.unwrap();
        assert_eq!(identity.status, SessionStatus::Authenticated);
        let playlist = |id: usize| json!({"id":id,"name":"合成歌单","userId":OWNER,"creator":{"userId":OWNER},"trackCount":0});
        f.script.push(
            LIST,
            reply(json!({"code":200,"playlist":[playlist(42)],"more":true})),
        );
        f.script.push(
            LIST,
            reply(json!({"code":200,"playlist":[playlist(43)],"more":false})),
        );
        f.script
            .push(LIKES, reply(json!({"code":200,"ids":[41,71]})));
        assert_eq!(client.playlists().await.unwrap().value.len(), 2);
        assert_eq!(client.liked_ids().await.unwrap().value.len(), 2);
        f.search(&client, &identity.epoch, v3).await;
        f.authorize(source()).await.unwrap();
        let valid = MediaUrl::parse(MEDIA).unwrap();
        let incorrect = MediaUrl::parse(&MEDIA.replace("only=1", "only=2")).unwrap();
        let media = Source::new(200, BYTES);
        let cancel = Cancellation::default();
        assert!(
            f.gate
                .fetch_verified(&media, &incorrect, f.root.path(), &cancel)
                .await
                .is_err()
        );
        let mut file = f
            .gate
            .fetch_verified(&media, &valid, f.root.path(), &cancel)
            .await
            .unwrap();
        assert_eq!(file.metadata().unwrap().nlink(), 0);
        assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o600);
        let mut verified = Vec::new();
        file.read_to_end(&mut verified).unwrap();
        assert_eq!(verified, BYTES);
        assert!(f.gate.take_media(&valid).await.is_err());
        let facts = f.gate.facts().await;
        assert!(
            facts.expected_account_verified && facts.selected_song_found && facts.song_detail_read
        );
        assert!(facts.media_authorized && facts.media_consumed && !facts.closed_after_failure);
        assert_eq!(facts.playlist_count, Some(2));
        assert_eq!(facts.playlist_pages, 2);
        assert_eq!(facts.liked_count, Some(2));
        assert_eq!(facts.search_v3, Some(v3));
        assert_eq!(facts.search_tab_read, v3);
        assert!(facts.comprehensive_read);
        assert_eq!(facts.attempted_reads, 9 + usize::from(v3));
        assert_eq!(facts.denied_before_dispatch, 0);
        assert_eq!(media.calls.load(Ordering::Relaxed), 1);
        assert_eq!(f.aux.0.load(Ordering::Relaxed), 0);
        assert!(f.script.steps.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn distinct_cookie_names_can_reorder_but_same_name_precedence_and_values_are_exact() {
    let f = Fixture::with_extra_cookies(&[
        "window_a=alpha; Path=/eapi; Secure",
        "window_b=beta=synthetic; Path=/eapi; Secure",
        "window_scope=more-specific; Path=/eapi/w; Secure",
        "window_scope=root; Path=/; Secure",
    ]);
    let mut plan = f.plan(ACCOUNT, json!({}), false);
    let original = plan.headers.get("Cookie").unwrap().clone();
    let mut pairs: Vec<_> = original.split("; ").collect();
    let locate = |value| pairs.iter().position(|&pair| pair == value).unwrap();
    let (a, b) = (locate("window_a=alpha"), locate("window_b=beta=synthetic"));
    let (specific, root) = (
        locate("window_scope=more-specific"),
        locate("window_scope=root"),
    );
    assert!(specific < root, "scoped same-name cookie must come first");
    pairs.swap(a, b);
    let reordered = pairs.join("; ");
    assert_ne!(reordered, original);
    pairs.swap(specific, root);

    let invalid = [
        pairs.join("; "),
        reordered.replace("window_b=beta=synthetic", "window_b=wrong=synthetic"),
        format!("{reordered}; window_scope=root"),
        reordered.replace("window_a=alpha; ", ""),
    ];
    for cookie in invalid {
        plan.headers.insert("Cookie".into(), cookie);
        assert!(f.gate.post(plan).await.is_err());
        plan = f.plan(ACCOUNT, json!({}), false);
    }
    assert_eq!(f.script.calls.load(Ordering::Relaxed), 0);
    let refused = f.gate.facts().await;
    assert_eq!(refused.denied_before_dispatch, 4);
    assert_eq!(refused.first_plan_rejection, Some("headers"));
    assert_eq!(refused.unfinished_read, None);
    assert!(!refused.closed_after_failure);

    plan.headers.insert("Cookie".into(), reordered);
    f.script.push(ACCOUNT, account(OWNER));
    assert!(f.gate.post(plan).await.is_ok());
    assert_eq!(f.script.calls.load(Ordering::Relaxed), 1);
    let accepted = f.gate.facts().await;
    assert!(accepted.expected_account_verified);
    assert_eq!(accepted.first_plan_rejection, Some("headers"));
    assert_eq!(accepted.unfinished_read, None);
}

#[tokio::test]
async fn denied_wire_shapes_do_not_reach_the_transport() {
    let f = Fixture::new();
    assert!(f.send(LIKES, json!({"uid":OWNER})).await.is_err());
    f.enter().await;
    for (path, args) in [
        (ACCOUNT, json!({})),
        (LIKES, json!({"uid":"84"})),
        (
            LIST,
            json!({"uid":OWNER,"includeVideo":true,"limit":200,"offset":1}),
        ),
        (ASSIGNMENT_PATH, json!({"expNames":"PH-PC-searchHot"})),
        (V3, json!({"keyword":QUERY})),
        (SONGS, json!({"keyword":"非预设搜索"})),
        (DETAIL, json!({"c":"[{\"id\":\"41\",\"v\":0}]"})),
        ("/api/playlist/create", json!({})),
        ("/api/login/token/refresh/v1", json!({})),
    ] {
        assert!(f.send(path, args).await.is_err());
    }
    for edit in ["origin", "query", "token", "cookie", "config", "payload"] {
        let mut plan = f.plan(ASSIGNMENT_PATH, assignment_arguments(), false);
        match edit {
            "origin" => {
                plan.url.set_host(Some("interface.music.163.com")).unwrap();
            }
            "query" => plan.url.set_query(Some("other=1")),
            "token" => {
                plan.headers
                    .insert("X-antiCheatToken".into(), "synthetic-sdk".into());
            }
            "cookie" => plan
                .headers
                .get_mut("Cookie")
                .unwrap()
                .push_str("; MUSIC_U=other"),
            "config" => {
                plan.headers.remove("MConfig-Info");
            }
            "payload" => plan.body.extend_from_slice(b"&extra=1"),
            _ => unreachable!(),
        }
        assert!(f.gate.post(plan).await.is_err());
    }
    let facts = f.gate.facts().await;
    assert_eq!(facts.denied_before_dispatch, 16);
    assert_eq!(facts.first_plan_rejection, Some("allowlist"));
    assert_eq!(facts.unfinished_read, None);
    assert_eq!(f.script.calls.load(Ordering::Relaxed), 1);
    assert!(!facts.closed_after_failure);
}

#[tokio::test]
async fn unexpected_owner_service_errors_redirects_and_rotation_close_the_gate() {
    for fault in [
        "owner", "expired", "redirect", "cookie", "refresh", "partial",
    ] {
        let f = Fixture::new();
        let mut bad = account(if fault == "owner" { "84" } else { OWNER });
        match fault {
            "expired" => bad = reply(json!({"code":301})),
            "redirect" => bad.status = 302,
            "cookie" => bad.set_cookies.push("Music_U=changed; Path=/".into()),
            "refresh" => bad.refresh_token = Some("unexpected".into()),
            "partial" => bad.body_status = BodyStatus::Interrupted,
            _ => (),
        }
        f.script.push(ACCOUNT, bad);
        assert!(f.send(ACCOUNT, json!({})).await.is_err());
        assert!(f.send(ACCOUNT, json!({})).await.is_err());
        assert_eq!(f.script.calls.load(Ordering::Relaxed), 1);
        let facts = f.gate.facts().await;
        assert!(facts.closed_after_failure && !facts.expected_account_verified);
        assert_eq!(facts.unfinished_read, Some("account"));
    }
}

#[tokio::test]
async fn only_declared_encryption_is_inspected_and_original_body_is_preserved() {
    for encrypted in [false, true] {
        let f = Fixture::new();
        let mut reply = account(OWNER);
        reply.body = crypto::encrypt_ecb(&reply.body, eapi::EAPI_KEY).unwrap();
        let original = reply.body.clone();
        reply.set_cookies.push("unrelated=value".into());
        reply
            .metadata
            .insert("mconfig-info".into(), "not fetched".into());
        f.script.push(ACCOUNT, reply);
        let result = f.gate.post(f.plan(ACCOUNT, json!({}), encrypted)).await;
        if encrypted {
            let result = result.unwrap();
            assert_eq!(result.body, original);
            assert!(result.set_cookies.is_empty() && result.metadata.is_empty());
        } else {
            assert!(result.is_err());
        }
    }
}

#[tokio::test]
async fn cancellation_latches_a_dispatched_attempt_and_duplicate_pages_cannot_pass() {
    let f = Fixture::new();
    f.script
        .steps
        .lock()
        .unwrap()
        .push_back((ACCOUNT, Response::Never));
    assert!(
        tokio::time::timeout(Duration::from_millis(10), f.send(ACCOUNT, json!({})))
            .await
            .is_err()
    );
    let facts = f.gate.facts().await;
    assert!(facts.closed_after_failure);
    assert_eq!(facts.unfinished_read, Some("account"));
    assert_eq!(facts.first_plan_rejection, None);
    assert!(f.send(ACCOUNT, json!({})).await.is_err());
    assert_eq!(f.script.calls.load(Ordering::Relaxed), 1);

    let f = Fixture::new();
    f.enter().await;
    for offset in 0..=1 {
        f.script.push(
            LIST,
            reply(json!({"code":200,"playlist":[{"id":42}],"more":offset == 0})),
        );
        let result = f
            .send(
                LIST,
                json!({"uid":OWNER,"includeVideo":true,"limit":200,"offset":offset}),
            )
            .await;
        assert_eq!(result.is_ok(), offset == 0);
    }
    assert_eq!(f.gate.facts().await.playlist_count, None);
    assert!(f.gate.facts().await.closed_after_failure);
}

#[tokio::test]
async fn wrong_authorizations_and_unverified_bytes_never_make_a_native_file() {
    for error in [
        "wrong-id", "trial", "size", "digest", "host", "redirect", "range", "encoding", "length",
        "body",
    ] {
        let f = Fixture::new();
        f.script.push(ACCOUNT, account(OWNER));
        let client = f.client();
        let epoch = client.restore_account().await.unwrap().epoch;
        f.search(&client, &epoch, true).await;
        let mut row = source();
        match error {
            "wrong-id" => row["id"] = json!(44),
            "trial" => row["freeTrialInfo"] = json!({"start":3,"end":7}),
            "size" => row["size"] = json!(0),
            "digest" => row["md5"] = json!("not-a-digest"),
            "host" => row["url"] = json!("https://unapproved.invalid/audio.m4a"),
            _ => (),
        }
        let authorization = f.authorize(row).await;
        let mut media = Source::new(200, BYTES);
        match error {
            "redirect" => media.status = 302,
            "range" => {
                media
                    .headers
                    .insert("Content-Range".into(), "bytes 0-4/100".into());
            }
            "encoding" => {
                media
                    .headers
                    .insert("Content-Encoding".into(), "gzip".into());
            }
            "length" => {
                media.headers.insert("Content-Length".into(), "4".into());
            }
            "body" => media.bytes = b"unauthenticated bytes!",
            _ => (),
        }
        let url = MediaUrl::parse(MEDIA).unwrap();
        assert!(
            f.gate
                .fetch_verified(&media, &url, f.root.path(), &Cancellation::default())
                .await
                .is_err()
        );
        assert!(f.gate.take_media(&url).await.is_err());
        assert_eq!(
            media.calls.load(Ordering::Relaxed),
            usize::from(authorization.is_ok())
        );
        let facts = f.gate.facts().await;
        let diagnostic = match error {
            "redirect" => (Some("http_status"), Some(302)),
            "range" | "encoding" | "length" => (Some("headers"), Some(200)),
            "body" => (Some("integrity"), Some(200)),
            _ => (None, None),
        };
        assert_eq!((facts.media_stage, facts.media_http_status), diagnostic);
    }
}
