use crate::{Error, Result};

/// ncmdump buildKeyBox + audio loop; a 256-byte periodic mask, not RC4 PRGA.
/// Preserve upstream uint8 key_offset wrapping even for keys longer than 256.
pub(super) fn key_stream(key: &[u8]) -> Result<[u8; 256]> {
    if key.is_empty() {
        return Err(Error::Media("empty NCM audio key"));
    }
    let mut box_: [u8; 256] = std::array::from_fn(|i| i as u8);
    let (mut last, mut offset) = (0u8, 0u8);
    for i in 0..256 {
        let value = box_[i];
        let j = value
            .wrapping_add(last)
            .wrapping_add(key[usize::from(offset)]);
        offset = offset.wrapping_add(1);
        if usize::from(offset) >= key.len() {
            offset = 0;
        }
        box_[i] = box_[usize::from(j)];
        box_[usize::from(j)] = value;
        last = j;
    }
    Ok(std::array::from_fn(|i| {
        let j = (i as u8).wrapping_add(1);
        let first = box_[usize::from(j)];
        box_[usize::from(first.wrapping_add(box_[usize::from(first.wrapping_add(j))]))]
    }))
}

pub(super) fn apply(data: &mut [u8], mask: &[u8; 256], offset: u64) {
    let start = (offset & 255) as usize;
    for (i, byte) in data.iter_mut().enumerate() {
        *byte ^= mask[(start + (i & 255)) & 255];
    }
}
