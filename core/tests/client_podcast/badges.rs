#[allow(dead_code)]
#[path = "../support/http.rs"]
mod wire;

use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    client::{MusicClient, dto::PodcastQuery},
    podcast::PodcastScope,
    protocol::{
        api::Api,
        http::{ApiOrigin, ClientProfile, MusicHttpClient, RequestOptions},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use wire::{HttpResponse, ScriptHttp};

const CLEAR: &str = "/api/djradio/newcount/clear/byradio";

struct Fixture {
    _root: tempfile::TempDir,
    credentials: Arc<Credentials>,
    http: Arc<ScriptHttp>,
    session: Arc<AccountSession>,
    music: MusicClient,
}
fn fixture(replies: impl IntoIterator<Item = HttpResponse>) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("profile");
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let credentials = Arc::new(
        Credentials::open(Arc::new(StateStore::open(profile).unwrap()), ApiOrigin::Pc).unwrap(),
    );
    cookie(&credentials, "synthetic-badge-current");
    let http = Arc::new(ScriptHttp::new(replies));
    let api: Arc<dyn Api> = Arc::new(
        MusicHttpClient::with_backend(
            credentials.clone(),
            ClientProfile::default(),
            RequestOptions::default(),
            None,
            http.clone(),
        )
        .unwrap(),
    );
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":71}), &session.token().unwrap())
        .unwrap();
    let music = MusicClient::with_session(session.clone(), registry);
    Fixture {
        _root: root,
        credentials,
        http,
        session,
        music,
    }
}
fn cookie(credentials: &Credentials, value: &str) {
    let target = ApiOrigin::Pc.url();
    let before = credentials.request_credentials(&target).unwrap();
    credentials
        .record_response(
            &target,
            &[format!("MUSIC_U={value}; Path=/; Secure")],
            None,
            &before.revision,
        )
        .unwrap();
}
fn subscribed(count: u64, latest: u64) -> HttpResponse {
    json!({"code":200,"data":{"radioIdList":[7,8,9],"myPodcastVOList":[
        {"id":7,"name":"Observed","newVoiceCount":count,"newProgramCount":1,
         "lastProgramCreateTime":latest,"cookie":"not-renderer-data"},
        {"id":8,"name":"Other","newProgramCount":4},
        {"id":9,"name":"Malformed","newVoiceCount":"garbage","newProgramCount":8}
    ]}})
    .into()
}
fn liked() -> HttpResponse {
    json!({"code":200,"data":{"data":[],"hasMore":false}}).into()
}
fn query(offset: usize, limit: usize) -> PodcastQuery {
    PodcastQuery {
        scope: PodcastScope::Podcasts,
        keyword: String::new(),
        offset,
        limit,
        revision: None,
    }
}
fn clear_wires(fixture: &Fixture) -> Vec<Value> {
    fixture
        .http
        .calls()
        .into_iter()
        .filter_map(|call| {
            (call.path == CLEAR).then(|| {
                assert!(
                    call.cookie
                        .unwrap()
                        .contains("MUSIC_U=synthetic-badge-current")
                );
                let mut payload = call.payload;
                // EAPI always owns common transport metadata. Only the typed
                // business field can vary for this write.
                payload.as_object_mut().unwrap().remove("header");
                payload
            })
        })
        .collect()
}

