use crate::{Error, Result};
use aes::{
    Aes128, Aes192, Aes256,
    cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit, block_padding::Pkcs7},
};

pub fn encrypt_ecb(data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    macro_rules! encrypt {
        ($cipher:ty) => {
            ecb::Encryptor::<$cipher>::new_from_slice(key)
                .map_err(|_| Error::Protocol("invalid AES key"))?
                .encrypt_padded_vec_mut::<Pkcs7>(data)
        };
    }
    Ok(match key.len() {
        16 => encrypt!(Aes128),
        24 => encrypt!(Aes192),
        32 => encrypt!(Aes256),
        _ => return Err(Error::Protocol("invalid AES key length")),
    })
}

pub fn decrypt_ecb(data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err(Error::Protocol("invalid AES ciphertext length"));
    }
    macro_rules! decrypt {
        ($cipher:ty) => {
            ecb::Decryptor::<$cipher>::new_from_slice(key)
                .map_err(|_| Error::Protocol("invalid AES key"))?
                .decrypt_padded_vec_mut::<Pkcs7>(data)
        };
    }
    match key.len() {
        16 => decrypt!(Aes128),
        24 => decrypt!(Aes192),
        32 => decrypt!(Aes256),
        _ => return Err(Error::Protocol("invalid AES key length")),
    }
    .map_err(|_| Error::Protocol("invalid AES padding"))
}

pub fn encrypt_cbc(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    cbc::Encryptor::<Aes128>::new(key.into(), iv.into()).encrypt_padded_vec_mut::<Pkcs7>(data)
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| Error::Entropy)?;
    Ok(bytes)
}
