use super::{
    LATEST_ENDPOINT, LatestReleaseReply, LatestReleaseSource, ReleaseFuture, UpdateFailure,
};
use std::time::Duration;

pub(super) const MAX_REPLY: usize = 512 * 1024;
const API_VERSION: &str = "2026-03-10";

#[derive(Default)]
pub struct GitHubReleases;

fn http_status(code: u16, exhausted: bool, retry_after: bool) -> Result<bool, UpdateFailure> {
    match code {
        200 => Ok(true),
        404 => Ok(false),
        429 => Err(UpdateFailure::RateLimited),
        403 if exhausted || retry_after => Err(UpdateFailure::RateLimited),
        _ => Err(UpdateFailure::Unavailable),
    }
}

impl LatestReleaseSource for GitHubReleases {
    fn latest(&self) -> ReleaseFuture<'_> {
        Box::pin(async {
            // One fixed anonymous read. Do not read proxy credentials, follow
            // redirects, negotiate retries, or share music session state.
            let client = reqwest::Client::builder()
                .https_only(true)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .timeout(Duration::from_secs(8))
                .build()
                .map_err(|_| UpdateFailure::Network)?;
            let mut reply = client
                .get(LATEST_ENDPOINT)
                .header(
                    reqwest::header::USER_AGENT,
                    "Cumulus-Music/manual-update-check",
                )
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .header("X-GitHub-Api-Version", API_VERSION)
                .send()
                .await
                .map_err(|_| UpdateFailure::Network)?;
            if !http_status(
                reply.status().as_u16(),
                reply
                    .headers()
                    .get("x-ratelimit-remaining")
                    .is_some_and(|value| value == "0"),
                reply.headers().contains_key(reqwest::header::RETRY_AFTER),
            )? {
                return Ok(LatestReleaseReply::NotFound);
            }
            if reply
                .content_length()
                .is_some_and(|len| len > MAX_REPLY as u64)
            {
                return Err(UpdateFailure::InvalidMetadata);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = reply.chunk().await.map_err(|_| UpdateFailure::Network)? {
                if chunk.len() > MAX_REPLY - bytes.len() {
                    return Err(UpdateFailure::InvalidMetadata);
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(LatestReleaseReply::Published(bytes))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_published_success_or_an_absent_release_is_accepted() {
        assert_eq!(http_status(200, false, false), Ok(true));
        assert_eq!(http_status(404, false, false), Ok(false));
        assert_eq!(
            http_status(403, true, false),
            Err(UpdateFailure::RateLimited)
        );
        assert_eq!(
            http_status(403, false, true),
            Err(UpdateFailure::RateLimited)
        );
        assert_eq!(
            http_status(403, false, false),
            Err(UpdateFailure::Unavailable)
        );
        assert_eq!(
            http_status(429, false, false),
            Err(UpdateFailure::RateLimited)
        );
        assert_eq!(
            http_status(301, false, false),
            Err(UpdateFailure::Unavailable)
        );
    }
}
