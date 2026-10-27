use cumulus_core::cloud::sort::{CloudTextOrder, stable_order};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;

#[test]
fn official_chinese_numeric_comparator_matches_executed_javascript_matrix() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/cloud_sort.json")).unwrap();
    let values = fixture["values"].as_array().unwrap();
    let order = CloudTextOrder::new().unwrap();
    for (i, a) in values.iter().enumerate() {
        for (j, b) in values.iter().enumerate() {
            let result = match order.compare(a.as_str().unwrap(), b.as_str().unwrap()) {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            };
            assert_eq!(
                result,
                fixture["matrix"][i][j].as_i64().unwrap(),
                "{a} vs {b}"
            );
        }
    }
}
#[test]
fn utf16_recursion_keeps_surrogate_order_against_every_valid_bmp_scalar() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/cloud_sort.json")).unwrap();
    let order = CloudTextOrder::new().unwrap();
    let values = (0..=65535)
        .filter_map(char::from_u32)
        .map(|c| match order.compare("😀a", &format!("{c}a")) {
            Ordering::Less => "-1",
            Ordering::Equal => "0",
            Ordering::Greater => "1",
        })
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        hex::encode(Sha256::digest(values)),
        fixture["utf16_boundary_sha256"].as_str().unwrap()
    );
}
#[test]
fn v8_sort_order_matches_all_run_boundaries_galloping_and_inconsistent_comparators() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/cloud_sort.json")).unwrap();
    let count = fixture["values"].as_array().unwrap().len();
    let matrix = fixture["matrix"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            row.as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_i64().unwrap().cmp(&0))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for sample in fixture["workloads"].as_array().unwrap() {
        let len = sample["len"].as_u64().unwrap() as usize;
        let mut random = sample["seed"].as_u64().unwrap() as u32;
        let input = (0..len)
            .map(|id| {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                match sample["pattern"].as_str().unwrap() {
                    "random" => random as usize % count,
                    "ascending" => (id / 4) % count,
                    "descending" => count - 1 - (id / 4) % count,
                    _ => {
                        (if (id / 64) % 2 == 1 {
                            63 - id % 64
                        } else {
                            id % 64
                        }) % count
                    }
                }
            })
            .collect::<Vec<_>>();
        let reverse = sample["reverse"].as_bool().unwrap();
        let sorted = stable_order(len, |i, j| {
            let order = matrix[input[i]][input[j]];
            if reverse { order.reverse() } else { order }
        });
        let text = sorted
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            hex::encode(Sha256::digest(text)),
            sample["sha256"].as_str().unwrap(),
            "{sample}"
        );
        let mut permutation = sorted;
        permutation.sort_unstable();
        assert!(permutation.iter().enumerate().all(|(i, n)| i == *n));
    }
}
