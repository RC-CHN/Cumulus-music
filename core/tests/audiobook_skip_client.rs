#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod engine;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    podcast::{AudiobookSkip, PodcastScope},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::Duration;

#[derive(Default)]
struct ApiFixture {
    calls: Mutex<Vec<(String, Value)>>,
    category: AtomicU8,
    smart: AtomicBool,
    hold_smart: AtomicBool,
    smart_started: tokio::sync::Notify,
    release_smart: tokio::sync::Notify,
}

impl ApiFixture {
    fn radio(&self) -> Value {
        let mut radio = json!({"id":7,"name":"书","programCount":1});
        match self.category.load(Ordering::SeqCst) {
            1 => radio["categoryId"] = json!(10001),
            2 => radio["categoryId"] = json!("10001"),
            3 => radio["category"] = json!("文学出版"),
            _ => {}
        }
        radio
    }

    fn program(&self) -> Value {
        json!({"id":71,"name":"测试节目","mainTrackId":801,
            "duration":300000,"radio":self.radio()})
    }

    fn smart_calls(&self) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == "/api/voice/alg/skip/switch/get")
            .map(|(_, payload)| payload.clone())
            .collect()
    }
}

impl Api for ApiFixture {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.to_owned(), payload.clone()));
            let response = match path {
                "/api/djradio/v3/get" => json!({"code":200,"data":self.radio()}),
                "/api/v6/dj/program/byradio" => json!({"code":200,"data":{
                    "programs":[self.program()],
                    "count":1,"more":false,"asc":true}}),
                "/api/dj/program/detail" => {
                    assert_eq!(payload["id"], "71");
                    json!({"code":200,"program":self.program()})
                }
                "/api/content/my/liked/voice" => {
                    json!({"code":200,"data":{"data":[],"hasMore":false}})
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    json!({"code":200,"data":{"resumePlayValue":0}})
                }
                "/api/dj/play/url/check" => {
                    assert_eq!(payload["voiceId"], "71");
                    json!({"code":200,"data":{"voiceId":71,"songId":801,"canPlay":true}})
                }
                "/api/song/enhance/player/url/v1" => json!({"code":200,"data":[{
                    "id":801,"code":200,"url":"https://m1.music.126.net/audiobook-fixture.mp3",
                    "md5":"abc","expi":120,"level":"standard"
                }]}),
                "/api/voice/alg/skip/switch/get" => {
                    assert!(!cache);
                    assert_eq!(payload, json!({"radioId":7}));
                    if self.hold_smart.load(Ordering::SeqCst) {
                        self.smart_started.notify_one();
                        self.release_smart.notified().await;
                    }
                    json!({"code":200,"data":self.smart.load(Ordering::SeqCst)})
                }
                _ => panic!("unexpected synthetic endpoint {path}"),
            };
            Ok(response)
        })
    }
}

fn fixture() -> (
    tempfile::TempDir,
    Arc<ApiFixture>,
    Arc<AccountSession>,
    MusicClient,
) {
    let dir = tempfile::tempdir().unwrap();
    let api = Arc::new(ApiFixture::default());
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(session.clone(), registry)
        .with_audio_factory(|| Ok(engine::audio_engine(Arc::new(engine::Device::default()))));
    (dir, api, session, client)
}

fn query() -> PodcastQuery {
    PodcastQuery {
        scope: PodcastScope::Radio {
            id: "7".into(),
            ascending: true,
        },
        keyword: String::new(),
        offset: 0,
        limit: 50,
        revision: None,
    }
}

fn detail(page: &PodcastPage) -> AudiobookSkipContext {
    AudiobookSkipContext::Detail {
        scope: page.scope.clone(),
        revision: page.revision.clone(),
    }
}

async fn open(
    client: &MusicClient,
    epoch: &str,
    context: &AudiobookSkipContext,
) -> AudiobookSkipView {
    client
        .audiobook_skip_settings(epoch, context.clone(), AudiobookSkipAction::Open)
        .await
        .unwrap()
        .value
}

async fn smart(
    client: &MusicClient,
    epoch: &str,
    context: &AudiobookSkipContext,
    edit: &AudiobookSkipView,
) -> cumulus_core::Result<AccountData<AudiobookSkipView>> {
    client
        .audiobook_skip_settings(
            epoch,
            context.clone(),
            AudiobookSkipAction::Smart {
                edit_id: edit.edit_id.clone(),
            },
        )
        .await
}

async fn save(
    client: &MusicClient,
    epoch: &str,
    context: &AudiobookSkipContext,
    edit: &AudiobookSkipView,
    setting: AudiobookSkip,
) -> cumulus_core::Result<AccountData<AudiobookSkipView>> {
    client
        .audiobook_skip_settings(
            epoch,
            context.clone(),
            AudiobookSkipAction::Save {
                edit_id: edit.edit_id.clone(),
                setting,
            },
        )
        .await
}

