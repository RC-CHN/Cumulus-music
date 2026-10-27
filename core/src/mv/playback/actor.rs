use super::*;
use crate::{
    mv::MvService,
    playback::engine::{EndReason, EngineEvent},
    protocol::api::identifier,
};
use serde_json::json;
use std::time::{Duration, Instant};
pub(super) struct Actor {
    pub(super) session: Arc<AccountSession>,
    pub(super) token: SessionToken,
    pub(super) factory: Factory,
    pub(super) sender: mpsc::UnboundedSender<Message>,
    pub(super) frames: FrameSlot,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) generation: Arc<AtomicU64>,
    pub(super) pending: Option<tokio::task::JoinHandle<()>>,
    pub(super) engine: Option<Engine>,
    pub(super) entry: Option<i64>,
    pub view: VideoSnapshot,
    pub(super) checkpoint_at: Instant,
    pub(super) wants_play: bool,
    pub(super) local_source: Option<local::LocalSource>,
    pub(super) waiting_local: bool,
}
impl Actor {
    pub fn new(
        session: Arc<AccountSession>,
        token: SessionToken,
        factory: Factory,
        sender: mpsc::UnboundedSender<Message>,
        frames: FrameSlot,
        stop: Arc<AtomicBool>,
    ) -> Result<Self> {
        let settings = session
            .with_token(&token, |uid| {
                session
                    .persistent_store()?
                    .get(uid, "mv-player-v1", "settings")
            })?
            .unwrap_or_default();
        let volume = settings["volume"].as_f64().unwrap_or(60.0);
        let speed = settings["speed"].as_f64().unwrap_or(1.0);
        if !volume.is_finite()
            || !(0.0..=100.0).contains(&volume)
            || !speed.is_finite()
            || !(0.25..=4.0).contains(&speed)
        {
            return Err(Error::State("invalid stored video preferences"));
        }
        let view = VideoSnapshot {
            account: token.clone(),
            revision: 0,
            play_id: String::new(),
            id: None,
            status: VideoStatus::Idle,
            detail: None,
            requested_resolution: 480,
            actual_resolution: None,
            local: false,
            position: None,
            duration: None,
            paused: true,
            buffering: false,
            volume,
            speed,
            error: None,
        };
        Ok(Self {
            session,
            token,
            factory,
            sender,
            frames,
            stop,
            generation: Arc::new(AtomicU64::new(0)),
            pending: None,
            engine: None,
            entry: None,
            view,
            checkpoint_at: Instant::now(),
            wants_play: false,
            local_source: None,
            waiting_local: false,
        })
    }
    fn valid(&self) -> bool {
        !self.stop.load(Ordering::Acquire) && self.session.token().is_ok_and(|t| t == self.token)
    }
    fn publish(&mut self, publisher: &watch::Sender<VideoSnapshot>) {
        self.view.revision = self.view.revision.saturating_add(1);
        publisher.send_replace(self.view.clone());
    }
    pub(super) fn checkpoint(&mut self, completed: bool) -> Result<()> {
        if let (Some(id), Some(position), Some(duration)) =
            (&self.view.id, self.view.position, self.view.duration)
            && duration > 0.0
            && position.is_finite()
            && duration.is_finite()
        {
            MvService::new(self.session.clone())?.save_position_for(
                &self.token,
                &json!(id),
                (position.max(0.0).min(duration) * 1000.0) as u64,
                (duration * 1000.0) as u64,
                completed,
            )?;
            self.checkpoint_at = Instant::now();
        }
        Ok(())
    }
    pub(super) fn cancel_load(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        if let Some(task) = self.pending.take() {
            task.abort();
        }
    }
    pub(super) async fn release_engine(&mut self) -> Result<()> {
        *self.frames.lock().map_err(|_| Error::LockPoisoned)? = None;
        if let Some(engine) = self.engine.take() {
            device(engine, |e| e.stop()).await?;
        }
        self.entry = None;
        Ok(())
    }
    pub(super) async fn open(&mut self, load: resolve::Load, play: bool) -> Result<()> {
        identifier(&json!(load.id))?;
        if load.resolution == 0 || load.resolution > 8640 {
            return Err(Error::InvalidInput("invalid MV resolution"));
        }
        self.checkpoint(self.view.status == VideoStatus::Ended)?;
        self.cancel_load();
        self.release_engine().await?;
        self.local_source = None;
        self.waiting_local = false;
        self.wants_play = play;
        self.view.id = Some(load.id.clone());
        self.view.play_id = uuid::Uuid::new_v4().simple().to_string();
        self.view.status = VideoStatus::Resolving;
        self.view.error = None;
        self.view.detail = None;
        self.view.position = None;
        self.view.duration = None;
        self.view.requested_resolution = load.resolution;
        self.view.actual_resolution = None;
        self.view.local = false;
        self.view.paused = !play;
        self.view.buffering = true;
        let generation = self.generation.load(Ordering::Acquire);
        let current = self.generation.clone();
        let stopped = self.stop.clone();
        let token = self.token.clone();
        let session = self.session.clone();
        let check_session = session.clone();
        let check_token = token.clone();
        let cancel = Cancellation::new(move || {
            !stopped.load(Ordering::Acquire)
                && current.load(Ordering::Acquire) == generation
                && check_session.token().is_ok_and(|t| t == check_token)
        });
        let sender = self.sender.clone();
        let factory = self.factory.clone();
        self.pending = Some(tokio::spawn(async move {
            let ready = sender.clone();
            let selected = sender.clone();
            let result = resolve::prepare(
                session,
                token,
                factory,
                load,
                cancel,
                move || {
                    let _ = ready.send(Message::Loading(generation));
                },
                move |source| {
                    let _ = selected.send(Message::Local(generation, source));
                },
            )
            .await;
            let _ = sender.send(Message::Loaded(generation, Box::new(result)));
        }));
        Ok(())
    }
    async fn loaded(&mut self, generation: u64, result: Result<resolve::Prepared>) -> Result<()> {
        if generation != self.generation.load(Ordering::Acquire) || !self.valid() {
            if let Ok(p) = result {
                tokio::task::spawn_blocking(move || drop(p));
            }
            return Ok(());
        }
        self.pending = None;
        let prepared = result?;
        let engine = prepared.engine.clone();
        let volume = self.view.volume;
        let speed = self.view.speed;
        let paused = !self.wants_play;
        device(engine.clone(), move |e| {
            e.volume(volume)?;
            e.speed(speed)?;
            e.pause(paused)
        })
        .await?;
        if !self.valid() {
            device(engine, |e| e.stop()).await?;
            return Err(Error::StaleResponse);
        }
        self.view.detail = Some(prepared.detail);
        self.view.actual_resolution = Some(prepared.actual_resolution);
        self.view.local = prepared.local;
        self.view.duration = prepared.duration.or_else(|| {
            self.view
                .detail
                .as_ref()
                .map(|v| v.duration_ms as f64 / 1000.0)
        });
        self.view.position = prepared.position;
        self.view.paused = paused;
        self.view.buffering = false;
        self.view.status = if paused {
            VideoStatus::Paused
        } else {
            VideoStatus::Playing
        };
        self.entry = Some(prepared.entry);
        self.engine = Some(engine.clone());
        *self.frames.lock().map_err(|_| Error::LockPoisoned)? =
            Some((self.view.play_id.clone(), engine));
        self.checkpoint(false)
    }
    async fn tick(&mut self) -> Result<()> {
        let Some(engine) = self.engine.clone() else {
            return self.sync_local().await;
        };
        let snapshot = device(engine, |e| e.poll()).await?;
        self.view.position = snapshot.position.or(self.view.position);
        self.view.duration = snapshot.duration.or(self.view.duration);
        self.view.buffering = snapshot.buffering;
        if let Some(volume) = snapshot
            .volume
            .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
        {
            self.view.volume = volume;
        }
        if let Some(speed) = snapshot
            .speed
            .filter(|v| v.is_finite() && (0.25..=4.0).contains(v))
        {
            self.view.speed = speed;
        }
        if matches!(self.view.status, VideoStatus::Playing | VideoStatus::Paused) {
            self.view.paused = snapshot.paused;
            self.view.status = if snapshot.paused {
                VideoStatus::Paused
            } else {
                VideoStatus::Playing
            };
        }
        for event in snapshot.events {
            if let EngineEvent::Ended {
                entry_id,
                reason,
                error,
            } = event
                && self.entry == Some(entry_id)
            {
                if reason == EndReason::Eof {
                    self.view.status = VideoStatus::Ended;
                    self.view.paused = true;
                    self.view.position = self.view.duration;
                    self.wants_play = false;
                    self.checkpoint(true)?;
                } else if reason == EndReason::Error {
                    return Err(Error::Playback {
                        operation: "video playback",
                        code: error,
                    });
                }
            }
        }
        if self.checkpoint_at.elapsed() > Duration::from_secs(2)
            && matches!(self.view.status, VideoStatus::Playing | VideoStatus::Paused)
        {
            self.checkpoint(false)?;
        }
        self.sync_local().await
    }
    fn failure(&mut self, error: Error) {
        self.local_source = None;
        self.waiting_local = false;
        self.view.status = VideoStatus::Error;
        self.view.buffering = false;
        self.view.paused = true;
        self.view.error = Some(Arc::new(error));
    }
    pub async fn run(
        mut self,
        mut receiver: mpsc::UnboundedReceiver<Message>,
        publisher: watch::Sender<VideoSnapshot>,
    ) {
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if !self.valid() {
                break;
            }
            tokio::select! {
                message=receiver.recv()=>match message {
                    Some(Message::Command(command,reply))=>{
                        if reply.is_closed(){continue;}
                        if !self.valid(){let _=reply.send(Err(Error::StaleResponse));break;}
                        let result=self.command(command).await;self.publish(&publisher);let _=reply.send(result.map(|()|self.view.clone()));
                    },
                    Some(Message::Loaded(generation,result))=>{if let Err(error)=self.loaded(generation,*result).await{let _=self.release_engine().await;self.failure(error);}self.publish(&publisher);},
                    Some(Message::Loading(generation))=>{if generation==self.generation.load(Ordering::Acquire){self.view.status=VideoStatus::Loading;self.publish(&publisher);}},
                    Some(Message::Local(generation,source))=>{if generation==self.generation.load(Ordering::Acquire){self.local_source=Some(source);self.view.local=true;}},
                    Some(Message::Shutdown)|None=>break,
                },
                _=tick.tick()=>{if let Err(error)=self.tick().await{self.failure(error);let _=self.release_engine().await;}self.publish(&publisher);},
            }
        }
        let _ = self.checkpoint(self.view.status == VideoStatus::Ended);
        self.cancel_load();
        let _ = self.release_engine().await;
        self.view.status = VideoStatus::Idle;
        self.view.detail = None;
        self.view.id = None;
        self.view.play_id.clear();
        self.view.paused = true;
        self.view.buffering = false;
        self.publish(&publisher);
    }
}
