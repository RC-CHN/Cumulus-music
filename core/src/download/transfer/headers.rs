use super::{MediaReply, TransferJob};
use crate::{Error, Result};

pub(super) struct RangePlan {
    pub offset: u64,
    pub total: Option<u64>,
    pub length: Option<u64>,
    pub etag: Option<String>,
    pub modified: Option<String>,
}
fn integer(text: &str) -> Result<u64> {
    if text.is_empty() || !text.bytes().all(|c| c.is_ascii_digit()) {
        return Err(Error::Response("invalid media byte count"));
    }
    text.parse()
        .map_err(|_| Error::Response("media byte count overflow"))
}
fn etag(text: &str) -> bool {
    let text = text.strip_prefix("W/").unwrap_or(text);
    text.len() >= 2
        && text.starts_with('"')
        && text.ends_with('"')
        && text.as_bytes()[1..text.len() - 1]
            .iter()
            .all(|b| *b >= 0x21 && *b != b'"' && *b != 0x7f)
}
pub(super) fn validate(
    reply: &MediaReply,
    job: &TransferJob,
    offset: u64,
    limit: u64,
) -> Result<RangePlan> {
    if !matches!(reply.status, 200 | 206) {
        return Err(Error::HttpStatus {
            endpoint: None,
            status: reply.status,
            body: None,
        });
    }
    let h = &reply.headers;
    if h.get("content-encoding")
        .is_some_and(|e| !e.eq_ignore_ascii_case("identity"))
    {
        return Err(Error::Response("encoded media cannot be byte resumed"));
    }
    let mut length = h.get("content-length").map(|s| integer(s)).transpose()?;
    let new_etag = h.get("etag").filter(|s| etag(s)).cloned();
    let modified = h
        .get("last-modified")
        .filter(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(|c| c.is_control()))
        .cloned();
    let (offset, total) = match reply.status {
        200 => (0, length),
        206 => {
            let range = h
                .get("content-range")
                .and_then(|s| s.strip_prefix("bytes "))
                .ok_or(Error::Response("missing or invalid media Content-Range"))?;
            let (span, total) = range
                .split_once('/')
                .ok_or(Error::Response("invalid media Content-Range"))?;
            let (start, end) = span
                .split_once('-')
                .ok_or(Error::Response("invalid media Content-Range"))?;
            let (start, end, total) = (integer(start)?, integer(end)?, integer(total)?);
            if start != offset || end < start || end >= total {
                return Err(Error::Response("server returned a different byte range"));
            }
            let count = end - start + 1;
            if length.is_some_and(|n| n != count) {
                return Err(Error::Response("media range length mismatch"));
            }
            length = Some(count);
            if offset > 0
                && (job
                    .etag
                    .as_ref()
                    .zip(new_etag.as_ref())
                    .is_some_and(|(a, b)| a != b)
                    || job.etag.is_none()
                        && job
                            .modified
                            .as_ref()
                            .zip(modified.as_ref())
                            .is_some_and(|(a, b)| a != b))
            {
                return Err(Error::Response(
                    "media representation changed during resume",
                ));
            }
            (offset, Some(total))
        }
        status => {
            return Err(Error::HttpStatus {
                endpoint: None,
                status,
                body: None,
            });
        }
    };
    if total.is_some_and(|n| n == 0 || n > limit)
        || total.zip(job.resource.size).is_some_and(|(a, b)| a != b)
    {
        return Err(Error::Response(
            "media size differs from authorized resource or limit",
        ));
    }
    Ok(RangePlan {
        offset,
        total: total.or(job.resource.size),
        length,
        etag: new_etag.filter(|s| !s.starts_with("W/")),
        modified,
    })
}
