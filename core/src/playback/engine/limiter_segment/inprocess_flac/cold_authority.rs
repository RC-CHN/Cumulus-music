//! Real profile, ledger and opened-file handoff; HTTP and data are synthetic.
use super::{Api, Decoder, FLAC_BLOCK, Generation, RATE, expect_packet, sdk_path, source_pair};
use crate::{
    Result,
    account::{
        AccountSession, SessionStatus,
        profiles::{ColdOfflineGrant, ProfileRegistry},
    },
    download::{
        file_identity::identity,
        offline::OfflineRecord,
        service::{DownloadOptions, DownloadTask},
    },
    media::{
        integrity::inspect_file,
        ncm::{NcmWriteOptions, write_stream},
    },
    membership::{
        Quality,
        clock::{ClockReading, MembershipClock},
    },
    playback::engine::Cancellation,
    protocol::{
        api::ApiFuture,
        http::{
            ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply, RequestPlan, WireRoute,
        },
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

const FLAC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/tag_audio/seed.flac"
));

mod fallback;
mod production;
mod sink;
mod worker;

#[derive(Default)]
struct SyntheticService(AtomicUsize);
impl HttpBackend for SyntheticService {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            assert_eq!(plan.url.path(), "/eapi/w/nuser/account/get");
            self.0.fetch_add(1, Ordering::AcqRel);
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(
                    &json!({"code":200,"profile":{"userId":1,"nickname":"Cold FLAC fixture"}}),
                )
                .unwrap(),
                body_status: BodyStatus::Complete,
                set_cookies: vec![],
                refresh_token: None,
                metadata: Default::default(),
            })
        })
    }
}

#[derive(Default)]
struct Clock(AtomicU64);
impl MembershipClock for Clock {
    fn now(&self) -> Result<ClockReading> {
        let seconds = self.0.load(Ordering::Acquire) as f64;
        Ok(ClockReading {
            wall_seconds: 1000.0 + seconds,
            monotonic_seconds: seconds,
        })
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    registry: Arc<ProfileRegistry>,
    cold: Arc<AccountSession>,
    grant: Arc<ColdOfflineGrant>,
    clock: Arc<Clock>,
    service: Arc<SyntheticService>,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
        let service = Arc::new(SyntheticService::default());
        let candidate = registry
            .candidate_with_backend(
                ClientProfile::default(),
                Default::default(),
                None,
                service.clone(),
            )
            .unwrap();
        let url = ApiOrigin::Pc
            .request_url(WireRoute::Eapi, "/api/w/nuser/account/get")
            .unwrap();
        let credentials = candidate.credentials();
        let revision = credentials.request_credentials(&url).unwrap().revision;
        credentials
            .record_response(
                &url,
                &["MUSIC_U=synthetic-cold-flac; Path=/; Secure".into()],
                None,
                &revision,
            )
            .unwrap();
        let writer = AccountSession::new(candidate.api(), Some(registry.store()));
        candidate
            .verify_and_adopt(&writer, &writer.token().unwrap())
            .await
            .unwrap();
        let cold = Arc::new(AccountSession::new(candidate.api(), Some(registry.store())));
        assert_eq!(cold.snapshot().unwrap().status, SessionStatus::Unknown);
        let clock = Arc::new(Clock::default());
        let grant = Arc::new(
            registry
                .grant_cold_offline(cold.clone())
                .unwrap()
                .with_membership_clock_for_test(clock.clone()),
        );
        Self {
            dir,
            registry,
            cold,
            grant,
            clock,
            service,
        }
    }

    fn membership(&self) -> Value {
        let value = json!({
            "data":{"userId":1,"now":100000,"associator":{"expireTime":200000},"redplus":{"expireTime":200000}},
            "server_ms":100000,"wall":1000
        });
        self.registry
            .store()
            .put("1", "membership", "current", &value)
            .unwrap();
        value
    }

