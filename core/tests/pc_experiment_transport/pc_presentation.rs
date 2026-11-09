//! Full signed native transport; all HTTP is sealed synthetic data.
use super::{HttpResponse, ScriptHttp, gate, reply, vinyl_http_with_credentials};
use cumulus_core::{
    Error,
    account::{AccountSession, SessionStatus, credentials::Credentials, profiles::ProfileRegistry},
    client::{
        MusicClient,
        pc_experiments::{OrdinarySkipAssignment, PcPresentationRuntimeStatus, PcPresentationView},
    },
    protocol::{
        api::Api,
        http::{ApiOrigin, MusicHttpClient},
        pc_experiments::{ANONYMOUS_PATH, ASSIGNMENT_PATH, PcAnonymousState, assignment_arguments},
    },
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

const ACCOUNT: &str = "/api/w/nuser/account/get";
const COOKIE: &str = "MUSIC_U=synthetic-presentation; Path=/; Secure";

#[path = "pc_presentation/guest_search.rs"]
mod guest_search;

struct Fixture {
    root: tempfile::TempDir,
    client: Arc<MusicClient>,
    native: Arc<MusicHttpClient>,
    credentials: Arc<Credentials>,
    registry: Arc<ProfileRegistry>,
    session: Arc<AccountSession>,
    http: Arc<ScriptHttp>,
}

impl Fixture {
    fn new(authenticated: bool, responses: impl IntoIterator<Item = HttpResponse>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let cookies: &[&str] = if authenticated { &[COOKIE] } else { &[] };
        let (native, http, credentials) =
            vinyl_http_with_credentials(root.path(), cookies, responses);
        let registry = super::fixture_registry(&root.path().join("host"));
        let session = Arc::new(AccountSession::new(native.clone(), Some(registry.store())));
        if authenticated {
            session
                .adopt(
                    native.clone(),
                    json!({"userId":71}),
                    &session.token().unwrap(),
                )
                .unwrap();
        }
        let client = Arc::new(MusicClient::with_session(session.clone(), registry.clone()));
        Self {
            root,
            client,
            native,
            credentials,
            registry,
            session,
            http,
        }
    }
    fn epoch(&self) -> String {
        self.session.token().unwrap().generation.to_string()
    }
    fn paths(&self) -> Vec<String> {
        self.http
            .calls()
            .into_iter()
            .map(|call| call.path)
            .collect()
    }
}

fn groups(treatment: bool) -> Value {
    let group = if treatment { "t1" } else { "c" };
    json!({"code":200,"data":[
        {"expName":"PH-PC-searchHot","expGroupName":group},
        {"expName":"PH-PC-search-result","expGroupName":group},
        {"expName":"PH-PC-searchMore2","expGroupName":group},
        {"expName":"PH-PC-searchPageEfficiency","expGroupName":group},
        {"expName":"PH-PC-skipSongs2","expGroupName":group}
    ]})
}

async fn status(
    receiver: &mut watch::Receiver<PcPresentationRuntimeStatus>,
    expected: PcPresentationRuntimeStatus,
) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if *receiver.borrow_and_update() == expected {
                return;
            }
            receiver.changed().await.expect("native runtime stopped");
        }
    })
    .await
    .expect("native runtime did not reach expected state");
}

async fn flags(
    receiver: &mut watch::Receiver<PcPresentationView>,
    expected: bool,
) -> PcPresentationView {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let view = receiver.borrow_and_update().clone();
            if view.search_footer == expected {
                return view;
            }
            receiver
                .changed()
                .await
                .expect("native presentation stopped");
        }
    })
    .await
    .expect("native presentation did not change")
}

fn assert_batch(http: &ScriptHttp) {
    for call in http
        .calls()
        .iter()
        .filter(|call| call.path == ASSIGNMENT_PATH)
    {
        let mut payload = call.payload.clone();
        assert!(
            payload
                .as_object_mut()
                .unwrap()
                .remove("header")
                .unwrap()
                .is_string()
        );
        assert_eq!(payload, assignment_arguments());
        assert!(call.login_chain.is_none());
    }
}

