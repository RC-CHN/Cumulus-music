use super::*;
use crate::{
    account::AccountSession,
    lyrics::{
        LyricBountyKind, LyricBountyStatus, LyricDemand, LyricRequest, LyricTarget, LyricsService,
        bounty::Bounties,
    },
    protocol::{
        context::TokenProvider,
        http::{ApiOrigin, BodyStatus, HttpReply, OFFICIAL_WINDOWS_USER_AGENT, RequestPlan},
        mconfig::{ConfigBackend, ConfigUrl, PcConfig},
        wire::crypto::encrypt_ecb,
    },
    storage::StateStore,
};
use std::collections::VecDeque;
use tokio::sync::Notify;

struct OfflineConfig;
impl ConfigBackend for OfflineConfig {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("synthetic reply has no remote config") })
    }
}
impl TokenProvider for OfflineConfig {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async { panic!("synthetic bounty has no configured token rule") })
    }
}

enum Reply {
    Json(u16, Value),
    Interrupted,
    Pending,
}

#[derive(Default)]
struct Script {
    requests: Mutex<Vec<RequestPlan>>,
    replies: Mutex<VecDeque<Reply>>,
    entered: Notify,
}

impl HttpBackend for Script {
    fn post(&self, request: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let encrypted = request.decrypt_response;
            self.requests.lock().unwrap().push(request);
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request");
            match reply {
                Reply::Json(status, value) => Ok(HttpReply {
                    status,
                    body: if encrypted {
                        encrypt_ecb(&serde_json::to_vec(&value).unwrap(), eapi::EAPI_KEY).unwrap()
                    } else {
                        serde_json::to_vec(&value).unwrap()
                    },
                    body_status: BodyStatus::Complete,
                    set_cookies: Vec::new(),
                    refresh_token: None,
                    metadata: BTreeMap::new(),
                }),
                // Test a transport that itself claims "not sent" only *after*
                // we entered it. That is not evidence on which to retry.
                Reply::Interrupted => Err(Error::RequestNotSent(Box::new(Error::Cancelled))),
                Reply::Pending => {
                    self.entered.notify_one();
                    std::future::pending().await
                }
            }
        })
    }
}

struct Fixture {
    root: tempfile::TempDir,
    credentials: Arc<Credentials>,
    native: Arc<MusicHttpClient>,
    session: Arc<AccountSession>,
    script: Arc<Script>,
}
impl Fixture {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let origin = ApiOrigin::Pc;
        let credentials = Arc::new(Credentials::open(store.clone(), origin).unwrap());
        let scoped = credentials.request_credentials(&origin.url()).unwrap();
        credentials
            .record_response(
                &origin.url(),
                &["MUSIC_U=synthetic-bounty; Path=/; Secure".into()],
                None,
                &scoped.revision,
            )
            .unwrap();
        let config = PcConfig::new(store.clone(), Arc::new(OfflineConfig)).unwrap();
        let context =
            RequestContext::from_pc(config, "3.1.42.205594", Arc::new(OfflineConfig)).unwrap();
        let script = Arc::new(Script {
            replies: Mutex::new(replies.into_iter().collect()),
            ..Default::default()
        });
        let native = Arc::new(
            MusicHttpClient::with_backend(
                credentials.clone(),
                ClientProfile {
                    user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
                    os_version: "Microsoft Windows 10".into(),
                    client_sign: Some("synthetic-sign".into()),
                },
                RequestOptions::default(),
                Some(Arc::new(context)),
                script.clone(),
            )
            .unwrap(),
        );
        let session = Arc::new(AccountSession::new(native.clone(), Some(store)));
        adopt(&session, &native, 11);
        Self {
            root,
            credentials,
            native,
            session,
            script,
        }
    }
    fn journal(&self) -> Bounties {
        journal(&self.session)
    }
    fn revision(&self) -> String {
        self.native.lyric_bounty_revision().unwrap()
    }
    fn reopen(&self, uid: u32) -> Bounties {
        let store = Arc::new(StateStore::open(self.root.path()).unwrap());
        let session = Arc::new(AccountSession::new(self.native.clone(), Some(store)));
        adopt(&session, &self.native, uid);
        journal(&session)
    }
    fn count(&self) -> usize {
        self.script.requests.lock().unwrap().len()
    }
}

fn adopt(session: &AccountSession, api: &Arc<MusicHttpClient>, uid: u32) {
    session
        .adopt(
            api.clone(),
            json!({"userId": uid}),
            &session.token().unwrap(),
        )
        .unwrap();
}