    fn publish(&self, id: &str, audio: &[u8], encrypted: bool) -> PathBuf {
        let media = self.dir.path().join("media");
        let work = self.dir.path().join("work");
        fs::create_dir_all(&media).unwrap();
        fs::create_dir_all(&work).unwrap();
        let file = media.join(format!("{id}.{}", if encrypted { "ncm" } else { "flac" }));
        if encrypted {
            write_stream(
                &mut Cursor::new(audio),
                &mut fs::File::create(&file).unwrap(),
                NcmWriteOptions {
                    metadata: Some(&json!({"musicId":999,"level":"standard","fee":0})),
                    cover: &[],
                    audio_key: Some(b"synthetic cold flac envelope"),
                },
            )
            .unwrap();
        } else {
            fs::write(&file, audio).unwrap();
        }
        let file = file.canonicalize().unwrap();
        let integrity = inspect_file(&file).unwrap();
        let task: DownloadTask = serde_json::from_value(json!({
            "id":id,"song_id":"123","origin":{"kind":"song"},"options":DownloadOptions::default(),
            "destination":{"kind":"path","path":file},"target":file,"work":work,
            "status":"complete","phase":"complete","created_ms":1,"completed_ms":2,
            "bytes":integrity.bytes,"authorization_count":1,"encrypted":encrypted,"official_encryption":false,
            "output_integrity":integrity,"cleanup_pending":false,"lyrics_status":"missing",
            "root":media,"root_identity":identity(&media).unwrap(),"work_identity":identity(&work).unwrap()
        })).unwrap();
        let key = hex::encode(Sha256::digest(file.to_str().unwrap().as_bytes()));
        let ledger = OfflineRecord {
            id: key.clone(),
            revision: "synthetic-published".into(),
            path: file.clone(),
            task_id: id.into(),
            song_id: "123".into(),
            quality: if encrypted {
                Quality::Sky
            } else {
                Quality::Lossless
            },
            privilege: if encrypted {
                json!({"fee":1,"payed":2})
            } else {
                json!({"fee":0})
            },
            encrypted,
            integrity,
        };
        let store = self.registry.store();
        store
            .put(
                "1",
                "download_tasks_v1",
                id,
                &serde_json::to_value(task).unwrap(),
            )
            .unwrap();
        store
            .put(
                "1",
                "offline_music_v1",
                &key,
                &serde_json::to_value(ledger).unwrap(),
            )
            .unwrap();
        file
    }
}

fn replace(path: &Path) {
    // File::open shares delete/rename on Windows. Move the held file to a
    // sibling on the same volume and never truncate an existing pathname.
    let retired = path.with_extension("retired");
    fs::rename(path, &retired).unwrap();
    let mut replacement = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    replacement
        .write_all(b"replacing the name must not replace the held file")
        .unwrap();
    assert!(!same_file::is_same_file(path, retired).unwrap());
}

