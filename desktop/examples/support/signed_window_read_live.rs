//! Optional single-window live arm. The launcher supplies the broker and jail.
use super::{
    ProofContext, STAGES, audio, capture_ack, prepare_context, sandbox,
    signed_window_read_gate::{Facts, Gate},
};
use cumulus_core::{
    Error,
    account::{SessionStatus, credentials::Credentials},
    client::{MusicClient, dto::PlayerView},
    download::transfer::{MediaBackend, MediaReply, MediaRequest, ReqwestMediaBackend},
    playback::{TimelineMode, controller::PlaybackStatus},
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{APP_VERSION, ApiOrigin, HttpBackend, HttpReply, RequestPlan, ReqwestBackend},
        mconfig::{ConfigBackend, ConfigUrl, PcConfig},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tauri::{Listener, Manager, webview::PageLoadEvent};

#[derive(Default)]
struct ClosedServices {
    auxiliary: AtomicUsize,
    other_media: AtomicUsize,
}
impl TokenProvider for ClosedServices {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        self.auxiliary.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Err(Error::State("proof SDK is closed")) })
    }
}
impl ConfigBackend for ClosedServices {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        self.auxiliary.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Err(Error::State("proof config download is closed")) })
    }
}
impl MediaBackend for ClosedServices {
    fn get(&self, _: MediaRequest) -> ApiFuture<'_, MediaReply> {
        self.other_media.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Err(Error::State("proof ordinary downloads are closed")) })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Read {
    Account,
    Playlist,
    Likes,
    DefaultWord,
    Assignment,
    Tabs,
    Comprehensive,
    Songs,
    Detail,
    Source,
}

struct ApiScope {
    backend: ReqwestBackend,
    reads: Mutex<Vec<Read>>,
}
impl ApiScope {
    fn account_resources_first(&self) -> bool {
        let Ok(reads) = self.reads.lock() else {
            return false;
        };
        let search = reads
            .iter()
            .position(|read| *read == Read::Assignment)
            .unwrap_or(reads.len());
        reads.first() == Some(&Read::Account)
            && reads[..search].contains(&Read::Playlist)
            && reads[..search].contains(&Read::Likes)
            && reads[search..]
                .iter()
                .all(|read| !matches!(read, Read::Playlist | Read::Likes))
    }
}
impl HttpBackend for ApiScope {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let read = match plan.url.path() {
                "/eapi/w/nuser/account/get" => Read::Account,
                "/eapi/user/playlist" => Read::Playlist,
                "/eapi/song/like/get" => Read::Likes,
                "/eapi/search/default/keyword/get" => Read::DefaultWord,
                "/eapi/rtrs/abt/front/expinfo/list" => Read::Assignment,
                "/eapi/search/pc/result/tab" => Read::Tabs,
                "/eapi/search/pc/complex/page" | "/eapi/search/pc/complex/page/v3" => {
                    Read::Comprehensive
                }
                "/eapi/search/song/list/page" => Read::Songs,
                "/eapi/v3/song/detail" => Read::Detail,
                "/eapi/song/enhance/player/url/v1" => Read::Source,
                _ => return Err(Error::State("proof backend refused a route")),
            };
            self.reads
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .push(read);
            self.backend.post(plan).await
        })
    }
}

struct MediaScope {
    backend: ReqwestMediaBackend,
    host: String,
    reads: AtomicUsize,
}
impl MediaBackend for MediaScope {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let url = url::Url::parse(request.url.as_str())
                .map_err(|_| Error::State("proof media URL is invalid"))?;
            if url.scheme() != "https"
                || url.host_str() != Some(self.host.as_str())
                || request.offset != 0
                || request.if_range.is_some()
                || self.reads.fetch_add(1, Ordering::AcqRel) != 0
            {
                return Err(Error::State(
                    "proof media escaped the one approved authority",
                ));
            }
            self.backend.get(request).await
        })
    }
}

#[derive(Default)]
struct Stages {
    count: usize,
    epoch: Option<String>,
    selected: Option<(String, String)>,
    clocks: [f64; 3],
}

impl Stages {
    fn accept_account(&mut self, client: &MusicClient, uid: &str) -> Option<String> {
        let a = client.account().ok()?;
        if a.status != SessionStatus::Authenticated
            || a.profile_shortcut.as_ref().is_none_or(|p| p.id != uid)
        {
            return None;
        }
        let epoch = self.epoch.get_or_insert_with(|| a.epoch.clone());
        (epoch == &a.epoch).then_some(a.epoch)
    }