fn journal(session: &Arc<AccountSession>) -> Bounties {
    Bounties::new(session.clone(), session.token().unwrap()).unwrap()
}

#[tokio::test]
async fn lyric_bounty_exact_signed_pc_envelopes_and_distinct_journal_keys() {
    use LyricBountyKind::*;
    let fixture = Fixture::new([
        Reply::Json(200, json!({"code":200})),
        Reply::Json(200, json!({"code":200})),
        Reply::Json(200, json!({"code":200})),
    ]);
    let offers = [
        (
            MissingLyrics,
            "6",
            r#"{"lyric_error":{"txt":"补歌词","errType":6,"errTitle":"求歌词"},"detail":""}"#,
        ),
        (
            Translation,
            "7",
            r#"{"lyric_error":{"txt":"翻译歌词","errType":7,"errTitle":"求翻译"},"detail":""}"#,
        ),
        (
            Scrolling,
            "8",
            r#"{"lyric_error":{"txt":"补歌词","errType":8,"errTitle":"求滚动"},"detail":""}"#,
        ),
    ];
    let revision = fixture.revision();
    let song = "9007199254740993";
    for (kind, _, _) in offers {
        let write = || LyricBountyWrite::new(song, kind).unwrap();
        assert_eq!(
            fixture
                .journal()
                .submit(write(), &revision, &|| Ok(()))
                .await
                .unwrap(),
            LyricBountyStatus::Accepted
        );
        assert_eq!(
            fixture.reopen(11).status(&write()).unwrap(),
            Some(LyricBountyStatus::Accepted)
        );
        assert_eq!(fixture.reopen(12).status(&write()).unwrap(), None);
        let alias = LyricBountyWrite::new(&format!("000{song}"), kind).unwrap();
        assert_eq!(
            fixture
                .reopen(11)
                .submit(alias, &revision, &|| panic!(
                    "accepted journal must not replay"
                ))
                .await
                .unwrap(),
            LyricBountyStatus::Accepted
        );
    }
    assert!(fixture.native.response_info().unwrap().unwrap().decrypted);
    let requests = fixture.script.requests.lock().unwrap();
    assert_eq!(requests.len(), offers.len());
    for (request, (_, error_type, info)) in requests.iter().zip(offers) {
        assert_eq!(
            request.url.as_str(),
            "https://interfacepc.music.163.com/eapi/v1/feedback/lyric"
        );
        assert_eq!(request.route, WireRoute::Eapi);
        assert!(request.decrypt_response);
        assert_eq!(request.headers["User-Agent"], OFFICIAL_WINDOWS_USER_AGENT);
        assert!(request.headers["Cookie"].contains("MUSIC_U=synthetic-bounty"));
        let form = url::form_urlencoded::parse(&request.body).collect::<Vec<_>>();
        assert_eq!(form.len(), 1);
        assert_eq!(form[0].0, "params");
        let (endpoint, mut data) = eapi::unwrap(&form[0].1).unwrap();
        assert_eq!(endpoint, LyricBountyWrite::PATH);
        let header = data.as_object_mut().unwrap().remove("header").unwrap();
        let header: Value = serde_json::from_str(header.as_str().unwrap()).unwrap();
        assert_eq!(
            header,
            json!({
                "clientSign":"synthetic-sign", "os":"pc", "appver":"3.1.42.205594",
                "deviceId":fixture.credentials.request_credentials(&ApiOrigin::Pc.url()).unwrap().device_id,
                "requestId":0, "osver":"Microsoft Windows 10",
            })
        );
        assert_eq!(
            data,
            json!({
                "songId":song, "errorType":error_type, "lyricType":"lrc", "lyricVersion":"0",
                "xInfo":info, "e_r":true,
            })
        );
    }
}

