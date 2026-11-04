use super::*;
use crate::protocol::wire::eapi;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

const UUID: &str = "A6F10324-8B20-46D4-B182-36A22E73211C";
const MISS: &str = r#"{"code":200,"data":{"type":0,"result":[]}}"#;
const HIT: &str =
    r#"{"code":200,"data":{"type":1,"result":[{"song":{"id":12}},{"song":{"id":"13"}}]}}"#;

fn start() -> (Scheduler, Generation) {
    let mut scheduler = Scheduler::default();
    let generation = scheduler
        .start(0, SessionUuid::from_host(UUID).unwrap())
        .unwrap();
    (scheduler, generation)
}
fn ready<T>(event: Event<T>) -> T {
    match event {
        Event::Ready(value) => value,
        _ => panic!("expected one ready result"),
    }
}
fn completed<T>(event: Event<T>) -> Completed {
    match event {
        Event::Completed(value) => value,
        _ => panic!("expected terminal result"),
    }
}

fn request_fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/song-recognition-request.json"
    ))
    .unwrap()
}
fn fixture_fingerprint() -> SealedFingerprint {
    let fixture = request_fixture();
    let body: Value = serde_json::from_str(fixture["compactJson"].as_str().unwrap()).unwrap();
    SealedFingerprint(STANDARD.decode(body["rawdata"].as_str().unwrap()).unwrap())
}

/// In-memory receipt queue. No socket, URL resolver or HTTP implementation.
#[derive(Default)]
struct FixtureTransport(Vec<FixtureRequest>);
impl FixtureTransport {
    fn dispatch(
        &mut self,
        scheduler: &mut Scheduler,
        receipt: Receipt,
        at: u64,
        tick_count: u32,
    ) -> &FixtureRequest {
        let context =
            RequestContext::new("3.1.42.205594", "synthetic-device", "synthetic-sign").unwrap();
        let request = ready(
            scheduler
                .prepare(receipt, at, &fixture_fingerprint(), &context, tick_count)
                .unwrap(),
        );
        self.0.push(request);
        self.0.last().unwrap()
    }

    fn deliver(
        &self,
        scheduler: &mut Scheduler,
        index: usize,
        at: u64,
        code: Option<i32>,
        body: &str,
    ) -> Event<()> {
        scheduler
            .receive(self.0[index].receipt(), at, code, body)
            .unwrap()
    }
}

fn all_prefixes() -> (Scheduler, Generation, FixtureTransport) {
    let (mut scheduler, generation) = start();
    let mut transport = FixtureTransport::default();
    let pcm = vec![0; MAX_PCM];
    assert_eq!(
        ready(scheduler.push_pcm(generation, 15_000, &pcm).unwrap()),
        120_000
    );
    for _ in 0..MAX_ATTEMPTS {
        let job = ready(scheduler.next_prefix(generation, 15_000).unwrap());
        transport.dispatch(&mut scheduler, job.receipt(), 15_000, 0);
    }
    (scheduler, generation, transport)
}