    fn sample(
        &self,
        player: &PlayerView,
        stage: PlaybackStatus,
        epoch: &str,
        song: &str,
    ) -> Option<((String, String), f64)> {
        if player.epoch != epoch
            || player.state != stage
            || player.queue_len != 1
            || player.error.is_some()
            || !player.search_origin
            || player.track.as_ref().is_none_or(|track| track.id != song)
            || player
                .timeline
                .as_ref()
                .is_none_or(|t| t.mode != TimelineMode::Full)
        {
            return None;
        }
        let identity = (player.play_id.clone()?, player.entry_id.clone()?);
        let clock = player.position.filter(|v| v.is_finite() && *v >= 0.25)?;
        if self
            .selected
            .as_ref()
            .is_some_and(|selected| selected != &identity)
        {
            return None;
        }
        Some((identity, clock))
    }

    async fn check_player(
        &mut self,
        client: &MusicClient,
        stage: &str,
        epoch: &str,
        uid: &str,
        song: &str,
    ) -> bool {
        let Ok(player) = client.player_snapshot(epoch).await else {
            return false;
        };
        match stage {
            "ux_playing" => {
                let Some((id, clock)) = self.sample(&player, PlaybackStatus::Playing, epoch, song)
                else {
                    return false;
                };
                self.selected = Some(id);
                self.clocks[0] = clock;
                true
            }
            "ux_paused" => {
                let Some((_, clock)) = self.sample(&player, PlaybackStatus::Paused, epoch, song)
                else {
                    return false;
                };
                tokio::time::sleep(Duration::from_millis(350)).await;
                if self.accept_account(client, uid).as_deref() != Some(epoch) {
                    return false;
                }
                let Ok(next) = client.player_snapshot(epoch).await else {
                    return false;
                };
                let Some((_, fixed)) = self.sample(&next, PlaybackStatus::Paused, epoch, song)
                else {
                    return false;
                };
                self.clocks[1] = fixed;
                fixed >= self.clocks[0] && (clock - fixed).abs() < 0.05
            }
            "ux_resumed" => {
                let Some((_, clock)) = self.sample(&player, PlaybackStatus::Playing, epoch, song)
                else {
                    return false;
                };
                self.clocks[2] = clock;
                clock > self.clocks[1] + 0.15
            }
            "ux_stopped" | "complete" => {
                player.epoch == epoch
                    && player.state == PlaybackStatus::Idle
                    && player.position.is_none()
                    && player.error.is_none()
            }
            _ => false,
        }
    }
}

struct Counters<'a> {
    media: &'a MediaScope,
    native: &'a audio::NativeFacts,
    services: &'a ClosedServices,
}

fn report(
    ok: bool,
    assets: &Value,
    stages: &Stages,
    gate: Facts,
    counters: Counters<'_>,
    resources_first: bool,
) -> Value {
    let Counters {
        media,
        native,
        services,
    } = counters;
    let [playing, paused, resumed] = stages.clocks.map(|value| (value * 1000.0).round() as u64);
    json!({
        "ok":ok,"liveReadOnly":true,"productionWebView":true,"isolatedNetwork":true,
        "physicalAudioOutput":false,"assets":assets,"gate":gate,
        "stages":STAGES[..stages.count.min(STAGES.len())],
        "searchAfterResources":resources_first,
        "pauseHeldSameEntry":stages.count >= 4,"resumedSameEntry":stages.count >= 5,
        "stopped":stages.count >= 6,"fullTimeline":stages.count >= 3,
        "playbackClocksMs":{"playing":playing,"paused":paused,"resumed":resumed},
        "mediaReads":media.reads.load(Ordering::Acquire),
        "anonymousPinnedLoads":native.pinned_loads.load(Ordering::Acquire),
        "observedLibmpvNullClock":native.null_clock.load(Ordering::Acquire) > 0,
        "auxiliaryAttempts":services.auxiliary.load(Ordering::Acquire),
        "otherMediaAttempts":services.other_media.load(Ordering::Acquire),
    })
}

pub fn run() -> i32 {
    std::panic::set_hook(Box::new(|_| {}));
    if std::env::args_os().count() != 2 {
        return 2;
    }
    match sandbox::admit() {
        Ok(prepared) => {
            if start(prepared).is_ok() {
                0
            } else {
                let _ = sandbox::write_report(
                    &json!({"ok":false,"liveReadOnly":true,"initialized":false}),
                );
                1
            }
        }
        Err(_) => 2,
    }
}

