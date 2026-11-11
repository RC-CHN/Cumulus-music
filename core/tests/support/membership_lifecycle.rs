use super::*;
use cumulus_core::{
    account::{profiles::ProfileRegistry, session::MembershipObservation},
    client::MusicClient,
    download::service::{DownloadOptions, OutputContainer},
    playback::quality_preferences::QualityPreferences,
    protocol::http::ApiOrigin,
};
use std::time::Duration;

const NOW: u64 = 1_000_000;
const CAP: u64 = 2_147_483_647;
const PATH: &str = "/api/music-vip-membership/client/vip/info";

fn rights(uid: u64, now: u64, first: u64, second: u64) -> Reply {
    json!({"code":200,"data":{"userId":uid,"now":now,
        "associator":{"expireTime":first,"vipLevel":7},
        "redplus":{"expireTime":second,"vipLevel":1}}})
    .into()
}

fn client(api: Arc<ScriptApi>) -> (tempfile::TempDir, Arc<AccountSession>, Arc<MusicClient>) {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let account = Arc::new(AccountSession::new(api, Some(registry.store())));
    let client = Arc::new(MusicClient::with_session(account.clone(), registry));
    (root, account, client)
}

fn adopt(account: &AccountSession, api: Arc<ScriptApi>, uid: u64) {
    account
        .adopt(api, json!({"userId":uid}), &account.token().unwrap())
        .unwrap();
}

async fn wait(ms: u64) {
    tokio::time::advance(Duration::from_millis(ms)).await;
    tokio::task::yield_now().await;
}

async fn observation(receiver: &mut tokio::sync::watch::Receiver<Option<MembershipObservation>>) {
    tokio::time::timeout(Duration::from_secs(2), receiver.changed())
        .await
        .expect("membership observation did not arrive")
        .unwrap();
}

fn calls(api: &ScriptApi, uid: &str, count: usize) {
    let calls = api.calls();
    assert_eq!(calls.len(), count);
    assert!(
        calls.iter().all(|call| {
            call.path == PATH && call.payload == json!({"userId":uid}) && !call.cache
        })
    );
}

fn set_download_default(store: &StateStore, owner: &str, quality: Quality) -> DownloadOptions {
    let options = DownloadOptions {
        quality,
        tags: false,
        cover: true,
        lyrics: true,
        container: OutputContainer::Ncm,
    };
    store
        .put(
            owner,
            "download_preferences_v1",
            "options",
            &serde_json::to_value(&options).unwrap(),
        )
        .unwrap();
    options
}

