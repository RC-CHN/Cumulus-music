//! Shared native PC business signature. The fixed v1 suffix was confirmed in
//! both installers' App.getCommonData(secretKey) and the M/1368 renewal wire.
use super::eapi::md5_hex;

/// The caller provides endpoint-specific canonical fields including trailing `&`.
pub(crate) fn v1_md5(mut canonical_fields: String) -> String {
    canonical_fields.push_str("_scver=1&_sckey=7ada0f7ccadbe165e6e7fbe01113f4df");
    md5_hex(canonical_fields.as_bytes())
}
