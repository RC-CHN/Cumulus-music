use cumulus_core::{
    Error,
    membership::Quality,
    playback::quality::*,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn subscriber(v: &Value) -> QualitySubscriber {
    QualitySubscriber {
        vip: v["vip"].as_bool().unwrap(),
        svip: v["svip"].as_bool().unwrap(),
        music_package: v["music_package"].as_bool().unwrap(),
    }
}
fn policy_for<'a>(
    privilege: &'a QualityPrivilege,
    subscriber: QualitySubscriber,
    rights: &'a BTreeMap<QualityRight, Option<QualityUseRight>>,
) -> QualityPolicy<'a> {
    QualityPolicy {
        privilege: Some(privilege),
        subscriber: Some(subscriber),
        use_rights: rights,
        experiments: Default::default(),
        trial: false,
    }
}
#[test]
fn original_official_capabilities_and_cashier_decisions_match_the_frozen_vectors() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/playback-quality-official.json")).unwrap();
    assert_eq!(fixture["vectors"].as_array().unwrap().len(), 82);
    let denied: BTreeMap<_, _> = QualityRight::ALL
        .into_iter()
        .map(|r| {
            (
                r,
                Some(QualityUseRight {
                    can_use: false,
                    user_source: None,
                    trial_status: None,
                    remaining_ms: None,
                }),
            )
        })
        .collect();
    for v in fixture["vectors"].as_array().unwrap() {
        let privilege = QualityPrivilege::from_official(&v["privilege"]).unwrap();
        let quality = Quality::parse(v["quality"].as_str().unwrap()).unwrap();
        let policy = QualityPolicy {
            privilege: Some(&privilege),
            subscriber: Some(subscriber(&v["subscriber"])),
            use_rights: &denied,
            experiments: QualityExperiments {
                music_package_hires: v["experiments"]["music_package_hires"].as_bool(),
                standard_trial_requires_purchase:
                    v["experiments"]["standard_trial_requires_purchase"].as_bool(),
            },
            trial: v["trial"].as_bool().unwrap(),
        };
        assert_eq!(
            policy.availability(quality) == QualityAvailability::Available,
            v["available"].as_bool().unwrap(),
            "availability {v}"
        );
        if v["available"] == true {
            assert_eq!(
                policy.access(quality) == QualityAccess::Allowed,
                v["baseAllowed"].as_bool().unwrap(),
                "entitlement {v}"
            );
        }
    }
}
#[test]
fn unknown_rights_and_config_are_not_converted_into_a_false_membership_denial() {
    let privilege = QualityPrivilege::from_official(
        &json!({"id":7,"maxbr":999000,"fl":0,"payed":0,"fee":0,"flag":(1<<12)|(1<<16)}),
    )
    .unwrap();
    let mut rights = BTreeMap::new();
    let subscriber = QualitySubscriber {
        vip: false,
        svip: false,
        music_package: false,
    };
    assert_eq!(
        policy_for(&privilege, subscriber, &rights).access(Quality::JyMaster),
        QualityAccess::Unknown
    );
    rights.insert(
        QualityRight::JyMaster,
        Some(QualityUseRight {
            can_use: true,
            user_source: Some(0),
            trial_status: Some(1),
            remaining_ms: Some(60000),
        }),
    );
    assert_eq!(
        policy_for(&privilege, subscriber, &rights).access(Quality::JyMaster),
        QualityAccess::TrialEligible
    );
    rights
        .get_mut(&QualityRight::JyMaster)
        .unwrap()
        .as_mut()
        .unwrap()
        .trial_status = Some(2);
    assert_eq!(
        policy_for(&privilege, subscriber, &rights).access(Quality::JyMaster),
        QualityAccess::TrialActive
    );
    rights
        .get_mut(&QualityRight::JyMaster)
        .unwrap()
        .as_mut()
        .unwrap()
        .can_use = false;
    assert_eq!(
        policy_for(&privilege, subscriber, &rights).access(Quality::JyMaster),
        QualityAccess::RequiresSvip
    );
    let mut music = policy_for(&privilege, subscriber, &rights);
    music.subscriber = Some(QualitySubscriber {
        music_package: true,
        ..subscriber
    });
    assert_eq!(music.access(Quality::HiRes), QualityAccess::Unknown);
    music.experiments.music_package_hires = Some(true);
    assert_eq!(music.access(Quality::HiRes), QualityAccess::Allowed);
}
#[test]
fn current_trial_cannot_use_vivid_and_missing_privilege_fields_do_not_mean_missing_assets() {
    let privilege = QualityPrivilege::from_official(
        &json!({"id":7,"flag":1<<21,"maxbr":999000,"fl":0,"payed":4,"fee":1}),
    )
    .unwrap();
    let rights = BTreeMap::new();
    let policy = QualityPolicy {
        privilege: Some(&privilege),
        subscriber: Some(QualitySubscriber {
            vip: true,
            svip: true,
            music_package: false,
        }),
        use_rights: &rights,
        experiments: Default::default(),
        trial: true,
    };
    assert_eq!(
        policy.access(Quality::Vivid),
        QualityAccess::TrialUnsupported
    );
    let missing = QualityPrivilege::from_official(&json!({"id":7})).unwrap();
    assert_eq!(
        missing.availability(Quality::HiRes),
        QualityAvailability::Unknown
    );
    assert_eq!(
        missing.availability(Quality::Lossless),
        QualityAvailability::Unknown
    );
}
#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<(String, Value, bool)>>,
    response: Mutex<Value>,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload, cache));
            Ok(self.response.lock().unwrap().clone())
        })
    }
}
#[tokio::test]
async fn official_size_and_pc_right_requests_have_exact_payloads_and_reading_never_activates_a_trial()
 {
    let backend = Arc::new(Backend::default());
    let api = QualityApi::new(backend.clone());
    let song = "900719925474099312345";
    *backend.response.lock().unwrap() = json!({"code":200,"data":{"id":song,"l":{"size":128},"m":{"size":192},"h":{"size":320},"sq":{"size":999},"hr":{"size":1999},"db":{"size":2999},"je":{"size":3999},"jm":{"size":4999},"sk":{"size":5999},"vi":{"size":6999}}});
    let sizes = api
        .sizes(&json!(song), QualityFormat::Surround51)
        .await
        .unwrap();
    assert_eq!(sizes.bytes(Quality::Standard), Some(128));
    assert_eq!(sizes.bytes(Quality::Lossless), Some(999));
    assert_eq!(sizes.bytes(Quality::Vivid), Some(6999));
    assert_eq!(sizes.bytes(Quality::Aac64), None);
    assert_eq!(
        backend.calls.lock().unwrap()[0],
        (
            "/api/song/music/detail/get".into(),
            json!({"songId":song,"immerseType":"c51"}),
            false
        )
    );
    for right in QualityRight::ALL {
        *backend.response.lock().unwrap() = json!({"code":200,"data":[{"resourceId":0,"resourceType":right.resource_type(),"canUse":true,"userSrc":0,"trialStatus":1,"leftTime":60000}]});
        let use_right = api.use_right(right).await.unwrap().unwrap();
        assert_eq!(use_right.phase(), TrialPhase::Eligible);
        assert_eq!(
            backend.calls.lock().unwrap().last().unwrap().1,
            json!({"resourceIdList":0,"resourceTypeList":right.resource_type()})
        );
    }
    assert!(
        backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|(p, _, _)| p != "/api/vipauth/app/auth/set")
    );
    *backend.response.lock().unwrap() = json!({"code":200,"data":true});
    api.activate(QualityRight::Sky).await.unwrap();
    assert_eq!(
        backend.calls.lock().unwrap().last().unwrap().1,
        json!({"resourceId":0,"resourceType":32})
    );
}
#[tokio::test]
async fn malformed_or_mismatched_quality_responses_stay_unknown_and_false_ack_is_not_success() {
    let backend = Arc::new(Backend::default());
    let api = QualityApi::new(backend.clone());
    for data in [
        json!([{"canUse":true,"resourceType":33}]),
        json!([{"canUse":true},{"canUse":false}]),
        json!([{"canUse":"true"}]),
    ] {
        *backend.response.lock().unwrap() = json!({"code":200,"data":data});
        assert!(api.use_right(QualityRight::Vivid).await.is_err());
    }
    *backend.response.lock().unwrap() = json!({"code":200,"data":[]});
    assert!(api.use_right(QualityRight::Vivid).await.unwrap().is_none());
    *backend.response.lock().unwrap() = json!({"code":200,"data":[{"canUse":false}]});
    assert_eq!(
        api.use_right(QualityRight::Vivid)
            .await
            .unwrap()
            .unwrap()
            .phase(),
        TrialPhase::Unavailable
    );
    *backend.response.lock().unwrap() = json!({"code":200,"data":[{"canUse":true,"userSrc":0,"trialStatus":2,"leftTime":u64::MAX/4}]});
    assert!(api.use_right(QualityRight::Vivid).await.is_err());
    *backend.response.lock().unwrap() = json!({"code":200,"data":{"id":"8","l":{"size":100}}});
    assert!(api.sizes(&json!(7), QualityFormat::Stereo).await.is_err());
    for data in [Value::Null, json!(false), json!({"success":true})] {
        *backend.response.lock().unwrap() = json!({"code":200,"data":data});
        assert!(matches!(
            api.activate(QualityRight::Vivid).await,
            Err(Error::Response(_))
        ));
    }
}

