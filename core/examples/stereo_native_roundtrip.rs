//! Real dynamic SE1 ABI + actual libmpv decoder-format observation.
//! Synthetic media/null output only; SE1 is NOT installed in the decoder graph.
use cumulus_core::{
    Error, Result,
    effects::{
        dsp::{Format, PlanarBlock, StereoAmbienceOneStage},
        stereo::{StereoHost, StereoInstance, StereoSettings, abi},
    },
    playback::engine::{
        AudioEngine, Cancellation, DecodedAudioFormat, LoadRequest, MediaInput, MpvConfig,
        MpvEngine,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

const RATE: u32 = 48_000;

fn settings(enabled: bool, presence: f32, width: f32) -> Result<StereoSettings> {
    StereoSettings::new(enabled, presence, width, true)
}

fn configure_reference(stage: &mut StereoAmbienceOneStage, value: StereoSettings) -> Result<()> {
    stage.set_controls(value.controls())?;
    stage.set_bypass(!value.enabled())
}

fn synthetic_pcm(block: &mut PlanarBlock, index: usize) {
    let cap = block.format().capacity();
    for frame in 0..cap {
        // Exact binary fractions keep source generation independent of libm.
        block.samples_mut()[frame] = ((frame + index * 3) % 31) as f32 / 64.0 - 0.25;
        block.samples_mut()[cap + frame] = ((frame * 3 + index * 7) % 37) as f32 / 128.0 - 0.125;
    }
}

struct ComparedBlock {
    frames: usize,
    end: bool,
    nonzero: bool,
    sha256: String,
}

fn compare_block(
    instance: &mut StereoInstance,
    reference: &mut StereoAmbienceOneStage,
    actual: &mut PlanarBlock,
    expected: &mut PlanarBlock,
    frames: usize,
    end: bool,
) -> Result<ComparedBlock> {
    expected.samples_mut().copy_from_slice(actual.samples());
    let wanted = reference.process(expected, frames, end)?;
    let got = instance.process(actual, frames, end)?;
    assert_eq!(got, wanted, "dynamic SE1 frame/EOF protocol");
    let cap = actual.format().capacity();
    let mut hash = Sha256::new();
    let mut nonzero = false;
    for channel in 0..2 {
        for frame in 0..got.valid_frames {
            let index = channel * cap + frame;
            let value = actual.samples()[index];
            assert_eq!(
                value.to_bits(),
                expected.samples()[index].to_bits(),
                "dynamic SE1 valid PCM ch{channel} frame{frame}"
            );
            nonzero |= value != 0.0;
            hash.update(value.to_bits().to_le_bytes());
        }
    }
    Ok(ComparedBlock {
        frames: got.valid_frames,
        end: got.end,
        nonzero,
        sha256: hex::encode(hash.finalize()),
    })
}

fn dynamic_component(library: &Path) -> Result<Value> {
    let initial = settings(true, 0.0, 5.0)?;
    // This opens a separately built module through libloading, never linked
    // exports from this executable or a function-pointer test substitute.
    let mut host = StereoHost::open(library, initial)?;
    let created = host.stats()?;
    assert_eq!(created.revision, 1);
    assert_eq!(created.observed_revision, 0);
    assert_eq!(created.active_instances, 0);
    assert_eq!(created.input_frames, 0);
    assert_eq!(created.output_frames, 0);
    assert!(host.instantiate(RATE, 1).is_err());
    assert!(host.instantiate(RATE, 6).is_err());
    assert!(host.instantiate(0, 2).is_err());
    let mut instance = host.instantiate(RATE, 2)?;
    let unused = host.instantiate(RATE, 2)?;
    assert_eq!(host.stats()?.active_instances, 2);
    drop(unused);
    let bound = host.stats()?;
    assert_eq!(bound.instances_created, 2);
    assert_eq!(bound.active_instances, 1);
    assert_eq!(bound.observed_revision, 0);

    let format = Format::new(RATE as f32, 2, abi::CAPACITY)?;
    let mut reference = StereoAmbienceOneStage::new(format)?;
    configure_reference(&mut reference, initial)?;
    let mut actual = PlanarBlock::new(format);
    let mut expected = PlanarBlock::new(format);
    let mut blocks = Vec::new();
    let mut input_frames = 0_u64;
    let mut output_frames = 0_u64;
    for index in 0..20 {
        let change = match index {
            3 => Some(settings(true, 2.0, 7.0)?),
            5 => Some(settings(false, 2.0, 7.0)?),
            6 => Some(initial),
            _ => None,
        };
        if let Some(change) = change {
            let before = host.stats()?;
            host.update(change)?;
            configure_reference(&mut reference, change)?;
            let after = host.stats()?;
            assert_eq!(after.revision, before.revision + 1);
            assert_eq!(after.observed_revision, before.observed_revision);
            // An identical tuple is not a fresh applied revision.
            host.update(change)?;
            assert_eq!(host.stats()?.revision, after.revision);
        }
        let reset = index == 9;
        if reset {
            instance.reset()?;
            reference.reset()?;
        }
        synthetic_pcm(&mut actual, index);
        let frames = [2048, 2047, 1, 3, 7][index % 5];
        let got = compare_block(
            &mut instance,
            &mut reference,
            &mut actual,
            &mut expected,
            frames,
            false,
        )?;
        assert_eq!(got.frames, frames);
        assert!(!got.end);
        input_frames += frames as u64;
        output_frames += got.frames as u64;
        let stats = host.stats()?;
        assert_eq!(stats.observed_revision, stats.revision);
        assert_eq!(stats.input_frames, input_frames);
        assert_eq!(stats.output_frames, output_frames);
        assert_eq!(stats.failed_blocks, 0);
        blocks.push(json!({"phase":"source","index":index,"inputFrames":frames,
            "outputFrames":got.frames,"end":got.end,"reset":reset,
            "revision":stats.revision,"validPcmSha256":got.sha256}));
    }
    let mut tail_frames = 0;
    let mut tail_nonzero = false;
    let mut ended = false;
    for index in 0..128 {
        // Zero valid input still supplies full initialized planar storage.
        actual.samples_mut().fill(0.0);
        let got = compare_block(
            &mut instance,
            &mut reference,
            &mut actual,
            &mut expected,
            0,
            true,
        )?;
        tail_frames += got.frames;
        output_frames += got.frames as u64;
        tail_nonzero |= got.nonzero;
        blocks.push(json!({"phase":"eof","index":index,"inputFrames":0,
            "outputFrames":got.frames,"end":got.end,"validPcmSha256":got.sha256}));
        if got.end {
            ended = true;
            break;
        }
    }
    assert!(ended, "bounded explicit native EOF drain must finish");
    assert_eq!(tail_frames, RATE as usize, "complete one-second SE1 tail");
    assert!(
        tail_nonzero,
        "tail contains real recovered processing output"
    );
    let completed = host.stats()?;
    assert_eq!(completed.input_frames, input_frames);
    assert_eq!(completed.output_frames, output_frames);
    assert_eq!(completed.resets, 1);
    assert_eq!(completed.instances_created, 2);
    assert_eq!(completed.active_instances, 1);
    assert_eq!(completed.observed_revision, completed.revision);
    assert_eq!(completed.failed_blocks, 0);

    // Releasing config must not unload callbacks used by a retained instance.
    drop(host);
    actual.samples_mut().fill(0.125);
    assert!(instance.process(&mut actual, 64, false).is_err());
    assert!(actual.samples().iter().all(|sample| *sample == 0.0));
    assert!(
        instance.reset().is_err(),
        "revoked instance cannot be revived"
    );
    drop(instance);

    // Both handles are now gone. A fresh dynamic load/config can process again.
    let reopened = StereoHost::open(library, initial)?;
    let mut reopened_instance = reopened.instantiate(RATE, 2)?;
    let mut reopened_reference = StereoAmbienceOneStage::new(format)?;
    configure_reference(&mut reopened_reference, initial)?;
    synthetic_pcm(&mut actual, 0);
    compare_block(
        &mut reopened_instance,
        &mut reopened_reference,
        &mut actual,
        &mut expected,
        abi::CAPACITY as usize,
        false,
    )?;
    assert_eq!(reopened.stats()?.active_instances, 1);
    drop(reopened_instance);
    assert_eq!(reopened.stats()?.active_instances, 0);
    drop(reopened);
    Ok(
        json!({"ok":true,"realDynamicLibrary":true,"versionCheckedByHost":abi::VERSION,
        "librarySha256":hex::encode(Sha256::digest(std::fs::read(library)?)),
        "sampleRate":RATE,"capacity":abi::CAPACITY,"channels":2,"blocks":blocks,
        "tailFrames":tail_frames,"tailNonzero":tail_nonzero,"completeEof":ended,
        "finalStats":completed,"configurationRevoked":true,"retainedInstanceMuted":true,
        "reopenedFreshInstance":true,"validPcmBitIdenticalToCore":true}),
    )
}

fn wave(path: &Path, channels: u16) -> Result<()> {
    let frames = RATE * 4;
    let bytes = frames * u32::from(channels) * 2;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&RATE.to_le_bytes())?;
    file.write_all(&(RATE * u32::from(channels) * 2).to_le_bytes())?;
    file.write_all(&(channels * 2).to_le_bytes())?;
    file.write_all(&16_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&bytes.to_le_bytes())?;
    for frame in 0..frames {
        for channel in 0..channels {
            let sample = ((frame + u32::from(channel) * 7) % 32) as i16 * 128 - 2048;
            file.write_all(&sample.to_le_bytes())?;
        }
    }
    Ok(())
}

fn wait_format(engine: &mut MpvEngine, entry: i64) -> Result<(DecodedAudioFormat, f64, String)> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let snapshot = engine.poll()?;
        if snapshot.entry_id != Some(entry) {
            return Err(Error::StaleResponse);
        }
        if !snapshot.seeking
            && snapshot.position.is_some_and(|position| position > 0.08)
            && let Some(format) = engine.decoded_audio_format(entry)?
        {
            let driver = snapshot.audio_output.unwrap_or_default();
            if driver != "null" {
                return Err(Error::State(
                    "decoder format proof requires actual null output",
                ));
            }
            return Ok((format, snapshot.position.unwrap_or_default(), driver));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err(Error::State(
        "actual decoder audio format did not become ready",
    ))
}

fn decoded_formats(library: &Path, root: &Path) -> Result<Value> {
    let mono = root.join("mono.wav");
    let stereo = root.join("stereo.wav");
    wave(&mono, 1)?;
    wave(&stereo, 2)?;
    let mut engine = MpvEngine::open(MpvConfig {
        null_output: true,
        ..Default::default()
    })?;
    let version = engine.api_version();
    let format_host = StereoHost::open(library, settings(false, 0.0, 0.0)?)?;
    engine.volume(0.0)?;
    assert!(engine.output_devices()?.is_none());
    let mut cases = Vec::new();
    let mut previous = None;
    for (path, channels) in [(&mono, 1), (&stereo, 2)] {
        let mut request = LoadRequest::new(MediaInput::File(path.clone()));
        request.timeout = Duration::from_secs(5);
        let media = engine.load(request, &Cancellation::default())?;
        assert_eq!(media.codec.as_deref(), Some("pcm_s16le"));
        if let Some(previous) = previous {
            assert!(matches!(
                engine.decoded_audio_format(previous),
                Err(Error::StaleResponse)
            ));
        }
        engine.pause(false)?;
        let (format, position, driver) = wait_format(&mut engine, media.entry_id)?;
        engine.pause(true)?;
        assert_eq!(format.entry_id, media.entry_id);
        assert_eq!(format.sample_rate, RATE);
        assert_eq!(format.channels, channels);
        let qualification = format_host.instantiate(format.sample_rate, format.channels);
        if channels == 1 {
            assert!(
                qualification.is_err(),
                "actual decoded mono cannot qualify SE1"
            );
        } else {
            drop(qualification?);
        }
        cases.push(
            json!({"channels":format.channels,"sampleRate":format.sample_rate,
            "entryId":format.entry_id,"decoder":media.codec,"output":driver,
            "actualPosition":position,"expectedChannels":channels,
            "nativeStereoQualification":channels==2}),
        );
        previous = Some(media.entry_id);
    }
    engine.stop()?;
    assert!(matches!(
        engine.decoded_audio_format(previous.expect("two decoded cases")),
        Err(Error::StaleResponse)
    ));
    let stats = format_host.stats()?;
    assert_eq!(stats.active_instances, 0);
    assert_eq!(stats.instances_created, 1);
    assert_eq!(stats.observed_revision, 0);
    assert_eq!(stats.input_frames, 0);
    assert_eq!(stats.output_frames, 0);
    Ok(json!({"ok":true,"libmpvApi":version,"cases":cases,
        "staleEntryRejected":true,"stoppedEntryRejected":true,
        "monoPreserved":true,"outputDeviceCoercionUsed":false,
        "observedFormatQualification":stats}))
}

pub fn verify_with_observer(
    library: &Path,
    root: &Path,
    mut progress: impl FnMut(&'static str),
) -> Result<Value> {
    std::fs::create_dir(root)?;
    progress("stereo_dynamic_abi_start");
    let component = dynamic_component(library)?;
    progress("stereo_dynamic_abi_verified");
    let formats = decoded_formats(library, root)?;
    progress("stereo_actual_decoder_formats_verified");
    Ok(
        json!({"ok":true,"stereoNativeBoundary":true,"dynamicComponent":component,
        "actualDecodedFormats":formats,"remoteRequests":0,"accountWrites":0,
        "syntheticMediaOnly":true,"nativeNullOutputOnly":true,
        "productionStereoAttachment":false,"wholeMaterialVerified":false,
        "physicalAudioOutputVerified":false,
        "boundary":"Dynamic SE1 output is compared against the frozen-vector-verified core component. Separate real libmpv WAV reads prove decoder format only; SE1 is not attached to that decoder or a complete material graph."}),
    )
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        eprintln!(
            "usage: stereo_native_roundtrip <trusted-filter-library> <new-fixture-directory>"
        );
        std::process::exit(2);
    }
    match verify_with_observer(Path::new(&args[0]), Path::new(&args[1]), |stage| {
        eprintln!("stereo native proof: {stage}");
    }) {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(error) => {
            eprintln!("stereo native proof failed: {error}");
            std::process::exit(1);
        }
    }
}
