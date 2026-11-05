//! Synthetic material and rights with actual desktop-hosted native PCM.
#[allow(dead_code)]
#[path = "../../../core/tests/support/transfer.rs"]
mod transfer;

use cumulus_core::{
    Error, Result,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{LocalCommand, LocalQuery, PlaySelection, PlayerAction},
    },
    download::transfer::RunOptions,
    effects::{
        authorization::{ActiveParametricObservation, EffectAuthorizations, TrialConsent},
        catalog::EffectId,
        equalizer::abi::EqualizerStats,
        parametric::ParametricPlan,
        service::{EffectsLibrary, ParametricPreparation},
    },
    playback::{
        controller::{ParametricApplication, PlaybackStatus},
        engine::{
            AudioEngine, Cancellation, EngineSnapshot, LoadRequest, LoadedMedia, MediaInput,
            MpvConfig, MpvEngine,
        },
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const EFFECT: u64 = 300;
const URL: &str = "https://m.test.music.126.net/synthetic-bound-peq.ncae";

struct FixtureApi {
    len: usize,
    revoke: AtomicBool,
    paths: Mutex<Vec<String>>,
}
impl Api for FixtureApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.paths.lock().unwrap().push(path.to_owned());
            Ok(match path {
                "/api/usertool/home/sound/info" => {
                    assert_eq!(
                        payload["reqListStr"],
                        r#"[{"soundId":"300","soundType":10}]"#
                    );
                    json!({"code":200,"data":{"soundList":[{"soundId":"300","soundType":10,"pcSoundDetail":{
                        "id":EFFECT,"name":"Synthetic PEQ","soundType":3,
                        "soundMaterialList":[{"type":1,"size":self.len,"md5":"3d7172c914c75c5b32df037f967142a9","link":URL}]
                    }}]}})
                }
                "/api/vipauth/app/interests/auth/query" => {
                    assert_eq!(
                        payload,
                        json!({"queryInfo":{"interestsMap":{"127":[EFFECT]}}})
                    );
                    json!({"code":200,"data":[{"interestsId":EFFECT,"interestsType":127,"authResp":{"interestsVipType":6,"authResult":{"canUse":true,"canUseType":11}}}]})
                }
                "/api/vipauth/app/interests/recyclequery" => {
                    assert_eq!(payload, json!({"queryInfo":{"127":["300"]}}));
                    json!({"code":200,"data":{"127":[{"interestsId":EFFECT,"canRecycle":self.revoke.load(Ordering::Acquire)}]}})
                }
                "/api/vipauth/app/interests/auth/set" => {
                    assert_eq!(
                        payload,
                        json!({"interestsType":127,"interestsId":"300","type":0})
                    );
                    json!({"code":200,"data":true})
                }
                _ => return Err(Error::State("unexpected synthetic PEQ API path")),
            })
        })
    }
}

#[derive(Default)]
struct Trace {
    stats: Option<EqualizerStats>,
    cleared: bool,
}

struct Engine {
    inner: MpvEngine,
    wave: PathBuf,
    trace: Arc<Mutex<Trace>>,
}
impl AudioEngine for Engine {
    fn load(&mut self, request: LoadRequest, cancel: &Cancellation) -> Result<LoadedMedia> {
        if !matches!(&request.source, MediaInput::File(path) if path.canonicalize()? == self.wave) {
            return Err(Error::State("PEQ fixture accepts only its generated WAV"));
        }
        self.inner.load(request, cancel)
    }
    fn pause(&mut self, value: bool) -> Result<()> {
        self.inner.pause(value)
    }
    fn seek(&mut self, value: f64) -> Result<()> {
        self.inner.seek(value)
    }
    fn volume(&mut self, value: f64) -> Result<()> {
        self.inner.volume(value)
    }
    fn speed(&mut self, value: f64) -> Result<()> {
        self.inner.speed(value)
    }
    fn stop(&mut self) -> Result<()> {
        self.inner.stop()
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        let view = self.inner.poll()?;
        if let Ok(stats) = self.inner.parametric_equalizer_stats() {
            self.trace.lock().unwrap().stats = Some(stats);
        }
        Ok(view)
    }
    fn apply_parametric_component(&mut self, plan: ParametricPlan) -> Result<bool> {
        self.inner.apply_parametric_component(plan)
    }
    fn bypass_parametric_component(&mut self) -> Result<()> {
        self.inner.bypass_parametric_component()
    }
    fn restore_parametric_component(&mut self, plan: ParametricPlan) -> Result<()> {
        self.inner.restore_parametric_component(plan)
    }
    fn clear_parametric_component(&mut self) -> Result<()> {
        let attached = self.inner.parametric_equalizer_stats().is_ok();
        self.inner.clear_parametric_component()?;
        if attached {
            assert!(self.inner.parametric_equalizer_stats().is_err());
            self.trace.lock().unwrap().cleared = true;
        }
        Ok(())
    }
}

