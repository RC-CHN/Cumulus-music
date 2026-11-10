//! Synthetic owner proof on the real actor and held worker. There is no
//! registration, production message, engine-wrapper forwarding, or IPC.
use super::{Actor, ModernEffectSource, Request};
use crate::{
    Error, Result,
    effects::{material::ModernLimiterPlan, stereo::LocalEqualizerMaterialPlan},
    playback::engine::{Cancellation, TestModernCommand},
};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Default)]
pub(in crate::playback::controller) struct TestModernOwner(Option<Installed>);

struct Grant(Cancellation);
impl Drop for Grant {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

struct Installed {
    source: Arc<ModernEffectSource>,
    grant: Grant,
    id: Uuid,
    revision: u64,
    active: bool,
}

async fn issue(
    source: Arc<ModernEffectSource>,
    revision: u64,
    command: TestModernCommand,
) -> Result<u64> {
    tokio::task::spawn_blocking(move || source.native.test_modern_command(revision, command))
        .await
        .map_err(|_| Error::State("synthetic modern worker did not complete"))?
}

impl Actor {
    fn test_effect_entry(&self, source: &ModernEffectSource) -> Result<()> {
        self.session.check_generation(&self.token)?;
        source.playback.check()?;
        if !self.state.active()
            || self.pending.is_some()
            || self.native_seeking
            || !self.parametric.admits_local_equalizer()
            || !self.loaded.as_ref().is_some_and(|load| {
                load.local_equalizer && load.media.entry_id == source.native.test_native_entry()
            })
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }

    async fn test_attach_modern(
        &mut self,
        source: Arc<ModernEffectSource>,
        plan: ModernLimiterPlan,
        policy: Cancellation,
    ) -> Result<()> {
        if self.test_modern.0.is_some() {
            return Err(Error::State("synthetic modern owner is already attached"));
        }
        self.test_effect_entry(&source)?;
        source.check_for(&self.session, &self.token)?;
        policy.check()?;
        let playback = source.playback.clone();
        // Losing the actor operation drops this grant even if an already
        // queued command finishes. The worker checks it before the next PCM.
        let grant = Grant(Cancellation::new(move || {
            !policy.is_cancelled() && !playback.is_cancelled()
        }));
        let id = Uuid::new_v4();
        let revision = issue(
            source.clone(),
            source.native.test_graph_revision(),
            TestModernCommand::Attach {
                owner: id,
                plan: Box::new(plan),
                authority: grant.0.clone(),
            },
        )
        .await?;
        self.test_effect_entry(&source)?;
        self.test_modern.0 = Some(Installed {
            source,
            grant,
            id,
            revision,
            active: true,
        });
        if !self.test_supervise_modern().await? {
            return Err(Error::Cancelled);
        }
        Ok(())
    }

    async fn test_supervise_modern(&mut self) -> Result<bool> {
        let Some(mut owner) = self.test_modern.0.take() else {
            return Ok(false);
        };
        // A stale generation never issues Close against a different entry.
        // Dropping its old grant still prevents further active old quanta.
        self.test_effect_entry(&owner.source)?;
        if owner.active && owner.grant.0.is_cancelled() {
            owner.revision = issue(
                owner.source.clone(),
                owner.revision,
                TestModernCommand::Close(owner.id),
            )
            .await?;
            owner.active = false;
        }
        let active = owner.active;
        self.test_modern.0 = Some(owner);
        Ok(active)
    }

    async fn test_return_legacy(&mut self, plan: LocalEqualizerMaterialPlan) -> Result<()> {
        let owner = self
            .test_modern
            .0
            .take()
            .ok_or(Error::State("no synthetic modern owner"))?;
        self.test_effect_entry(&owner.source)?;
        issue(
            owner.source.clone(),
            owner.revision,
            TestModernCommand::ReturnToGeq(owner.id, plan),
        )
        .await?;
        Ok(())
    }
}

mod evidence {
    use super::*;
    use crate::{
        account::AccountSession,
        effects::equalizer::EqualizerSettings,
        library::{
            AudioMetadata, LocalLibrary, native_open::inspect_opened_file, probe::AudioProbe,
        },
        playback::{
            controller::{PlaybackStatus, state::Loaded},
            engine::{
                AudioEngine, EndReason, EngineEvent, LoadRequest, MpvConfig, MpvEngine,
                StereoPcmSnapshot, pcm_wave::tests::wave_at,
            },
            source::SourceResolver,
        },
        protocol::api::{Api, ApiFuture},
        storage::StateStore,
    };
    use serde_json::{Value, json};
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    use tokio::sync::{mpsc, oneshot};

    const RATE: u32 = 48_000;
    const TOTAL: u64 = 72 * 2048 + 7;

    struct NoRemote;
    impl Api for NoRemote {
        fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
            panic!("synthetic held actor must not call any API")
        }
    }
    struct NoPathProbe;
    impl AudioProbe for NoPathProbe {
        fn inspect(&self, _: &Path) -> Result<AudioMetadata> {
            panic!("held WAV metadata must come from its original descriptor")
        }
    }

