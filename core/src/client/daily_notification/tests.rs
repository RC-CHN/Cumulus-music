//! Sealed native timing and arbitration. No renderer, network, or real profile.
use super::{policy::Trigger, *};
use crate::{
    account::{AccountSession, profiles::ProfileRegistry},
    protocol::{
        api::{
            ApiFuture, CopyrightNotificationRead, DailyNotificationRead,
            VinylPlaybackReadPermission,
        },
        http::ApiOrigin,
        mconfig::{APPLICATION_KEY, ConfigBackend, ConfigUrl},
        pc_experiments::PcExperimentGroups,
    },
};
use chrono::{Local, TimeZone};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::SeqCst},
};

const CONFIG: &str = "https://d5.music.126.net/obj/fixture/recurring-notification.data";
const POPUP: &str = CopyrightNotificationRead::POPUP;
const DETAILS: &str = CopyrightNotificationRead::DETAILS;
const SETTINGS: &str = DailyNotificationRead::SETTINGS;
const RECOMMEND: &str = "/api/v3/discovery/recommend/songs";

struct NativeConfig;

impl ConfigBackend for NativeConfig {
    fn get<'a>(&'a self, url: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        assert_eq!(url.as_str(), CONFIG);
        Box::pin(async {
            Ok(json!({"preload#push":{
                "t1":0.01,"day":3,"count":1,"firstTime":{"t1":9},
                "durationLimit":{"t1":30},"secondPushTime":17
            }}))
        })
    }
}

#[derive(Clone, Copy)]
enum Popup {
    EmptyIds,
    EmptyDetails,
    BusinessError,
    Rows,
}

struct NativeApi {
    uid: AtomicU64,
    credential: AtomicU64,
    calls: Mutex<Vec<&'static str>>,
    popup: Popup,
    wall: Arc<AtomicI64>,
    jump_after_popup_ms: AtomicI64,
    revoke_before_daily_dispatch: AtomicBool,
}

impl NativeApi {
    fn drain(&self) -> Vec<&'static str> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }
}

impl Api for NativeApi {
    fn request<'a>(&'a self, path: &'a str, args: Value, cached: bool) -> ApiFuture<'a, Value> {
        assert_eq!(path, "/api/w/nuser/account/get");
        assert_eq!(args, json!({}));
        assert!(!cached);
        Box::pin(async move {
            Ok(
                json!({"code":200,"profile":{"userId":self.uid.load(SeqCst),"nickname":"Synthetic"}}),
            )
        })
    }

    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        Ok(Some(self.credential.load(SeqCst).to_string()))
    }

    fn verified_pc_experiment_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, PcExperimentGroups> {
        assert_eq!(
            self.verified_pc_experiment_revision().unwrap().as_deref(),
            Some(revision)
        );
        self.calls.lock().unwrap().push("assignment");
        Box::pin(async {
            PcExperimentGroups::refreshed(&json!({"code":200,"data":[
                {"expName":"PH-PC-push","expGroupName":"t1"},
                {"expName":"PH-PC-push2","expGroupName":"t1"},
                {"expName":"PH-PC-push-firstTime","expGroupName":"t1"},
                {"expName":"PH-PC-push-durationLimit","expGroupName":"t1"},
                {"expName":"PH-PC-Pop-Visual","expGroupName":"c"},
                {"expName":"PH-PC-Blacklist","expGroupName":"t1"}
            ]}))
        })
    }

    fn request_copyright_notification<'a>(
        &'a self,
        read: CopyrightNotificationRead,
        revision: &'a str,
        permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            permission()?;
            assert_eq!(
                self.verified_pc_experiment_revision()?.as_deref(),
                Some(revision)
            );
            let (path, args) = read.into_request();
            self.calls.lock().unwrap().push(path);
            match path {
                POPUP => {
                    assert_eq!(args, json!({}));
                    let jump = self.jump_after_popup_ms.swap(0, SeqCst);
                    if jump != 0 {
                        self.wall.store(jump, SeqCst);
                    }
                    Ok(match self.popup {
                        Popup::EmptyIds => json!({"code":200,"data":{"songIdList":[]}}),
                        Popup::BusinessError => json!({"code":500}),
                        _ => json!({"code":200,"data":{"songIdList":[81],"songNum":1}}),
                    })
                }
                DETAILS => {
                    assert_eq!(args, json!({"c":r#"[{"id":"81","v":0}]"#,"trialMode":-1}));
                    Ok(match self.popup {
                        Popup::Rows => {
                            json!({"code":200,"songs":[song(81)],"privileges":[{"id":81,"pl":320000}]})
                        }
                        Popup::EmptyDetails => json!({"code":200,"songs":[]}),
                        _ => panic!("empty popup or failed read cannot send details"),
                    })
                }
                _ => panic!("unexpected copyright read"),
            }
        })
    }

    fn request_daily_notification<'a>(
        &'a self,
        read: DailyNotificationRead,
        revision: &'a str,
        permission: &'a VinylPlaybackReadPermission<'a>,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if self.revoke_before_daily_dispatch.swap(false, SeqCst) {
                self.credential.fetch_add(1, SeqCst);
            }
            permission()?;
            assert_eq!(
                self.verified_pc_experiment_revision()?.as_deref(),
                Some(revision)
            );
            let (path, args) = read.into_request();
            self.calls.lock().unwrap().push(path);
            match path {
                SETTINGS => {
                    assert_eq!(args, json!({}));
                    Ok(json!({"code":200,"setting":{"personalServiceSetting":0}}))
                }
                RECOMMEND => {
                    assert_eq!(args, json!({"limit":30}));
                    Ok(json!({"code":200,"data":{"dailySongs":[song(91)]}}))
                }
                _ => panic!("unexpected daily read"),
            }
        })
    }
}

