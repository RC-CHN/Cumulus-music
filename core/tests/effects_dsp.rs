use base64::{Engine, engine::general_purpose::STANDARD};
use cumulus_core::effects::{
    dsp::*,
    material::{self, PeqCall},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io::Read, sync::OnceLock};

#[path = "../examples/support/effects_dsp_wire.rs"]
mod wire;

#[derive(Deserialize)]
struct Vector {
    mode: String,
    label: String,
    input_b64: String,
    output_b64: String,
}
#[derive(Deserialize)]
struct Material {
    mode: String,
    input: Value,
    calls: Value,
}
#[derive(Deserialize)]
struct Corpus {
    vectors: Vec<Vector>,
    materials: Vec<Material>,
}

fn corpus() -> &'static Corpus {
    static CORPUS: OnceLock<Corpus> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let decoder = flate2::read::GzDecoder::new(
            &include_bytes!("fixtures/effects-dsp-official.json.gz")[..],
        );
        let mut bytes = Vec::new();
        decoder
            .take(32 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    })
}

fn check(mode: &str, expected_count: usize) {
    let mut count = 0;
    for vector in corpus().vectors.iter().filter(|case| case.mode == mode) {
        count += 1;
        let input = STANDARD.decode(&vector.input_b64).unwrap();
        let expected = STANDARD.decode(&vector.output_b64).unwrap();
        let actual = wire::render(mode, &input).unwrap();
        let encoded: Vec<_> = actual
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        assert_eq!(encoded.len(), expected.len());
        if mode != "geq-outer" && cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(encoded, expected, "{mode} {} #{count}", vector.label);
        } else {
            // Original GEQ SSE processing has a small observed rounding
            // difference from the recovered C scalar matrix implementation.
            for (actual, expected) in actual.iter().zip(expected.chunks_exact(4)) {
                let expected = f32::from_le_bytes(expected.try_into().unwrap());
                assert!(
                    actual.is_finite() && (actual - expected).abs() <= 2e-6,
                    "{mode} {} #{count}: {actual} vs {expected}",
                    vector.label
                );
            }
        }
    }
    assert_eq!(count, expected_count);
}

#[test]
fn official_graphic_and_parametric_coefficient_vectors() {
    check("coeff", 648);
    check("peq-coeff", 792);
}
#[test]
fn official_biquad_alignment_reconfiguration_and_reset_streams() {
    check("biquad", 9);
}
#[test]
fn official_graphic_eq_bypass_padding_and_ftz_streams() {
    check("geq-outer", 28);
}
#[test]
fn official_bass_treble_bypass_clamp_and_ftz_streams() {
    check("bt", 28);
}
#[test]
fn official_peq_all_types_slots_dynamic_gain_and_lfe_streams() {
    check("peq", 117);
}

#[test]
fn official_material_type_defaults_order_and_inherited_locals() {
    assert_eq!(corpus().materials.len(), 51);
    for case in &corpus().materials {
        let actual = match case.mode.as_str() {
            "peq" => serde_json::to_value(material::peq_calls(&case.input).unwrap()).unwrap(),
            "rotate" => {
                serde_json::to_value(material::rotation_calls(&case.input).unwrap()).unwrap()
            }
            "stereo" => serde_json::to_value(material::stereo_calls(&case.input).unwrap()).unwrap(),
            _ => unreachable!(),
        };
        assert_eq!(actual, case.calls, "{} {:?}", case.mode, case.input);
    }
}

#[test]
fn material_branch_selection_is_distinct_from_fragment_enumeration() {
    let legacy = json!({"peq":{"on":true},"se":{"on":true},"rotate":{"on":true}});
    assert_eq!(material::peq_sections(&legacy).unwrap().len(), 1);
    assert_eq!(material::spatial_sections(&legacy).unwrap().len(), 2);
    let mut modern = legacy;
    modern["aev2"] = json!({});
    assert!(material::spatial_sections(&modern).unwrap().is_empty());
    assert_eq!(material::peq_sections(&modern).unwrap().len(), 1);
    modern["aev2"] = json!({"peq":{"on":false},"se":{"on":false}});
    assert_eq!(material::peq_sections(&modern).unwrap().len(), 2);
    assert_eq!(material::spatial_sections(&modern).unwrap()[0].0, "aev2.se");
    assert!(material::peq_calls(&json!([])).is_err());
    assert!(material::peq_calls(&json!({"on":true,"gain":1e100})).is_err());
    let mut wire = Vec::new();
    for value in [48000u32, 2, 13, 1, 13] {
        wire.extend(value.to_le_bytes());
    }
    for _ in 0..26 {
        wire.extend(0.1f32.to_le_bytes());
    }
    let output = wire::render_peq_material(&modern, "aev2.peq", &wire).unwrap();
    assert_eq!(output.len(), 26);
    assert!(wire::render_peq_material(&modern, "absent", &wire).is_err());
}

fn input_block(format: Format) -> PlanarBlock {
    let mut block = PlanarBlock::new(format);
    for (i, sample) in block.samples_mut().iter_mut().enumerate() {
        *sample = ((i as f32) * 0.07).sin() * 0.1;
    }
    block
}