#[test]
fn exact_thresholds_use_prefixes_and_never_dispatch_twice() {
    let (mut scheduler, generation) = start();
    let mut transport = FixtureTransport::default();
    assert!(matches!(
        scheduler.next_prefix(generation, 2_999).unwrap(),
        Event::Waiting
    ));
    let pcm: Vec<i16> = (0..120_001).map(|n| (n % 30_001) as i16).collect();
    for times in 1_u8..=5 {
        let at = u64::from(times) * 3_000;
        let to = usize::from(times) * 24_000;
        let from = to - 24_000;
        assert_eq!(
            ready(
                scheduler
                    .push_pcm(generation, at - 1, &pcm[from..to - 1])
                    .unwrap()
            ),
            23_999
        );
        assert!(matches!(
            scheduler.next_prefix(generation, at - 1).unwrap(),
            Event::Waiting
        ));
        assert_eq!(
            ready(
                scheduler
                    .push_pcm(generation, at, &pcm[to - 1..to])
                    .unwrap()
            ),
            1
        );
        let job = ready(scheduler.next_prefix(generation, at).unwrap());
        assert_eq!(job.samples(), &pcm[..to]);
        assert_eq!(job.receipt().times(), times);
        assert_eq!(job.receipt().duration_seconds(), 3 * times);

        let tick = u32::MAX.wrapping_add(u32::from(times - 1));
        let request = transport.dispatch(&mut scheduler, job.receipt(), at, tick);
        let (path, value) =
            eapi::unwrap(request.encrypted_form().strip_prefix("params=").unwrap()).unwrap();
        assert_eq!(path, "/api/music/audio/match");
        assert_eq!(value["times"], times);
        assert_eq!(value["duration"], times * 3);
        assert_eq!(value["sessionId"], UUID);
        assert_eq!(value["header"]["requestId"], tick.to_string());
        let context = RequestContext::new("3.1.42.205594", "", "").unwrap();
        assert!(matches!(
            scheduler
                .prepare(job.receipt(), at, &fixture_fingerprint(), &context, 0)
                .unwrap(),
            Event::Ignored
        ));
        assert!(matches!(
            scheduler.next_prefix(generation, at).unwrap(),
            Event::Waiting
        ));
    }
    assert_eq!(
        ready(
            scheduler
                .push_pcm(generation, 15_000, &pcm[120_000..])
                .unwrap()
        ),
        0
    );
    assert_eq!(transport.0.len(), 5);
}

#[test]
fn exact_synthetic_json_and_independent_eapi_cipher() {
    let (mut scheduler, generation) = start();
    let mut transport = FixtureTransport::default();
    let pcm = vec![0; 24_000];
    ready(scheduler.push_pcm(generation, 3_000, &pcm).unwrap());
    let job = ready(scheduler.next_prefix(generation, 3_000).unwrap());
    let request = transport.dispatch(&mut scheduler, job.receipt(), 3_000, u32::MAX);
    let fixture = request_fixture();
    assert_eq!(request.encrypted_form(), fixture["encryptedForm"]);
    let cipher = hex::decode(request.encrypted_form().strip_prefix("params=").unwrap()).unwrap();
    let plain = crate::protocol::wire::crypto::decrypt_ecb(&cipher, eapi::EAPI_KEY).unwrap();
    let signed = std::str::from_utf8(&plain).unwrap();
    let (_, rest) = signed.split_once("-36cd479b6b5-").unwrap();
    assert_eq!(
        rest.rsplit_once("-36cd479b6b5-").unwrap().0,
        fixture["compactJson"]
    );
    assert_eq!(
        DEFAULT_POST_URL,
        "https://music.163.com/eapi/music/audio/match"
    );
    assert_eq!(
        [USER_AGENT, REFERER, OS],
        ["Windows_Curl;", "music.163.com", "pc"]
    );
    assert!(SessionUuid::from_host("{A6F10324-8B20-46D4-B182-36A22E73211C}").is_err());
    assert!(SessionUuid::from_host("00000000-0000-0000-0000-000000000000").is_err());
}

#[test]
fn native_hit_wins_even_when_newer_jobs_are_awaiting_response() {
    let (mut scheduler, generation, transport) = all_prefixes();
    assert!(matches!(
        transport.deliver(&mut scheduler, 3, 15_001, Some(200), MISS),
        Event::Waiting
    ));
    assert!(matches!(
        transport.deliver(&mut scheduler, 3, 15_002, Some(200), HIT),
        Event::Ignored
    ));
    let result = completed(transport.deliver(&mut scheduler, 1, 15_003, Some(0), HIT));
    assert_eq!(
        result,
        Completed {
            code: Some(0),
            times: 2,
            status: ResultStatus::Matched {
                song_ids: vec![12, 13]
            }
        }
    );
    assert!(matches!(
        transport.deliver(&mut scheduler, 4, 15_004, Some(403), MISS),
        Event::Ignored
    ));
    assert!(matches!(
        scheduler.next_prefix(generation, 15_004).unwrap(),
        Event::Ignored
    ));
}

