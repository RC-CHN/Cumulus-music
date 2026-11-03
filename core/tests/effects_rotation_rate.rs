use cumulus_core::effects::dsp::{RotationRateConversion, RotationRatePlan};

#[test]
fn both_directions_match_native_soxr_setup_and_frame_capacity() {
    let mut count = 0;
    for line in include_str!("fixtures/effects-rotation-rate-official.tsv").lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_ascii_whitespace().collect();
        assert_eq!(fields.len(), 11);
        let uint = |index: usize| fields[index].parse::<u32>().unwrap();
        let RotationRatePlan::Convert { ingress, egress } = RotationRatePlan::new(uint(0)).unwrap()
        else {
            panic!("non-44.1 kHz input did not route through converters");
        };
        let direction = if uint(1) == 0 { ingress } else { egress };
        assert_eq!(direction.input_rate(), uint(5), "{line}");
        assert_eq!(direction.output_rate(), uint(6), "{line}");
        assert_eq!(RotationRateConversion::CHANNELS, uint(7));
        assert_eq!(RotationRateConversion::SOXR_DATATYPE, uint(8));
        assert_eq!(RotationRateConversion::SOXR_DATATYPE, uint(9));
        assert_eq!(
            RotationRateConversion::SOXR_SCALE,
            fields[10].parse::<f64>().unwrap()
        );
        assert_eq!(
            direction
                .output_capacity(uint(2), fields[3].parse().unwrap())
                .unwrap(),
            uint(4),
            "{line}",
        );
        count += 1;
    }
    assert_eq!(count, 128);
}

#[test]
fn direct_rate_and_unsupported_buffer_extents_never_admit_a_converter() {
    assert_eq!(
        RotationRatePlan::new(44_100).unwrap(),
        RotationRatePlan::Direct
    );
    for bad in [0, 7999, 384_001, u32::MAX] {
        assert!(RotationRatePlan::new(bad).is_err());
    }
    let RotationRatePlan::Convert { ingress, .. } = RotationRatePlan::new(48_000).unwrap() else {
        panic!("48 kHz must convert");
    };
    for delay in [-0.5, f64::NAN, f64::INFINITY, f64::from(u32::MAX)] {
        assert!(ingress.output_capacity(256, delay).is_err());
    }
    assert!(ingress.output_capacity(4097, 0.0).is_err());
}