#[test]
fn invalid_peq_plan_and_invalid_setters_leave_live_history_unchanged() {
    let format = Format::new(48000.0, 2, 13).unwrap();
    let mut actual = ParametricEq::new(format, false, true).unwrap();
    let mut reference = ParametricEq::new(format, false, true).unwrap();
    for _ in 0..2 {
        actual.process(&mut input_block(format), 13).unwrap();
        reference.process(&mut input_block(format), 13).unwrap();
    }
    let bad = [
        PeqCall::Gain { value: 9.0 },
        PeqCall::Band {
            band: 2,
            kind: 100,
            gain: 0.0,
            freq: 1000.0,
            q: 1.0,
            on: true,
        },
    ];
    assert!(material::apply_peq(&mut actual, &bad).is_err());
    assert!(actual.set_gain(f32::NAN).is_err());
    assert!(
        actual
            .set_band(
                usize::MAX,
                ParametricBand {
                    kind: ParametricKind::Peak,
                    gain_db: 0.0,
                    frequency: 1000.0,
                    q: 1.0,
                    enabled: true
                }
            )
            .is_err()
    );
    assert!(
        actual
            .set_band(
                0,
                ParametricBand {
                    kind: ParametricKind::Peak,
                    gain_db: 6.0,
                    frequency: 1000.0,
                    q: 0.0,
                    enabled: true
                }
            )
            .is_err()
    );
    // Native outer setter ignores out-of-range slots, including wrapped -1.
    material::apply_peq(
        &mut actual,
        &[PeqCall::Band {
            band: u32::MAX,
            kind: u32::MAX,
            gain: 1.0,
            freq: 1.0,
            q: 0.0,
            on: true,
        }],
    )
    .unwrap();
    let (mut a, mut b) = (input_block(format), input_block(format));
    actual.process(&mut a, 13).unwrap();
    reference.process(&mut b, 13).unwrap();
    assert_eq!(a.samples(), b.samples());
    let calls =
        material::peq_calls(&json!({"on":true,"gain":6,"f":[{"band":0,"type":1,"freq":20,"q":0}]}))
            .unwrap();
    material::apply_peq(&mut actual, &calls).unwrap();
    actual.process(&mut a, 13).unwrap();
    assert_ne!(a.samples(), b.samples());
}

#[test]
fn dimensions_bad_pcm_and_failed_updates_do_not_advance_graphic_state() {
    let format = Format::new(48000.0, 6, 13).unwrap();
    let mut actual = GraphicEq::new(format).unwrap();
    let mut reference = GraphicEq::new(format).unwrap();
    let mut wrong = input_block(Format::new(48000.0, 2, 13).unwrap());
    assert!(actual.process(&mut wrong, 13).is_err());
    let mut good = input_block(format);
    let before = good.samples().to_vec();
    assert!(actual.process(&mut good, 14).is_err());
    assert_eq!(good.samples(), before);
    good.samples_mut()[2] = f32::INFINITY;
    assert!(actual.process(&mut good, 13).is_err());
    let mut gains = [6.0; 10];
    gains[9] = f32::NAN;
    assert!(actual.set_gains(&gains).is_err());
    let (mut a, mut b) = (input_block(format), input_block(format));
    actual.process(&mut a, 13).unwrap();
    reference.process(&mut b, 13).unwrap();
    assert_eq!(a.samples(), b.samples());
}

#[test]
fn interleaved_conversion_zero_pads_and_checks_bounds() {
    let format = Format::new(48000.0, 2, 13).unwrap();
    let mut block = PlanarBlock::new(format);
    let input = [1.0, -1.0, 0.5, -0.5, 0.25, -0.25];
    assert_eq!(block.load_interleaved(&input).unwrap(), 3);
    assert_eq!(&block.samples()[..3], &[1.0, 0.5, 0.25]);
    assert_eq!(&block.samples()[13..16], &[-1.0, -0.5, -0.25]);
    assert!(block.samples()[3..13].iter().all(|value| *value == 0.0));
    let mut output = [0.0; 6];
    block.copy_interleaved(3, &mut output).unwrap();
    assert_eq!(input, output);
    let before = block.samples().to_vec();
    assert!(block.load_interleaved(&[1.0]).is_err());
    assert!(block.load_interleaved(&[f32::NAN, 0.0]).is_err());
    assert_eq!(block.samples(), before);
    assert!(block.copy_interleaved(14, &mut output).is_err());
    for (rate, channels, capacity) in [
        (0.0, 2, 13),
        (f32::NAN, 2, 13),
        (48000.0, 0, 13),
        (48000.0, 9, 13),
        (48000.0, 2, 0),
        (48000.0, 2, 4097),
    ] {
        assert!(Format::new(rate, channels, capacity).is_err());
    }
}

#[test]
fn non_finite_output_is_silenced_and_failed_stage_requires_recreation() {
    let format = Format::new(48000.0, 2, 13).unwrap();
    let mut stage = BassTreble::new(format).unwrap();
    stage
        .configure(BassTrebleSettings {
            bass_db: 24.0,
            treble_db: 24.0,
            ..Default::default()
        })
        .unwrap();
    let mut block = PlanarBlock::new(format);
    block.samples_mut().fill(f32::MAX);
    assert!(stage.process(&mut block, 13).is_err());
    assert!(block.samples().iter().all(|value| *value == 0.0));
    assert!(stage.process(&mut block, 13).is_err());
    // Ownership can move to an audio thread; processing remains exclusive.
    let mut fresh = BassTreble::new(format).unwrap();
    std::thread::spawn(move || fresh.process(&mut block, 13).unwrap())
        .join()
        .unwrap();
}