#[tokio::test]
async fn subscribed_badge_clear_only_from_current_visible_row_and_exact_pc_wire() {
    let fixture = fixture([
        subscribed(108, 10),
        liked(),
        json!({"code":200}).into(),
        subscribed(108, 10),
        liked(),
        subscribed(108, 11),
        liked(),
    ]);
    let c = &fixture.music;
    let page = c.podcast_page("1", query(0, 1), true).await.unwrap();
    assert_eq!(page.cards[0].new_episode_count, Some(108));
    let receipt = page.subscribed_badge_receipt.unwrap();
    assert!(clear_wires(&fixture).is_empty(), "list reads never consume");
    assert!(matches!(
        c.podcast_clear_subscribed_badge("1", &receipt, "8").await,
        Err(Error::StaleResponse)
    ));
    let next = c.podcast_page("1", query(1, 2), false).await.unwrap();
    assert_eq!(next.cards[0].new_episode_count, Some(4));
    assert_eq!(next.cards[1].new_episode_count, None);
    assert!(matches!(
        c.podcast_clear_subscribed_badge("1", &receipt, "7").await,
        Err(Error::StaleResponse)
    ));
    let current = c.podcast_page("1", query(0, 1), false).await.unwrap();
    c.podcast_clear_subscribed_badge("1", &current.subscribed_badge_receipt.unwrap(), "7")
        .await
        .unwrap();
    assert_eq!(clear_wires(&fixture), [json!({"radioId":"7"})]);
    let back = c.podcast_page("1", query(0, 3), true).await.unwrap();
    assert_eq!(
        back.cards
            .iter()
            .map(|c| c.new_episode_count)
            .collect::<Vec<_>>(),
        [None, Some(4), None],
        "same observed count is acknowledged only for its own row"
    );
    let update = c.podcast_page("1", query(0, 3), true).await.unwrap();
    assert_eq!(
        update.cards[0].new_episode_count,
        Some(108),
        "different server episode revision re-arms"
    );
    assert!(
        !serde_json::to_string(&update)
            .unwrap()
            .contains("not-renderer-data")
    );
    assert_eq!(clear_wires(&fixture), [json!({"radioId":"7"})]);
}

#[tokio::test]
async fn subscribed_badge_clear_lost_reply_never_replays_and_revokes_on_credential_change() {
    let fixture = fixture([
        subscribed(2, 10),
        liked(),
        HttpResponse::Ready(Err(Error::Transport("synthetic lost response".into()))),
        subscribed(2, 10),
        liked(),
    ]);
    let c = &fixture.music;
    let first = c.podcast_page("1", query(0, 1), true).await.unwrap();
    assert!(matches!(
        c.podcast_clear_subscribed_badge("1", &first.subscribed_badge_receipt.unwrap(), "7")
            .await,
        Err(Error::Transport(_))
    ));
    let reopened = c.podcast_page("1", query(0, 3), true).await.unwrap();
    assert_eq!(
        reopened.cards[0].new_episode_count,
        Some(2),
        "uncertain does not pretend to be cleared"
    );
    let receipt = reopened.subscribed_badge_receipt.unwrap();
    assert!(matches!(
        c.podcast_clear_subscribed_badge("1", &receipt, "7").await,
        Err(Error::StaleResponse)
    ));
    cookie(&fixture.credentials, "synthetic-badge-rotated");
    assert!(matches!(
        c.podcast_clear_subscribed_badge("1", &receipt, "8").await,
        Err(Error::CredentialChanged)
    ));
    let api = Arc::new(wire::ScriptHttp::new([]));
    let other: Arc<dyn Api> = Arc::new(
        MusicHttpClient::with_backend(
            fixture.credentials.clone(),
            ClientProfile::default(),
            RequestOptions::default(),
            None,
            api.clone(),
        )
        .unwrap(),
    );
    fixture
        .session
        .adopt(
            other,
            json!({"userId":99}),
            &fixture.session.token().unwrap(),
        )
        .unwrap();
    for epoch in ["1", "2"] {
        assert!(matches!(
            c.podcast_clear_subscribed_badge(epoch, &receipt, "8").await,
            Err(Error::StaleResponse)
        ));
    }
    assert!(api.calls().is_empty());
    assert_eq!(clear_wires(&fixture), [json!({"radioId":"7"})]);
}

#[tokio::test]
async fn subscribed_badge_clear_rejects_replaced_native_observation_with_equal_page_revision() {
    let fixture = fixture([subscribed(2, 10), liked(), subscribed(2, 11), liked()]);
    let c = &fixture.music;
    let shown = c.podcast_page("1", query(0, 1), true).await.unwrap();
    let registry = ProfileRegistry::open(fixture._root.path(), ApiOrigin::Pc).unwrap();
    let another = MusicClient::with_session(fixture.session.clone(), registry);
    let newer = another.podcast_page("1", query(0, 1), true).await.unwrap();
    assert_eq!(
        shown.revision, newer.revision,
        "same structural list and playback revision"
    );
    assert!(matches!(
        c.podcast_clear_subscribed_badge("1", &shown.subscribed_badge_receipt.unwrap(), "7")
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(clear_wires(&fixture).is_empty());
}
