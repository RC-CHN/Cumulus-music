use super::{Origin, ScriptApi, URL, auth, id, nontrial_recycle, pair, sample, sound, world};
use crate::playback_support;
use cumulus_core::{
    account::profiles::ProfileRegistry,
    client::{
        MusicClient, NativeParametricActivation, NativeParametricActivationAttempt,
        dto::{PlaySelection, PlayerView},
    },
    download::transfer::RunOptions,
    effects::service::{EffectsLibrary, StandaloneParametricPreflight},
    membership::Quality,
    playback::controller::{ParametricBindingStatus as Status, ParametricBindingStatus},
    protocol::http::ApiOrigin,
};
use serde_json::json;
use std::sync::{Arc, atomic::Ordering::SeqCst};

struct Proof {
    _root: tempfile::TempDir,
    client: MusicClient,
    device: Arc<playback_support::Device>,
    api: Arc<ScriptApi>,
    origin: Arc<Origin>,
    epoch: String,
    target: cumulus_core::effects::authorization::BoundParametricTarget,
}

impl Proof {
    async fn open(levels: &[(&str, u64)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let (bytes, _) = sample("first");
        let api = Arc::new(ScriptApi::new(
            pair(&sound(&bytes, URL), true)
                .into_iter()
                .chain([
                    auth(true),
                    json!({"code":200,"data":true}).into(),
                    auth(true),
                    nontrial_recycle(false),
                ])
                .chain(levels.iter().enumerate().flat_map(|(index, (level, br))| {
                    let song = index + 1;
                    [
                        json!({"code":200,"songs":[{"id":song,"name":"Fixture","dt":120000,"ar":[],"al":{"id":1,"name":"Fixture"}}],"privileges":[]}).into(),
                        json!({"code":200,"data":[{"id":song,"code":200,"url":"https://m1.music.126.net/fixture.mp3","md5":"abcd","expi":120,"level":level,"br":br}]}).into(),
                    ]
                })),
        ));
        let session = world(api.clone(), registry.store());
        let token = session.token().unwrap();
        let epoch = token.generation.to_string();
        let origin = Arc::new(Origin::new(bytes));
        let library = EffectsLibrary::with_backend(session.clone(), origin.clone()).unwrap();
        let StandaloneParametricPreflight::Ready(receipt) = library
            .preflight_parametric_activation_for(&token, id(), RunOptions::default())
            .await
            .unwrap()
        else {
            panic!("fixture must have standalone PEQ");
        };
        let device = Arc::new(playback_support::Device::default());
        device.parametric_supported.store(true, SeqCst);
        let audio = device.clone();
        let client = MusicClient::with_session(session, registry)
            .with_audio_factory(move || Ok(playback_support::audio_engine(audio.clone())));
        let mut attempt = NativeParametricActivationAttempt::new(receipt);
        let NativeParametricActivation::Configured(target) = client
            .activate_native_effect_parametric(&epoch, &mut attempt)
            .await
            .unwrap()
        else {
            panic!("fixture must have an attached PEQ");
        };
        Self {
            _root: root,
            client,
            device,
            api,
            origin,
            epoch,
            target,
        }
    }

    async fn play(&self, song: usize, quality: Quality) -> cumulus_core::Result<PlayerView> {
        self.client
            .play_selection(
                &self.epoch,
                PlaySelection::Songs {
                    ids: vec![song.to_string()],
                    index: 0,
                    quality,
                },
            )
            .await
    }

    async fn inspect(&self) -> ParametricBindingStatus {
        self.client
            .inspect_native_bound_parametric(&self.epoch, self.target)
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn registered_ordinary_peq_source_latch_keeps_exact_binding_and_unpause_order() {
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
        assert_eq!(view.bound_parametric, Some(expected));
        assert_eq!(proof.inspect().await, expected);
        if expected == Status::BypassedForSource {
            assert!(!view.parametric_processing);
        }
    }
    assert_eq!(
        *proof.device.parametric_bypassed_at_unpause.lock().unwrap(),
        [false, true, false, true, false, false, true, false]
    );
    assert_eq!(
        *proof.device.parametric_at_unpause.lock().unwrap(),
        [true; 8]
    );
    assert_eq!(proof.device.state.lock().unwrap().loads, levels.len());
    let calls = proof.api.calls();
    assert_eq!(calls.len(), 6 + levels.len() * 2);
    assert_eq!(calls[7].payload["level"], "vivid");
    assert_eq!(calls[9].payload["level"], "standard");
    assert!(calls[6..].chunks_exact(2).all(|pair| {
        pair[0].path == "/api/v3/song/detail" && pair[1].path == "/api/song/enhance/player/url/v1"
    }));
    assert_eq!(proof.origin.calls().len(), 1);
}

#[tokio::test]
async fn registered_ordinary_peq_source_restore_failure_requires_confirmed_removal() {
    for can_remove in [true, false] {
        let proof = Proof::open(&[("vivid", 128000), ("dolby", 128000)]).await;
        let dry = proof.play(1, Quality::Standard).await.unwrap();
        assert_eq!(dry.bound_parametric, Some(Status::BypassedForSource));
        proof.device.parametric_restore_fail.store(true, SeqCst);
        proof
            .device
            .parametric_clear_fail
            .store(!can_remove, SeqCst);
        let result = proof.play(2, Quality::Standard).await;
        if can_remove {
            assert_eq!(
                result.unwrap().bound_parametric,
                Some(Status::ProcessorFailed)
            );
            assert_eq!(proof.inspect().await, Status::ProcessorFailed);
            assert!(!proof.device.parametric_attached.load(SeqCst));
            assert_eq!(
                *proof.device.parametric_at_unpause.lock().unwrap(),
                [true, false]
            );
        } else {
            assert!(result.is_err());
            assert_eq!(proof.inspect().await, Status::DetachRequired);
            assert_eq!(*proof.device.parametric_at_unpause.lock().unwrap(), [true]);
            proof.device.parametric_clear_fail.store(false, SeqCst);
        }
        assert_eq!(proof.api.calls().len(), 10);
    }
}
