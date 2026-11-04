use super::*;
use crate::{
    protocol::api::{Api, ApiFuture},
    song_recognition::{
        request::{Event, RequestContext, Scheduler, SessionUuid},
        seal_compressed_packet,
    },
};
use serde_json::json;
use tokio::{sync::Notify, task::JoinHandle};

const PRIVILEGES: &str = "/api/song/enhance/privilege";
const SONG_DETAIL: &str = "/api/v3/song/detail";
const LARGE: &str = "18446744073709551616";
const SECRET_URL: &str = "https://example.invalid/private-signed-media";
const ACCOUNT: &str = "86753090001";

#[derive(Default)]
struct FixtureApi {
    requests: Mutex<Vec<(String, Value, bool)>>,
    hold: bool,
    reject: bool,
    entered: Notify,
    release: Notify,
}

impl Api for FixtureApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.requests
                .lock()
                .unwrap()
                .push((path.into(), payload, cache));
            match path {
                PRIVILEGES => {
                    if self.hold {
                        self.entered.notify_one();
                        self.release.notified().await;
                    }
                    if self.reject {
                        return Ok(json!({"code":403,"message":SECRET_URL}));
                    }
                    Ok(json!({"code":200,"data":[
                        {"id":42,"pl":0,"flag":1024,"url":SECRET_URL},
                        {"id":"003","pl":320000},
                        {"id":"9999","pl":320000},
                        {"id":"003","pl":0,"flag":0},
                        {"id":LARGE,"pl":96000}
                    ]}))
                }
                SONG_DETAIL => Ok(json!({"code":200,"songs":[
                    {"id":LARGE,"name":"Huge text ID"},
                    {"id":"9999","name":"Unsolicited"},
                    {"id":"003","name":"Exact text ID","ar":[{"name":"Artist","id":6}],
                     "al":{"name":"Album","picUrl":SECRET_URL},"dt":98,"fee":1,
                     "mediaUrl":SECRET_URL,"token":"synthetic-credential",
                     "privilege":{"id":"003","pl":320000}}
                ],"privileges":[{"id":"77","pl":320000}]})),
                _ => panic!("unexpected account request"),
            }
        })
    }
}

struct Fixture {
    api: Arc<FixtureApi>,
    session: Arc<AccountSession>,
    results: Arc<RecognitionResults>,
    scheduler: Scheduler,
    generation: Generation,
}

impl Fixture {
    fn new(hold: bool) -> Self {
        Self::with_api(FixtureApi {
            hold,
            ..Default::default()
        })
    }

    fn with_api(api: FixtureApi) -> Self {
        let api = Arc::new(api);
        let session = Arc::new(AccountSession::new(api.clone(), None));
        adopt(&session, api.clone(), ACCOUNT);
        let results = Arc::new(RecognitionResults::new(session.clone()));
        let mut scheduler = Scheduler::default();
        let generation = start(&mut scheduler);
        results
            .begin(&session.token().unwrap(), generation)
            .unwrap();
        Self {
            api,
            session,
            results,
            scheduler,
            generation,
        }
    }

    fn complete(&mut self) {
        self.results
            .complete(&completion(&mut self.scheduler, self.generation))
            .unwrap();
    }

    fn read(&self) -> JoinHandle<Result<AccountData<Vec<RecognitionTrack>>>> {
        let results = self.results.clone();
        let generation = self.generation;
        tokio::spawn(async move { results.read(generation).await })
    }
}

fn adopt(session: &AccountSession, api: Arc<FixtureApi>, uid: &str) {
    session
        .adopt(api, json!({"userId":uid}), &session.token().unwrap())
        .unwrap();
}

fn start(scheduler: &mut Scheduler) -> Generation {
    scheduler
        .start(
            0,
            SessionUuid::from_host("A6F10324-8B20-46D4-B182-36A22E73211C").unwrap(),
        )
        .unwrap()
}

fn completion(scheduler: &mut Scheduler, generation: Generation) -> Completed {
    assert!(matches!(
        scheduler.push_pcm(generation, 3_000, &[0; 24_000]).unwrap(),
        Event::Ready(24_000)
    ));
    let Event::Ready(job) = scheduler.next_prefix(generation, 3_000).unwrap() else {
        panic!("missing first prefix");
    };
    let sealed = seal_compressed_packet(&[1]).unwrap().unwrap();
    let context =
        RequestContext::new("3.1.42.205594", "synthetic-device", "synthetic-sign").unwrap();
    let receipt = job.receipt();
    assert!(matches!(
        scheduler
            .prepare(receipt, 3_000, &sealed, &context, 0)
            .unwrap(),
        Event::Ready(_)
    ));
    let body = json!({"code":200,"data":{"type":1,"result":[
        {"song":{"id":"003"}},
        {"song":{"id":42}},
        {"song":{"id":"003"}},
        {"song":{"id":LARGE}},
        {"song":{"id":"77"}},
        {"song":{"id":"000"}},
        {"song":{"id":"0x9"}},
        {"song":{"id":9007199254740993_u64}},
        {"song":{"id":"1".repeat(65)}}
    ]}});
    let Event::Completed(completed) = scheduler
        .receive(receipt, 3_000, Some(0), &body.to_string())
        .unwrap()
    else {
        panic!("expected terminal native match");
    };
    completed
}

