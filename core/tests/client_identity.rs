use cumulus_core::{
    Error,
    account::SessionStatus,
    client::{
        MusicClient,
        dto::{LoginCommand, LoginMode},
    },
    identity::{HardwareFacts, IdentityService, IdentitySource},
    protocol::{
        api::ApiFuture,
        http::{BodyStatus, ClientProfile, HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
struct Backend {
    calls: Mutex<Vec<(String, Value, bool)>>,
    sign: String,
    user_agent: String,
    os_version: String,
    protected: bool,
}
impl HttpBackend for Backend {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            assert_eq!(plan.headers["User-Agent"], self.user_agent);
            let form: BTreeMap<_, _> = url::form_urlencoded::parse(&plan.body)
                .into_owned()
                .collect();
            let (path, data) = eapi::unwrap(&form["params"])?;
            let header: Value = serde_json::from_str(data["header"].as_str().unwrap())?;
            if self.protected {
                let config: Value = serde_json::from_str(&plan.headers["MConfig-Info"])?;
                assert_eq!(config["IuRPVVmc3WWul9fT"]["version"], 1);
                assert!(header.get("MConfig-Info").is_none());
                let expected = format!("synthetic-token-{}", self.calls.lock().unwrap().len() + 1);
                assert_eq!(data["checkToken"], expected);
                assert_eq!(plan.headers["X-antiCheatToken"], expected);
                assert_eq!(header["X-antiCheatToken"], expected);
            }
            assert_eq!(header["clientSign"], self.sign);
            assert_eq!(header["osver"], self.os_version);
            assert_ne!(header["deviceId"], self.sign);
            assert_eq!(header["deviceId"].as_str().unwrap().len(), 48);
            let cookie = &plan.headers["Cookie"];
            let signed = cookie
                .split(';')
                .any(|v| v.trim().starts_with("clientSign="));
            assert!(signed);
            let authenticated = cookie.contains("MUSIC_U=synthetic-native-identity");
            self.calls
                .lock()
                .unwrap()
                .push((path.clone(), header, authenticated));
            let body = match path.as_str() {
                "/api/w/nuser/account/get" => {
                    if authenticated {
                        json!({"code":200,"profile":{"userId":"1","nickname":"Synthetic"}})
                    } else {
                        json!({"code":301})
                    }
                }
                "/api/login/qrcode/unikey" => json!({"code":200,"unikey":"fixture-native-profile"}),
                "/api/w/login" => json!({"code":200}),
                "/api/user/playlist" if self.protected => {
                    json!({"code":200,"playlist":[],"more":false})
                }
                "/api/playlist/create" if self.protected => json!({"code":403}),
                _ => return Err(Error::State("unexpected native identity test route")),
            };
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&body)?,
                body_status: BodyStatus::Complete,
                set_cookies: if path == "/api/w/login" {
                    vec!["MUSIC_U=synthetic-native-identity; Path=/; Secure; HttpOnly".into()]
                } else {
                    vec![]
                },
                refresh_token: None,
                metadata: BTreeMap::new(),
            })
        })
    }
}
#[tokio::test]
async fn selected_identity_reaches_initial_qr_email_saved_candidates_and_reopened_profile() {
    exercise_native_profile(false, false).await;
}
#[tokio::test]
async fn windows_compatibility_keeps_hardware_identity_and_each_candidate_device_id() {
    exercise_native_profile(true, false).await;
}
#[tokio::test]
async fn pc_configuration_and_fresh_token_context_follow_initial_qr_email_saved_and_reopened_profiles()
 {
    exercise_native_profile(true, true).await;
}
struct FreshTokens(std::sync::atomic::AtomicUsize);
impl cumulus_core::protocol::context::TokenProvider for FreshTokens {
    fn token<'a>(&'a self, business: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async move {
            assert_eq!(business, "fixture");
            Ok(format!(
                "synthetic-token-{}",
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
            ))
        })
    }
}
struct ConfigBackend;
impl cumulus_core::protocol::mconfig::ConfigBackend for ConfigBackend {
    fn get<'a>(
        &'a self,
        _: &'a cumulus_core::protocol::mconfig::ConfigUrl,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Ok(json!({"yidun#paths":{"whiteList":[{"businessId":"fixture","list":["/api/"]}]}}))
        })
    }
}
async fn exercise_native_profile(windows: bool, protected: bool) {
    if windows {
        let source: Value =
            serde_json::from_str(include_str!("fixtures/windows-compatibility-native.json"))
                .unwrap();
        assert_eq!(source["osverInstructionBytesVerified"], true);
        for case in source["descriptionCases"].as_array().unwrap() {
            let profile =
                ClientProfile::windows_compatibility(case["input"].as_str().unwrap()).unwrap();
            assert_eq!(profile.user_agent, source["officialUserAgent"]);
            assert_eq!(profile.os_version, case["output"]);
        }
        assert!(ClientProfile::windows_compatibility("Windows\r\nHeader: invalid").is_err());
    }
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(StateStore::open(root.path()).unwrap());
    let facts = HardwareFacts::from_encoded(
        ["10:20:30:40:50:61".into(), "DISK".into(), String::new()],
        b"10:20:30:40:50:61DISK".to_vec(),
        vec![],
        IdentitySource {
            platform: "fixture".into(),
            interface: "fake0".into(),
            disk_device: "disk0".into(),
            host_description_available: false,
        },
    )
    .unwrap();
    let context = if protected {
        use cumulus_core::protocol::{
            context::RequestContext,
            mconfig::{APPLICATION_KEY, PcConfig},
        };
        let service = PcConfig::new(store.clone(), Arc::new(ConfigBackend)).unwrap();
        service.resolve(&BTreeMap::from([("MConfig-Info".into(),json!({APPLICATION_KEY:{"version":1,"os":"pc","fullUrl":"https://d5.music.126.net/obj/fixture/config.data"}}).to_string())])).await.unwrap();
        Some(Arc::new(
            RequestContext::from_pc(
                service,
                "3.1.42.205594",
                Arc::new(FreshTokens(std::sync::atomic::AtomicUsize::new(0))),
            )
            .unwrap(),
        ))
    } else {
        None
    };
    let identity = IdentityService::new(store.clone()).resolve(&facts).unwrap();
    let profile = identity
        .profile(if windows {
            ClientProfile::windows_compatibility(
                "Microsoft Windows 10 Professional (build 19045),64bit",
            )
            .unwrap()
        } else {
            ClientProfile {
                user_agent: "Cumulus synthetic identity host".into(),
                os_version: "Linux".into(),
                client_sign: None,
            }
        })
        .unwrap();
    let backend = Arc::new(Backend {
        calls: Mutex::default(),
        sign: identity.client_sign().unwrap(),
        user_agent: profile.user_agent.clone(),
        os_version: profile.os_version.clone(),
        protected,
    });
    let client = MusicClient::open_with_request_context(
        root.path(),
        None,
        profile.clone(),
        Some(backend.clone()),
        context.clone(),
    )
    .unwrap();
    let guest = client.restore_account().await.unwrap();
    assert_ne!(guest.status, SessionStatus::Authenticated);
    let qr = client.begin_qr().await.unwrap();
    client.cancel_login(&qr.operation_id).unwrap();
    let flow = client
        .begin_login(&guest.epoch, LoginMode::Email, None)
        .await
        .unwrap();
    client
        .login_command(
            &flow.operation_id,
            LoginCommand::Email {
                address: "fixture@example.test".into(),
                password: "fixture-password".into(),
                remember: true,
            },
        )
        .await
        .unwrap();
    let account = client.account().unwrap();
    assert_eq!(account.status, SessionStatus::Authenticated);
    if protected {
        assert_official_playlist_write(&client, "after-email").await;
    }
    let saved = client.saved_accounts(&account.epoch).unwrap();
    assert_eq!(saved.value.len(), 1);
    let flow = client
        .begin_login(
            &account.epoch,
            LoginMode::SavedAccount,
            Some(&saved.value[0].id),
        )
        .await
        .unwrap();
    client
        .login_command(&flow.operation_id, LoginCommand::Reconcile)
        .await
        .unwrap();
    if protected {
        assert_official_playlist_write(&client, "after-saved-account").await;
    }
    let selected = store
        .get("transport", "profiles", "active")
        .unwrap()
        .unwrap();
    let session_store = StateStore::open(
        root.path()
            .join("sessions")
            .join(selected["session_id"].as_str().unwrap()),
    )
    .unwrap();
    let before = session_store
        .get("transport", "credentials", "session")
        .unwrap()
        .unwrap();
    drop(client);
    let client = MusicClient::open_with_request_context(
        root.path(),
        None,
        profile,
        Some(backend.clone()),
        context,
    )
    .unwrap();
    assert_eq!(
        client.restore_account().await.unwrap().status,
        SessionStatus::Authenticated
    );
    let after = session_store
        .get("transport", "credentials", "session")
        .unwrap()
        .unwrap();
    assert_eq!(before, after);
    if protected {
        assert_official_playlist_write(&client, "after-reopen").await;
    }
    let calls = backend.calls.lock().unwrap();
    let guest_device = &calls[0].1["deviceId"];
    let qr_device = &calls
        .iter()
        .find(|c| c.0 == "/api/login/qrcode/unikey")
        .unwrap()
        .1["deviceId"];
    let login_device = &calls.iter().find(|c| c.0 == "/api/w/login").unwrap().1["deviceId"];
    assert_ne!(guest_device, qr_device);
    assert_ne!(qr_device, login_device);
    assert_eq!(&calls.last().unwrap().1["deviceId"], login_device);
    assert_ne!(login_device, identity.persistent_id());
}

async fn assert_official_playlist_write(client: &MusicClient, name: &str) {
    let epoch = client.account().unwrap().epoch;
    let outcome = client
        .mutate_playlist(
            &epoch,
            cumulus_core::playlist::PlaylistCommand::Create {
                name: name.into(),
                private: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        outcome.value.phase,
        cumulus_core::playlist::PlaylistPhase::Rejected
    );
    assert_eq!(outcome.value.service_codes["main"], 403);
}