#[test]
fn body_gate_is_distinct_from_bridge_status_and_fifth_miss_is_terminal() {
    let (mut scheduler, _, transport) = all_prefixes();
    assert!(matches!(
        transport.deliver(
            &mut scheduler,
            0,
            15_001,
            Some(200),
            r#"{"code":500,"data":{"type":1,"result":[{"song":{"id":12}}]}}"#
        ),
        Event::Waiting
    ));
    assert!(matches!(
        transport.deliver(
            &mut scheduler,
            1,
            15_002,
            Some(200),
            r#"{"code":200,"data":{"type":"1"}}"#
        ),
        Event::Waiting
    ));
    let result = completed(transport.deliver(&mut scheduler, 4, 15_003, Some(200), MISS));
    assert_eq!(result.status, ResultStatus::NoTrack);
    assert!(matches!(
        transport.deliver(&mut scheduler, 2, 15_004, Some(200), HIT),
        Event::Ignored
    ));

    for (bridge, status) in [
        (Some(403), ResultStatus::Offline),
        (Some(-10), ResultStatus::Timeout),
        (None, ResultStatus::Unknown),
    ] {
        let (mut scheduler, _, transport) = all_prefixes();
        assert_eq!(
            completed(transport.deliver(&mut scheduler, 0, 15_001, bridge, HIT)).status,
            status
        );
    }
    let (mut scheduler, _, transport) = all_prefixes();
    assert_eq!(completed(transport.deliver(&mut scheduler, 0, 15_001, Some(200), r#"{"code":200,"data":{"type":1,"result":[{"song":{"id":-1}},{"song":{"id":9007199254740993}},{}]}}"#)).status,
        ResultStatus::NoTrack);
}

#[test]
fn cancellation_restart_and_deadline_revoke_in_flight_jobs() {
    let (mut scheduler, old, transport) = all_prefixes();
    let current = scheduler
        .start(15_001, SessionUuid::from_host(UUID).unwrap())
        .unwrap();
    assert!(matches!(scheduler.cancel(old), Event::Ignored));
    assert!(matches!(
        scheduler.push_pcm(old, 99_999, &[9]).unwrap(),
        Event::Ignored
    ));
    assert!(matches!(
        transport.deliver(&mut scheduler, 0, 99_999, Some(200), HIT),
        Event::Ignored
    ));
    assert!(scheduler.tick(current, 15_000).is_err());
    assert!(matches!(
        scheduler.tick(current, 31_001).unwrap(),
        Event::Waiting
    ));
    assert_eq!(
        completed(scheduler.tick(current, 31_002).unwrap()),
        Completed {
            code: Some(-5),
            times: 0,
            status: ResultStatus::Timeout
        }
    );
    assert!(matches!(
        scheduler.tick(current, 31_003).unwrap(),
        Event::Ignored
    ));

    let (mut scheduler, generation, transport) = all_prefixes();
    assert_eq!(
        completed(scheduler.cancel(generation)).status,
        ResultStatus::Cancelled
    );
    assert!(matches!(
        transport.deliver(&mut scheduler, 0, 15_001, Some(200), HIT),
        Event::Ignored
    ));

    let (mut scheduler, _, transport) = all_prefixes();
    assert_eq!(
        completed(transport.deliver(&mut scheduler, 0, 16_001, Some(200), HIT)).status,
        ResultStatus::Timeout
    );
}

#[test]
fn encoder_failure_cannot_deliver_a_server_result_or_be_replayed() {
    let (mut scheduler, generation) = start();
    ready(
        scheduler
            .push_pcm(generation, 15_000, &vec![0; MAX_PCM])
            .unwrap(),
    );
    for times in 1..=5 {
        let job = ready(scheduler.next_prefix(generation, 15_000).unwrap());
        let receipt = job.receipt();
        assert!(matches!(
            scheduler.receive(receipt, 15_000, Some(200), HIT).unwrap(),
            Event::Ignored
        ));
        let outcome = scheduler.empty_fingerprint(receipt, 15_000).unwrap();
        if times < 5 {
            assert!(matches!(outcome, Event::Waiting));
        } else {
            assert_eq!(
                completed(outcome),
                Completed {
                    code: Some(-6),
                    times: 5,
                    status: ResultStatus::Timeout
                }
            );
        }
        assert!(matches!(
            scheduler.empty_fingerprint(receipt, 15_000).unwrap(),
            Event::Ignored
        ));
    }
}
