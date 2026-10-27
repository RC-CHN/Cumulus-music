use crate::{Error, Result};
use serde_json::Value;
use time::{
    Date, OffsetDateTime,
    format_description::well_known::{Rfc2822, Rfc3339},
};
/// Supported service Date representations: numeric milliseconds, ISO/RFC3339,
/// RFC2822, UTC calendar dates and JS primitive boolean/null. Ambiguous local
/// strings and arbitrary JS object coercions fail instead of guessing a zone.
pub fn parse_server_time(value: &Value) -> Result<i64> {
    let invalid = || Error::Response("unsupported Watchman server time");
    let n = match value {
        Value::Number(n) => n
            .as_f64()
            .filter(|n| n.is_finite() && n.abs() <= 8_640_000_000_000_000.0)
            .map(|n| n.trunc() as i64)
            .ok_or_else(invalid)?,
        Value::Null => 0,
        Value::Bool(b) => i64::from(*b),
        Value::String(s) if s.len() <= 128 => {
            let parsed =
                OffsetDateTime::parse(s, &Rfc3339).or_else(|_| OffsetDateTime::parse(s, &Rfc2822));
            if let Ok(date) = parsed {
                i64::try_from(date.unix_timestamp_nanos() / 1_000_000).map_err(|_| invalid())?
            } else {
                let format = time::format_description::parse_borrowed::<2>("[year]-[month]-[day]")
                    .map_err(|_| invalid())?;
                Date::parse(s, &format)
                    .map_err(|_| invalid())?
                    .midnight()
                    .assume_utc()
                    .unix_timestamp()
                    * 1000
            }
        }
        _ => return Err(invalid()),
    };
    Ok(n)
}