#[tokio::test]
async fn lyric_bounty_pre_dispatch_failure_is_only_safe_retry_boundary() {
    use LyricBountyKind::*;
    let fixture = Fixture::new([
        Reply::Interrupted,
        Reply::Json(403, json!({"code":200})),
        Reply::Json(200, json!({"code":"200"})),
    ]);
    let revision = fixture.revision();
    let journal = fixture.journal();
    let write = |kind| LyricBountyWrite::new("44", kind).unwrap();
    for path in [LyricBountyWrite::PATH, "/api/v1/feedback/lyric/"] {
        assert!(
            fixture
                .native
                .request(path, json!({}), false)
                .await
                .is_err()
        );
    }
    assert!(
        fixture
            .native
            .request("/api/batch", json!({(LyricBountyWrite::PATH): {}}), false)
            .await
            .is_err()
    );
    assert!(
        journal
            .submit(write(MissingLyrics), "expired-revision", &|| Ok(()))
            .await
            .is_err()
    );
    assert_eq!(journal.status(&write(MissingLyrics)).unwrap(), None);
    assert_eq!(
        journal
            .submit(write(MissingLyrics), &revision, &|| Err(
                Error::StaleResponse
            ))
            .await
            .unwrap(),
        LyricBountyStatus::NotSent
    );
    assert_eq!(fixture.count(), 0);
    for kind in [MissingLyrics, Translation, Scrolling] {
        assert_eq!(
            journal
                .submit(write(kind), &revision, &|| Ok(()))
                .await
                .unwrap(),
            LyricBountyStatus::Unknown
        );
        assert_eq!(
            fixture
                .reopen(11)
                .submit(write(kind), &revision, &|| panic!(
                    "unknown must stay frozen"
                ))
                .await
                .unwrap(),
            LyricBountyStatus::Unknown
        );
    }
    assert_eq!(fixture.count(), 3);
    let target = ApiOrigin::Pc
        .request_url(WireRoute::Eapi, LyricBountyWrite::PATH)
        .unwrap();
    fixture
        .credentials
        .record_response(
            &target,
            &["MUSIC_U=synthetic-different; Path=/eapi/v1/feedback/lyric; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    assert!(fixture.native.lyric_bounty_credential_revision().is_err());
    assert!(
        journal
            .submit(
                LyricBountyWrite::new("45", Translation).unwrap(),
                &revision,
                &|| Ok(())
            )
            .await
            .is_err()
    );
    assert_eq!(fixture.count(), 3);
}

#[tokio::test]
async fn lyric_bounty_crash_recovery_and_current_result_are_conservative() {
    use LyricBountyKind::*;
    let fixture = Fixture::new([
        Reply::Json(200, json!({"code":200,"sgc":false})),
        Reply::Json(
            200,
            json!({"code":200,"qfy":true,"lrc":{"lyric":"合成未同步","version":4}}),
        ),
        Reply::Json(200, json!({"code":200,"qfy":true})),
        Reply::Pending,
    ]);
    let service = LyricsService::new(fixture.session.clone()).unwrap();
    let read = |song_id: &str| LyricRequest {
        target: LyricTarget::Song(song_id.into()),
        demand: LyricDemand {
            detail_page: true,
            ..Default::default()
        },
        downloaded_audio: None,
        track_mv: false,
    };
    let none = read("21");
    let shown = read("22");
    service.fetch(none.clone()).await.unwrap();
    assert!(service.bounty_revision(&none.target, MissingLyrics).is_ok());
    service.fetch(shown.clone()).await.unwrap();
    assert!(service.bounty_revision(&shown.target, Scrolling).is_ok());
    assert!(service.bounty_revision(&shown.target, Translation).is_ok());
    assert!(
        service
            .bounty_revision(&shown.target, MissingLyrics)
            .is_err()
    );
    assert!(
        service
            .bounty_revision(&none.target, MissingLyrics)
            .is_err()
    );
    service.clear().unwrap();
    // A version-only reply backed by cache still displays the lyrics but
    // cannot create a new spend intent from that presentation state.
    let displayed = service.fetch(shown.clone()).await.unwrap();
    assert!(!displayed.lyric.lyric_lines.is_empty());
    assert!(service.bounty_revision(&shown.target, Scrolling).is_err());
    assert!(service.bounty_revision(&shown.target, Translation).is_err());
    let journal = fixture.journal();
    let revision = fixture.revision();
    let write = || LyricBountyWrite::new("22", Scrolling).unwrap();
    let attempt = tokio::spawn(async move { journal.submit(write(), &revision, &|| Ok(())).await });
    tokio::time::timeout(Duration::from_secs(2), fixture.script.entered.notified())
        .await
        .unwrap();
    assert!(matches!(
        fixture
            .reopen(11)
            .submit(write(), &fixture.revision(), &|| Ok(()))
            .await,
        Err(Error::MutationBusy)
    ));
    attempt.abort();
    assert!(attempt.await.unwrap_err().is_cancelled());
    assert_eq!(
        fixture
            .reopen(11)
            .submit(write(), &fixture.revision(), &|| panic!(
                "crash may have sent the debit"
            ))
            .await
            .unwrap(),
        LyricBountyStatus::Unknown
    );
    assert_eq!(fixture.count(), 4);
}
