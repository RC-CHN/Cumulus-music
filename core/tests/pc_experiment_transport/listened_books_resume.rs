use super::{
    HttpResponse, ScriptHttp, assignment_arguments, gate, reply, vinyl_http_with_credentials,
};
use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{ListenedBooksQuery, PodcastQuery},
    },
    podcast::{PodcastAction, PodcastScope, PodcastService},
    protocol::{
        http::{ApiOrigin, MusicHttpClient},
        pc_experiments::ASSIGNMENT_PATH,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

const BOOKS: &str = "/api/voice/book/recent";
const RESUME: &str = "/api/dj/user/setting/resumeplay/get";

struct Fixture {
    _root: tempfile::TempDir,
    client: MusicClient,
    session: Arc<AccountSession>,
    native: Arc<MusicHttpClient>,
    credentials: Arc<Credentials>,
    http: Arc<ScriptHttp>,
    epoch: String,
}

impl Fixture {
    fn new(responses: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let (native, http, credentials) = vinyl_http_with_credentials(
            root.path(),
            &["MUSIC_U=synthetic-books; Path=/; Secure"],
            responses,
        );
        let registry = ProfileRegistry::open(&root.path().join("host"), ApiOrigin::Pc).unwrap();
        let session = Arc::new(AccountSession::new(native.clone(), Some(registry.store())));
        session
            .adopt(
                native.clone(),
                json!({"userId":71}),
                &session.token().unwrap(),
            )
            .unwrap();
        let epoch = session.token().unwrap().generation.to_string();
        let client = MusicClient::with_session(session.clone(), registry);
        Self {
            _root: root,
            client,
            session,
            native,
            credentials,
            http,
            epoch,
        }
    }

    async fn page(&self, query: ListenedBooksQuery) -> Value {
        let reply = self
            .client
            .listened_books(&self.epoch, query)
            .await
            .unwrap();
        assert_eq!(reply.epoch, self.epoch);
        serde_json::to_value(reply.value).unwrap()
    }

    async fn same_page(&self, opened: &Value) -> Value {
        let revision = opened["revision"].as_str().unwrap();
        let page = self
            .page(ListenedBooksQuery::Page {
                revision: revision.into(),
                number: 1,
            })
            .await;
        assert_eq!(page["revision"], revision);
        page
    }

    async fn setting(&self, radio: &str) -> bool {
        PodcastService::new(self.session.clone())
            .unwrap()
            .state_for(&self.session.token().unwrap(), PodcastAction::Resume, radio)
            .await
            .unwrap()
    }
}

fn assignment(group: &str) -> Value {
    json!({"code":200,"data":[{"expName":"Podcast_ResumePlaySwitch","expGroupName":group}]})
}

fn radio(id: u64) -> Value {
    json!({"id":id,"name":format!("合成书 {id}"),"programCount":1})
}

fn books() -> Value {
    let chapter = |id: u64, radio_id, completed| {
        json!({"id":id,"name":format!("合成章节 {id}"),
        "duration":100000,"mainSong":{"id":1000+id},"radio":radio(radio_id),
        "djPlayRecordVo":{"listenLocation":37000,"isListened":completed}})
    };
    json!({"code":200,"data":{"currentList":[chapter(6,22,false),chapter(7,23,true),chapter(8,24,false)]}})
}

fn setting(enabled: bool) -> Value {
    json!({"code":200,"data":{"resumePlayValue":u8::from(enabled)}})
}

#[tokio::test]
async fn listened_books_hides_only_seen_off_progress_in_t1_without_new_reads() {
    let f = Fixture::new(
        [
            books(),
            assignment("t1"),
            json!({"code":200,"data":radio(22)}),
            json!({"code":200,"data":{"programs":[],"count":0,"more":false,"asc":false}}),
            json!({"code":200,"data":{"data":[],"hasMore":false}}),
            setting(false),
            setting(false),
            assignment("c"),
            assignment("t1"),
            setting(true),
        ]
        .map(HttpResponse::from),
    );
    let opened = f.page(ListenedBooksQuery::Open).await;
    assert_eq!(opened["rows"][0]["chapter"]["percent"], 37);
    assert_eq!(opened["rows"][1]["chapter"]["completed"], true);
    f.client.refresh_pc_presentation().await.unwrap();
    assert_eq!(f.same_page(&opened).await, opened);
    let detail = f
        .client
        .podcast_page(
            &f.epoch,
            PodcastQuery {
                scope: PodcastScope::Radio {
                    id: "22".into(),
                    ascending: false,
                },
                keyword: String::new(),
                offset: 0,
                limit: 20,
                revision: None,
            },
            true,
        )
        .await
        .unwrap();
    assert_eq!(detail.resume_enabled, Some(false));
    assert!(!f.setting("23").await);
    let hidden = f.same_page(&opened).await;
    let mut expected = opened.clone();
    expected["rows"][0]["chapter"]["percent"] = Value::Null;
    expected["rows"][1]["chapter"]["completed"] = json!(false);
    assert_eq!(
        hidden, expected,
        "only the actual progress labels are hidden"
    );
    f.client.refresh_pc_presentation().await.unwrap();
    assert_eq!(
        f.same_page(&opened).await,
        opened,
        "control ignores seen Off"
    );
    f.client.refresh_pc_presentation().await.unwrap();
    assert_eq!(f.same_page(&opened).await, hidden);
    assert!(f.setting("22").await);
    expected["rows"][0]["chapter"]["percent"] = json!(37);
    assert_eq!(
        f.same_page(&opened).await,
        expected,
        "an observed On restores the label"
    );

    let calls = f.http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [
            BOOKS,
            ASSIGNMENT_PATH,
            "/api/djradio/v3/get",
            "/api/v6/dj/program/byradio",
            "/api/content/my/liked/voice",
            RESUME,
            RESUME,
            ASSIGNMENT_PATH,
            ASSIGNMENT_PATH,
            RESUME,
        ]
    );
    for call in calls.iter().filter(|call| call.path == ASSIGNMENT_PATH) {
        assert_eq!(call.payload["expNames"], assignment_arguments()["expNames"]);
    }
    for (index, id) in [(5, "22"), (6, "23"), (9, "22")] {
        assert_eq!(calls[index].payload["radioId"], id);
    }
}