#[tokio::test]
async fn cold_flac_reader_fences_lease_and_strict_ncm_time_and_membership() {
    let fixture = Fixture::new().await;
    let _plain_path = fixture.publish("plain", FLAC, false);
    let _protected_path = fixture.publish("protected", FLAC, true);
    assert!(
        fixture
            .grant
            .open_flac_reader("protected", Cancellation::default())
            .is_err()
    );

    let checks = Arc::new(AtomicUsize::new(0));
    let allowed_until = Arc::new(AtomicUsize::new(usize::MAX));
    let lease = Cancellation::new({
        let checks = checks.clone();
        let allowed_until = allowed_until.clone();
        move || checks.fetch_add(1, Ordering::AcqRel) < allowed_until.load(Ordering::Acquire)
    });
    let mut plain = fixture.grant.open_flac_reader("plain", lease).unwrap();
    let len = plain.seek(SeekFrom::End(0)).unwrap();
    assert_eq!(len, FLAC.len() as u64);
    assert_eq!(plain.read(&mut [0; 3]).unwrap(), 0);
    assert_eq!(
        plain.seek(SeekFrom::End(1)).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    plain.rewind().unwrap();
    let before = checks.load(Ordering::Acquire);
    // Permit the pre-read check, then revoke before the scratch bytes can
    // be copied out by the post-read check.
    allowed_until.store(before + 1, Ordering::Release);
    let mut untouched = [0x5a; 8];
    assert_eq!(
        plain.read(&mut untouched).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(untouched, [0x5a; 8]);
    assert_eq!(checks.load(Ordering::Acquire), before + 2);
    allowed_until.store(usize::MAX, Ordering::Release);
    assert!(plain.authority().cancellation().is_cancelled());

    let mut rights = fixture.membership();
    let mut protected = fixture
        .grant
        .open_flac_reader("protected", Cancellation::default())
        .unwrap();
    let authority = protected.authority();
    assert_eq!(protected.seek(SeekFrom::End(0)).unwrap(), FLAC.len() as u64);
    assert!(
        protected
            .seek(SeekFrom::Start(FLAC.len() as u64 + 1))
            .is_err()
    );
    let start = FLAC.len() / 2;
    protected.seek(SeekFrom::Start(start as u64)).unwrap();
    let mut tail = Vec::new();
    protected.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, FLAC[start..]);
    fixture
        .registry
        .store()
        .delete("1", "membership", "current")
        .unwrap();
    assert!(
        authority.cancellation().check().is_ok(),
        "callback checks must not read the database"
    );
    assert!(authority.validate().is_err());
    rights["data"]["fixtureRevision"] = json!(2);
    fixture
        .registry
        .store()
        .put("1", "membership", "current", &rights)
        .unwrap();
    assert!(
        authority.cancellation().is_cancelled(),
        "a denied source cannot be revived"
    );

    let mut protected = fixture
        .grant
        .open_flac_reader("protected", Cancellation::default())
        .unwrap();
    let live_plain = fixture
        .grant
        .open_flac_reader("plain", Cancellation::default())
        .unwrap();
    fixture.clock.0.store(99, Ordering::Release);
    assert!(protected.authority().cancellation().check().is_ok());
    fixture.clock.0.store(100, Ordering::Release);
    assert_eq!(
        protected.seek(SeekFrom::Start(0)).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(live_plain.authority().validate().is_ok());
    fixture.grant.revoke();
    assert!(live_plain.authority().cancellation().is_cancelled());
    assert_eq!(
        fixture.service.0.load(Ordering::Acquire),
        1,
        "only synthetic profile adoption may use HTTP"
    );
}

#[tokio::test]
#[ignore = "requires trusted libFLAC 1.5.0; verifies real cold-grant handoff without any child or audio device"]
async fn limiter_inprocess_flac_cold_grant_retains_open_file_and_rejects_buffered_pcm_after_detach()
{
    let api = Api::open(&sdk_path()).unwrap();
    let fixture = Fixture::new().await;
    let source = fixture.dir.path().join("generated.flac");
    const FRAMES: u64 = 32768 + 57;
    api.encode_fixture(&source, RATE, FLAC_BLOCK, FRAMES, source_pair)
        .unwrap();
    let bytes = fs::read(source).unwrap();
    fixture.membership();
    let mut decoders = Vec::new();
    for (id, encrypted) in [("native-plain", false), ("native-protected", true)] {
        let path = fixture.publish(id, &bytes, encrypted);
        let mut reader = fixture
            .grant
            .open_flac_reader(id, Cancellation::default())
            .unwrap();
        let authority = reader.authority();
        replace(&path);
        assert!(
            fixture
                .grant
                .open_flac_reader(id, Cancellation::default())
                .is_err()
        );
        authority.validate().unwrap();
        let mut prefix = [0; 4];
        reader.read_exact(&mut prefix).unwrap();
        assert_eq!(&prefix, b"fLaC");
        let mut decoder = Decoder::open(
            &api,
            Box::new(reader),
            Generation(1),
            authority.cancellation(),
        )
        .unwrap();
        assert_eq!(decoder.total, FRAMES);
        let start = 24_251;
        decoder.seek(Generation(2), start).unwrap();
        let actual = expect_packet(decoder.next().unwrap().unwrap(), Generation(2), start);
        assert_eq!(
            actual as u64,
            u64::from(FLAC_BLOCK) - start % u64::from(FLAC_BLOCK)
        );
        decoder.seek(Generation(3), 8197).unwrap();
        assert!(
            decoder.source().pending.is_some(),
            "native seek already queued a trimmed PCM block"
        );
        decoders.push((decoder, authority));
    }
    fixture.cold.detach().unwrap();
    for (mut decoder, authority) in decoders {
        assert!(authority.cancellation().is_cancelled());
        assert!(
            decoder.next().is_err(),
            "the already-buffered native PCM must not be delivered"
        );
        assert!(authority.validate().is_err());
    }
    assert_eq!(fixture.service.0.load(Ordering::Acquire), 1);
    eprintln!(
        "native libFLAC decoded plain and strict NCM-FLAC from the original verified handles after path replacement; real cold session detach rejected both already-buffered PCM blocks; no extra HTTP or process"
    );
}
