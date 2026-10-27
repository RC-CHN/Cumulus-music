use crate::{Error, Result};
use aes::{
    Aes128,
    cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::NoPadding},
};
use md5::{Digest, Md5};
use subtle::ConstantTimeEq;

/// Recovered little-endian immediates, not an account secret (D04A00/D044F0).
pub const STORAGE_KEY: [u8; 16] = [
    0x45, 0x67, 0x89, 0xf1, 0x12, 0x45, 0x77, 0xc2, 0xb1, 0x45, 0x90, 0x12, 0x42, 0x90, 0x35, 0xe2,
];

pub fn storage_transform(data: &[u8], decrypt: bool) -> Result<Vec<u8>> {
    if data.is_empty() || data.len() % 16 == 1 || data.len() > super::MAX_CONTAINER_BYTES {
        return Err(Error::InvalidInput(
            "invalid or noninvertible device record length",
        ));
    }
    let full = data.len() & !15;
    let mut out = if full == 0 {
        Vec::new()
    } else if decrypt {
        cbc::Decryptor::<Aes128>::new(&STORAGE_KEY.into(), &[0; 16].into())
            .decrypt_padded_vec_mut::<NoPadding>(&data[..full])
            .map_err(|_| Error::Protocol("invalid device CBC block"))?
    } else {
        cbc::Encryptor::<Aes128>::new(&STORAGE_KEY.into(), &[0; 16].into())
            .encrypt_padded_vec_mut::<NoPadding>(&data[..full])
    };
    let tail = &data[full..];
    if !tail.is_empty() {
        let mut bytes = vec![0; tail.len()];
        let last = tail.len() - 1;
        if decrypt {
            bytes[last] = tail[last] ^ tail[0];
            for i in (0..last).rev() {
                bytes[i] = tail[i] ^ bytes[i + 1];
            }
        } else {
            for i in 0..last {
                bytes[i] = tail[i] ^ tail[i + 1];
            }
            bytes[last] = tail[last] ^ bytes[0];
        }
        out.extend(bytes);
    }
    Ok(out)
}
pub fn encode_record(plaintext: &[u8]) -> Result<Vec<u8>> {
    let encrypted = storage_transform(plaintext, false)?;
    let mut result = Md5::digest(plaintext).to_vec();
    result.extend(encrypted);
    Ok(result)
}
pub fn decode_record(record: &[u8]) -> Result<Vec<u8>> {
    if record.len() <= 16 {
        return Err(Error::Protocol("truncated device record"));
    }
    let plain = storage_transform(&record[16..], true)?;
    if !bool::from(record[..16].ct_eq(Md5::digest(&plain).as_slice())) {
        return Err(Error::Protocol("device record digest mismatch"));
    }
    Ok(plain)
}
