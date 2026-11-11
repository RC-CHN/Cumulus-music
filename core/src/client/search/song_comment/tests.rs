use super::*;
use crate::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::dto::SearchResultQuery,
    protocol::{api::ApiFuture, http::ApiOrigin},
};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::Notify;

const SEARCH: &str = "/api/search/song/list/page";
const LYRICS: &str = "/api/search/resource/lyric";
const COUNT: &str = "/api/resource/commentInfo/list";

#[derive(Default)]
struct Wire {
    revision: AtomicU64,
    hold: AtomicBool,
    entered: Notify,
    release: Notify,
    counts: Mutex<Vec<(String, Value, bool)>>,
}

fn search_reply() -> Value {
    let resource = |row| json!({"baseInfo":{"simpleSongData":row}});
    json!({"code":200,"data":{"totalCount":10,"hasMore":false,"resources":[
        resource(json!({"id":1,"name":"paid","fee":1,"privilege":{"st":0,"fee":0,"dl":128000,"pl":128000}})),
        resource(json!({"id":2,"name":"copyright","privilege":{"st":-100,"pl":0}})),
        resource(json!({"id":3,"name":"wrong thread"})),
        resource(json!({"id":4,"name":"service failure"})),
        resource(json!({"id":5,"name":"special","t":1})),
        resource(json!({"id":6,"name":"cloud","privilege":{"cs":true}})),
        resource(json!({"id":7,"name":"local file","fileId":"private","filename":"private.mp3"})),
        resource(json!({"id":8,"name":"mismatched alternative","onlineTrackId":9})),
        resource(json!({"id":10,"name":"mixed voice","programId":9})),
        resource(json!({"name":"missing identifier"}))
    ]}})
}

fn lyric_reply() -> Value {
    json!({"code":200,"data":{"totalCount":11,"resources":[
        {"id":1,"songId":"1","song":{"id":"1","name":"paid","fee":1},"lyrics":"合成行"},
        {"song":{"id":2,"name":"copyright","privilege":{"st":-100,"pl":0}},"lyric":"合成零评论"},
        {"song":{"id":3,"name":"wrong thread"}},
        {"song":{"id":4,"name":"service failure"}},
        {"song":{"id":5,"name":"special","t":1}},
        {"song":{"id":6,"name":"outer special"},"songType":1},
        {"song":{"id":7,"name":"outer local"},"fileId":"private"},
        {"song":{"id":8,"name":"outer cloud"},"privilege":{"cs":true}},
        {"song":{"id":9,"name":"outer identity mismatch"},"id":90},
        {"song":{"id":10,"name":"mixed voice"},"voice_id":90},
        {"song":{"name":"no nested id"},"id":11}
    ]}})
}

impl Api for Wire {
    fn request<'a>(&'a self, path: &'a str, args: Value, cached: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            match path {
                SEARCH => Ok(search_reply()),
                LYRICS => Ok(lyric_reply()),
                COUNT => {
                    self.counts
                        .lock()
                        .unwrap()
                        .push((path.into(), args.clone(), cached));
                    assert_eq!(args["resourceType"], 4);
                    assert_eq!(args.as_object().unwrap().len(), 2);
                    assert!(!cached);
                    if self.hold.load(Ordering::Acquire) {
                        self.entered.notify_one();
                        self.release.notified().await;
                    }
                    let (thread, count) = match args["resourceIds"].as_str() {
                        Some("[\"1\"]") => ("R_SO_4_1", 36),
                        Some("[\"2\"]") => ("R_SO_4_2", 0),
                        Some("[\"3\"]") => ("R_SO_4_30", 99),
                        Some("[\"4\"]") => {
                            return Err(Error::Transport("sealed optional read failed".into()));
                        }
                        _ => panic!("unexpected synthetic comment target"),
                    };
                    Ok(json!({"code":200,"data":[{"threadId":thread,"commentCount":count}]}))
                }
                _ => panic!("unexpected synthetic request: {path}"),
            }
        })
    }

    fn search_privilege_credential_revision(&self) -> Result<Option<String>> {
        Ok(Some(self.revision.load(Ordering::Acquire).to_string()))
    }

    fn search_song_share_credential_revision(&self) -> Result<Option<String>> {
        self.search_privilege_credential_revision()
    }

    fn song_card_download_scope_revision(&self) -> Result<Option<String>> {
        self.search_privilege_credential_revision()
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    session: Arc<AccountSession>,
    wire: Arc<Wire>,
    client: MusicClient,
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
        let session = Arc::new(AccountSession::new(wire.clone(), Some(registry.store())));
        session
            .adopt(wire.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let client = MusicClient::with_session(session.clone(), registry);
        Self {
            _root: root,
            session,
            wire,
            client,
        }
    }

    async fn view(&self) -> Value {
        self.view_for(SearchCategory::Songs).await
    }

    async fn view_for(&self, category: SearchCategory) -> Value {
        let result = self
            .client
            .search_results(
                "1",
                SearchResultQuery {
                    category,
                    ..query()
                },
            )
            .await
            .unwrap();
        assert_eq!(result.epoch, "1");
        serde_json::to_value(result.value).unwrap()
    }
}

fn receipt(row: &Value) -> &str {
    assert_eq!(row["songCommentMenu"]["kind"], "eligible");
    row["songCommentMenu"]["receipt"].as_str().unwrap()
}

