// Explicit offline Linux proof, not a Tauri command or a desktop build target.
use cumulus_core::song_recognition::{
    FingerprintPcm8k, ResamplerLibraries, StereoPcmFormat, StereoPcmPacket, StereoSampleRate,
};
use pulse::{
    CaptureExtent, Format, Fragment, PrivateServer, ProbeError,
    selection::{Consent, OutputChoice},
};
use roundtrip::RoundTrip;
use std::{
    env,
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

#[path = "../../src/recognition_capture/mod.rs"]
mod pulse;
mod roundtrip;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    Capture,
    Cancel,
    RoundTrip,
    CancelInFlight,
}

fn main() {
    if let Err(error) = run() {
        match error {
            ProbeError::Rejected(reason) => eprintln!("rejected: {reason}"),
            ProbeError::Cancelled => eprintln!("unexpected cancellation"),
            ProbeError::TimedOut => eprintln!("private capture timed out"),
        }
        std::process::exit(1);
    }
}

fn run() -> pulse::Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 7 {
        return Err(ProbeError::Rejected(
            "expected: runtime peer-pid sink source stock-dir i16|f32 discover|review|capture|cancel|roundtrip|cancel-in-flight",
        ));
    }
    let text = |i: usize| {
        args[i]
            .to_str()
            .ok_or(ProbeError::Rejected("invalid probe option"))
    };
    let pid = text(1)?
        .parse()
        .map_err(|_| ProbeError::Rejected("invalid fixture peer pid"))?;
    let stop = AtomicBool::new(false);
    let options = PrivateServer::new(Path::new(&args[0]), pid)?.discover(&stop)?;
    if text(6)? == "discover" {
        let outputs: Vec<_> = options
            .choices()
            .map(|(_, sink, source)| [sink.to_string_lossy(), source.to_string_lossy()])
            .collect();
        println!("{{\"outputs\":{outputs:?}}}");
        return Ok(());
    }
    let choice: OutputChoice = options
        .choices()
        .find(|(_, sink, source)| {
            sink.to_bytes() == args[2].as_encoded_bytes()
                && source.to_bytes() == args[3].as_encoded_bytes()
        })
        .map(|(choice, _, _)| choice)
        .ok_or(ProbeError::Rejected(
            "not the expected private output monitor",
        ))?;
    let consent = if text(6)? == "review" {
        println!("selection-ready");
        let _ = io::stdout().flush();
        let mut decision = String::new();
        io::stdin()
            .read_line(&mut decision)
            .map_err(|_| ProbeError::Rejected("private capture confirmation unavailable"))?;
        if decision.trim() == "confirm" {
            Consent::Confirmed
        } else {
            Consent::Declined
        }
    } else {
        // The explicitly invoked synthetic modes have their own trusted,
        // non-interactive approval; the application never calls this probe.
        Consent::Confirmed
    };
    let permit = options.decide(choice, consent, &stop)?;
    let (format, native_format) = match text(5)? {
        "i16" => (Format::I16, StereoPcmFormat::I16),
        "f32" => (Format::F32, StereoPcmFormat::F32),
        _ => return Err(ProbeError::Rejected("unsupported test sample format")),
    };
    let mode = match text(6)? {
        "capture" | "review" => Mode::Capture,
        "cancel" => Mode::Cancel,
        "roundtrip" => Mode::RoundTrip,
        "cancel-in-flight" => Mode::CancelInFlight,
        _ => return Err(ProbeError::Rejected("unsupported proof mode")),
    };
    let mut recognition = match mode {
        Mode::RoundTrip | Mode::CancelInFlight => {
            Some(RoundTrip::new(mode == Mode::CancelInFlight)?)
        }
        _ => None,
    };
    let (extent, output_limit) = if recognition.is_some() {
        (CaptureExtent::SevenSeconds, 56_000)
    } else {
        (CaptureExtent::FourSeconds, 32_000)
    };
    // SAFETY: this standalone test is given a previously verified stock
    // library directory by the offline research driver.
    let stock = unsafe { ResamplerLibraries::load_local(Path::new(&args[4])) }
        .map_err(|_| ProbeError::Rejected("stock converter is unavailable"))?;
    let mut converter = stock
        .stream(StereoSampleRate::Hz48000, native_format)
        .map_err(|_| ProbeError::Rejected("stock converter initialization failed"))?;
    let mut output = Vec::with_capacity(output_limit);
    let mut input_frames = 0;
    let captured = thread::scope(|scope| {
        pulse::capture(
            permit,
            format,
            extent,
            &stop,
            || {
                println!("monitor-ready");
                let _ = io::stdout().flush();
                if mode == Mode::Cancel {
                    scope.spawn(|| {
                        thread::sleep(Duration::from_millis(120));
                        stop.store(true, Ordering::Release);
                    });
                }
            },
            |fragment| {
                input_frames += fragment.frames;
                let pcm = decode(format, fragment);
                let samples = match &pcm {
                    Decoded::I16(samples) => converter.push(StereoPcmPacket::I16(samples)),
                    Decoded::F32(samples) => converter.push(StereoPcmPacket::F32(samples)),
                }
                .map_err(|_| ProbeError::Rejected("stock conversion failed"))?;
                if output.len() + samples.len() > output_limit {
                    return Err(ProbeError::Rejected("converted audio exceeds its bound"));
                }
                output.extend_from_slice(samples);
                if let Some(recognition) = recognition.as_mut()
                    && recognition.push(samples)?
                {
                    // Only the isolated synthetic run may take this path.
                    // The Pulse worker still drops its current native fragment.
                    stop.store(true, Ordering::Release);
                }
                Ok(())
            },
        )
    });
    match mode {
        Mode::Cancel if matches!(captured, Err(ProbeError::Cancelled)) => {
            println!("{{\"status\":\"cancelled\",\"produced\":{}}}", output.len());
            return Ok(());
        }
        Mode::Cancel => {
            captured?;
            return Err(ProbeError::Rejected(
                "capture completed despite requested cancellation",
            ));
        }
        Mode::Capture => {
            let frames = captured?;
            if frames != input_frames {
                return Err(ProbeError::Rejected(
                    "private capture frame accounting changed",
                ));
            }
        }
        Mode::RoundTrip | Mode::CancelInFlight => {
            if !matches!(captured, Err(ProbeError::Cancelled)) {
                captured?;
                return Err(ProbeError::Rejected(
                    "recognition fixture did not stop capture",
                ));
            }
            let proof = recognition
                .as_mut()
                .ok_or(ProbeError::Rejected("missing round-trip fixture"))?;
            if mode == Mode::RoundTrip {
                proof.verify_matched()?;
            } else {
                proof.verify_cancelled()?;
            }
        }
    }
    FingerprintPcm8k::new(&output)
        .map_err(|_| ProbeError::Rejected("invalid converted PCM extent"))?;
    if output.len() < 24_000 {
        return Err(ProbeError::Rejected(
            "converted PCM did not reach three seconds",
        ));
    }
    let middle = &output[8_000..24_000];
    let left = amplitude(middle, 440.0);
    let right = amplitude(middle, 880.0);
    let decoy = amplitude(middle, 1760.0);
    // The two source-format-preserving stock mix paths have distinct
    // gain: the frozen DLL mono fixture scales float by 1/sqrt(2) and
    // normalizes integer full-scale downmix to 1/2 per channel.
    let gain = match format {
        Format::I16 => 0.5,
        Format::F32 => std::f64::consts::FRAC_1_SQRT_2,
    };
    let near = |measured: f64, expected: f64| (measured / expected - 1.0).abs() < 0.05;
    if !near(left, 8000.0 * gain) || !near(right, 6000.0 * gain) || decoy >= 250.0 {
        eprintln!("synthetic tone amplitudes: {left:.3}, {right:.3}, decoy {decoy:.3}");
        return Err(ProbeError::Rejected(
            "private waveform did not match the fixture",
        ));
    }
    match recognition {
        Some(proof) => {
            let status = if mode == Mode::RoundTrip {
                "matched"
            } else {
                "cancelled-in-flight"
            };
            println!(
                "{{\"status\":\"{status}\",\"frames\":{input_frames},\"produced\":{},\"tone440\":{left:.3},\"tone880\":{right:.3},\"decoy1760\":{decoy:.3},\"requests\":{},\"peakCounts\":{:?},\"lateCallbackIgnored\":true}}",
                output.len(),
                proof.peak_counts().len(),
                proof.peak_counts(),
            );
        }
        None => println!(
            "{{\"status\":\"captured\",\"frames\":{input_frames},\"produced\":{},\"tone440\":{left:.3},\"tone880\":{right:.3},\"decoy1760\":{decoy:.3}}}",
            output.len(),
        ),
    }
    Ok(())
}

