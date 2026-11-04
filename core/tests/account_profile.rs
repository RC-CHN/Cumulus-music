mod support;

use cumulus_core::{
    Error,
    account::{
        AccountSession, SessionToken,
        profile::{AccountProfile, BindingProvider},
    },
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

fn setup(
    replies: impl IntoIterator<Item = Reply>,
) -> (Arc<ScriptApi>, Arc<AccountSession>, SessionToken) {
    let api = Arc::new(ScriptApi::new(replies));
    let session = Arc::new(AccountSession::new(api.clone(), None));
    session
        .adopt(
            api.clone(),
            json!({"userId":7,"nickname":"Cached","private":"cache-secret"}),
            &session.token().unwrap(),
        )
        .unwrap();
    let token = session.token().unwrap();
    (api, session, token)
}

fn binding(kind: u32, token: Value) -> Value {
    json!({"type":kind,"tokenJsonStr":token.to_string(),"url":"https://auth.example.test/private-authorize"})
}

#[tokio::test]
async fn self_reads_exact_official_paths_and_projects_no_raw_binding_secrets() {
    let (api, session, token) = setup([
        json!({"code":"200","level":8,"profile":{
            "userId":"7","nickname":"Fresh","avatarUrl":"http://p1.music.126.net/path/avatar.jpg?credential=secret-avatar#secret-fragment",
            "gender":9,"birthday":-1_000_000_000,"city":0,"province":123,"signature":"",
            "sensitive":"secret-profile"
        }})
        .into(),
        json!({"code":200,"bindings":[
            binding(0,json!({"email":"alice@example.test","access_token":"secret-access"})),
            binding(1,json!({"cellphone":"13800138000","countrycode":"1242","hasPassword":false})),
            binding(2,json!({"name":"Public Weibo","uid":"secret-provider-id"})),
            binding(5,json!({"nickname":"Public QQ","openid":"secret-openid"})),
            binding(10,json!({"nickname":"Public WeChat","unionid":"secret-unionid"})),
            binding(99,json!({"nickname":"unknown-must-not-project"})),
            json!({"type":1,"tokenJsonStr":"bad JSON secret-malformed"})
        ]})
        .into(),
    ]);
    let service = AccountProfile::new(session);
    let cached = service.snapshot_for(&token).unwrap();
    assert_eq!(cached.nickname.as_deref(), Some("Cached"));
    assert!(cached.level.is_none());
    assert!(api.calls().is_empty());

    let fresh = service.read_for(&token).await.unwrap();
    let calls = api.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].path, "/api/w/v1/user/detail/7");
    assert_eq!(calls[0].payload, json!({"all":true,"userId":"7"}));
    assert_eq!(calls[1].path, "/api/w/v1/user/bindings/7");
    assert_eq!(calls[1].payload, json!({"userId":"7"}));
    assert!(calls.iter().all(|call| !call.cache));
    assert_eq!(fresh.profile.nickname.as_deref(), Some("Fresh"));
    assert_eq!(fresh.profile.birthday, Some(-1_000_000_000));
    assert_eq!(fresh.profile.signature.as_deref(), Some(""));
    assert_eq!(fresh.profile.level, Some(8));
    assert_eq!(fresh.profile.province, Some(123));
    assert!(fresh.profile.gender.is_none());
    assert!(fresh.profile.city.is_none());
    assert_eq!(
        fresh.profile.avatar_url.as_deref(),
        Some("https://p1.music.126.net/path/avatar.jpg?param=320y320")
    );
    assert_eq!(
        fresh.bindings[0].masked_address.as_deref(),
        Some("a***@e***")
    );
    assert_eq!(fresh.bindings[1].masked_address.as_deref(), Some("***8000"));
    assert_eq!(fresh.bindings[1].has_password, Some(false));
    assert_eq!(fresh.bindings[1].country_code.as_deref(), Some("1242"));
    assert_eq!(
        fresh.bindings[4].display_name.as_deref(),
        Some("Public WeChat")
    );
    assert_eq!(fresh.bindings[5].provider, BindingProvider::Unknown);
    assert!(!fresh.bindings[5].details_available);
    assert!(!fresh.bindings[6].details_available);
    let encoded = serde_json::to_string(&fresh).unwrap();
    for forbidden in [
        "secret-",
        "private-authorize",
        "alice@example.test",
        "13800138000",
        "tokenJsonStr",
        "unknown-must-not-project",
    ] {
        assert!(!encoded.contains(forbidden), "leaked {forbidden}");
    }
    assert_eq!(service.snapshot_for(&token).unwrap(), cached);
}

#[tokio::test]
async fn empty_bindings_differ_from_failed_or_malformed_reads() {
    let (_, session, token) = setup([
        json!({"code":200,"bindings":[]}).into(),
        json!({"code":200}).into(),
        json!({"code":503,"bindings":[]}).into(),
        json!({"code":200,"bindings":[{"type":"not numeric"}]}).into(),
    ]);
    let service = AccountProfile::new(session);
    assert!(service.bindings_for(&token).await.unwrap().is_empty());
    assert!(matches!(
        service.bindings_for(&token).await,
        Err(Error::Response(_))
    ));
    assert!(
        service
            .bindings_for(&token)
            .await
            .unwrap_err()
            .is_service_code(503)
    );
    assert!(matches!(
        service.bindings_for(&token).await,
        Err(Error::Response(_))
    ));
}