#[tokio::test]
async fn only_verified_search_song_comments_include_paid_and_copyright_rows() {
    let fixture = Fixture::new();
    let view = fixture.view().await;
    let rows = view["sections"][0]["items"].as_array().unwrap();
    assert_eq!(rows.len(), 9);
    assert_eq!(view["unavailableCount"], 1);
    assert_eq!(rows[0]["track"]["fee"], 1);
    assert!(rows[1].get("songShareReceipt").is_none());
    assert!(
        rows[4..]
            .iter()
            .all(|row| row["songCommentMenu"] == json!({"kind":"hidden"}))
    );
    assert!(fixture.wire.counts.lock().unwrap().is_empty());

    for other in [
        &rows[0]["songShareReceipt"],
        &rows[0]["queueDropReceipt"],
        &rows[0]["groupDownload"]["receipt"],
    ] {
        let other = other.as_str().unwrap();
        assert_ne!(receipt(&rows[0]), other);
        assert!(matches!(
            fixture
                .client
                .search_song_row_comment_count("1", other)
                .await,
            Err(Error::QueueChanged)
        ));
    }
    assert!(fixture.wire.counts.lock().unwrap().is_empty());
    for (row, expected) in rows.iter().zip([Some(36), Some(0), None, None]) {
        let result = fixture
            .client
            .search_song_row_comment_count("1", receipt(row))
            .await
            .unwrap();
        assert_eq!((result.epoch.as_str(), result.value), ("1", expected));
    }
    let calls = fixture.wire.counts.lock().unwrap();
    assert_eq!(calls.len(), 4);
    for (index, call) in calls.iter().enumerate() {
        assert_eq!(
            call,
            &(
                COUNT.into(),
                json!({"resourceType":4,"resourceIds":format!("[\"{}\"]", index + 1)}),
                false
            )
        );
    }
}

#[tokio::test]
async fn lyric_rows_issue_only_comment_receipts_for_unambiguous_nested_songs() {
    let fixture = Fixture::new();
    let view = fixture.view_for(SearchCategory::Lyrics).await;
    let rows = view["sections"][0]["items"].as_array().unwrap();
    assert_eq!(rows.len(), 10);
    assert_eq!(view["unavailableCount"], 1);
    assert_eq!(rows[0]["kind"], "song");
    assert_eq!(rows[0]["excerpt"], json!(["合成行"]));
    assert!(rows.iter().all(|row| {
        [
            "queueDropReceipt",
            "groupDownload",
            "songShareReceipt",
            "songPublicLinkAvailable",
        ]
        .iter()
        .all(|field| row.get(*field).is_none())
    }));
    assert!(
        rows[4..]
            .iter()
            .all(|row| row["songCommentMenu"] == json!({"kind":"hidden"}))
    );
    assert!(fixture.wire.counts.lock().unwrap().is_empty());

    // Reopen a different exact row, including zero and invalid thread fallback.
    for (index, expected) in [(2, None), (0, Some(36)), (1, Some(0)), (3, None)] {
        assert_eq!(
            fixture
                .client
                .search_song_row_comment_count("1", receipt(&rows[index]))
                .await
                .unwrap()
                .value,
            expected
        );
    }
    assert_eq!(
        *fixture.wire.counts.lock().unwrap(),
        [3, 1, 2, 4].map(|id| (
            COUNT.to_owned(),
            json!({"resourceType":4,"resourceIds":format!("[\"{id}\"]")}),
            false
        ))
    );
}

#[tokio::test(start_paused = true)]
async fn search_comment_rejects_late_credential_and_account_handoffs_and_bounds_optional_wait() {
    let fixture = Fixture::new();
    let view = fixture.view().await;
    let row = &view["sections"][0]["items"][0];
    let wire = &fixture.wire;
    wire.hold.store(true, Ordering::Release);
    let before = tokio::time::Instant::now();
    assert_eq!(
        fixture
            .client
            .search_song_row_comment_count("1", receipt(row))
            .await
            .unwrap()
            .value,
        None
    );
    assert_eq!(before.elapsed(), COUNT_LIMIT);
    wire.entered.notified().await;

    let in_flight = fixture
        .client
        .search_song_row_comment_count("1", receipt(row));
    tokio::pin!(in_flight);
    tokio::select! {
        _ = wire.entered.notified() => (),
        _ = &mut in_flight => panic!("count completed before credential rotation"),
    }
    wire.revision.fetch_add(1, Ordering::Release);
    wire.release.notify_one();
    assert!(matches!(in_flight.await, Err(Error::StaleResponse)));
    let baseline = wire.counts.lock().unwrap().len();
    assert!(matches!(
        fixture
            .client
            .search_song_row_comment_count("1", receipt(row))
            .await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(wire.counts.lock().unwrap().len(), baseline);

    let newer = fixture.view().await;
    let next_row = &newer["sections"][0]["items"][0];
    let detaching = fixture
        .client
        .search_song_row_comment_count("1", receipt(next_row));
    tokio::pin!(detaching);
    tokio::select! {
        _ = wire.entered.notified() => (),
        _ = &mut detaching => panic!("count completed before account detach"),
    }
    fixture.session.detach().unwrap();
    wire.release.notify_one();
    assert!(matches!(detaching.await, Err(Error::StaleResponse)));
}

#[test]
fn projection_has_no_search_comment_receipt_without_native_publication() {
    // Guest results use the same projection but never invoke authenticated capture.
    for (raw, category) in [
        (search_reply(), SearchCategory::Songs),
        (lyric_reply(), SearchCategory::Lyrics),
    ] {
        let (view, _, _) = super::super::results::project(
            raw,
            SearchResultQuery {
                category,
                ..query()
            },
            "",
            false,
        )
        .unwrap();
        let view = serde_json::to_value(view).unwrap();
        assert!(
            view["sections"][0]["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row.get("songCommentMenu").is_none())
        );
    }
}
