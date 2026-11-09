//! A bounded, credential-free fetch after the read gate authorizes one URL.
use super::Gate;
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaRequest, MediaUrl},
    playback::engine::Cancellation,
    protocol::wire::eapi,
};
use std::{
    fs::File,
    io::{Seek, Write},
    path::Path,
    time::Duration,
};

impl Gate {
    /// Bind the one-use receipt to the exact normalized URL, fetch one
    /// complete response through the caller's fenced media transport, and
    /// hand back an anonymous file. The engine never receives the CDN URL.
    pub async fn fetch_verified(
        &self,
        backend: &dyn MediaBackend,
        url: &MediaUrl,
        private_dir: &Path,
        cancel: &Cancellation,
    ) -> Result<File> {
        cancel.check()?;
        let receipt = self.take_media(url).await?;
        tokio::time::timeout(Duration::from_secs(12), async {
            cancel.check()?;
            let mut response = backend
                .get(MediaRequest {
                    url: url.clone(),
                    offset: 0,
                    if_range: None,
                })
                .await
                .map_err(|_| Error::Transport("signed window media transport refused".into()))?;
            let failure = || Error::Response("signed window media integrity refused");
            if response.status != 200
                || response
                    .headers
                    .keys()
                    .any(|key| key.eq_ignore_ascii_case("content-range"))
                || header(&response.headers, "content-encoding")?
                    .is_some_and(|value| !value.eq_ignore_ascii_case("identity"))
                || header(&response.headers, "content-length")?.and_then(|value| value.parse().ok())
                    != Some(receipt.bytes)
            {
                return Err(failure());
            }
            let mut payload = Vec::with_capacity(receipt.bytes as usize);
            let mut block = [0_u8; 64 * 1024];
            loop {
                cancel.check()?;
                let count = response
                    .body
                    .read(&mut block)
                    .await
                    .map_err(|_| failure())?;
                if count == 0 {
                    break;
                }
                if count > block.len() || payload.len() + count > receipt.bytes as usize {
                    return Err(failure());
                }
                payload.extend_from_slice(&block[..count]);
            }
            if payload.len() as u64 != receipt.bytes || eapi::md5_hex(&payload) != receipt.md5 {
                return Err(failure());
            }
            cancel.check()?;
            let mut file = tempfile::tempfile_in(private_dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            }
            file.write_all(&payload)?;
            file.rewind()?;
            cancel.check()?;
            self.require_original_account()?;
            Ok(file)
        })
        .await
        .map_err(|_| Error::Transport("signed window media timed out".into()))?
    }
}

fn header<'a>(
    headers: &'a std::collections::BTreeMap<String, String>,
    name: &str,
) -> Result<Option<&'a str>> {
    let mut values = headers
        .iter()
        .filter_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()));
    let value = values.next();
    if values.next().is_some() {
        return Err(Error::Response("ambiguous signed window media header"));
    }
    Ok(value)
}