#[tokio::test]
async fn detail_for_rejects_wrong_identity_before_fetching_bindings() {
    let (api, session, token) = setup([json!({"code":200,"profile":{"userId":8}}).into()]);
    assert!(matches!(
        AccountProfile::new(session).read_for(&token).await,
        Err(Error::Response(_))
    ));
    assert_eq!(api.calls().len(), 1);
}

#[tokio::test]
async fn switching_account_during_bindings_discards_the_combined_read() {
    let (pending, release) = gate();
    let (api, session, token) = setup([json!({"code":200,"profile":{"userId":7}}).into(), pending]);
    let service = AccountProfile::new(session.clone());
    let work = tokio::spawn(async move { service.read_for(&token).await });
    api.wait_for_calls(2).await;
    let replacement = Arc::new(ScriptApi::new([]));
    session
        .adopt(
            replacement.clone(),
            json!({"userId":8}),
            &session.token().unwrap(),
        )
        .unwrap();
    release.send(Ok(json!({"code":200,"bindings":[]}))).unwrap();
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert!(replacement.calls().is_empty());
}

#[tokio::test]
async fn binding_settings_take_registration_only_from_the_verified_account_envelope() {
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":7},"account":{"id":7,"type":1,"userName":"private-registration"}}).into(),
        json!({"code":200,"bindings":[
            binding(5,json!({"nickname":"QQ Fixture","openid":"private-openid"})),
            binding(1,json!({"cellphone":"1234567890","countrycode":"1242","hasPassword":false})),
            binding(99,json!({"nickname":"unsupported-fixture"})),
            binding(0,json!({"email":"one@example.test"})),
            binding(5,json!({"nickname":"Latest QQ Fixture","access_token":"private-token"}))
        ]}).into(),
        // This otherwise valid identity read has no authoritative registration type.
        json!({"code":200,"profile":{"userId":7},"account":{"id":8,"type":0}}).into(),
        json!({"code":200,"bindings":[
            binding(1,json!({"cellphone":"1234567890","countrycode":"1242","hasPassword":false}))
        ]}).into(),
    ]));
    let session = Arc::new(AccountSession::new(api.clone(), None));
    let service = AccountProfile::new(session.clone());
    session.refresh().await.unwrap();
    let token = session.token().unwrap();
    let registered = service.binding_settings_for(&token).await.unwrap();
    assert_eq!(
        registered.registration_provider,
        Some(BindingProvider::Cellphone)
    );
    assert!(registered.foreign_registered_phone);
    assert_eq!(
        registered
            .bindings
            .iter()
            .map(|row| row.provider)
            .collect::<Vec<_>>(),
        [
            BindingProvider::Cellphone,
            BindingProvider::Email,
            BindingProvider::Qq
        ]
    );
    assert_eq!(
        registered.bindings[2].display_name.as_deref(),
        Some("Latest QQ Fixture")
    );
    let output = serde_json::to_string(&registered).unwrap();
    for secret in [
        "private-",
        "tokenJsonStr",
        "one@example.test",
        "1234567890",
        "unsupported-fixture",
    ] {
        assert!(
            !output.contains(secret),
            "binding projection leaked {secret}"
        );
    }

    session.refresh().await.unwrap();
    assert_eq!(session.token().unwrap(), token);
    let unclassified = service.binding_settings_for(&token).await.unwrap();
    assert_eq!(unclassified.registration_provider, None);
    assert!(!unclassified.foreign_registered_phone);
    assert_eq!(unclassified.bindings.len(), 1);
    assert_eq!(
        unclassified.bindings[0].provider,
        BindingProvider::Cellphone
    );

    let calls = api.calls();
    assert_eq!(calls.len(), 4);
    for [identity, bindings] in calls.as_chunks::<2>().0 {
        assert_eq!(identity.path, "/api/w/nuser/account/get");
        assert_eq!(identity.payload, json!({}));
        assert_eq!(bindings.path, "/api/w/v1/user/bindings/7");
        assert_eq!(bindings.payload, json!({"userId":"7"}));
        assert!(!identity.cache && !bindings.cache);
    }
}

#[tokio::test]
async fn binding_settings_never_publish_an_old_accounts_registration_or_rows() {
    let (pending, release) = gate();
    let api = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":7},"account":{"id":7,"type":1}}).into(),
        pending,
    ]));
    let session = Arc::new(AccountSession::new(api.clone(), None));
    session.refresh().await.unwrap();
    let owner = session.token().unwrap();
    let profile = AccountProfile::new(session.clone());
    let task = tokio::spawn(async move { profile.binding_settings_for(&owner).await });
    api.wait_for_calls(2).await;

    let other = Arc::new(ScriptApi::new([
        json!({"code":200,"profile":{"userId":8},"account":{"id":8,"type":10}}).into(),
        json!({"code":200,"bindings":[]}).into(),
    ]));
    session.attach(other.clone()).unwrap();
    session.refresh().await.unwrap();
    release
        .send(Ok(json!({"code":200,"bindings":[
            binding(1,json!({"cellphone":"1234567890","countrycode":"1242"}))
        ]})))
        .unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    let page = AccountProfile::new(session.clone())
        .binding_settings_for(&session.token().unwrap())
        .await
        .unwrap();
    assert_eq!(page.registration_provider, Some(BindingProvider::Wechat));
    assert!(page.bindings.is_empty());
    assert!(!page.foreign_registered_phone);
    assert_eq!(other.calls()[1].path, "/api/w/v1/user/bindings/8");
}