fn song(id: u64) -> Value {
    json!({"id":id,"name":"Synthetic song","al":{"id":7,"name":"Album"},"fee":0,"dt":235000})
}

fn at(day: u32, h: u32, m: u32, s: u32, ms: u32) -> i64 {
    Local
        .with_ymd_and_hms(2026, 2, day, h, m, s)
        .unwrap()
        .timestamp_millis()
        + i64::from(ms)
}

struct Fixture {
    _root: tempfile::TempDir,
    client: MusicClient,
    api: Arc<NativeApi>,
    wall: Arc<AtomicI64>,
    schedule: DailyNotificationSchedule,
}

impl Fixture {
    async fn new(popup: Popup) -> Self {
        let root = tempfile::tempdir().unwrap();
        let wall = Arc::new(AtomicI64::new(at(2, 9, 0, 0, 0)));
        let api = Arc::new(NativeApi {
            uid: AtomicU64::new(2),
            credential: AtomicU64::new(1),
            calls: Mutex::default(),
            popup,
            wall: wall.clone(),
            jump_after_popup_ms: AtomicI64::new(0),
            revoke_before_daily_dispatch: AtomicBool::new(false),
        });
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session.refresh().await.unwrap();
        let config = PcConfig::new(registry.store(), Arc::new(NativeConfig)).unwrap();
        config
            .resolve(&BTreeMap::from([(
                "MConfig-Info".into(),
                json!({APPLICATION_KEY:{"version":1,"os":"pc","fullUrl":CONFIG}}).to_string(),
            )]))
            .await
            .unwrap();
        let mut client = MusicClient::with_session(session, registry)
            .with_native_daily_notification_config(config);
        let clock = wall.clone();
        client.daily_notification.clock = Some(Arc::new(move || clock.load(SeqCst)));
        let owner = client.session.token().unwrap();
        client
            .verified_pc_experiment_groups(&owner.generation.to_string())
            .await
            .unwrap();
        assert_eq!(api.drain(), ["assignment"]);
        let schedule = client.daily_notification_startup_plan().unwrap().unwrap();
        let fixture = Self {
            _root: root,
            client,
            api,
            wall,
            schedule,
        };
        assert!(fixture.offer(Trigger::Startup).await.unwrap().is_none());
        assert!(fixture.offer(Trigger::HalfHour).await.unwrap().is_none());
        assert!(fixture.api.drain().is_empty());
        tokio::time::advance(fixture.schedule.delay()).await;
        fixture
    }

