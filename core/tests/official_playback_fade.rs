use cumulus_core::playback::output::official_fade::{FadeDurations, FadeRamp, FadeTransition};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Oracle {
    default_durations: [u32; 9],
    overridden_durations: [u32; 9],
    traces: Vec<Trace>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Trace {
    name: String,
    duration: u32,
    from: String,
    to: String,
    force_after: usize,
    result: u32,
    emissions: Vec<Emission>,
}

#[derive(Deserialize)]
struct Emission {
    bits: String,
    terminal: bool,
}

fn raw(bits: &str) -> u64 {
    u64::from_str_radix(bits, 16).unwrap()
}

#[test]
fn native_volume_control_ramp_matches_each_recorded_callback_bit_for_bit() {
    let oracle: Oracle =
        serde_json::from_str(include_str!("fixtures/official-playback-fade.json")).unwrap();
    let triggers = [
        FadeTransition::Resume,
        FadeTransition::SeekIn,
        FadeTransition::ManualTrackIn,
        FadeTransition::AutomaticTrackIn,
        FadeTransition::Pause,
        FadeTransition::SeekOut,
        FadeTransition::Stop,
        FadeTransition::ManualTrackOut,
        FadeTransition::AutomaticTrackOut,
    ];
    let mut durations = FadeDurations::default();
    let effective = |durations: &FadeDurations| {
        triggers.map(|trigger| durations.effective_duration(trigger).unwrap_or(0))
    };
    assert_eq!(effective(&durations), oracle.default_durations);
    durations.apply_override(FadeTransition::SeekIn, 79);
    durations.apply_override(FadeTransition::SeekIn, 0);
    durations.apply_override(FadeTransition::SeekOut, -1);
    assert_eq!(effective(&durations), oracle.overridden_durations);

    for trace in oracle.traces {
        let ramp = FadeRamp::new(
            f64::from_bits(raw(&trace.from)),
            f64::from_bits(raw(&trace.to)),
            trace.duration,
        );
        assert_eq!(ramp.is_some(), trace.result != 0, "{}", trace.name);
        let Some(mut ramp) = ramp else {
            assert!(trace.emissions.is_empty());
            continue;
        };
        assert_eq!(trace.result, trace.duration);
        for (i, tick) in trace.emissions.into_iter().enumerate() {
            let actual = if trace.force_after != 0 && i == trace.force_after {
                ramp.finish()
            } else {
                ramp.next()
            }
            .expect("missing expected native callback");
            assert_eq!(actual.terminal, tick.terminal, "{} at tick {i}", trace.name);
            assert_eq!(
                actual.logical_volume.to_bits(),
                raw(&tick.bits),
                "{} at tick {i}",
                trace.name
            );
        }
        assert_eq!(ramp.next(), None, "extra tick after {}", trace.name);
        assert_eq!(
            ramp.finish(),
            None,
            "extra forced tick after {}",
            trace.name
        );
    }
}

#[test]
fn rejects_non_finite_or_out_of_range_abi_duration() {
    assert!(FadeRamp::new(f64::NAN, 0.8, 600).is_none());
    assert!(FadeRamp::new(0.0, f64::INFINITY, 600).is_none());
    assert!(FadeRamp::new(0.0, 0.8, i32::MAX as u32).is_none());
    let mut table = FadeDurations::default();
    table.apply_override(FadeTransition::Pause, 20);
    assert!(table.effective_duration(FadeTransition::Pause).is_none());
}
