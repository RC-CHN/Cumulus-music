use super::{EffectId, Md5, Origin, ScriptApi, id, sample, world};
use crate::playback_support;
use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{PlaySelection, PlayerAction, PlayerView},
    },
    membership::Quality,
    playback::controller::{DeviceParametricStatus as Status, ParametricApplication},
    protocol::http::ApiOrigin,
};
use serde_json::json;
use sha2::Digest;
use std::sync::{Arc, atomic::Ordering::SeqCst};

struct Proof {
    _root: tempfile::TempDir,
    client: MusicClient,
    session: Arc<AccountSession>,
    device: Arc<playback_support::Device>,
    api: Arc<ScriptApi>,
    origin: Arc<Origin>,
}

impl Proof {
    async fn open(levels: &[(&str, u64)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let (bytes, _) = sample("first");
        let model = json!({"code":200,"data":[{"soundId":300,"soundType":2,"deviceModel":"Fixture",
            "size":bytes.len(),"soundMd5":hex::encode(Md5::digest(&bytes)),"deviceSoundUrl":super::URL}]});
        let replies = std::iter::once(model).chain(levels.iter().enumerate().flat_map(
            |(index, (level, br))| {
                let song = index + 1;
                [
                    json!({"code":200,"songs":[{"id":song,"name":"Fixture","dt":120000,"ar":[],"al":{"id":1,"name":"Fixture"}}],"privileges":[]}),
                    json!({"code":200,"data":[{"id":song,"code":200,"url":"https://m1.music.126.net/fixture.mp3","md5":"abcd","expi":120,"level":level,"br":br}]}),
                ]
            },
        ));
        let api = Arc::new(ScriptApi::new(replies.map(Into::into)));
        let session = world(api.clone(), registry.store());
        let device = Arc::new(playback_support::Device::default());
        device.parametric_supported.store(true, SeqCst);
        let audio = device.clone();
        let origin = Arc::new(Origin::new(bytes));
        let client = MusicClient::with_session(session.clone(), registry)
            .with_effect_material_backend(origin.clone())
            .with_audio_factory(move || Ok(playback_support::audio_engine(audio.clone())));
        let epoch = session.token().unwrap().generation.to_string();
        assert_eq!(
            client
                .apply_native_device_parametric(&epoch, EffectId::new(7002).unwrap(), id())
                .await
                .unwrap(),
            ParametricApplication::Configured
        );
        Self {
            _root: root,
            client,
            session,
            device,
            api,
            origin,
        }
    }

    fn epoch(&self) -> String {
        self.session.token().unwrap().generation.to_string()
    }

    async fn play(&self, song: usize, quality: Quality) -> cumulus_core::Result<PlayerView> {
        self.client
            .play_selection(
                &self.epoch(),
                PlaySelection::Songs {
                    ids: vec![song.to_string()],
                    index: 0,
                    quality,
                },
            )
            .await
    }
}

#[tokio::test]
async fn source_bypass_follows_the_authorized_level_and_the_official_two_special_sequence() {
    // Conflicting br and requested quality are intentional: neither is the
    // effective level mapped by M/163 before M/308 checks playInfo.bitrate.
    let levels = [
        ("standard", 6999000, Status::Configured),
        ("vivid", 128000, Status::BypassedForSource),
        ("dolby", 128000, Status::Configured),
        ("sky", 128000, Status::BypassedForSource),
        ("jymaster", 4999000, Status::Configured),
        ("jymaster", 4999000, Status::Configured),
        ("jyeffect", 128000, Status::BypassedForSource),
        ("standard", 128000, Status::Configured),
    ];
    let proof = Proof::open(&levels.map(|(level, br, _)| (level, br))).await;
    let epoch = proof.epoch();
    let saved = proof.client.remembered_native_headphone(&epoch).unwrap();
    let mut identity = None;
    for (index, (actual, _, expected)) in levels.into_iter().enumerate() {
        let view = proof
            .play(
                index + 1,
                if index == 0 {
                    Quality::Vivid
                } else {
                    Quality::Standard
                },
            )
            .await
            .unwrap();
        assert_eq!(view.actual_quality.as_deref(), Some(actual));
        let device = view.device_parametric.unwrap();
        let bypassed = expected == Status::BypassedForSource;
        assert_eq!(device.status, expected);
        assert_eq!(device.comparison.is_none(), bypassed);
        assert_eq!(
            *identity.get_or_insert(device.activation_id),
            device.activation_id
        );
        assert_eq!(proof.device.parametric_bypassed.load(SeqCst), bypassed);
        assert!(proof.device.parametric_attached.load(SeqCst));
        if bypassed {
            assert!(!view.parametric_processing);
        }
    }
    assert_eq!(
        *proof.device.parametric_bypassed_at_unpause.lock().unwrap(),
        [false, true, false, true, false, false, true, false]
    );
    assert_eq!(proof.device.state.lock().unwrap().loads, levels.len());
    assert_eq!(
        proof.client.remembered_native_headphone(&epoch).unwrap(),
        saved
    );
    assert_eq!(proof.origin.calls().len(), 1);
    let calls = proof.api.calls();
    assert_eq!(calls.len(), 1 + 2 * levels.len());
    assert_eq!(
        calls[0].path,
        "/api/usertool/home/soundeffect/devicesound/list"
    );
    assert_eq!(calls[2].payload["level"], "vivid");
    assert_eq!(calls[4].payload["level"], "standard");
    assert!(
        calls[1..]
            .chunks_exact(2)
            .all(|pair| pair[0].path == "/api/v3/song/detail"
                && pair[1].path == "/api/song/enhance/player/url/v1")
    );

    proof
        .client
        .player_action(&epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(!proof.device.parametric_attached.load(SeqCst));
    assert!(
        proof
            .client
            .player_snapshot(&epoch)
            .await
            .unwrap()
            .device_parametric
            .is_none()
    );
    assert_eq!(
        proof.client.remembered_native_headphone(&epoch).unwrap(),
        saved
    );
}

#[tokio::test]
async fn source_bypass_failure_never_claims_dry_processing_or_unpauses_an_uncertain_graph() {
    for unsafe_detach in [false, true] {
        let proof = Proof::open(&[("vivid", 128000)]).await;
        proof.device.parametric_bypass_fail.store(true, SeqCst);
        proof
            .device
            .parametric_clear_fail
            .store(unsafe_detach, SeqCst);
        let result = proof.play(1, Quality::Standard).await;
        if unsafe_detach {
            assert!(matches!(
                result,
                Err(Error::State("injected parametric cleanup failure"))
            ));
            assert!(
                proof
                    .device
                    .parametric_bypassed_at_unpause
                    .lock()
                    .unwrap()
                    .is_empty()
            );
        } else {
            assert_eq!(
                result.unwrap().device_parametric.unwrap().status,
                Status::ProcessorFailed
            );
            assert_eq!(*proof.device.parametric_at_unpause.lock().unwrap(), [false]);
        }
        assert_eq!(proof.device.parametric_attached.load(SeqCst), unsafe_detach);
        assert!(!proof.device.parametric_bypassed.load(SeqCst));
        assert_eq!(proof.api.calls().len(), 3);
    }
}

#[tokio::test]
async fn account_change_retires_a_bypassed_device_without_reopening_it_from_an_old_load() {
    let proof = Proof::open(&[("vivid", 128000), ("standard", 128000)]).await;
    let epoch = proof.epoch();
    assert_eq!(
        proof
            .play(1, Quality::Standard)
            .await
            .unwrap()
            .device_parametric
            .unwrap()
            .status,
        Status::BypassedForSource
    );
    // Consume the first load's notification before waiting for the second.
    proof.device.started.notified().await;
    proof.device.hold_load.store(true, SeqCst);
    let replace_account = async {
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            proof.device.started.notified(),
        )
        .await
        .unwrap();
        proof
            .session
            .adopt(
                proof.api.clone(),
                json!({"userId":2}),
                &proof.session.token().unwrap(),
            )
            .unwrap();
    };
    let (result, ()) = tokio::join!(proof.play(2, Quality::Standard), replace_account);
    assert!(matches!(
        result,
        Err(Error::StaleResponse | Error::Cancelled)
    ));
    assert!(
        proof
            .client
            .player_snapshot(&proof.epoch())
            .await
            .unwrap()
            .device_parametric
            .is_none()
    );
    assert!(!proof.device.parametric_attached.load(SeqCst));
    assert!(!proof.device.parametric_bypassed.load(SeqCst));
    assert_eq!(
        *proof.device.parametric_bypassed_at_unpause.lock().unwrap(),
        [true]
    );
    assert!(matches!(
        proof.client.remembered_native_headphone(&epoch),
        Err(Error::StaleResponse)
    ));
    assert!(
        proof
            .client
            .remembered_native_headphone(&proof.epoch())
            .unwrap()
            .is_none()
    );
}