#[tokio::test]
async fn listened_books_ignores_rotated_and_differently_scoped_credentials() {
    let (old_credentials, release_rotated) = gate();
    let f = Fixture::new([
        assignment("t1").into(),
        books().into(),
        setting(false).into(),
        old_credentials,
        assignment("t1").into(),
        assignment("t1").into(),
        setting(false).into(),
        books().into(),
    ]);
    f.client.refresh_pc_presentation().await.unwrap();
    let opened = f.page(ListenedBooksQuery::Open).await;
    assert!(!f.setting("22").await);
    assert_eq!(
        f.same_page(&opened).await["rows"][0]["chapter"]["percent"],
        Value::Null
    );

    let stale = tokio::spawn({
        let session = f.session.clone();
        async move {
            PodcastService::new(session.clone())
                .unwrap()
                .state_for(&session.token().unwrap(), PodcastAction::Resume, "22")
                .await
        }
    });
    f.http.wait(4).await;
    let mut rotated = reply(setting(false));
    rotated
        .set_cookies
        .push("MUSIC_U=synthetic-new; Path=/; Secure".into());
    assert!(release_rotated.send(Ok(rotated)).is_ok());
    assert!(
        !stale.await.unwrap().unwrap(),
        "existing read result survives its own credential rotation"
    );
    f.client.refresh_pc_presentation().await.unwrap();
    assert_eq!(
        f.same_page(&opened).await,
        opened,
        "old credential evidence is never reused"
    );

    let url = ApiOrigin::Pc
        .request_url(cumulus_core::protocol::http::WireRoute::Eapi, RESUME)
        .unwrap();
    let old = f.credentials.request_credentials(&url).unwrap().revision;
    f.credentials
        .record_response(
            &url,
            &[format!(
                "MUSIC_U=synthetic-foreign; Path={}; Secure",
                url.path()
            )],
            None,
            &old,
        )
        .unwrap();
    f.client.refresh_pc_presentation().await.unwrap();
    assert!(!f.setting("22").await);
    assert_eq!(
        f.same_page(&opened).await,
        opened,
        "the resume URL's mismatched cookie cannot project Off"
    );

    let revision = opened["revision"].as_str().unwrap().to_owned();
    f.session
        .adopt(
            f.native.clone(),
            json!({"userId":72}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let new_epoch = f.session.token().unwrap().generation.to_string();
    assert!(matches!(
        f.client
            .listened_books(
                &new_epoch,
                ListenedBooksQuery::Page {
                    revision,
                    number: 1
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    let new = f
        .client
        .listened_books(&new_epoch, ListenedBooksQuery::Open)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(new.value).unwrap()["rows"][0]["chapter"]["percent"],
        37
    );
    assert_eq!(f.http.calls().len(), 8);
}
