use super::*;
const fn table() -> [u64; 256] {
    let mut table = [0; 256];
    let mut n = 0;
    while n < 256 {
        let mut value = n as u64;
        let mut bit = 0;
        while bit < 8 {
            value = (value >> 1)
                ^ if value & 1 != 0 {
                    0xc96c5795d7870f42
                } else {
                    0
                };
            bit += 1;
        }
        table[n] = value;
        n += 1;
    }
    table
}
const TABLE: [u64; 256] = table();
/// Reflected ECMA CRC64 with all-one initial/final XOR. `previous` is the
/// previous finalized value; zero starts a new independent byte sequence.
pub fn crc64(data: &[u8], previous: u64) -> u64 {
    let mut value = previous ^ u64::MAX;
    for byte in data {
        value = TABLE[((value ^ u64::from(*byte)) & 255) as usize] ^ (value >> 8);
    }
    value ^ u64::MAX
}
pub fn check_crc64(value: Option<&str>, expected: u64) -> Result<bool> {
    let Some(value) = value else { return Ok(false) };
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|c| c.is_ascii_digit()) {
        return Err(Error::Response("invalid OSS CRC64 response"));
    }
    let value = value
        .parse::<u64>()
        .map_err(|_| Error::Response("OSS CRC64 overflow"))?;
    if value != 0 && value != expected {
        return Err(Error::Media("OSS object CRC64 mismatch"));
    }
    Ok(value != 0)
}
