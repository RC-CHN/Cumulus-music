use super::*;
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    small_cases: Vec<SmallCase>,
    full_matrix: FullMatrix,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SmallCase {
    name: String,
    rows: usize,
    columns: usize,
    default_bits: u32,
    cells: Vec<(usize, usize, u32)>,
    matrix_sha256: String,
    native_preliminary: Vec<(usize, usize, u32)>,
    native_positive_means: Vec<(usize, usize, u32)>,
    native_final: Vec<(i32, i32, u32)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FullMatrix {
    original_case: String,
    input_sha256: String,
    rows: usize,
    columns: usize,
    matrix_sha256: String,
    native_preliminary_count: usize,
    native_first_preliminary: Vec<(usize, usize, u32)>,
    native_final_count: usize,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/song-recognition-peaks-official.json"
    ))
    .unwrap()
}

fn final_bits(matrix: &CenteredPeakMatrix<'_>) -> Vec<(i32, i32, u32)> {
    select_default_peaks(matrix)
        .into_iter()
        .map(|peak| (peak.bin, peak.frame, peak.value_bits))
        .collect()
}

#[test]
fn matches_native_clipping_ties_skip_strict_thresholds_and_order() {
    for case in fixture().small_cases {
        let mut values = vec![f32::from_bits(case.default_bits); case.rows * case.columns];
        for (row, frame, bits) in case.cells {
            values[row * case.columns + frame] = f32::from_bits(bits);
        }
        let mut digest = Sha256::new();
        for value in &values {
            digest.update(value.to_le_bytes());
        }
        assert_eq!(
            hex::encode(digest.finalize()),
            case.matrix_sha256,
            "{}",
            case.name
        );
        let matrix = CenteredPeakMatrix::new(case.rows, case.columns, &values).unwrap();
        let preliminary: Vec<_> = matrix.local_maxima().collect();
        assert_eq!(
            preliminary
                .iter()
                .map(|&(row, frame, value)| (row, frame, value.to_bits()))
                .collect::<Vec<_>>(),
            case.native_preliminary,
            "{}",
            case.name
        );
        assert_eq!(
            preliminary
                .into_iter()
                .filter(|&(_, _, value)| value > 0.0)
                .map(|(row, frame, _)| (row, frame, matrix.local_mean(row, frame).to_bits()))
                .collect::<Vec<_>>(),
            case.native_positive_means,
            "{}",
            case.name
        );
        assert_eq!(final_bits(&matrix), case.native_final, "{}", case.name);
    }
}

#[test]
fn matches_every_peak_from_complete_native_centered_matrix() {
    #[derive(Deserialize)]
    struct Official {
        cases: Vec<OfficialCase>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct OfficialCase {
        name: String,
        input_sha256: String,
        native_peaks: Vec<(i32, i32, u32)>,
    }

    let case = fixture().full_matrix;
    let original: Official = serde_json::from_str(include_str!(
        "../../../tests/fixtures/song-recognition-official.json"
    ))
    .unwrap();
    let original = original
        .cases
        .into_iter()
        .find(|item| item.name == case.original_case)
        .unwrap();
    assert_eq!(original.input_sha256, case.input_sha256);

    let expected_len = case.rows * case.columns * size_of::<f32>();
    let mut bytes = Vec::new();
    GzDecoder::new(
        &include_bytes!("../../../tests/fixtures/song-recognition-noise-centered.f32le.gz")[..],
    )
    .take(expected_len as u64 + 1)
    .read_to_end(&mut bytes)
    .unwrap();
    assert_eq!(bytes.len(), expected_len);
    assert_eq!(hex::encode(Sha256::digest(&bytes)), case.matrix_sha256);
    let values: Vec<_> = bytes
        .chunks_exact(size_of::<f32>())
        .map(|value| f32::from_le_bytes(value.try_into().unwrap()))
        .collect();

    let matrix = CenteredPeakMatrix::new(case.rows, case.columns, &values).unwrap();
    assert_eq!(matrix.local_maxima().count(), case.native_preliminary_count);
    assert_eq!(
        matrix
            .local_maxima()
            .take(case.native_first_preliminary.len())
            .map(|(row, frame, value)| (row, frame, value.to_bits()))
            .collect::<Vec<_>>(),
        case.native_first_preliminary
    );
    assert_eq!(original.native_peaks.len(), case.native_final_count);
    assert_eq!(final_bits(&matrix), original.native_peaks);
}

#[test]
fn rejects_oversized_or_nonfinite_matrices_before_selection() {
    assert!(CenteredPeakMatrix::new(0, 1, &[]).is_err());
    assert!(CenteredPeakMatrix::new(1, 0, &[]).is_err());
    assert!(CenteredPeakMatrix::new(MAX_ROWS + 1, 1, &[]).is_err());
    assert!(CenteredPeakMatrix::new(1, MAX_FRAMES + 1, &[]).is_err());
    assert!(CenteredPeakMatrix::new(usize::MAX, usize::MAX, &[]).is_err());
    assert!(CenteredPeakMatrix::new(1, 2, &[0.0]).is_err());
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(CenteredPeakMatrix::new(1, 1, &[invalid]).is_err());
    }
}
