use super::{SdkBuild, encode_fields};
use crate::{Error, Result};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

pub trait EntropyClock: Send + Sync {
    fn now_ms(&self) -> Result<u64>;
    fn fill(&self, bytes: &mut [u8]) -> Result<()>;
}
pub struct SystemEntropyClock;
impl EntropyClock for SystemEntropyClock {
    fn now_ms(&self) -> Result<u64> {
        u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::State("clock precedes Unix epoch"))?
                .as_millis(),
        )
        .map_err(|_| Error::State("clock exceeds range"))
    }
    fn fill(&self, bytes: &mut [u8]) -> Result<()> {
        getrandom::fill(bytes).map_err(|_| Error::Entropy)
    }
}
/// Za(): 31 random nibbles, fixed version nibble, RFC-shaped variant nibble.
pub fn sdk_uuid(random: [u8; 31]) -> String {
    let mut index = 0;
    "xxxxxxxxxxxx4xxxyxxxxxxxxxxxxxxx"
        .chars()
        .map(|c| {
            if c == '4' {
                return c;
            }
            let value = random[index] >> 4;
            index += 1;
            char::from(
                b"0123456789abcdef"[if c == 'y' {
                    (value & 3 | 8) as usize
                } else {
                    value as usize
                }],
            )
        })
        .collect()
}
/// Ya's inputs after actual collection/storage reads. No Debug/Serialize: DID,
/// TID and track identifiers stay in the native request boundary.
pub struct CommonFields<'a> {
    pub product: &'a str,
    pub business: Option<&'a str>,
    pub nonce: Option<&'a str>,
    pub tid: &'a str,
    pub did: &'a str,
    pub track: Option<&'a str>,
    pub local_ms: u64,
    pub server_offset_ms: i64,
    pub sequence: u8,
}
/// Pure official order before ua(). Production shuffles the individual encoded
/// fields, not arbitrary bytes. The sequence is the SDK's saturating 1..255.
pub fn public_fields(context: CommonFields<'_>, uuid_random: [u8; 31]) -> Result<Vec<Vec<u8>>> {
    public_fields_for(SdkBuild::Af2952a4, context, uuid_random)
}
pub fn public_fields_for(
    build: SdkBuild,
    context: CommonFields<'_>,
    uuid_random: [u8; 31],
) -> Result<Vec<Vec<u8>>> {
    let time = i128::from(context.local_ms) + i128::from(context.server_offset_ms);
    if time.unsigned_abs() > 8_640_000_000_000_000 {
        return Err(Error::InvalidInput("Watchman Date outside JS range"));
    }
    let mut fields: Vec<(&str, Value, bool)> = vec![
        ("bc", json!("200"), false),
        ("Lb", json!(context.sequence), false),
        ("Db", json!(sdk_uuid(uuid_random)), false),
        ("Ub", json!((time / 1000) as i64), false),
        ("Oa", json!(build.version()), false),
    ];
    if let Some(business) = context.business {
        fields.push(("Pa", json!(business), false));
    }
    if let Some(nonce) = context.nonce {
        fields.push(("Ma", json!(nonce), false));
    }
    fields.push(("Zb", json!(context.tid), false));
    if let Some(track) = context.track {
        fields.push(("ab", json!(track), false));
    }
    fields.extend([
        ("cb", json!(context.did), false),
        ("Kb", json!(context.product), false),
    ]);
    fields.extend(match build {
        SdkBuild::Af2952a4 => [
            ("Wa", json!("f5071d2c"), false),
            ("Xa", json!("0c0209f8"), false),
            ("$a", json!("1569d65e"), false),
        ],
        SdkBuild::B602a5ad7 => [
            ("Wa", json!("e13487eb"), false),
            ("Ya", json!("20343de2"), false),
            ("$a", json!("94342c3d"), false),
        ],
    });
    fields.into_iter().map(|f| encode_fields(&[f])).collect()
}