async fn proof(library_path: &Path, root: &Path) -> Result<Value> {
    std::fs::create_dir(root)?;
    let media = root.join("media");
    std::fs::create_dir(&media)?;
    let wave = media.join("synthetic.wav");
    super::native_media::wav(&wave)?;
    let wave = wave.canonicalize()?;
    // Frozen synthetic raw-DEFLATE NCAE (archive netease_ncae.crypt,
    // key abcd1234, marker 0x63); aev2.peq gain=-6 with a 440 Hz band.
    let bytes = hex::decode(concat!(
        "4e4341454f000000000000000100010009020100076352515057924ea58f1ff3ec",
        "1c3887235c6c92bd5a53ca960d2d51ed640714dece76e313f9e42a644e1e1ed9",
        "4e0ec25f14c3085cab80dc7d6188e5d87472637298cd3ae778afb67376e9a909",
        "ac32171644744961",
    ))
    .expect("frozen synthetic NCAE hex");
    let api = Arc::new(FixtureApi {
        len: bytes.len(),
        revoke: AtomicBool::new(false),
        paths: Mutex::new(Vec::new()),
    });
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session.adopt(api.clone(), json!({"userId":90001}), &session.token()?)?;
    let token = session.token()?;
    let epoch = token.generation.to_string();
    let id = EffectId::new(EFFECT)?;
    let material_origin = Arc::new(transfer::Origin::new(bytes));
    let materials = EffectsLibrary::with_backend(session.clone(), material_origin.clone())?;
    let ParametricPreparation::Prepared(prepared) = materials
        .prepare_parametric_for(&token, id, RunOptions::default())
        .await?
    else {
        return Err(Error::State("fixture PEQ material not ready"));
    };
    // Deliberate fake setup; runtime activation itself must not register.
    EffectAuthorizations::new(session.clone())?
        .begin_for(&token, id, TrialConsent::DoNotStart)
        .await?;

    let trace = Arc::new(Mutex::new(Trace::default()));
    let client = MusicClient::with_session(session, registry).with_audio_factory({
        let trace = trace.clone();
        let library_path = library_path.to_path_buf();
        move || {
            Ok(Box::new(Engine {
                inner: MpvEngine::open(MpvConfig {
                    null_output: true,
                    ..Default::default()
                })?
                .with_effects_library(library_path.clone()),
                wave: wave.clone(),
                trace: trace.clone(),
            }))
        }
    });
    let binding = client
        .bind_native_effect_parametric(&epoch, prepared)
        .await?;
    assert_eq!(
        client
            .apply_native_bound_parametric(&epoch, binding)
            .await?,
        ParametricApplication::Configured
    );
    client.start_local_library().await?;
    client
        .local_command(LocalCommand::Watch {
            path: media.to_string_lossy().into_owned(),
        })
        .await?;
    let query = LocalQuery::default();
    let page = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let page = client.local_page(query.clone()).await?;
            if page.playable == 1 {
                return Ok::<_, Error>(page);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| Error::State("synthetic local WAV was not indexed"))??;
    client
        .play_selection(
            &epoch,
            PlaySelection::Local {
                query,
                start_id: None,
                revision: page.revision,
            },
        )
        .await?;
    let (stats, position, play_id) = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let player = client.player_snapshot(&epoch).await?;
            let stats = trace.lock().unwrap().stats;
            if let (Some(stats), Some(position)) = (stats, player.position)
                && player.state == PlaybackStatus::Playing
                && position > 0.15
                && stats.processed_samples > 1024
                && stats.active_instances != 0
                && stats.failed_blocks == 0
                && stats.observed_revision == stats.revision
            {
                let play_id = player
                    .play_id
                    .ok_or(Error::State("fixture play ID missing"))?;
                return Ok::<_, Error>((stats, position, play_id));
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .map_err(|_| Error::State("bound PEQ did not process actual PCM"))??;
    assert_eq!(
        client.observe_native_bound_parametric(&epoch).await?,
        ActiveParametricObservation::Confirmed
    );
    assert!(!trace.lock().unwrap().cleared);
    api.revoke.store(true, Ordering::Release);
    assert_eq!(
        client.observe_native_bound_parametric(&epoch).await?,
        ActiveParametricObservation::Revoked
    );
    let after = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let player = client.player_snapshot(&epoch).await?;
            if trace.lock().unwrap().cleared
                && player.state == PlaybackStatus::Playing
                && player.play_id.as_deref() == Some(play_id.as_str())
                && let Some(after) = player.position.filter(|after| *after > position)
            {
                return Ok::<_, Error>(after);
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .map_err(|_| Error::State("revoked PEQ did not detach with clock advancing"))??;
    client.player_action(&epoch, PlayerAction::Stop).await?;
    let paths = api.paths.lock().unwrap();
    assert_eq!(
        paths
            .iter()
            .filter(|path| path.ends_with("/auth/set"))
            .count(),
        1
    );
    assert_eq!(
        paths
            .iter()
            .filter(|path| path.ends_with("/auth/stop"))
            .count(),
        0
    );
    assert_eq!(
        paths
            .iter()
            .filter(|path| path.ends_with("/recyclequery"))
            .count(),
        4
    );
    assert_eq!(paths.len(), 12);
    assert_eq!(material_origin.calls().len(), 1);
    Ok(json!({
        "ok":true,"boundParametric":true,"actualDecoder":true,"realMusicRequests":0,
        "physicalAudioOutputVerified":false,"syntheticRegistrationSetup":1,"materialReads":1,
        "processed":stats,"confirmedWithoutClearing":true,"revocationDetached":true,
        "positionBefore":position,"positionAfter":after,"samePlay":true
    }))
}

pub fn verify(library: &Path, root: &Path) -> Result<Value> {
    tauri::async_runtime::block_on(proof(library, root))
}