fn download_default(store: &StateStore, owner: &str) -> DownloadOptions {
    serde_json::from_value(
        store
            .get(owner, "download_preferences_v1", "options")
            .unwrap()
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test(start_paused = true)]
async fn verified_member_downgrade_clamps_only_saved_defaults_in_the_current_owner() {
    let api = Arc::new(ScriptApi::new([
        rights(1, NOW, NOW + 5_000, NOW + 1_000),
        rights(1, NOW + 2_000, NOW + 5_000, 0),
        rights(1, NOW + 6_000, 0, 0),
    ]));
    let (root, account, client) = client(api.clone());
    let store = ProfileRegistry::open(root.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let high = QualityPreferences::replace(&store, "1", "initial", Quality::Vivid).unwrap();
    let mut downloads = set_download_default(&store, "1", Quality::Sky);
    let guest =
        QualityPreferences::replace(&store, "local-guest", "initial", Quality::Higher).unwrap();
    let other = QualityPreferences::replace(&store, "2", "initial", Quality::HiRes).unwrap();
    let other_downloads = set_download_default(&store, "2", Quality::JyMaster);
    let mut observed = client.subscribe_membership_observations().unwrap();
    let host = client.start_membership_lifecycle().await.unwrap();

    // An unsigned account does not read membership or act like a free member.
    wait(1_000).await;
    calls(&api, "1", 0);
    assert_eq!(QualityPreferences::read(&store, "1").unwrap(), high);
    adopt(&account, api.clone(), 1);
    observation(&mut observed).await;
    calls(&api, "1", 1);
    assert_eq!(QualityPreferences::read(&store, "1").unwrap(), high);
    assert_eq!(download_default(&store, "1"), downloads);

    let mut previous = high;
    for (advance, count, ceiling) in [(2_000, 2, Quality::JyEffect), (4_000, 3, Quality::ExHigh)] {
        wait(advance).await;
        observation(&mut observed).await;
        calls(&api, "1", count);
        let next = QualityPreferences::read(&store, "1").unwrap();
        assert_eq!(next.quality, ceiling);
        assert_ne!(next.revision, previous.revision);
        downloads.quality = ceiling;
        assert_eq!(download_default(&store, "1"), downloads);
        previous = next;
    }
    assert_eq!(
        QualityPreferences::read(&store, "local-guest").unwrap(),
        guest
    );
    assert_eq!(QualityPreferences::read(&store, "2").unwrap(), other);
    assert_eq!(download_default(&store, "2"), other_downloads);
    assert!(
        store
            .get("local-guest", "download_preferences_v1", "options")
            .unwrap()
            .is_none()
    );
    drop(host);
}

#[tokio::test(start_paused = true)]
async fn wrong_owner_and_failed_membership_reads_never_clamp_local_settings() {
    for reply in [
        rights(2, NOW, 0, 0),
        Reply::Ready(Err(Error::Transport("fixture".into()))),
    ] {
        let api = Arc::new(ScriptApi::new([reply]));
        let (root, account, client) = client(api.clone());
        let store = ProfileRegistry::open(root.path(), ApiOrigin::Pc)
            .unwrap()
            .store();
        let listen = QualityPreferences::replace(&store, "1", "initial", Quality::Vivid).unwrap();
        let options = set_download_default(&store, "1", Quality::Lossless);
        let host = client.start_membership_lifecycle().await.unwrap();
        adopt(&account, api.clone(), 1);
        api.wait_for_calls(1).await;
        wait(60_000).await;
        calls(&api, "1", 1);
        assert_eq!(QualityPreferences::read(&store, "1").unwrap(), listen);
        assert_eq!(download_default(&store, "1"), options);
        drop(host);
    }
}

#[tokio::test(start_paused = true)]
async fn failed_download_default_write_cannot_publish_partial_member_preferences() {
    let api = Arc::new(ScriptApi::new([
        rights(1, NOW, 0, 0),
        rights(1, NOW + 1, 0, 0),
    ]));
    let (root, account, client) = client(api.clone());
    let store = ProfileRegistry::open(root.path(), ApiOrigin::Pc)
        .unwrap()
        .store();
    let listen = QualityPreferences::replace(&store, "1", "initial", Quality::HiRes).unwrap();
    let mut options = set_download_default(&store, "1", Quality::Lossless);
    let database = rusqlite::Connection::open(root.path().join("core.sqlite3")).unwrap();
    database
        .execute_batch(
            "CREATE TRIGGER reject_quality_default BEFORE INSERT ON state
         WHEN NEW.owner='1' AND NEW.kind='download_preferences_v1' AND NEW.key='options'
         BEGIN SELECT RAISE(ABORT, 'synthetic local write failure'); END;",
        )
        .unwrap();
    let mut observed = client.subscribe_membership_observations().unwrap();
    let host = client.start_membership_lifecycle().await.unwrap();
    adopt(&account, api.clone(), 1);
    api.wait_for_calls(1).await;
    wait(1_000).await;
    calls(&api, "1", 1);
    assert!(!observed.has_changed().unwrap());
    assert!(store.get("1", "membership", "current").unwrap().is_none());
    assert_eq!(QualityPreferences::read(&store, "1").unwrap(), listen);
    assert_eq!(download_default(&store, "1"), options);

    database
        .execute_batch("DROP TRIGGER reject_quality_default")
        .unwrap();
    client.member_rights("1", true).await.unwrap();
    observation(&mut observed).await;
    calls(&api, "1", 2);
    assert!(store.get("1", "membership", "current").unwrap().is_some());
    assert_eq!(
        QualityPreferences::read(&store, "1").unwrap().quality,
        Quality::ExHigh
    );
    options.quality = Quality::ExHigh;
    assert_eq!(download_default(&store, "1"), options);
    drop(host);
}

#[tokio::test(start_paused = true)]
async fn verified_startup_and_changed_dates_rearm_exactly_one_deadline() {
    for changed in [false, true] {
        let expiry1 = NOW + 1_000;
        let expiry2 = NOW + 6_000;
        let api = Arc::new(ScriptApi::new([
            rights(1, NOW, expiry1, expiry2),
            rights(1, NOW + 2_000, if changed { 0 } else { expiry1 }, expiry2),
            rights(1, NOW + 7_000, 0, 0),
        ]));
        let (_root, account, client) = client(api.clone());
        let mut observed = client.subscribe_membership_observations().unwrap();
        let host = client.start_membership_lifecycle().await.unwrap();
        wait(5_000).await;
        calls(&api, "1", 0);
        adopt(&account, api.clone(), 1);
        observation(&mut observed).await;
        calls(&api, "1", 1);
        wait(1_999).await;
        calls(&api, "1", 1);
        wait(1).await;
        observation(&mut observed).await;
        calls(&api, "1", 2);
        if changed {
            wait(4_999).await;
            calls(&api, "1", 2);
            wait(1).await;
            observation(&mut observed).await;
            calls(&api, "1", 3);
        }
        wait(120_000).await;
        calls(&api, "1", if changed { 3 } else { 2 });
        drop(host);
    }
}

#[tokio::test(start_paused = true)]
async fn startup_or_new_account_cancels_old_read_and_no_guest_request_is_sent() {
    let (held, release) = gate();
    let api = Arc::new(ScriptApi::new([held]));
    let next = Arc::new(ScriptApi::new([rights(2, NOW, NOW + 500, 0)]));
    let (_root, account, client) = client(api.clone());
    let mut observed = client.subscribe_membership_observations().unwrap();
    let host = client.start_membership_lifecycle().await.unwrap();
    adopt(&account, api.clone(), 1);
    api.wait_for_calls(1).await;
    adopt(&account, next.clone(), 2);
    observation(&mut observed).await;
    calls(&api, "1", 1);
    calls(&next, "2", 1);
    assert!(
        release
            .send(Ok(json!({"code":200,"data":{"userId":1,"now":NOW}})))
            .is_err()
    );
    assert_eq!(observed.borrow().as_ref().unwrap().epoch, "2");
    account.detach().unwrap();
    wait(60_000).await;
    calls(&next, "2", 1);
    drop(host);
}

#[tokio::test(start_paused = true)]
async fn official_timer_cap_reads_once_even_before_expiry() {
    let expires = NOW + CAP + 60_000;
    let api = Arc::new(ScriptApi::new([
        rights(1, NOW, expires, 0),
        rights(1, NOW + CAP, expires, 0),
    ]));
    let (_root, account, client) = client(api.clone());
    adopt(&account, api.clone(), 1);
    let mut observed = client.subscribe_membership_observations().unwrap();
    let host = client.start_membership_lifecycle().await.unwrap();
    observation(&mut observed).await;
    wait(CAP - 1).await;
    calls(&api, "1", 1);
    wait(1).await;
    api.wait_for_calls(2).await;
    assert!(!observed.has_changed().unwrap());
    wait(120_000).await;
    calls(&api, "1", 2);
    drop(host);
}

#[tokio::test(start_paused = true)]
async fn manual_and_startup_share_a_pending_read_and_same_tuple_can_move_the_deadline() {
    let (held, release) = gate();
    let api = Arc::new(ScriptApi::new([
        held,
        rights(1, NOW + 500, NOW + 5_000, 0),
        rights(1, NOW + 6_000, 0, 0),
    ]));
    let (_root, account, client) = client(api.clone());
    adopt(&account, api.clone(), 1);
    let mut observed = client.subscribe_membership_observations().unwrap();
    let host = client.start_membership_lifecycle().await.unwrap();
    api.wait_for_calls(1).await;
    let manual = {
        let client = client.clone();
        tokio::spawn(async move { client.member_rights("1", true).await })
    };
    tokio::task::yield_now().await;
    release
        .send(Ok(json!({"code":200,"data":{"userId":1,"now":NOW,
        "associator":{"expireTime":NOW+1_000,"vipLevel":7}}})))
        .unwrap();
    manual.await.unwrap().unwrap();
    observation(&mut observed).await;
    calls(&api, "1", 1);
    wait(500).await;
    client.member_rights("1", true).await.unwrap();
    assert!(!observed.has_changed().unwrap());
    wait(1_500).await;
    calls(&api, "1", 2);
    wait(3_999).await;
    calls(&api, "1", 2);
    wait(1).await;
    observation(&mut observed).await;
    calls(&api, "1", 3);
    drop(host);
}

#[tokio::test(start_paused = true)]
async fn failed_initial_read_does_not_automatically_retry() {
    let api = Arc::new(ScriptApi::new([Reply::Ready(Err(Error::Transport(
        "fixture".into(),
    )))]));
    let (_root, account, client) = client(api.clone());
    adopt(&account, api.clone(), 1);
    let host = client.start_membership_lifecycle().await.unwrap();
    api.wait_for_calls(1).await;
    wait(180_000).await;
    calls(&api, "1", 1);
    drop(host);
}

#[tokio::test(start_paused = true)]
async fn verified_cold_restore_still_reads_once_but_identical_seeded_dates_do_not_arm() {
    let expiry = NOW + 2_000;
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":1}}).into(),
        rights(1, NOW, expiry, 0),
    ]));
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    registry
        .store()
        .put(
            "1",
            "membership",
            "current",
            &json!({
                "data":{"userId":"1","now":NOW,"associator":{"expireTime":expiry}},
                "server_ms":NOW,"wall":wall
            }),
        )
        .unwrap();
    let account = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = MusicClient::with_session(account.clone(), registry);
    let mut observed = client.subscribe_membership_observations().unwrap();
    let host = client.start_membership_lifecycle().await.unwrap();
    wait(10_000).await;
    assert!(api.calls().is_empty());
    account.refresh().await.unwrap();
    observation(&mut observed).await;
    let calls = api.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].path, "/api/w/nuser/account/get");
    assert_eq!(calls[1].path, PATH);
    assert_eq!(calls[1].payload, json!({"userId":"1"}));
    wait(120_000).await;
    assert_eq!(api.calls().len(), 2);
    drop(host);
}