    async fn offer(&self, trigger: Trigger) -> Result<Option<DailyNotificationView>> {
        match trigger {
            Trigger::Startup => {
                self.client
                    .prepare_daily_notification(&self.schedule, || true)
                    .await
            }
            Trigger::HalfHour => {
                self.client
                    .prepare_recurring_daily_notification(&self.schedule, || true)
                    .await
            }
        }
    }

    async fn command(
        &self,
        offer: &DailyNotificationView,
        action: DailyNotificationCommand,
    ) -> Result<DailyNotificationOutcome> {
        self.client
            .daily_notification_command(&offer.epoch, &offer.receipt, action, || true)
            .await
    }

    fn history(&self, kind: DailyNotificationKind) -> History {
        history(
            self.client
                .registry
                .store()
                .get(OWNER, KIND, kind.key())
                .unwrap(),
        )
        .unwrap()
    }
}

#[tokio::test(start_paused = true)]
async fn startup_and_periodic_only_diverge_when_the_successful_popup_has_no_ids() {
    for trigger in [Trigger::Startup, Trigger::HalfHour] {
        for popup in [Popup::EmptyIds, Popup::BusinessError, Popup::EmptyDetails] {
            let fixture = Fixture::new(popup).await;
            let result = fixture.offer(trigger).await.unwrap();
            let mut expected = vec![POPUP];
            if matches!(popup, Popup::EmptyDetails) {
                expected.push(DETAILS);
            }
            if trigger == Trigger::HalfHour && matches!(popup, Popup::EmptyIds) {
                assert!(result.is_none());
            } else {
                assert!(result.is_some_and(|offer| offer.kind == DailyNotificationKind::Daily));
                expected.extend([SETTINGS, RECOMMEND]);
            }
            assert_eq!(fixture.api.drain(), expected);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn held_copyright_keeps_its_minute_but_daily_fallthrough_resamples() {
    for popup in [Popup::Rows, Popup::BusinessError] {
        let fixture = Fixture::new(popup).await;
        fixture.wall.store(at(2, 9, 0, 59, 999), SeqCst);
        fixture
            .api
            .jump_after_popup_ms
            .store(at(2, 9, 1, 0, 1), SeqCst);
        let result = fixture.offer(Trigger::HalfHour).await.unwrap();
        if matches!(popup, Popup::Rows) {
            assert!(result.is_some_and(|offer| offer.kind == DailyNotificationKind::Copyright));
            assert_eq!(fixture.api.drain(), [POPUP, DETAILS]);
        } else {
            assert!(
                result.is_none(),
                "Daily must not inherit the old candidate's minute"
            );
            assert_eq!(fixture.api.drain(), [POPUP]);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn admitted_minute_survives_ack_but_daily_counter_and_foreground_stay_live() {
    let fixture = Fixture::new(Popup::Rows).await;
    let wall = |h, m, s, ms| fixture.wall.store(at(2, h, m, s, ms), SeqCst);
    wall(8, 59, 59, 999);
    assert!(fixture.offer(Trigger::HalfHour).await.unwrap().is_none());
    wall(9, 0, 59, 999);
    assert!(
        fixture
            .client
            .prepare_recurring_daily_notification(&fixture.schedule, || false)
            .await
            .unwrap()
            .is_none()
    );
    assert!(fixture.api.drain().is_empty());
    let copyright = fixture.offer(Trigger::HalfHour).await.unwrap().unwrap();
    assert!(copyright.kind == DailyNotificationKind::Copyright);
    assert_eq!(fixture.api.drain(), [POPUP, DETAILS]);

    wall(9, 1, 0, 10); // It was offered in minute zero; painting finishes in minute one.
    for _ in 0..2 {
        assert!(matches!(
            fixture
                .command(&copyright, DailyNotificationCommand::Shown)
                .await
                .unwrap(),
            DailyNotificationOutcome::Shown
        ));
    }
    assert_eq!(
        fixture.history(DailyNotificationKind::Copyright).shown,
        [at(2, 9, 1, 0, 10)]
    );
    fixture
        .command(&copyright, DailyNotificationCommand::Close)
        .await
        .unwrap();
    wall(16, 59, 59, 999);
    assert!(fixture.offer(Trigger::HalfHour).await.unwrap().is_none());
    assert!(fixture.api.drain().is_empty());

    wall(17, 0, 0, 0);
    let daily = fixture.offer(Trigger::HalfHour).await.unwrap().unwrap();
    assert!(daily.kind == DailyNotificationKind::Daily);
    assert_eq!(fixture.api.drain(), [SETTINGS, RECOMMEND]);
    fixture
        .command(&daily, DailyNotificationCommand::Shown)
        .await
        .unwrap();
    fixture
        .command(&daily, DailyNotificationCommand::Close)
        .await
        .unwrap();
    wall(18, 0, 0, 0);
    assert!(fixture.offer(Trigger::HalfHour).await.unwrap().is_none());
    assert_eq!(
        fixture.history(DailyNotificationKind::Daily).shown,
        [at(2, 17, 0, 0, 0)]
    );
    assert!(fixture.api.drain().is_empty());
}

#[tokio::test(start_paused = true)]
async fn expired_unshown_offer_does_not_block_the_next_tick_and_never_counts() {
    let fixture = Fixture::new(Popup::Rows).await;
    let first = fixture.offer(Trigger::HalfHour).await.unwrap().unwrap();
    assert_eq!(fixture.api.drain(), [POPUP, DETAILS]);
    assert!(fixture.offer(Trigger::HalfHour).await.unwrap().is_none());
    assert!(fixture.api.drain().is_empty());
    tokio::time::advance(PRESENTATION_LIMIT).await;
    let replacement = fixture.offer(Trigger::HalfHour).await.unwrap().unwrap();
    assert_ne!(first.receipt, replacement.receipt);
    assert!(matches!(
        fixture
            .command(&first, DailyNotificationCommand::Shown)
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(
        fixture
            .history(DailyNotificationKind::Copyright)
            .shown
            .is_empty()
    );
    assert_eq!(fixture.api.drain(), [POPUP, DETAILS]);
}

#[tokio::test(start_paused = true)]
async fn lost_credential_or_account_cannot_send_or_ack_and_fresh_same_account_can_resume_ticks() {
    let fixture = Fixture::new(Popup::Rows).await;
    let first = fixture.offer(Trigger::HalfHour).await.unwrap().unwrap();
    assert_eq!(fixture.api.drain(), [POPUP, DETAILS]);
    fixture.api.credential.fetch_add(1, SeqCst);
    assert!(fixture.offer(Trigger::HalfHour).await.unwrap().is_none());
    assert!(matches!(
        fixture
            .command(&first, DailyNotificationCommand::Shown)
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(
        fixture
            .history(DailyNotificationKind::Copyright)
            .shown
            .is_empty()
    );
    assert!(fixture.api.drain().is_empty());
    fixture
        .client
        .verified_pc_experiment_groups(&first.epoch)
        .await
        .unwrap();
    let second = fixture.offer(Trigger::HalfHour).await.unwrap().unwrap();
    assert_ne!(first.receipt, second.receipt);
    assert_eq!(fixture.api.drain(), ["assignment", POPUP, DETAILS]);

    fixture.api.uid.store(3, SeqCst);
    fixture.client.session.refresh().await.unwrap();
    let current = fixture.client.session.token().unwrap();
    assert_ne!(current, fixture.schedule.owner);
    fixture
        .client
        .verified_pc_experiment_groups(&current.generation.to_string())
        .await
        .unwrap();
    assert!(matches!(
        fixture.offer(Trigger::HalfHour).await,
        Err(Error::StaleResponse)
    ));
    assert!(
        fixture
            .client
            .current_daily_notification(&current.generation.to_string())
            .unwrap()
            .is_none()
    );
    assert!(
        fixture
            .history(DailyNotificationKind::Copyright)
            .shown
            .is_empty()
    );
    assert_eq!(fixture.api.drain(), ["assignment"]);
}

#[tokio::test(start_paused = true)]
async fn daily_late_dispatch_guard_blocks_rotated_scope_and_does_not_retry() {
    let fixture = Fixture::new(Popup::BusinessError).await;
    fixture.api.revoke_before_daily_dispatch.store(true, SeqCst);
    assert!(matches!(
        fixture.offer(Trigger::HalfHour).await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(fixture.api.drain(), [POPUP]);
    assert!(
        fixture
            .history(DailyNotificationKind::Daily)
            .shown
            .is_empty()
    );
}
