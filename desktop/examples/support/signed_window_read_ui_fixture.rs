//! Closed synthetic PC/CDN responders and an actual libmpv audio adapter.
use super::signed_window_read_gate::Gate;
use cumulus_core::{
    Error, Result,
    account::credentials::Credentials,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{
            APP_VERSION, ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply,
            OFFICIAL_WINDOWS_USER_AGENT, RequestPlan, WireRoute,
        },
        mconfig::{ConfigBackend, ConfigUrl, PcConfig},
        wire::eapi,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

pub const OWNER: &str = "83";
pub const SONG: &str = "41";
pub const QUERY: &str = "合成窗口检索";
const MEDIA: &str = "https://m1.music.126.net/signed-window-fixture.m4a?only=1";
pub(super) const AAC: &[u8] =
    include_bytes!("../../../core/tests/fixtures/tag_audio/playback-12s.m4a");

fn song() -> Value {
    json!({"id":SONG,"name":"合成窗口音频","ar":[{"name":"合成歌手"}],
        "al":{"name":"合成专辑"},"dt":12000})
}

fn playlist() -> Value {
    json!({"id":42,"name":"合成窗口歌单","userId":OWNER,"creator":{"userId":OWNER},
        "trackCount":1,"ordered":true})
}

#[derive(Default)]
pub struct Closed {
    pub calls: Mutex<Vec<&'static str>>,
    pub media_reads: AtomicUsize,
    pub auxiliary_attempts: AtomicUsize,
    pub other_media_attempts: AtomicUsize,
}

impl HttpBackend for Closed {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            ApiOrigin::Pc.require_url(&plan.url)?;
            let (kind, value) = match plan.url.path() {
                "/eapi/w/nuser/account/get" => (
                    "account",
                    json!({"code":200,"profile":{
                    "userId":OWNER,"nickname":"合成窗口账号"}}),
                ),
                "/eapi/user/playlist" => (
                    "playlists",
                    json!({"code":200,"playlist":[playlist()],"more":false}),
                ),
                "/eapi/song/like/get" => ("likes", json!({"code":200,"ids":[41]})),
                "/eapi/search/default/keyword/get" => ("default", json!({"code":200,"data":{}})),
                "/eapi/rtrs/abt/front/expinfo/list" => (
                    "assignment",
                    json!({"code":200,
                    "data":[{"expName":"PH-PC-searchHot","expGroupName":"t1"}]}),
                ),
                "/eapi/search/pc/result/tab" => (
                    "tabs",
                    json!({"code":200,"data":{"defaultTabs":[
                    {"title":"综合"},{"title":"单曲"}]}}),
                ),
                "/eapi/search/pc/complex/page/v3" => (
                    "comprehensive",
                    json!({"code":200,"data":{"blocks":[{
                        "blockCode":"search_block_song","resources":[{"type":"song","baseInfo":{"simpleSongData":song()}}]
                    }]}}),
                ),
                "/eapi/v3/song/detail" => ("detail", json!({"code":200,"songs":[song()]})),
                "/eapi/song/enhance/player/url/v1" => (
                    "source",
                    json!({"code":200,"data":[{
                    "id":SONG,"code":200,"url":MEDIA,"size":AAC.len(),"md5":eapi::md5_hex(AAC),
                    "level":"standard","expi":60}]}),
                ),
                _ => return Err(Error::State("unexpected synthetic signed endpoint")),
            };
            self.calls
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .push(kind);
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&value)?,
                body_status: BodyStatus::Complete,
                set_cookies: vec![],
                refresh_token: None,
                metadata: BTreeMap::new(),
            })
        })
    }
}

impl TokenProvider for Closed {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        self.auxiliary_attempts.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Err(Error::State("closed synthetic SDK")) })
    }
}

impl ConfigBackend for Closed {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        self.auxiliary_attempts.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Err(Error::State("closed synthetic PC configuration")) })
    }
}

struct Body(Cursor<&'static [u8]>);
impl MediaBody for Body {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move { Ok(self.0.read(buffer)?) })
    }
}

impl MediaBackend for Closed {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            if request.url.as_str() != MEDIA
                || request.offset != 0
                || request.if_range.is_some()
                || self.media_reads.fetch_add(1, Ordering::AcqRel) != 0
            {
                return Err(Error::State(
                    "synthetic media receipt escaped its one-use scope",
                ));
            }
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([("content-length".into(), AAC.len().to_string())]),
                body: Box::new(Body(Cursor::new(AAC))),
            })
        })
    }
}

pub struct RejectMedia(pub Arc<Closed>);
impl MediaBackend for RejectMedia {
    fn get(&self, _: MediaRequest) -> ApiFuture<'_, MediaReply> {
        self.0.other_media_attempts.fetch_add(1, Ordering::Relaxed);
        Box::pin(async {
            Err(Error::State(
                "ordinary synthetic download transport is closed",
            ))
        })
    }
}

pub struct State {
    pub fixture: Arc<Closed>,
    pub gate: Arc<Gate>,
    pub native: ClientProfile,
    pub context: Arc<RequestContext>,
    pub state: PathBuf,
    pub profile: PathBuf,
}

impl State {
    pub fn open(root: &Path) -> Result<Self> {
        let state = root.join("state");
        let profile = root.join("profile");
        let credentials = Arc::new(Credentials::open(
            Arc::new(StateStore::open(&profile)?),
            ApiOrigin::Pc,
        )?);
        let target = ApiOrigin::Pc.request_url(WireRoute::Eapi, "/api/w/nuser/account/get")?;
        let revision = credentials.request_credentials(&target)?.revision;
        credentials.record_response(
            &target,
            &["MUSIC_U=synthetic-signed-window; Path=/eapi; Secure; HttpOnly".into()],
            None,
            &revision,
        )?;
        let native = ClientProfile {
            user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
            os_version: "synthetic Windows version".into(),
            client_sign: Some("synthetic-window-sign".into()),
        };
        let config_store = Arc::new(StateStore::open(&state)?);
        config_store.put(
            "transport",
            "pc-mconfig-v1",
            "pc",
            &json!({
                "origin":ApiOrigin::Pc,"version":3,"bucket":"","generation":1,
                "source_url":"https://m1.music.126.net/obj/synthetic.data","data":{}
            }),
        )?;
        let fixture = Arc::new(Closed::default());
        let config = PcConfig::new(config_store, fixture.clone())?;
        let context = Arc::new(RequestContext::from_pc(
            config.clone(),
            APP_VERSION,
            fixture.clone(),
        )?);
        let gate = Arc::new(
            Gate::new(
                fixture.clone(),
                credentials,
                native.clone(),
                &config.snapshot()?,
                OWNER.into(),
                QUERY.into(),
                SONG.into(),
            )?
            .with_comprehensive_track(),
        );
        Ok(Self {
            fixture,
            gate,
            native,
            context,
            state,
            profile,
        })
    }
}