mod reads {
    use super::*;
    use cumulus_core::{account::AccountSession, storage::StateStore};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;
    #[derive(Default)]
    struct ReaderBackend {
        calls: Mutex<Vec<(String, Value)>>,
        partial_failure: AtomicBool,
        hold: AtomicBool,
        timeout: AtomicBool,
        started: Notify,
        release: Notify,
    }
    impl Api for ReaderBackend {
        fn request<'a>(&'a self, path: &'a str, p: Value, _: bool) -> ApiFuture<'a, Value> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((path.into(), p.clone()));
                Ok(match path {
                    "/api/song/enhance/privilege" => {
                        json!({"code":200,"data":[{"id":7,"flag":(1<<12)|(1<<16),"maxbr":999000,"fl":0,"payed":0,"fee":0}]})
                    }
                    "/api/music-vip-membership/client/vip/info" => {
                        json!({"code":200,"data":{"userId":9,"now":1_000_000,"associator":{"expireTime":2_000_000},"cookie":"SECRET"}})
                    }
                    "/api/song/music/detail/get" if self.partial_failure.load(Ordering::SeqCst) => {
                        json!({"code":500})
                    }
                    "/api/song/music/detail/get" => {
                        json!({"code":200,"data":{"id":7,"sq":{"size":1234},"url":"https://private/?SECRET"}})
                    }
                    "/api/vipauth/app/auth/query" => {
                        if p["resourceTypeList"] == 41 {
                            if self.timeout.load(Ordering::SeqCst) {
                                return std::future::pending().await;
                            }
                            if self.hold.load(Ordering::SeqCst) {
                                self.started.notify_one();
                                self.release.notified().await;
                            }
                        }
                        if self.partial_failure.load(Ordering::SeqCst)
                            && p["resourceTypeList"] == 31
                        {
                            json!({"code":500})
                        } else {
                            json!({"code":200,"data":[{"canUse":true,"userSrc":0,"trialStatus":1,"leftTime":60000,"cookie":"SECRET"}]})
                        }
                    }
                    _ => panic!("unexpected quality read endpoint {path}"),
                })
            })
        }
    }
    fn fixture() -> (
        tempfile::TempDir,
        Arc<ReaderBackend>,
        Arc<AccountSession>,
        QualityReader,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(dir.path()).unwrap());
        let backend = Arc::new(ReaderBackend::default());
        let session = Arc::new(AccountSession::new(backend.clone(), Some(store)));
        session
            .adopt(
                backend.clone(),
                json!({"userId":9}),
                &session.token().unwrap(),
            )
            .unwrap();
        let reader = QualityReader::new(session.clone());
        (dir, backend, session, reader)
    }
    #[tokio::test]
    async fn independent_read_failures_do_not_erase_valid_sizes_privileges_or_account_rights() {
        let (_dir, backend, session, reader) = fixture();
        backend.partial_failure.store(true, Ordering::SeqCst);
        let info = reader
            .read_for(
                &session.token().unwrap(),
                "7",
                Some(35),
                QualityFormat::Surround51,
            )
            .await
            .unwrap();
        assert_eq!(info.status.privilege, QualityReadState::Ready);
        assert_eq!(info.status.subscriber, QualityReadState::Ready);
        assert_eq!(info.status.sizes, QualityReadState::Failed);
        assert_eq!(
            info.status.use_rights[&QualityRight::JyMaster],
            QualityReadState::Failed
        );
        assert!(info.subscriber.unwrap().vip);
        assert!(info.use_rights[&QualityRight::JyMaster].is_none());
        assert_eq!(
            info.use_rights[&QualityRight::Sky]
                .as_ref()
                .unwrap()
                .phase(),
            TrialPhase::Eligible
        );
        let calls = backend.calls.lock().unwrap();
        assert_eq!(calls.len(), 7);
        assert!(
            calls
                .iter()
                .any(|(p, v)| p == "/api/song/enhance/privilege" && v["trialMode"] == 35)
        );
        assert!(!calls.iter().any(|(p, _)| p.ends_with("/auth/set")));
    }
    #[tokio::test(start_paused = true)]
    async fn a_hung_right_read_is_bounded_and_other_read_results_survive() {
        let (_dir, backend, session, reader) = fixture();
        backend.timeout.store(true, Ordering::SeqCst);
        let info = reader
            .read_for(
                &session.token().unwrap(),
                "7",
                None,
                QualityFormat::Surround51,
            )
            .await
            .unwrap();
        assert_eq!(
            info.status.use_rights[&QualityRight::Vivid],
            QualityReadState::Failed
        );
        assert_eq!(info.status.sizes, QualityReadState::Ready);
        assert_eq!(info.sizes.unwrap().bytes(Quality::Lossless), Some(1234));
    }
    #[tokio::test]
    async fn an_account_change_during_reads_cannot_publish_the_old_accounts_entitlements() {
        let (dir, backend, session, reader) = fixture();
        backend.hold.store(true, Ordering::SeqCst);
        let token = session.token().unwrap();
        let pending = tokio::spawn(async move {
            reader
                .read_for(&token, "7", None, QualityFormat::Surround51)
                .await
        });
        backend.started.notified().await;
        session
            .adopt(
                backend.clone(),
                json!({"userId":10}),
                &session.token().unwrap(),
            )
            .unwrap();
        backend.release.notify_waiters();
        assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
        assert!(
            StateStore::open(dir.path())
                .unwrap()
                .get("10", "membership", "current")
                .unwrap()
                .is_none()
        );
    }
}