#[tokio::test]
async fn detail_requires_a_current_numeric_category_and_confirmation_changes_only_local_settings() {
    let (_dir, api, session, client) = fixture();
    api.category.store(2, Ordering::SeqCst);
    let page = client.podcast_page("1", query(), true).await.unwrap();
    assert_eq!(page.audiobook_skip_enabled, None);
    assert!(matches!(
        client
            .audiobook_skip_settings("1", detail(&page), AudiobookSkipAction::Open)
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert!(api.smart_calls().is_empty());

    api.category.store(1, Ordering::SeqCst);
    let page = client.podcast_page("1", query(), true).await.unwrap();
    let context = detail(&page);
    assert_eq!(page.audiobook_skip_enabled, Some(false));
    let wrong = AudiobookSkipContext::Detail {
        scope: page.scope.clone(),
        revision: "stale".into(),
    };
    assert!(matches!(
        client
            .audiobook_skip_settings("1", wrong, AudiobookSkipAction::Open)
            .await,
        Err(Error::StaleResponse)
    ));

    let opened = open(&client, "1", &context).await;
    assert!(!opened.smart_available);
    assert_eq!(opened.setting, AudiobookSkip::Close);
    assert!(api.smart_calls().is_empty());
    assert!(
        !smart(&client, "1", &context, &opened)
            .await
            .unwrap()
            .value
            .smart_available
    );
    assert!(
        !smart(&client, "1", &context, &opened)
            .await
            .unwrap()
            .value
            .smart_available
    );
    assert_eq!(api.smart_calls(), [json!({"radioId":7})]);
    assert!(matches!(
        save(&client, "1", &context, &opened, AudiobookSkip::Smart).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        save(
            &client,
            "1",
            &context,
            &opened,
            AudiobookSkip::Custom { start: 91, end: 0 }
        )
        .await,
        Err(Error::InvalidInput(_))
    ));
    let setting = AudiobookSkip::Custom { start: 18, end: 90 };
    assert_eq!(
        save(&client, "1", &context, &opened, setting)
            .await
            .unwrap()
            .value
            .setting,
        setting
    );
    assert_eq!(api.smart_calls().len(), 1);
    assert!(matches!(
        save(&client, "1", &context, &opened, AudiobookSkip::Close).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        client
            .podcast_page("1", query(), false)
            .await
            .unwrap()
            .audiobook_skip_enabled,
        Some(true)
    );

    api.smart.store(true, Ordering::SeqCst);
    let old = open(&client, "1", &context).await;
    assert!(
        smart(&client, "1", &context, &old)
            .await
            .unwrap()
            .value
            .smart_available
    );
    assert_eq!(old.setting, setting);
    let latest = open(&client, "1", &context).await;
    assert!(matches!(
        save(&client, "1", &context, &old, AudiobookSkip::Smart).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        smart(&client, "1", &context, &latest)
            .await
            .unwrap()
            .value
            .smart_available
    );
    save(&client, "1", &context, &latest, AudiobookSkip::Smart)
        .await
        .unwrap();
    let last = open(&client, "1", &context).await;
    assert_eq!(last.setting, AudiobookSkip::Smart);
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        save(&client, "1", &context, &last, AudiobookSkip::Close).await,
        Err(Error::StaleResponse)
    ));
    let page = client.podcast_page("2", query(), true).await.unwrap();
    assert_eq!(
        open(&client, "2", &detail(&page)).await.setting,
        AudiobookSkip::Close
    );
}

#[tokio::test]
async fn player_menu_accepts_the_official_name_and_cannot_save_for_a_departed_play() {
    let (_dir, api, _session, client) = fixture();
    api.category.store(3, Ordering::SeqCst);
    let page = client.podcast_page("1", query(), true).await.unwrap();
    assert_eq!(page.audiobook_skip_enabled, None);
    let player = tokio::time::timeout(
        Duration::from_secs(5),
        client.play_selection(
            "1",
            PlaySelection::Podcast {
                scope: page.scope,
                revision: page.revision,
                start_id: Some("71".into()),
            },
        ),
    )
    .await
    .expect("synthetic episode did not load")
    .unwrap();
    assert!(player.audiobook_skip_available);
    let entry_id = player.entry_id.unwrap();
    let wrong = AudiobookSkipContext::Player {
        play_id: "stale".into(),
        entry_id: entry_id.clone(),
    };
    assert!(matches!(
        client
            .audiobook_skip_settings("1", wrong, AudiobookSkipAction::Open)
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(api.smart_calls().is_empty());
    let context = AudiobookSkipContext::Player {
        play_id: player.play_id.unwrap(),
        entry_id,
    };
    let opened = open(&client, "1", &context).await;
    assert_eq!(opened.radio_id, "7");
    assert!(
        !smart(&client, "1", &context, &opened)
            .await
            .unwrap()
            .value
            .smart_available
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        client.player_action("1", PlayerAction::Stop),
    )
    .await
    .expect("synthetic episode did not stop")
    .unwrap();
    assert!(matches!(
        save(
            &client,
            "1",
            &context,
            &opened,
            AudiobookSkip::Custom { start: 1, end: 2 }
        )
        .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(api.smart_calls(), [json!({"radioId":7})]);
}

#[tokio::test]
async fn manual_save_does_not_wait_for_the_independent_smart_read() {
    let (_dir, api, _session, client) = fixture();
    api.category.store(1, Ordering::SeqCst);
    api.hold_smart.store(true, Ordering::SeqCst);
    api.smart.store(true, Ordering::SeqCst);
    let context = detail(&client.podcast_page("1", query(), true).await.unwrap());
    let opened = open(&client, "1", &context).await;
    let (result, ()) = tokio::join!(smart(&client, "1", &context, &opened), async {
        tokio::time::timeout(Duration::from_secs(2), api.smart_started.notified())
            .await
            .unwrap();
        let setting = AudiobookSkip::Custom { start: 0, end: 31 };
        let saved = tokio::time::timeout(
            Duration::from_secs(2),
            save(&client, "1", &context, &opened, setting),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(saved.value.setting, setting);
        api.release_smart.notify_one();
    });
    assert!(matches!(result, Err(Error::StaleResponse)));
    assert_eq!(api.smart_calls(), [json!({"radioId":7})]);
}
