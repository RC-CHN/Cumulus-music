use super::{Offline, ensure, observed, seed, state, until};
use cumulus_core::{
    Error, Result,
    account::SessionStatus,
    client::MusicClient,
    effects::stereo::LocalStereoMaterialPlan,
    playback::engine::{
        AudioEngine, Cancellation, ColdStereoPass, ColdStereoSelection, ColdStereoSource,
        ColdStereoStart, EndReason, EngineEvent, MpvConfig, MpvEngine,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

const RATE: u32 = 44100;
const TOTAL: u64 = 88207;
const SEEK: u64 = 22050;
fn plan(presence: i32) -> Result<LocalStereoMaterialPlan> {
    LocalStereoMaterialPlan::from_equalizer_json(
        &json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":true,"presence":presence,"stereoizer":5,"sshaper":true,"ambience":1}}),
    )
}
fn pass(source: ColdStereoSource, first: u64) -> Result<ColdStereoPass> {
    match source.start(first, &Cancellation::default())? {
        ColdStereoStart::Ready(pass) => Ok(*pass),
        ColdStereoStart::Stock(_) => Err(Error::State(
            "indexed native PCM fixture unexpectedly selected stock",
        )),
    }
}
fn position(view: &cumulus_core::playback::engine::EngineSnapshot, entry: i64) -> Option<f64> {
    view.position
        .filter(|_| view.entry_id == Some(entry) && !view.seeking)
}
fn check_lead(control: &cumulus_core::playback::engine::StereoPcmControl) -> Result<()> {
    let value = control.snapshot()?;
    ensure(
        value.produced_frames <= value.native_output_frame + u64::from(RATE.div_ceil(4)) + 2048,
        "native clock render lead exceeded its bound",
    )
}

pub fn verify(
    library: &Path,
    root: &Path,
    mut progress: impl FnMut(&'static str),
) -> Result<Value> {
    ensure(
        library.is_absolute() && library.is_file(),
        "explicit trusted FLAC library required",
    )?;
    ensure(!root.exists(), "stereo PCM proof root must be new")?;
    fs::create_dir_all(root)?;
    let mut stages = Vec::new();
    let mut stage = |value: &'static str| {
        stages.push(value);
        progress(value);
    };
    stage("stereo_pcm_seed_start");
    let manifest: Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/stereo-pcm-source.manifest.json"
    ))?;
    let frozen = include_bytes!("../../../tests/fixtures/stereo-pcm-source.flac");
    ensure(
        manifest["flacSha256"] == hex::encode(Sha256::digest(frozen)),
        "synthetic PCM FLAC hash mismatch",
    )?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (publication, original) =
        runtime.block_on(seed::seed_with_path(root, seed::SeedFormat::NcmStereoPcm))?;
    ensure(
        publication["published"] == true
            && publication["ncm"] == true
            && publication["mediaTransfers"] == 1,
        "synthetic normal download did not publish one NCM",
    )?;
    let id = publication["id"]
        .as_str()
        .ok_or(Error::State("missing published fixture ID"))?;
    stage("stereo_pcm_download_published");
    let offline = Arc::new(Offline::default());
    let cold = MusicClient::open_with_http_backend(&root.join("state"), None, offline.clone())?;
    ensure(
        cold.account()?.status == SessionStatus::Unknown,
        "cold fixture must remain unverified online",
    )?;
    let grant = Arc::new(cold.grant_cold_offline()?);
    let page = grant.list_audio(None, 64)?;
    ensure(
        page.rows
            .iter()
            .any(|row| row.id == id && row.actual_quality == "lossless"),
        "published NCM missing from actual cold library",
    )?;
    let reader = grant.open_flac_reader(id, Cancellation::default())?;
    let ColdStereoSelection::Stock(reader) =
        ColdStereoSource::select(reader, &root.join("unavailable-flac-runtime"), plan(3)?)?
    else {
        return Err(Error::State(
            "missing trusted runtime did not preserve stock source",
        ));
    };
    let ColdStereoSelection::Qualified(source) =
        ColdStereoSource::select(reader, library, plan(3)?)?
    else {
        return Err(Error::State(
            "real stereo FLAC fixture failed source qualification",
        ));
    };
    ensure(source.total_frames() == TOTAL, "source frame count changed")?;
    stage("stereo_pcm_cold_grant_opened");
    ensure(
        original.extension().is_some_and(|x| x == "ncm"),
        "authoritative publication was not a synthetic NCM",
    )?;
    fs::rename(&original, original.with_extension("held-retired"))?;
    fs::write(
        &original,
        b"replacement path cannot become authorized audio",
    )?;
    ensure(
        grant.open_flac_reader(id, Cancellation::default()).is_err(),
        "new path open ignored the production ledger hash",
    )?;
    let mut initial = pass(*source, 0)?;
    let control = initial.control();
    let mut engine = MpvEngine::open(MpvConfig {
        null_output: true,
        ..Default::default()
    })?;
    stage("stereo_pcm_native_load_start");
    let old = engine.load_stereo_pcm(initial.take_reader()?, &Cancellation::default())?;
    ensure(
        old.codec.as_deref() == Some("pcm_f32le") && old.duration.is_none(),
        "native sink invented a finite PCM length or wrong codec",
    )?;
    ensure(engine.poll()?.paused, "source load lost pause")?;
    let revision = control.configure(plan(-3)?)?;
    control.wait_applied(revision, Duration::from_secs(2))?;
    let paused = control.snapshot()?;
    ensure(
        paused.native_entry == Some(old.entry_id)
            && paused.paused
            && paused.applied_revision == revision,
        "paused same-instance control was not acknowledged",
    )?;
    ensure(
        paused.native_output_frame == 0 && !paused.graph_end && paused.decoded_source_frame < TOTAL,
        "paused startup decoded the entire short source",
    )?;
    ensure(
        paused.produced_frames <= u64::from(RATE.div_ceil(8)) + 4096,
        "startup credit exceeded its first-packet bound",
    )?;
    stage("stereo_pcm_paused_controls_verified");
    engine.pause(false)?;
    let mut decoded_format = Value::Null;
    let before = until("initial_playing_clock", |last| {
        let view = engine.poll()?;
        *last = observed(&view, &control)?;
        check_lead(&control)?;
        if let Some(format) = engine.decoded_audio_format(old.entry_id)? {
            ensure(
                format.sample_rate == RATE && format.channels == 2,
                "actual native PCM decoder format differs from held source",
            )?;
            decoded_format = json!({"rate":format.sample_rate,"channels":format.channels,"entry":format.entry_id});
        }
        Ok(
            (!decoded_format.is_null() && view.audio_output.as_deref() == Some("null"))
                .then(|| position(&view, old.entry_id))
                .flatten()
                .filter(|p| *p > 0.15),
        )
    })?;
    let live_revision = control.configure(plan(7)?)?;
    control.wait_applied(live_revision, Duration::from_secs(2))?;
    let after = until("same_entry_after_live_control", |last| {
        let view = engine.poll()?;
        *last = observed(&view, &control)?;
        ensure(
            view.entry_id == Some(old.entry_id),
            "control update reloaded native media",
        )?;
        check_lead(&control)?;
        Ok(position(&view, old.entry_id).filter(|p| *p > before + 0.05))
    })?;
    let live = control.snapshot()?;
    ensure(
        live.applied_revision == live_revision,
        "playing control was not applied",
    )?;
    stage("stereo_pcm_live_controls_verified");
    engine.pause(true)?;
    let paused_at = engine.poll()?;
    ensure(paused_at.paused, "native pause acknowledgement missing")?;
    ensure(
        engine.seek(0.5).is_err(),
        "raw PCM allowed an untracked byte seek",
    )?;
    engine.stop()?;
    until("old_native_reader_closed", |last| {
        let value = control.snapshot()?;
        *last = state(value);
        Ok(value.reader_closed.then_some(()))
    })?;
    let retired = control.snapshot()?;
    ensure(
        retired.native_sink_retired && retired.reader_closed,
        "old sink was not fully retired",
    )?;
    let source = initial.reclaim()?;
    stage("stereo_pcm_stop_close_join_verified");
    let mut resumed = pass(source, SEEK)?;
    let resumed_control = resumed.control();
    let new = engine.load_stereo_pcm(resumed.take_reader()?, &Cancellation::default())?;
    ensure(
        new.entry_id != old.entry_id && engine.poll()?.paused,
        "logical seek did not use a new paused native entry",
    )?;
    ensure(
        resumed_control.snapshot()?.first_source_frame == SEEK,
        "logical seek source coordinate lost",
    )?;
    stage("stereo_pcm_seek_replacement_verified");
    engine.pause(false)?;
    let native_end = until("seek_native_tail_eof", |last| {
        let view = engine.poll()?;
        *last = observed(&view, &resumed_control)?;
        check_lead(&resumed_control)?;
        Ok(view.events.iter().any(|event|matches!(event,EngineEvent::Ended{entry_id,reason:EndReason::Eof,error:0} if *entry_id==new.entry_id)).then_some(()))
    });
    native_end?;
    until("eof_native_reader_closed", |last| {
        let value = resumed_control.snapshot()?;
        *last = state(value);
        Ok(value.reader_closed.then_some(()))
    })?;
    let complete = resumed_control.snapshot()?;
    ensure(
        complete.graph_end && complete.native_sink_retired && complete.reader_closed,
        "native EOF preceded actual producer/reader termination",
    )?;
    ensure(
        complete.produced_frames == TOTAL - SEEK + u64::from(RATE)
            && complete.decoded_source_frame == TOTAL
            && complete.native_source_frame == TOTAL,
        "source/tail counts did not survive raw native EOF",
    )?;
    engine.stop()?;
    let source = resumed.reclaim()?;
    stage("stereo_pcm_native_tail_eof_verified");
    let mut revoked = pass(source, 0)?;
    let revoke_control = revoked.control();
    let revoked_media = engine.load_stereo_pcm(revoked.take_reader()?, &Cancellation::default())?;
    engine.pause(false)?;
    let ahead = until("revocation_buffered_native_clock", |last| {
        let view = engine.poll()?;
        *last = observed(&view, &revoke_control)?;
        let state = revoke_control.snapshot()?;
        Ok(position(&view, revoked_media.entry_id)
            .filter(|p| *p > 0.05 && *p < 1.0)
            .and_then(|_| {
                state
                    .delivered_frames
                    .checked_sub(state.native_output_frame)
            })
            .filter(|n| *n > 2048))
    })?;
    grant.revoke();
    ensure(
        matches!(engine.poll(), Err(Error::Cancelled)),
        "revoked queued PCM was not rejected by native host",
    )?;
    until("revoked_native_reader_closed", |last| {
        let value = revoke_control.snapshot()?;
        *last = state(value);
        Ok(value.reader_closed.then_some(()))
    })?;
    let revoked_state = revoke_control.snapshot()?;
    ensure(
        revoked_state.native_sink_retired && revoked_state.reader_closed,
        "revoked native sink remained live",
    )?;
    ensure(
        revoked.reclaim().is_err(),
        "revoked held source was publishable again",
    )?;
    ensure(
        engine.poll()?.entry_id.is_none(),
        "revoked native entry was retained",
    )?;
    stage("stereo_pcm_buffered_revocation_verified");
    ensure(
        offline.0.load(Ordering::Acquire) == 0,
        "cold native phase attempted HTTP",
    )?;
    drop(engine);
    stage("stereo_pcm_native_closed");
    Ok(json!({
        "ok":true,"kind":"stereo_pcm_native_v1","subgraph":"SE1->defaultB0",
        "fullSdkGraph":false,"wholeMaterial":false,"officialUiTested":false,"physicalAudioOutputVerified":false,
        "guiInitialized":false,"actualDecoder":true,"unknownLength":true,"strictNcm":true,
        "syntheticDownloadPublished":true,"syntheticMediaTransfers":publication["mediaTransfers"],
        "coldHttpAttempts":offline.0.load(Ordering::Acquire),"realMusicRequests":0,
        "format":{"rate":RATE,"channels":2,"bitsPerSample":16,"maxBlockFrames":1024,"sourceFrames":TOTAL},"nativeDecodedFormat":decoded_format,
        "paused":state(paused),"live":state(live),"positionBefore":before,"positionAfter":after,
        "sameControlEntry":old.entry_id,"requestedControls":[revision,live_revision],
        "retired":state(retired),"workerJoinedBeforeSeek":true,"sourcePathReplaced":true,"reopenRejected":true,
        "seek":{"oldEntry":old.entry_id,"newEntry":new.entry_id,"firstSourceFrame":SEEK,"paused":true,"sameHeldSource":true},
        "eof":state(complete),"actualNativeEof":true,"tailFrames":RATE,
        "revoked":state(revoked_state),"revokedEntry":revoked_media.entry_id,"bufferedAheadAtRevocation":ahead,"revokedReclaimDenied":true,
        "stages":stages,
    }))
}
