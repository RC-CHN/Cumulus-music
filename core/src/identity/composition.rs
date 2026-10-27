use crate::{Error, Result};
use md5::{Digest, Md5};

/// CF1940/CF1C80. Bytes must already use the caller's actual native code page.
/// The second digest hashes the first digest's lowercase ASCII hex, not bytes.
pub fn derive_persistent_id(hardware_bytes: &[u8], host_info_bytes: &[u8]) -> String {
    let mut md5 = Md5::new();
    md5.update(hardware_bytes);
    md5.update(host_info_bytes);
    let first = hex::encode(md5.finalize());
    format!("{first}{}", hex::encode(Md5::digest(first.as_bytes())))
}

/// CEDF80 enabled parts, preserving its accumulated-output separator rule.
/// This consumes normalized collector values, not Windows enumeration itself.
pub fn client_sign(parts: [&str; 4], mask: u8) -> Result<String> {
    if parts
        .iter()
        .any(|v| v.len() > 4096 || v.contains(['\r', '\n', '\0']))
    {
        return Err(Error::InvalidInput("invalid native identity component"));
    }
    let mut output = String::new();
    for (i, part) in parts.iter().enumerate() {
        if mask & (1 << i) != 0 {
            if !output.is_empty() {
                output.push('@');
            }
            output.push_str(part);
        }
    }
    Ok(output)
}

/// Same persistent-store acceptance as the archived Linux adapter. Do not
/// silently rotate an existing identifier merely because hardware changed.
pub fn valid_persistent_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
