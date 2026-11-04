use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{ProfileReportCommand, ProfileReportReason},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Notify;

const REPORT: &str = "/api/report/reportuser";
const RAW: &str = " \u{3000}合成描述 / 100%\n末行 \t";

#[derive(Default)]
struct Fixture {
    reports: Mutex<Vec<Value>>,
    details: Mutex<Vec<String>>,
    missed_send: AtomicBool,
    parked_detail: Notify,
    resume_detail: Notify,
}

impl Api for Fixture {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            if let Some(id) = path.strip_prefix("/api/w/v1/user/detail/") {
                assert_eq!(body, json!({"all":true,"userId":id}));
                self.details.lock().unwrap().push(id.into());
                if id == "91" {
                    self.parked_detail.notify_one();
                    self.resume_detail.notified().await;
                }
                return Ok(
                    json!({"code":200,"profile":{"userId": if id=="87" {"999"} else {id},
                    "accountStatus":if id=="86" {40}else{0},"blacklist":id=="85","inBlacklist":id=="84"}}),
                );
            }
            assert_eq!(path, REPORT);
            if self.missed_send.swap(false, Ordering::SeqCst) {
                return Err(Error::RequestNotSent(Box::new(Error::State(
                    "fixture failed to build the request",
                ))));
            }
            self.reports.lock().unwrap().push(body.clone());
            Ok(match body["userid"].as_str().unwrap() {
                "76" => json!({"code":"-1"}),
                "77" => {
                    return Err(Error::Service {
                        endpoint: REPORT.into(),
                        code: json!(-2),
                        body: Box::new(json!({"code":-2})),
                    });
                }
                "78" => return Err(Error::Transport("lost report response".into())),
                "79" => json!({"code":403}),
                _ => json!({"code":200,"ignored":"server data"}),
            })
        })
    }
}

#[tokio::test]
async fn a_session_change_during_target_verification_fences_the_report() {
    use ProfileReportCommand::{Inspect, Submit};
    let directory = tempfile::tempdir().unwrap();
    let fixture = Arc::new(Fixture::default());
    let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(fixture.clone(), Some(registry.store())));
    let adopt = |uid| {
        session
            .adopt(
                fixture.clone(),
                json!({"userId":uid}),
                &session.token().unwrap(),
            )
            .unwrap();
        session.token().unwrap().generation.to_string()
    };
    let first_epoch = adopt("2");
    let music = MusicClient::with_session(session.clone(), registry);
    let receipt = prepare(&music, &first_epoch, "91", ProfileReportReason::Political).await;
    let (result, next_epoch) = tokio::join!(
        music.profile_report(&first_epoch, "91", Submit { receipt }),
        async {
            fixture.parked_detail.notified().await;
            // Inspect cannot return an actionable Ready view while another
            // renderer's confirmed preflight is still using this target.
            assert!(matches!(
                music.profile_report(&first_epoch, "91", Inspect).await,
                Err(Error::MutationBusy)
            ));
            let epoch = adopt("3");
            fixture.resume_detail.notify_one();
            epoch
        },
    );
    assert!(matches!(result, Err(Error::StaleResponse)));
    assert_eq!(
        view(&music, &next_epoch, "91", Inspect).await["status"],
        "ready"
    );
    assert!(fixture.reports.lock().unwrap().is_empty());
}

fn client(root: &Path, fixture: &Arc<Fixture>, uid: Option<&str>) -> (MusicClient, String) {
    let registry = ProfileRegistry::open(root, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(fixture.clone(), Some(registry.store())));
    if let Some(uid) = uid {
        session
            .adopt(
                fixture.clone(),
                json!({"userId":uid}),
                &session.token().unwrap(),
            )
            .unwrap();
    }
    let epoch = session.token().unwrap().generation.to_string();
    (MusicClient::with_session(session, registry), epoch)
}

async fn view(music: &MusicClient, epoch: &str, id: &str, command: ProfileReportCommand) -> Value {
    let result = music.profile_report(epoch, id, command).await.unwrap();
    assert_eq!(result.epoch, epoch);
    serde_json::to_value(result.value).unwrap()
}

async fn prepare(
    music: &MusicClient,
    epoch: &str,
    id: &str,
    reason: ProfileReportReason,
) -> String {
    let dto = view(music, epoch, id, ProfileReportCommand::Prepare { reason }).await;
    assert_eq!(dto["status"], "ready");
    assert_eq!(dto.as_object().unwrap().len(), 2);
    dto["receipt"].as_str().unwrap().into()
}