#[tokio::test]
async fn cached_member_page_reads_the_latest_record_from_an_independent_native_reader() {
    let api = Arc::new(ScriptApi::new([
        rights(1, NOW, NOW + 20_000, 0),
        rights(1, NOW + 1_000, 0, 0),
    ]));
    let (_root, account, client) = client(api.clone());
    adopt(&account, api.clone(), 1);
    let active = |view: cumulus_core::client::dto::MemberRightsView| {
        serde_json::to_value(view).unwrap()["entitlements"][0]["active"] == true
    };
    assert!(active(client.member_rights("1", true).await.unwrap().value));
    MembershipManager::new(account)
        .unwrap()
        .refresh()
        .await
        .unwrap();
    assert!(!active(
        client.member_rights("1", false).await.unwrap().value
    ));
    calls(&api, "1", 2);
}

#[tokio::test(start_paused = true)]
async fn credential_rotation_during_native_read_discards_before_persistence_or_observation() {
    use cumulus_core::protocol::api::{Api, ApiFuture};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Rotating {
        script: ScriptApi,
        revision: AtomicU64,
    }
    impl Api for Rotating {
        fn request<'a>(
            &'a self,
            path: &'a str,
            payload: Value,
            cache: bool,
        ) -> ApiFuture<'a, Value> {
            self.script.request(path, payload, cache)
        }
        fn membership_credential_revision(&self) -> Result<Option<String>> {
            Ok(Some(self.revision.load(Ordering::Acquire).to_string()))
        }
    }

    let (held, release) = gate();
    let api = Arc::new(Rotating {
        script: ScriptApi::new([held]),
        revision: AtomicU64::new(1),
    });
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let store = registry.store();
    let listen = QualityPreferences::replace(&store, "1", "initial", Quality::Vivid).unwrap();
    let options = set_download_default(&store, "1", Quality::Sky);
    let account = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    account
        .adopt(api.clone(), json!({"userId":1}), &account.token().unwrap())
        .unwrap();
    let client = MusicClient::with_session(account, registry);
    let observed = client.subscribe_membership_observations().unwrap();
    let host = client.start_membership_lifecycle().await.unwrap();
    api.script.wait_for_calls(1).await;
    api.revision.store(2, Ordering::Release);
    release
        .send(Ok(json!({"code":200,"data":{"userId":1,"now":NOW,
        "associator":{"expireTime":NOW+500}}})))
        .unwrap();
    wait(5_000).await;
    calls(&api.script, "1", 1);
    assert!(!observed.has_changed().unwrap());
    assert!(store.get("1", "membership", "current").unwrap().is_none());
    assert_eq!(QualityPreferences::read(&store, "1").unwrap(), listen);
    assert_eq!(download_default(&store, "1"), options);
    drop(host);
}