#[tokio::test(start_paused = true)]
async fn sends_exact_no_trial_privilege_ids_and_projects_only_original_candidates() {
    let mut fixture = Fixture::new(false);
    fixture.complete();
    let read = fixture.read();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_millis(499)).await;
    assert!(fixture.api.requests.lock().unwrap().is_empty());

    let result = read.await.unwrap().unwrap();
    let requests = fixture.api.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests.contains(&(
        PRIVILEGES.into(),
        json!({"ids":format!(r#"["003","42","003","{LARGE}","77"]"#)}),
        false
    )));
    assert!(requests.contains(&(
        SONG_DETAIL.into(),
        json!({"c":format!(r#"[{{"id":"003","v":0}},{{"id":"42","v":0}},{{"id":"{LARGE}","v":0}},{{"id":"77","v":0}}]"#)}),
        false
    )));
    let rows = &result.value;
    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        ["003", "42", "003", LARGE, "77"]
    );
    assert_eq!(
        rows.iter().map(|row| row.privilege).collect::<Vec<_>>(),
        [
            PrivilegeObservation::NoPlaybackIndication,
            PrivilegeObservation::TrialIndicated,
            PrivilegeObservation::NoPlaybackIndication,
            PrivilegeObservation::PlaybackIndicated,
            PrivilegeObservation::Unknown,
        ]
    );
    assert_eq!(rows[0].metadata.as_ref().unwrap().name, "Exact text ID");
    assert!(rows[1].metadata.is_none());
    assert!(rows[4].metadata.is_none());
    assert_eq!(
        result.epoch,
        fixture.session.token().unwrap().generation.to_string()
    );
    let dto = serde_json::to_string(&result).unwrap();
    for forbidden in [
        SECRET_URL,
        "synthetic-credential",
        "9999",
        ACCOUNT,
        "url",
        "token",
    ] {
        assert!(!dto.contains(forbidden));
    }
}

#[tokio::test(start_paused = true)]
async fn cancellation_before_delay_or_during_transport_cannot_publish() {
    let mut fixture = Fixture::new(true);
    fixture.complete();
    let read = fixture.read();
    tokio::task::yield_now().await;
    fixture.results.cancel(fixture.generation).unwrap();
    assert!(matches!(read.await.unwrap(), Err(Error::Cancelled)));
    assert!(fixture.api.requests.lock().unwrap().is_empty());
    assert!(matches!(
        fixture
            .results
            .begin(&fixture.session.token().unwrap(), fixture.generation),
        Err(Error::StaleResponse)
    ));

    fixture.generation = start(&mut fixture.scheduler);
    fixture
        .results
        .begin(&fixture.session.token().unwrap(), fixture.generation)
        .unwrap();
    fixture.complete();
    let read = fixture.read();
    fixture.api.entered.notified().await;
    fixture.results.cancel(fixture.generation).unwrap();
    fixture.api.release.notify_one();
    assert!(matches!(read.await.unwrap(), Err(Error::Cancelled)));
}

#[tokio::test(start_paused = true)]
async fn same_uid_new_epoch_and_different_uid_revoke_in_flight_reads() {
    for next in [ACCOUNT, "86753090002"] {
        let mut fixture = Fixture::new(true);
        fixture.complete();
        let read = fixture.read();
        fixture.api.entered.notified().await;
        fixture.session.detach().unwrap();
        let next_api = Arc::new(FixtureApi::default());
        adopt(&fixture.session, next_api.clone(), next);
        fixture.api.release.notify_one();
        assert!(matches!(read.await.unwrap(), Err(Error::StaleResponse)));
        assert!(next_api.requests.lock().unwrap().is_empty());
        assert!(matches!(
            fixture.results.read(fixture.generation).await,
            Err(Error::StaleResponse)
        ));
    }
}

#[tokio::test(start_paused = true)]
async fn older_completion_and_older_refresh_cannot_replace_current_result() {
    let mut fixture = Fixture::new(true);
    let old = completion(&mut fixture.scheduler, fixture.generation);
    fixture.generation = start(&mut fixture.scheduler);
    let owner = fixture.session.token().unwrap();
    fixture.results.begin(&owner, fixture.generation).unwrap();
    assert!(matches!(
        fixture.results.complete(&old),
        Err(Error::StaleResponse)
    ));
    let mut unrelated = Scheduler::default();
    start(&mut unrelated);
    start(&mut unrelated);
    assert!(matches!(
        fixture.results.begin(&owner, start(&mut unrelated)),
        Err(Error::StaleResponse)
    ));
    fixture.complete();
    let first = fixture.read();
    fixture.api.entered.notified().await;
    let second = fixture.read();
    fixture.api.entered.notified().await;
    assert!(matches!(first.await.unwrap(), Err(Error::StaleResponse)));
    fixture.api.release.notify_one();
    assert_eq!(second.await.unwrap().unwrap().value.len(), 5);
}

#[tokio::test(start_paused = true)]
async fn stalled_catalog_read_has_a_budget_and_an_explicit_next_read_can_retry() {
    let mut fixture = Fixture::new(true);
    fixture.complete();
    let read = fixture.read();
    fixture.api.entered.notified().await;
    assert!(matches!(
        read.await.unwrap(),
        Err(Error::State("recognition catalog read timed out"))
    ));
    let next = fixture.read();
    fixture.api.entered.notified().await;
    fixture.api.release.notify_one();
    assert_eq!(next.await.unwrap().unwrap().value.len(), 5);
}

#[tokio::test(start_paused = true)]
async fn catalog_privilege_denial_does_not_fall_back_to_metadata_grants() {
    let mut fixture = Fixture::with_api(FixtureApi {
        reject: true,
        ..Default::default()
    });
    fixture.complete();
    let Err(error) = fixture.read().await.unwrap() else {
        panic!("metadata must not replace the privilege receipt");
    };
    assert!(error.is_service_code(403));
    assert!(matches!(
        error,
        Error::Service { body, .. } if body.is_null()
    ));
}