#[tokio::test]
async fn exact_reasons_one_confirmation_no_replay_and_owner_scoped_journal() {
    use ProfileReportCommand::{Cancel, Inspect, Prepare, Submit};
    use ProfileReportReason::*;
    let directory = tempfile::tempdir().unwrap();
    let fixture = Arc::new(Fixture::default());
    let (music, epoch) = client(directory.path(), &fixture, Some("2"));
    let cancelled = prepare(&music, &epoch, "71", Advertising).await;
    assert_eq!(
        view(
            &music,
            &epoch,
            "71",
            Cancel {
                receipt: cancelled.clone()
            }
        )
        .await["status"],
        "ready"
    );
    assert!(
        music
            .profile_report(&epoch, "71", Submit { receipt: cancelled })
            .await
            .is_err()
    );
    assert!(fixture.details.lock().unwrap().is_empty());
    let reasons = [
        ("73", Advertising, "广告或营销信息", "accepted"),
        ("74", PersonalAttack, "恶意人身攻击", "accepted"),
        ("75", HarmfulContent, "色情、低俗或有害内容", "accepted"),
        ("76", Political, "政治敏感言论", "duplicate"),
        ("77", Advertising, "广告或营销信息", "official_account"),
        (
            "78",
            Other {
                description: RAW.into(),
            },
            RAW,
            "uncertain",
        ),
        ("79", PersonalAttack, "恶意人身攻击", "uncertain"),
    ];
    let mut wanted = Vec::new();
    for (target, reason, expected_reason, expected) in reasons {
        let receipt = prepare(&music, &epoch, target, reason).await;
        assert_eq!(
            view(
                &music,
                &epoch,
                target,
                Submit {
                    receipt: receipt.clone()
                }
            )
            .await,
            json!({"status":expected,"receipt":null})
        );
        assert_eq!(
            view(&music, &epoch, target, Submit { receipt }).await["status"],
            expected
        );
        wanted.push(json!({"userid":target,"reason":expected_reason}));
    }
    assert_eq!(*fixture.reports.lock().unwrap(), wanted);
    drop(music);
    let (music, epoch) = client(directory.path(), &fixture, Some("2"));
    assert_eq!(
        view(&music, &epoch, "78", Inspect).await,
        json!({"status":"uncertain","receipt":null})
    );
    assert_eq!(
        view(&music, &epoch, "78", Prepare { reason: Political }).await,
        json!({"status":"uncertain","receipt":null})
    );
    let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
    assert_eq!(
        registry
            .store()
            .get("2", "profile-report-receipt-v1", "78")
            .unwrap(),
        Some(json!("uncertain"))
    );
    let (another, another_epoch) = client(directory.path(), &fixture, Some("3"));
    assert_eq!(
        view(&another, &another_epoch, "78", Inspect).await["status"],
        "ready"
    );
    assert_eq!(*fixture.reports.lock().unwrap(), wanted);
    assert_eq!(
        *fixture.details.lock().unwrap(),
        ["73", "74", "75", "76", "77", "78", "79"]
    );
}

#[tokio::test]
async fn confirms_same_target_bounds_and_only_releases_a_known_unsent_attempt() {
    use ProfileReportCommand::{Inspect, Prepare, Submit};
    use ProfileReportReason::*;
    let directory = tempfile::tempdir().unwrap();
    let fixture = Arc::new(Fixture::default());
    let (music, epoch) = client(directory.path(), &fixture, Some("2"));
    let (guest, guest_epoch) = client(directory.path(), &fixture, None);
    assert!(
        guest
            .profile_report(&guest_epoch, "73", Inspect)
            .await
            .is_err()
    );
    for id in ["2", "073", "bad"] {
        assert!(music.profile_report(&epoch, id, Inspect).await.is_err());
    }
    for text in [" \u{feff}\t\n".to_string(), "音".repeat(1366)] {
        assert!(
            music
                .profile_report(
                    &epoch,
                    "73",
                    Prepare {
                        reason: Other { description: text }
                    }
                )
                .await
                .is_err()
        );
    }
    let receipt = prepare(&music, &epoch, "73", Political).await;
    assert!(
        music
            .profile_report(
                &epoch,
                "74",
                Submit {
                    receipt: receipt.clone()
                }
            )
            .await
            .is_err()
    );
    assert!(fixture.details.lock().unwrap().is_empty());
    fixture.missed_send.store(true, Ordering::SeqCst);
    assert_eq!(
        view(
            &music,
            &epoch,
            "73",
            Submit {
                receipt: receipt.clone()
            }
        )
        .await["status"],
        "not_sent"
    );
    assert!(
        music
            .profile_report(&epoch, "73", Submit { receipt })
            .await
            .is_err()
    );
    assert_eq!(view(&music, &epoch, "73", Inspect).await["status"], "ready");
    for id in ["84", "85", "86", "87"] {
        let receipt = prepare(&music, &epoch, id, PersonalAttack).await;
        if id == "87" {
            assert!(
                music
                    .profile_report(&epoch, id, Submit { receipt })
                    .await
                    .is_err()
            );
        } else {
            assert_eq!(
                view(&music, &epoch, id, Submit { receipt }).await["status"],
                "target_unavailable"
            );
        }
    }
    assert!(fixture.reports.lock().unwrap().is_empty());
    let receipt = prepare(&music, &epoch, "73", Political).await;
    assert_eq!(
        view(&music, &epoch, "73", Submit { receipt }).await["status"],
        "accepted"
    );
    assert_eq!(
        *fixture.reports.lock().unwrap(),
        [json!({"userid":"73","reason":"政治敏感言论"})]
    );
}