enum Decoded {
    I16(Vec<[i16; 2]>),
    F32(Vec<[f32; 2]>),
}

fn decode(format: Format, fragment: Fragment<'_>) -> Decoded {
    match (format, fragment.bytes) {
        (Format::I16, Some(bytes)) => Decoded::I16(
            bytes
                .chunks_exact(4)
                .map(|c| {
                    [
                        i16::from_le_bytes([c[0], c[1]]),
                        i16::from_le_bytes([c[2], c[3]]),
                    ]
                })
                .collect(),
        ),
        (Format::F32, Some(bytes)) => Decoded::F32(
            bytes
                .chunks_exact(8)
                .map(|c| {
                    [
                        f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                        f32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                    ]
                })
                .collect(),
        ),
        (Format::I16, None) => Decoded::I16(vec![[0; 2]; fragment.frames]),
        (Format::F32, None) => Decoded::F32(vec![[0.0; 2]; fragment.frames]),
    }
}

fn amplitude(samples: &[i16], frequency: f64) -> f64 {
    let step = std::f64::consts::TAU * frequency / 8000.0;
    let (mut cosine, mut sine) = (0.0, 0.0);
    for (i, &sample) in samples.iter().enumerate() {
        let (s, c) = (i as f64 * step).sin_cos();
        sine += f64::from(sample) * s;
        cosine += f64::from(sample) * c;
    }
    2.0 * sine.hypot(cosine) / samples.len() as f64
}
