use cumulus_core::updates::{
    LatestReleaseReply, LatestReleaseSource, ReleaseFuture, UpdateFailure,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub fn synthetic_package(name: &str) -> Vec<u8> {
    let magic = if name.ends_with(".deb") {
        b"!<arch>\n".as_slice()
    } else if name.ends_with(".tar.gz") {
        b"\x1f\x8b\x08".as_slice()
    } else if name.ends_with(".zip") {
        b"PK\x03\x04".as_slice()
    } else if name.ends_with(".exe") {
        b"MZ".as_slice()
    } else {
        b"manifest".as_slice()
    };
    [magic, b"synthetic release fixture; never run"].concat()
}

pub fn release(version: &str) -> Value {
    let assets = [
        format!("Cumulus-{version}-linux-x64.deb"),
        format!("Cumulus-{version}-linux-x64.tar.gz"),
        format!("Cumulus-{version}-windows-x64.zip"),
        format!("Cumulus-{version}-windows-x64-setup.exe"),
        "manifest-linux-x64.json".into(),
        "manifest-windows-x64.json".into(),
        "SHA256SUMS".into(),
        "release-provenance.json".into(),
    ];
    json!({
        "tag_name": format!("v{version}"),
        "draft": false,
        "prerelease": false,
        "published_at": "2026-10-30T13:40:00Z",
        "html_url": format!("https://github.com/RC-CHN/Cumulus-music/releases/tag/v{version}"),
        "body": "<img src=PRIVATE_PAYLOAD>",
        "assets": assets.into_iter().map(|name| {
            let bytes = synthetic_package(&name);
            json!({
                "name":name,"state":"uploaded","size":bytes.len(),
                "digest":format!("sha256:{}", hex::encode(Sha256::digest(&bytes))),
                "browser_download_url":"https://private.invalid/PRIVATE_DOWNLOAD"
            })
        }).collect::<Vec<_>>()
    })
}

#[derive(Clone)]
pub struct Fixture {
    pub mode: Arc<AtomicU8>,
    pub reads: Arc<AtomicUsize>,
    pub installed: &'static str,
}

impl LatestReleaseSource for Fixture {
    fn latest(&self) -> ReleaseFuture<'_> {
        Box::pin(async move {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let mode = self.mode.load(Ordering::SeqCst);
            if mode == 6 {
                tokio::time::sleep(Duration::from_millis(900)).await;
            }
            match mode {
                2 => Err(UpdateFailure::Network),
                3 => Err(UpdateFailure::RateLimited),
                4 => Ok(LatestReleaseReply::NotFound),
                mode => {
                    let mut version = release(if mode == 0 { self.installed } else { "999.1.0" });
                    if mode == 5 {
                        version["assets"].as_array_mut().unwrap().pop();
                    }
                    Ok(LatestReleaseReply::Published(
                        serde_json::to_vec(&version).unwrap(),
                    ))
                }
            }
        })
    }
}
