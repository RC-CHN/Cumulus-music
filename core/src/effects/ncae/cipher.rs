//! cloudmusic.dll 3.1.42.205594 x64, RVAs CEB6A0 and CEB7E0.
//! Independently recovered fixed keybox; this is not RC4's evolving PRGA.

use crate::{Error, Result};

pub(super) fn key_stream(key: &[u8]) -> Result<[u8; 256]> {
    if key.is_empty() {
        return Err(Error::Media("empty NCAE key"));
    }
    let mut table: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut last = 0u8;
    for i in 0..256 {
        last = last.wrapping_add(table[i]).wrapping_add(key[i % key.len()]);
        table.swap(i, usize::from(last));
    }
    Ok(std::array::from_fn(|i| {
        let j = (i as u8).wrapping_add(1);
        let a = table[usize::from(j)];
        table[usize::from(a.wrapping_add(table[usize::from(j.wrapping_add(a))]))]
    }))
}

pub(super) fn apply(bytes: &mut [u8], stream: &[u8; 256], offset: u64) {
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte ^= stream[((offset as usize & 255) + (i & 255)) & 255];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use sha2::{Digest, Sha256};

    #[derive(Deserialize)]
    struct Vector {
        key_length: usize,
        length: usize,
        offset: u64,
        sha256: String,
    }

    #[derive(Deserialize)]
    struct Corpus {
        cipher_vectors: Vec<Vector>,
    }

    #[test]
    fn pinned_official_machine_code_vectors_and_chunk_boundaries() {
        let corpus: Corpus =
            serde_json::from_str(include_str!("../../../tests/fixtures/ncae-official.json"))
                .unwrap();
        assert_eq!(corpus.cipher_vectors.len(), 378);
        for case in corpus.cipher_vectors {
            let key: Vec<_> = (0..case.key_length)
                .map(|i| ((i * 73 + 19) & 255) as u8)
                .collect();
            let plain: Vec<_> = (0..case.length)
                .map(|i| ((i * 29 + (i >> 8) * 7 + 113) & 255) as u8)
                .collect();
            let stream = key_stream(&key).unwrap();
            let mut data = plain.clone();
            apply(&mut data, &stream, case.offset);
            assert_eq!(hex::encode(Sha256::digest(&data)), case.sha256);
            // Arbitrary chunk sizes must preserve the native offset semantics.
            for (i, chunk) in data.chunks_mut(137).enumerate() {
                apply(chunk, &stream, case.offset + (i * 137) as u64);
            }
            assert_eq!(data, plain);
        }
        assert!(key_stream(&[]).is_err());
    }
}