    fn settings(gain: f32) -> EqualizerSettings {
        let mut gains = [0.0; 10];
        gains[1] = gain;
        EqualizerSettings::new(true, gains).unwrap()
    }

    fn plan() -> ModernLimiterPlan {
        let oracle: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/effects-modern-limiter-graph-official.json"
        )))
        .unwrap();
        ModernLimiterPlan::from_material(&oracle["profiles"]["neutralTone"]).unwrap()
    }

    async fn until<T>(mut probe: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(value) = probe() {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "native actor clock never reached the expected phase"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    async fn fixture() -> (tempfile::TempDir, Actor, i64) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("actor-owned.wav");
        let wav = wave_at(
            RATE,
            (0..TOTAL).map(|index| {
                let sample = ((index * 97) % 8192) as i16 - 4096;
                [sample, -sample / 4]
            }),
        );
        fs::write(&path, wav).unwrap();
        let resource = inspect_opened_file(&path, &NoPathProbe).unwrap();
        let store = Arc::new(StateStore::open(dir.path()).unwrap());
        let api = Arc::new(NoRemote);
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(api, json!({"userId":71}), &session.token().unwrap())
            .unwrap();
        let library = Arc::new(LocalLibrary::new(store.clone()).unwrap());
        let resolver = Arc::new(SourceResolver::new(session.clone(), Some(library)));
        let mut engine = MpvEngine::open(MpvConfig {
            library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
            null_output: true,
        })
        .unwrap()
        .with_local_equalizer_pcm_wave();
        let token = session.token().unwrap();
        let source = resolver.resolve(&resource, &token).await.unwrap();
        let media = engine
            .load_with_local_equalizer(
                LoadRequest::new(source.input.clone()),
                settings(3.0),
                &Cancellation::default(),
            )
            .unwrap();
        assert_eq!(media.codec.as_deref(), Some("pcm_f32le"));
        let entry = media.entry_id;
        fs::rename(&path, dir.path().join("still-held.wav")).unwrap();
        fs::write(path, b"do not reopen this path").unwrap();
        engine.pause(false).unwrap();
        until(|| {
            let view = engine.poll().unwrap();
            (view.audio_output.as_deref() == Some("null")
                && view.position_for(entry).is_some_and(|p| p > 0.2))
            .then_some(())
        })
        .await;
        engine.pause(true).unwrap();
        until(|| {
            let view = engine.poll().unwrap();
            assert_eq!(view.entry_id, Some(entry));
            view.paused.then_some(())
        })
        .await;
        let (sender, _) = mpsc::unbounded_channel();
        let mut actor = Actor::new(
            session,
            store,
            resolver,
            Arc::new(Mutex::new(Box::new(engine))),
            sender,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        actor.queue.replace(vec![resource], 0, json!({})).unwrap();
        actor.loaded = Some(Loaded {
            item: actor.queue.selected().unwrap(),
            timeline: source.timeline(media.duration).unwrap(),
            source,
            media,
            history: Default::default(),
            podcast_progress: None,
            local_equalizer: true,
            indexed_flac: false,
            audiobook_skip: None,
        });
        actor.state = PlaybackStatus::Paused;
        actor.play_id = Some("synthetic-modern-actor".into());
        (dir, actor, entry)
    }

    async fn capture(actor: &Actor) -> Arc<ModernEffectSource> {
        let (reply, _) = oneshot::channel();
        Arc::new(
            actor
                .modern_effect_source(&Request {
                    token: actor.token.clone(),
                    reply,
                })
                .await
                .unwrap(),
        )
    }

    async fn pause_settled(actor: &mut Actor, source: &ModernEffectSource) -> StereoPcmSnapshot {
        actor.engine.lock().unwrap().pause(true).unwrap();
        actor.state = PlaybackStatus::Paused;
        let mut stable = None;
        until(|| {
            let view = actor.engine.lock().unwrap().poll().unwrap();
            let state = source.native.test_pcm_snapshot().unwrap();
            assert_eq!(view.entry_id, Some(source.native.test_native_entry()));
            if !view.paused || !state.paused || state.seeking {
                stable = None;
                return None;
            }
            if let Some((previous, since)) = stable
                && previous == state
            {
                return (Instant::now().duration_since(since) >= Duration::from_millis(80))
                    .then_some(state);
            }
            stable = Some((state, Instant::now()));
            None
        })
        .await
    }

    async fn advance(actor: &mut Actor, source: &ModernEffectSource, previous: StereoPcmSnapshot) {
        actor.engine.lock().unwrap().pause(false).unwrap();
        actor.state = PlaybackStatus::Playing;
        until(|| {
            let view = actor.engine.lock().unwrap().poll().unwrap();
            let state = source.native.test_pcm_snapshot().unwrap();
            assert_eq!(view.entry_id, Some(source.native.test_native_entry()));
            assert!(!state.failed && !state.cancelled);
            (state.decoded_source_frame >= previous.decoded_source_frame + 2048
                && state.native_output_frame >= previous.native_output_frame + u64::from(RATE) / 5)
                .then_some(())
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires compatible libmpv and uses actual held WAV, null output, synthetic authority"]
    async fn modern_actor_revokes_in_the_same_worker_then_explicitly_returns_to_legacy() {
        let (_dir, mut actor, entry) = fixture().await;
        let stale = capture(&actor).await;
        actor
            .engine
            .lock()
            .unwrap()
            .apply_local_equalizer(settings(2.0))
            .unwrap();
        assert!(matches!(
            actor
                .test_attach_modern(stale, plan(), Cancellation::default())
                .await,
            Err(Error::StaleResponse)
        ));
        let source = capture(&actor).await;
        let before = pause_settled(&mut actor, &source).await;
        let revoked = Cancellation::default();
        revoked.cancel();
        assert!(matches!(
            actor
                .test_attach_modern(source.clone(), plan(), revoked)
                .await,
            Err(Error::Cancelled)
        ));
        assert_eq!(source.native.test_pcm_snapshot().unwrap(), before);

        let grant = Cancellation::default();
        actor
            .test_attach_modern(source.clone(), plan(), grant.clone())
            .await
            .unwrap();
        assert!(actor.test_supervise_modern().await.unwrap());
        let installed = source.native.test_pcm_snapshot().unwrap();
        assert_eq!(
            installed,
            StereoPcmSnapshot {
                requested_revision: before.requested_revision + 1,
                applied_revision: before.applied_revision + 1,
                ..before
            }
        );
        assert!(matches!(source.check(), Err(Error::StaleResponse)));
        advance(&mut actor, &source, installed).await;
        let active = pause_settled(&mut actor, &source).await;
        assert_eq!(
            active.decoded_source_frame - active.produced_frames,
            238 + 191
        );

        // Revoke independently of the actor's periodic supervision. The
        // worker must process the next quantum using official Close, even
        // before the actor asks for its local acknowledgement.
        grant.cancel();
        advance(&mut actor, &source, active).await;
        let off = pause_settled(&mut actor, &source).await;
        assert_eq!(
            off.decoded_source_frame - off.produced_frames,
            238 + 191 + 238
        );
        assert_eq!(off.applied_revision, active.applied_revision);
        assert!(!actor.test_supervise_modern().await.unwrap());
        assert!(!actor.test_modern.0.as_ref().unwrap().active);
        assert_eq!(actor.loaded.as_ref().unwrap().media.entry_id, entry);
        let dry = source.native.test_pcm_snapshot().unwrap();
        assert_eq!(
            dry,
            StereoPcmSnapshot {
                requested_revision: off.requested_revision + 1,
                applied_revision: off.applied_revision + 1,
                ..off
            }
        );

        actor
            .test_return_legacy(LocalEqualizerMaterialPlan::from_saved(settings(-4.5)))
            .await
            .unwrap();
        assert!(actor.test_modern.0.is_none());
        assert_eq!(
            source.native.test_pcm_snapshot().unwrap(),
            StereoPcmSnapshot {
                requested_revision: dry.requested_revision + 1,
                applied_revision: dry.applied_revision + 1,
                ..dry
            }
        );
        actor.engine.lock().unwrap().pause(false).unwrap();
        let end = until(|| {
            let view = actor.engine.lock().unwrap().poll().unwrap();
            view.events
                .iter()
                .any(|event| {
                    matches!(event, EngineEvent::Ended {
                entry_id, reason: EndReason::Eof, error: 0,
            } if *entry_id == entry)
                })
                .then(|| source.native.test_pcm_snapshot().unwrap())
        })
        .await;
        assert!(end.graph_end && end.reader_closed && end.native_sink_retired && !end.failed);
        assert_eq!(end.decoded_source_frame, TOTAL);
        assert_eq!(
            end.produced_frames,
            TOTAL - 238 - 191,
            "legacy return must retain the default B0 already running since Close"
        );
        assert_eq!(end.delivered_frames, end.produced_frames);
        eprintln!(
            "actor-owned close/legacy receipt: entry={entry}, active={active:?}, dry={dry:?}, end={end:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires compatible libmpv and uses actual held WAV, null output, synthetic authority"]
    async fn modern_actor_stale_generation_drops_grant_without_submitting_old_close() {
        let (_dir, mut actor, _) = fixture().await;
        let source = capture(&actor).await;
        let before = pause_settled(&mut actor, &source).await;
        actor
            .test_attach_modern(source.clone(), plan(), Cancellation::default())
            .await
            .unwrap();
        let installed = source.native.test_pcm_snapshot().unwrap();
        assert_eq!(installed.applied_revision, before.applied_revision + 1);
        actor.generation.fetch_add(1, Ordering::AcqRel);
        assert!(matches!(
            actor.test_supervise_modern().await,
            Err(Error::Cancelled)
        ));
        assert!(actor.test_modern.0.is_none());
        assert_eq!(
            source.native.test_pcm_snapshot().unwrap(),
            installed,
            "stale owner must not submit a control"
        );
        advance(&mut actor, &source, installed).await;
        let off = pause_settled(&mut actor, &source).await;
        assert_eq!(
            off.decoded_source_frame - off.produced_frames,
            2 * 238,
            "the next quantum starts dry with old debt and one default B0, before any active modern block"
        );
        assert_eq!(off.applied_revision, installed.applied_revision);
    }
}