#[tokio::test]
async fn download_vivid_visibility_uses_both_live_signed_assignments_without_extra_requests() {
    let groups = |high, audio| {
        json!({"code":200,"data":[
            {"expName":"PH-PC-hyyinzhi","expGroupName":high},
            {"expName":"PH-PC-zhenyingn","expGroupName":audio}
        ]})
    };
    let f = Fixture::new(
        true,
        [
            groups("c", "t1").into(),
            groups("t1", "t1").into(),
            groups("t1", "c").into(),
        ],
    );
    let choice = || {
        let result = f.client.download_quality_preferences(&f.epoch()).unwrap();
        let view = serde_json::to_value(result).unwrap();
        view["choices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["quality"] == "vivid")
            .cloned()
    };
    assert_eq!(choice(), None);
    assert!(f.paths().is_empty());
    for expected in [
        None,
        Some(json!({"quality":"vivid","access":"unknown"})),
        None,
    ] {
        f.client.refresh_pc_presentation().await.unwrap();
        assert_eq!(choice(), expected);
    }
    assert_eq!(f.paths(), [ASSIGNMENT_PATH; 3]);
    assert_batch(&f.http);
}

#[tokio::test(start_paused = true)]
async fn startup_single_lease_fixed_thirty_minutes_and_shutdown_share_actual_full_batch() {
    let f = Fixture::new(
        true,
        [
            groups(true).into(),
            groups(false).into(),
            groups(true).into(),
        ],
    );
    let mut views = f.client.subscribe_pc_presentation().unwrap();
    let lease = f.client.start_pc_presentation_runtime().unwrap();
    assert!(matches!(
        f.client.start_pc_presentation_runtime(),
        Err(Error::MutationBusy)
    ));
    let mut states = lease.subscribe_status();
    status(&mut states, PcPresentationRuntimeStatus::Ready).await;
    let first = flags(&mut views, true).await;
    assert!(first.search_expanded && first.search_dense);
    assert_eq!(
        f.client
            .official_pc_ordinary_skip_assignment(&f.epoch())
            .await
            .unwrap(),
        OrdinarySkipAssignment::Enabled
    );
    assert_eq!(f.paths(), [ASSIGNMENT_PATH]);
    tokio::time::advance(Duration::from_secs(1799)).await;
    tokio::task::yield_now().await;
    assert_eq!(f.http.calls().len(), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    let second = flags(&mut views, false).await;
    assert!(second.revision.parse::<u64>().unwrap() > first.revision.parse::<u64>().unwrap());
    assert_eq!(f.http.calls().len(), 2);
    lease.shutdown().await.unwrap();
    tokio::time::advance(Duration::from_secs(3600)).await;
    assert_eq!(f.http.calls().len(), 2);
    let next = f.client.start_pc_presentation_runtime().unwrap();
    status(
        &mut next.subscribe_status(),
        PcPresentationRuntimeStatus::Ready,
    )
    .await;
    assert_eq!(f.http.calls().len(), 3);
    next.shutdown().await.unwrap();
    assert_batch(&f.http);
}

#[tokio::test]
async fn other_authorized_consumer_publishes_and_forced_refresh_uses_same_native_host() {
    let f = Fixture::new(true, [groups(true).into(), groups(false).into()]);
    let mut views = f.client.subscribe_pc_presentation().unwrap();
    assert_eq!(
        f.client
            .official_pc_ordinary_skip_assignment(&f.epoch())
            .await
            .unwrap(),
        OrdinarySkipAssignment::Enabled
    );
    let first = flags(&mut views, true).await;
    let second = f.client.refresh_pc_presentation().await.unwrap();
    assert!(!second.search_footer && !second.search_dense && !second.search_expanded);
    assert!(second.revision.parse::<u64>().unwrap() > first.revision.parse::<u64>().unwrap());
    assert_eq!(f.paths(), [ASSIGNMENT_PATH, ASSIGNMENT_PATH]);
    assert_batch(&f.http);
}

#[tokio::test]
async fn only_real_credential_commits_notify_and_invalidate_the_native_view() {
    let f = Fixture::new(true, [groups(true).into(), groups(false).into()]);
    let lease = f.client.start_pc_presentation_runtime().unwrap();
    let mut states = lease.subscribe_status();
    status(&mut states, PcPresentationRuntimeStatus::Ready).await;
    let mut commits = f
        .native
        .pc_presentation_credential_changes()
        .unwrap()
        .unwrap();
    let target = ApiOrigin::Pc.url();
    let stored = &f.credentials;
    let revision = stored.request_credentials(&target).unwrap().revision;
    stored
        .record_response(&target, &[COOKIE.into()], None, &revision)
        .unwrap();
    assert_eq!(
        stored.request_credentials(&target).unwrap().revision,
        revision
    );
    assert!(!commits.has_changed().unwrap());
    tokio::task::yield_now().await;
    assert_eq!(f.http.calls().len(), 1);
    stored
        .record_response(
            &target,
            &["MUSIC_U=synthetic-rotated; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    commits.changed().await.unwrap();
    f.http.wait(2).await;
    status(&mut states, PcPresentationRuntimeStatus::Ready).await;
    assert!(!f.client.pc_presentation().unwrap().search_footer);
    assert!(
        f.http.calls()[1]
            .cookie
            .as_ref()
            .unwrap()
            .contains("MUSIC_U=synthetic-rotated")
    );
    lease.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn persisted_bootstrap_survives_failure_but_is_never_fresh_authority() {
    let f = Fixture::new(
        true,
        [
            groups(true).into(),
            json!({"code":403}).into(),
            json!({"code":403}).into(),
        ],
    );
    f.client.refresh_pc_presentation().await.unwrap();
    let restarted = MusicClient::with_session(f.session.clone(), f.registry.clone());
    assert!(restarted.pc_presentation().unwrap().search_footer);
    assert_eq!(f.http.calls().len(), 1);
    assert!(
        restarted
            .official_pc_ordinary_skip_assignment(&f.epoch())
            .await
            .is_err()
    );
    assert!(restarted.pc_presentation().unwrap().search_footer);
    assert!(restarted.refresh_pc_presentation().await.is_err());
    assert!(restarted.pc_presentation().unwrap().search_footer);
    assert_eq!(
        f.paths(),
        [ASSIGNMENT_PATH, ASSIGNMENT_PATH, ASSIGNMENT_PATH]
    );
    let other: Arc<dyn Api> = f.native.clone();
    f.session
        .adopt(other, json!({"userId":72}), &f.session.token().unwrap())
        .unwrap();
    assert!(!restarted.pc_presentation().unwrap().search_footer);
}

#[tokio::test]
async fn known_credential_rotation_clears_live_flags_without_rehydrating_old_owner_cache() {
    let f = Fixture::new(
        true,
        [
            groups(true).into(),
            json!({"code":403}).into(),
            groups(true).into(),
        ],
    );
    let first = f.client.refresh_pc_presentation().await.unwrap();
    assert!(first.search_footer);
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(
            &target,
            &["MUSIC_U=synthetic-presentation-rotated; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    let invalidated = f.client.pc_presentation().unwrap();
    assert!(
        !invalidated.search_footer && !invalidated.search_dense && !invalidated.search_expanded
    );
    assert!(invalidated.revision.parse::<u64>().unwrap() > first.revision.parse::<u64>().unwrap());
    assert!(f.client.refresh_pc_presentation().await.is_err());
    assert_eq!(f.client.pc_presentation().unwrap(), invalidated);
    // Repeated local reads and failed refresh must not reload the old t1.
    assert_eq!(f.client.pc_presentation().unwrap(), invalidated);
    // A separate new host may still use the last owner display bootstrap.
    // It has no fresh authority and is not this known-invalidated context.
    let restarted = MusicClient::with_session(f.session.clone(), f.registry.clone());
    assert!(restarted.pc_presentation().unwrap().search_footer);
    let recovered = f.client.refresh_pc_presentation().await.unwrap();
    assert!(recovered.search_footer);
    assert!(
        recovered.revision.parse::<u64>().unwrap() > invalidated.revision.parse::<u64>().unwrap()
    );
    assert_eq!(
        f.paths(),
        [ASSIGNMENT_PATH, ASSIGNMENT_PATH, ASSIGNMENT_PATH]
    );
}

#[tokio::test]
async fn unknown_waits_then_definite_fresh_301_registers_before_ab_without_promoting_uid() {
    let mut registered = reply(json!({"code":200,"userId":9001}));
    registered.set_cookies = vec![
        "MUSIC_A=synthetic-anonymous; Path=/; Secure".into(),
        "MUSIC_U=must-not-persist; Path=/; Secure".into(),
    ];
    let mut assigned = reply(groups(true));
    assigned.set_cookies = vec!["MUSIC_U=must-not-persist-after-ab; Path=/; Secure".into()];
    let f = Fixture::new(
        false,
        [
            json!({"code":301}).into(),
            HttpResponse::Ready(Ok(registered)),
            HttpResponse::Ready(Ok(assigned)),
        ],
    );
    let lease = f.client.start_pc_presentation_runtime().unwrap();
    let mut states = lease.subscribe_status();
    status(&mut states, PcPresentationRuntimeStatus::WaitingForAccount).await;
    tokio::task::yield_now().await;
    assert!(f.http.calls().is_empty());
    let before = f.session.token().unwrap();
    assert!(f.session.refresh().await.unwrap_err().is_service_code(301));
    status(&mut states, PcPresentationRuntimeStatus::Ready).await;
    assert_eq!(f.session.token().unwrap(), before);
    assert_eq!(f.session.snapshot().unwrap().status, SessionStatus::Unknown);
    assert_eq!(f.paths(), [ACCOUNT, ANONYMOUS_PATH, ASSIGNMENT_PATH]);
    let calls = f.http.calls();
    let original_device = f.credentials.device_id().unwrap();
    assert_eq!(
        calls[1].payload["username"],
        cumulus_core::account::anonymous_identity::encode_anonymous_id(&original_device).unwrap()
    );
    assert!(
        calls[1]
            .cookie
            .as_ref()
            .is_none_or(|value| !value.contains("MUSIC_U"))
    );
    assert!(
        calls[2]
            .cookie
            .as_ref()
            .unwrap()
            .contains("MUSIC_A=synthetic-anonymous")
    );
    let native = f.session_native();
    assert!(
        matches!(native.pc_anonymous_state().unwrap(), PcAnonymousState::Registered{ref uid,..} if uid=="9001")
    );
    assert!(f.client.pc_presentation().unwrap().search_footer);
    assert!(
        f.client
            .official_pc_ordinary_skip_assignment(&f.epoch())
            .await
            .is_err()
    );
    let guest = cumulus_core::account::credentials::Credentials::open(
        Arc::new(
            cumulus_core::storage::StateStore::open(f.root.path().join("host/guest")).unwrap(),
        ),
        ApiOrigin::Pc,
    )
    .unwrap();
    assert!(guest.login_credentials().unwrap().token.is_none());
    assert!(
        guest
            .request_credentials(&ApiOrigin::Pc.url())
            .unwrap()
            .cookies
            .iter()
            .all(|(name, _)| name != "MUSIC_U")
    );
    assert!(f.credentials.login_credentials().unwrap().token.is_none());
    lease.shutdown().await.unwrap();
    assert_batch(&f.http);
}

#[tokio::test]
async fn unknown_with_stored_account_cookie_waits_for_authoritative_restore() {
    let f = Fixture::new(
        false,
        [
            json!({"code":200,"profile":{"userId":71}}).into(),
            groups(true).into(),
        ],
    );
    let target = ApiOrigin::Pc.url();
    let stored = &f.credentials;
    let revision = stored.request_credentials(&target).unwrap().revision;
    stored
        .record_response(&target, &[COOKIE.into()], None, &revision)
        .unwrap();
    let lease = f.client.start_pc_presentation_runtime().unwrap();
    tokio::task::yield_now().await;
    assert!(f.http.calls().is_empty());
    f.session.refresh().await.unwrap();
    status(
        &mut lease.subscribe_status(),
        PcPresentationRuntimeStatus::Ready,
    )
    .await;
    assert_eq!(f.session.token().unwrap().uid.as_deref(), Some("71"));
    assert_eq!(f.paths(), [ACCOUNT, ASSIGNMENT_PATH]);
    assert!(f.client.pc_presentation().unwrap().search_footer);
    lease.shutdown().await.unwrap();
}

impl Fixture {
    fn session_native(&self) -> Arc<dyn Api> {
        // Reopen only for inspection: production transport remains privately
        // owned by AccountSession. No request is made through this handle.
        self.native
            .isolated_pc_anonymous_transport(&self.root.path().join("host/guest"))
            .unwrap()
            .unwrap()
    }
}

#[tokio::test]
async fn lost_registration_ack_is_durable_and_a_second_attempt_never_replays() {
    let f = Fixture::new(
        false,
        [HttpResponse::Ready(Err(Error::Transport(
            "synthetic lost reply".into(),
        )))],
    );
    let directory = f.root.path().join("private-guest");
    let isolated = f
        .native
        .isolated_pc_anonymous_transport(&directory)
        .unwrap()
        .unwrap();
    assert!(isolated.register_pc_anonymous().await.is_err());
    assert!(matches!(
        isolated.pc_anonymous_state().unwrap(),
        PcAnonymousState::Uncertain
    ));
    assert!(matches!(
        isolated.register_pc_anonymous().await.unwrap(),
        PcAnonymousState::Uncertain
    ));
    drop(isolated);
    let reopened = f
        .native
        .isolated_pc_anonymous_transport(&directory)
        .unwrap()
        .unwrap();
    assert!(matches!(
        reopened.register_pc_anonymous().await.unwrap(),
        PcAnonymousState::Uncertain
    ));
    assert_eq!(f.paths(), [ANONYMOUS_PATH]);
}

#[tokio::test]
async fn parallel_anonymous_callers_dispatch_once_and_completed_identity_is_reused() {
    let (held, send) = gate();
    let f = Fixture::new(false, [held]);
    let isolated = f
        .native
        .isolated_pc_anonymous_transport(&f.root.path().join("private-guest"))
        .unwrap()
        .unwrap();
    let one = isolated.clone();
    let task = tokio::spawn(async move { one.register_pc_anonymous().await });
    f.http.wait(1).await;
    assert!(matches!(
        isolated.register_pc_anonymous().await.unwrap(),
        PcAnonymousState::Uncertain
    ));
    let mut response = reply(json!({"code":200,"userId":9001}));
    response
        .set_cookies
        .push("MUSIC_A=synthetic-anonymous; Path=/; Secure".into());
    assert!(send.send(Ok(response)).is_ok());
    assert!(matches!(
        task.await.unwrap().unwrap(),
        PcAnonymousState::Registered { .. }
    ));
    assert!(matches!(
        isolated.register_pc_anonymous().await.unwrap(),
        PcAnonymousState::Registered { .. }
    ));
    assert_eq!(f.paths(), [ANONYMOUS_PATH]);
}

#[tokio::test]
async fn only_definite_missing_identity_retries_to_the_bounded_three_attempts() {
    let f = Fixture::new(
        false,
        [
            json!({"code":200}).into(),
            json!({"code":200,"userId":0}).into(),
            json!({"code":200}).into(),
        ],
    );
    let isolated = f
        .native
        .isolated_pc_anonymous_transport(&f.root.path().join("private-guest"))
        .unwrap()
        .unwrap();
    assert!(isolated.register_pc_anonymous().await.is_err());
    assert!(matches!(
        isolated.pc_anonymous_state().unwrap(),
        PcAnonymousState::Unregistered
    ));
    assert_eq!(f.paths(), [ANONYMOUS_PATH, ANONYMOUS_PATH, ANONYMOUS_PATH]);
    assert_eq!(
        f.http.calls()[0].payload["username"],
        f.http.calls()[2].payload["username"]
    );
}

#[tokio::test]
async fn dropping_a_dispatched_registration_keeps_unknown_ack_and_stops_runtime() {
    let (held, send) = gate();
    let f = Fixture::new(false, [json!({"code":301}).into(), held]);
    f.session.refresh().await.unwrap_err();
    let lease = f.client.start_pc_presentation_runtime().unwrap();
    f.http.wait(2).await;
    lease.shutdown().await.unwrap();
    let native = f.session_native();
    assert!(matches!(
        native.pc_anonymous_state().unwrap(),
        PcAnonymousState::Uncertain
    ));
    let next = f.client.start_pc_presentation_runtime().unwrap();
    status(
        &mut next.subscribe_status(),
        PcPresentationRuntimeStatus::AnonymousUncertain,
    )
    .await;
    assert_eq!(f.paths(), [ACCOUNT, ANONYMOUS_PATH]);
    next.shutdown().await.unwrap();
    assert!(
        send.send(Ok(reply(json!({"code":200,"userId":9001}))))
            .is_err()
    );
}

#[tokio::test(start_paused = true)]
async fn account_change_cancels_old_read_and_the_same_runtime_keeps_its_timer() {
    let (held, old) = gate();
    let f = Fixture::new(true, [held, groups(false).into(), groups(true).into()]);
    let lease = f.client.start_pc_presentation_runtime().unwrap();
    let mut states = lease.subscribe_status();
    f.http.wait(1).await;
    f.session
        .adopt(
            f.native.clone(),
            json!({"userId":72}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    status(&mut states, PcPresentationRuntimeStatus::Ready).await;
    assert!(old.send(Ok(reply(groups(true)))).is_err());
    assert_eq!(f.client.pc_presentation().unwrap().epoch, f.epoch());
    assert!(!f.client.pc_presentation().unwrap().search_footer);
    tokio::time::advance(Duration::from_secs(1800)).await;
    let mut views = f.client.subscribe_pc_presentation().unwrap();
    flags(&mut views, true).await;
    assert_eq!(
        f.paths(),
        [ASSIGNMENT_PATH, ASSIGNMENT_PATH, ASSIGNMENT_PATH]
    );
    lease.shutdown().await.unwrap();
}

#[tokio::test]
async fn cookie_rotation_from_ab_response_has_one_restart_instead_of_a_self_response_loop() {
    let mut first = reply(groups(true));
    first
        .set_cookies
        .push("MUSIC_U=synthetic-ab-one; Path=/; Secure".into());
    let mut second = reply(groups(false));
    second
        .set_cookies
        .push("MUSIC_U=synthetic-ab-two; Path=/; Secure".into());
    let f = Fixture::new(
        true,
        [
            HttpResponse::Ready(Ok(first)),
            HttpResponse::Ready(Ok(second)),
            groups(true).into(),
        ],
    );
    let lease = f.client.start_pc_presentation_runtime().unwrap();
    status(
        &mut lease.subscribe_status(),
        PcPresentationRuntimeStatus::CredentialChurn,
    )
    .await;
    assert_eq!(f.paths(), [ASSIGNMENT_PATH, ASSIGNMENT_PATH]);
    tokio::task::yield_now().await;
    assert_eq!(f.http.calls().len(), 2);
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(
            &target,
            &["MUSIC_U=synthetic-external-change; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    let mut views = f.client.subscribe_pc_presentation().unwrap();
    flags(&mut views, true).await;
    assert_eq!(f.http.calls().len(), 3);
    assert!(
        f.http.calls()[2]
            .cookie
            .as_ref()
            .unwrap()
            .contains("MUSIC_U=synthetic-external-change")
    );
    lease.shutdown().await.unwrap();
}

#[tokio::test]
async fn history_sync_default_follows_successful_signed_refresh_until_an_explicit_choice() {
    use cumulus_core::history::HistorySyncAction::{Read, Set};
    let cohort = |group| {
        json!({"code":200,"data":[{
            "expName":"PH-PC-setting-multiterminal-record","expGroupName":group
        }]})
    };
    let f = Fixture::new(
        true,
        [
            json!({"code":200,"data":[]}).into(),
            cohort("t1").into(),
            json!({"code":403}).into(),
            cohort("c").into(),
            cohort("t1").into(),
        ],
    );
    let epoch = f.epoch();
    let pref = |action| {
        f.client
            .history_sync_preference(&epoch, action)
            .unwrap()
            .value
    };
    let mut observations = f.client.subscribe_history_sync();
    let initial = pref(Read);
    assert!(initial.enabled && !initial.first_disable_confirmed);
    assert!(f.paths().is_empty() && !observations.has_changed().unwrap());
    // A missing setting row in a successful full batch uses M147's empty group.
    f.client.refresh_pc_presentation().await.unwrap();
    let control = pref(Read);
    assert!(!control.enabled && !control.first_disable_confirmed);
    assert!(control.revision > initial.revision);
    assert_eq!(
        *observations.borrow_and_update(),
        Some((epoch.parse().unwrap(), control))
    );
    f.client.refresh_pc_presentation().await.unwrap();
    let treatment = pref(Read);
    assert!(treatment.enabled && treatment.revision > control.revision);
    assert_eq!(
        *observations.borrow_and_update(),
        Some((epoch.parse().unwrap(), treatment))
    );
    assert!(f.client.refresh_pc_presentation().await.is_err());
    assert!(pref(Read).enabled && !observations.has_changed().unwrap());
    f.client.refresh_pc_presentation().await.unwrap();
    assert!(!pref(Read).enabled);

    // Explicitly retaining the current value is still an override, but
    // does not pretend the first-disable confirmation has happened.
    let same = pref(Set {
        enabled: false,
        confirmed_first_disable: false,
    });
    assert!(!same.enabled && !same.first_disable_confirmed);
    f.client.refresh_pc_presentation().await.unwrap();
    assert!(!pref(Read).enabled);
    let restarted = MusicClient::with_session(f.session.clone(), f.registry.clone());
    assert!(
        !restarted
            .history_sync_preference(&epoch, Read)
            .unwrap()
            .value
            .enabled
    );

    assert!(
        pref(Set {
            enabled: true,
            confirmed_first_disable: false
        })
        .enabled
    );
    assert!(
        f.client
            .history_sync_preference(
                &epoch,
                Set {
                    enabled: false,
                    confirmed_first_disable: false,
                }
            )
            .is_err()
    );
    let confirmed = pref(Set {
        enabled: false,
        confirmed_first_disable: true,
    });
    assert!(!confirmed.enabled && confirmed.first_disable_confirmed);
    assert_eq!(f.paths(), [ASSIGNMENT_PATH; 5]);
}

#[tokio::test]
async fn history_sync_default_never_overwrites_an_existing_account_setting() {
    use cumulus_core::history::HistorySyncAction::Read;
    let f = Fixture::new(true, [json!({"code":200,"data":[]}).into()]);
    let store = f.registry.store();
    store
        .put("71", "play-history-v1", "sync-enabled", &json!(true))
        .unwrap();
    store
        .put(
            "72",
            "play-history-v1",
            "sync-experiment-default",
            &json!(false),
        )
        .unwrap();
    f.client.refresh_pc_presentation().await.unwrap();
    assert!(
        f.client
            .history_sync_preference(&f.epoch(), Read)
            .unwrap()
            .value
            .enabled
    );
    assert!(
        store
            .get("71", "play-history-v1", "sync-experiment-default")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get("72", "play-history-v1", "sync-experiment-default")
            .unwrap(),
        Some(json!(false))
    );
    assert!(
        store
            .get("local-guest", "play-history-v1", "sync-experiment-default")
            .unwrap()
            .is_none()
    );
    assert_eq!(f.paths(), [ASSIGNMENT_PATH]);
}

#[tokio::test]
async fn history_sync_confirmation_after_observed_experiment_off_is_a_distinct_user_commit() {
    use cumulus_core::history::HistorySyncAction::{Read, Set};
    let f = Fixture::new(true, [json!({"code":200,"data":[]}).into()]);
    let epoch = f.epoch();
    let mut changes = f.client.subscribe_history_sync();
    let before = f
        .client
        .history_sync_preference(&epoch, Read)
        .unwrap()
        .value;
    assert!(before.enabled && !before.first_disable_confirmed);

    // Modal opened on the original on state. Only the live cohort turns it
    // off, and the original confirmation remains a user action.
    f.client.refresh_pc_presentation().await.unwrap();
    let (_, auto) = changes.borrow_and_update().unwrap();
    assert!(!auto.enabled && !auto.first_disable_confirmed);
    let confirmed = f
        .client
        .history_sync_preference(
            &epoch,
            Set {
                enabled: false,
                confirmed_first_disable: true,
            },
        )
        .unwrap()
        .value;
    assert!(!confirmed.enabled && confirmed.first_disable_confirmed);
    assert!(confirmed.revision > auto.revision);
    assert_eq!(
        *changes.borrow_and_update(),
        Some((epoch.parse().unwrap(), confirmed))
    );
    assert_eq!(f.paths(), [ASSIGNMENT_PATH]);
}