fn start(prepared: sandbox::Prepared) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let ProofContext {
        context,
        assets,
        window,
    } = prepare_context()?;
    let params = Arc::new(prepared.params);
    let renderer = json!({"live":true,"keyword":params.keyword,"songId":params.song_id});
    let script = format!(
        "window.__signedWindowInput={renderer};\n{}",
        capture_ack::script(include_str!("../signed_window_read_ui_probe.js"))
    );
    let api = Arc::new(ApiScope {
        backend: ReqwestBackend::for_signed_read_proof(Duration::from_secs(8), prepared.broker)?,
        reads: Mutex::default(),
    });
    let media = Arc::new(MediaScope {
        backend: ReqwestMediaBackend::for_signed_read_proof(
            Duration::from_secs(12),
            prepared.broker,
        )?,
        host: params.media_host.clone(),
        reads: AtomicUsize::new(0),
    });
    let services = Arc::new(ClosedServices::default());
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(&prepared.credential_dir)?),
        ApiOrigin::Pc,
    )?);
    let config = PcConfig::new(
        Arc::new(StateStore::open(&prepared.config_dir)?),
        services.clone(),
    )?;
    let request_context = Arc::new(RequestContext::from_pc(
        config.clone(),
        APP_VERSION,
        services.clone(),
    )?);
    let gate = Arc::new(Gate::new(
        api.clone(),
        credentials,
        params.profile(),
        &config.snapshot()?,
        params.expected_uid.clone(),
        params.keyword.clone(),
        params.song_id.clone(),
    )?);
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let native = Arc::new(audio::NativeFacts::default());
            let (audio_gate, audio_media, audio_facts, audio_root) = (gate.clone(), media.clone(), native.clone(), prepared.work.clone());
            let client = MusicClient::open_with_request_context(
                &prepared.work.join("state"), Some(&prepared.credential_dir), params.profile(), Some(gate.clone()), Some(request_context),
            )?
            .with_official_search_experiment()
            .with_download_backend(services.clone())
            .with_audio_factory(move || Ok(Box::new(audio::Audio::open(audio_gate.clone(), audio_media.clone(),
                audio_facts.clone(), audio_root.clone(), audio::Expectation::Live)?)));
            app.manage(client);
            app.add_capability(r#"{"identifier":"signed-live-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let app_handle = app.handle().clone();
            let progress = Arc::new(tokio::sync::Mutex::new(Stages::default()));
            app.listen("ux-proof", move |event| {
                let Ok(value) = serde_json::from_str::<Value>(event.payload()) else { return; };
                let Some(stage) = value["stage"].as_str() else { return; };
                if !STAGES.contains(&stage) && !["complete", "failed"].contains(&stage) { return; }
                let stage = stage.to_owned();
                let (app, progress, params, api, media, gate, native, services, config, assets) = (
                    app_handle.clone(), progress.clone(), params.clone(), api.clone(), media.clone(), gate.clone(), native.clone(), services.clone(), config.clone(), assets.clone(),
                );
                tauri::async_runtime::spawn(async move {
                    let mut progress = progress.lock().await;
                    let client = app.state::<MusicClient>();
                    let resources_first = api.account_resources_first();
                    let observed = gate.facts().await;
                    let expected = STAGES.get(progress.count).copied().unwrap_or("complete");
                    let mut ok = stage == expected && observed.expected_account_verified
                        && observed.playlist_count.is_some() && observed.liked_count.is_some() && resources_first
                        && !observed.closed_after_failure && services.auxiliary.load(Ordering::Acquire) == 0
                        && services.other_media.load(Ordering::Acquire) == 0;
                    if progress.count >= 1 {
                        ok &= observed.selected_song_found && observed.comprehensive_read
                            && observed.search_v3.is_some_and(|v3| observed.search_tab_read == v3);
                    }
                    if progress.count >= 2 {
                        ok &= observed.song_detail_read && observed.media_consumed
                            && media.reads.load(Ordering::Acquire) == 1
                            && native.pinned_loads.load(Ordering::Acquire) == 1
                            && native.null_clock.load(Ordering::Acquire) > 0;
                    }
                    if ok {
                        ok = match progress.accept_account(&client, &params.expected_uid) {
                            Some(epoch) if progress.count >= 2 => progress.check_player(
                                &client, &stage, &epoch, &params.expected_uid, &params.song_id,
                            ).await,
                            Some(_) => true,
                            None => false,
                        };
                    }
                    if !ok || stage == "complete" {
                        let closed = config.close().is_ok();
                        let proof = report(ok && closed, &assets, &progress, observed,
                            Counters { media: &media, native: &native, services: &services }, resources_first);
                        let written = sandbox::write_report(&proof).is_ok();
                        app.exit(if ok && closed && written { 0 } else { 1 });
                        return;
                    }
                    progress.count += 1;
                    let acknowledgement = format!("window.__CUMULUS_PROOF_ACK__={};", json!(stage));
                    if app.get_webview_window("main").is_none_or(|view| view.eval(acknowledgement).is_err()) {
                        app.exit(1);
                    }
                });
            });
            tauri::WebviewWindowBuilder::from_config(app, &window)?
                .data_directory(prepared.work.join("webview"))
                .inner_size(1120.0, 760.0)
                .build()?;
            Ok(())
        })
        .on_page_load(move |view, page| {
            if page.event() == PageLoadEvent::Finished && view.eval(&script).is_err() {
                view.app_handle().exit(1);
            }
        })
        .run(context)?;
    Ok(())
}
