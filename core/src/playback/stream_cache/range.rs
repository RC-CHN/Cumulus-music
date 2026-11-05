use super::BLOCK;
use crate::{Error, Result, download::transfer::MediaUrl, protocol::api::ApiFuture};
use reqwest::header::{self, HeaderMap, HeaderName};
use std::time::Duration;

pub(crate) struct Chunk {
    pub bytes: Vec<u8>,
    pub etag: String,
}

pub(crate) trait RangeMedia: Send + Sync {
    fn get<'a>(
        &'a self,
        url: &'a MediaUrl,
        offset: u64,
        length: usize,
        size: u64,
        validator: Option<&'a str>,
    ) -> ApiFuture<'a, Chunk>;
}

pub(super) struct HttpsRanges(reqwest::Client);

impl HttpsRanges {
    pub(super) fn new() -> Result<Self> {
        reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .timeout(Duration::from_secs(3))
            .build()
            .map(Self)
            .map_err(|_| Error::Transport("streaming media client initialization failed".into()))
    }
}

fn unique(headers: &HeaderMap, name: HeaderName) -> Result<Option<&str>> {
    let values = headers.get_all(name);
    let mut values = values.iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(Error::Response("ambiguous streaming media header"));
    }
    value
        .map(|value| {
            value
                .to_str()
                .map_err(|_| Error::Response("invalid streaming media header"))
        })
        .transpose()
}

fn strong_etag(value: &str) -> bool {
    value.len() > 2
        && value.len() <= 130
        && value.starts_with('"')
        && value.ends_with('"')
        && value.as_bytes()[1..value.len() - 1]
            .iter()
            .all(|&byte| (0x21..=0x7e).contains(&byte) && byte != b'"')
}

fn end(offset: u64, length: usize, size: u64) -> Result<u64> {
    if !(1..=BLOCK).contains(&length) {
        return Err(Error::InvalidInput("invalid streaming media range length"));
    }
    offset
        .checked_add(length as u64 - 1)
        .filter(|end| *end < size)
        .ok_or(Error::InvalidInput(
            "streaming media range exceeds the source",
        ))
}

fn reply_headers(
    headers: &HeaderMap,
    offset: u64,
    end: u64,
    size: u64,
    expected: Option<&str>,
) -> Result<String> {
    if unique(headers, header::CONTENT_RANGE)? != Some(&format!("bytes {offset}-{end}/{size}"))
        || unique(headers, header::CONTENT_LENGTH)?.and_then(|value| value.parse::<u64>().ok())
            != Some(end - offset + 1)
        || unique(headers, header::CONTENT_ENCODING)?.is_some_and(|value| value != "identity")
    {
        return Err(Error::Response(
            "streaming media range disagrees with authorization",
        ));
    }
    let etag = unique(headers, header::ETAG)?
        .filter(|value| strong_etag(value))
        .ok_or(Error::Response(
            "streaming media needs a strong range validator",
        ))?;
    if expected.is_some_and(|value| etag != value) {
        return Err(Error::Response("streaming media validator changed"));
    }
    Ok(etag.to_owned())
}

impl RangeMedia for HttpsRanges {
    fn get<'a>(
        &'a self,
        url: &'a MediaUrl,
        offset: u64,
        length: usize,
        size: u64,
        validator: Option<&'a str>,
    ) -> ApiFuture<'a, Chunk> {
        Box::pin(async move {
            let end = end(offset, length, size)?;
            let mut request = self
                .0
                .get(url.as_str())
                .header(header::RANGE, format!("bytes={offset}-{end}"))
                .header(header::ACCEPT_ENCODING, "identity");
            if let Some(value) = validator {
                request = request.header(header::IF_MATCH, value);
            }
            let mut reply = request
                .send()
                .await
                .map_err(|_| Error::Transport("streaming media range request failed".into()))?;
            if reply.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                return Err(Error::Response("streaming media range was not honored"));
            }
            let etag = reply_headers(reply.headers(), offset, end, size, validator)?;
            let mut bytes = Vec::with_capacity(length);
            while let Some(chunk) = reply
                .chunk()
                .await
                .map_err(|_| Error::Transport("streaming media body failed".into()))?
            {
                if chunk.len() > length - bytes.len() {
                    return Err(Error::Response(
                        "streaming media range body exceeded its bounds",
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            if bytes.len() != length {
                return Err(Error::Response("streaming media range body was incomplete"));
            }
            Ok(Chunk { bytes, etag })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn exact_identity_ranges_only() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_static("bytes 0-31/64"),
        );
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("32"));
        headers.insert(header::ETAG, HeaderValue::from_static("\"representation\""));
        assert_eq!(
            reply_headers(&headers, 0, 31, 64, None).unwrap(),
            "\"representation\""
        );
        assert!(reply_headers(&headers, 0, 31, 64, Some("\"different\"")).is_err());
        assert!(reply_headers(&headers, 0, 31, 65, None).is_err());
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        assert!(reply_headers(&headers, 0, 31, 64, None).is_err());
        headers.remove(header::CONTENT_ENCODING);
        headers.append(header::CONTENT_LENGTH, HeaderValue::from_static("32"));
        assert!(reply_headers(&headers, 0, 31, 64, None).is_err());
        headers.remove(header::CONTENT_LENGTH);
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("32"));
        headers.insert(header::ETAG, HeaderValue::from_static("W/\"weak\""));
        assert!(reply_headers(&headers, 0, 31, 64, None).is_err());
    }
}
