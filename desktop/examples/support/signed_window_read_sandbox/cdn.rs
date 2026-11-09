//! One shot hostname handoff to the trusted host supervisor. No DNS or media I/O.
use super::{PROOF, own_uid, private_dir, valid_media_host};
use crate::proof::signed_window_read_gate::MediaArm;
use cumulus_core::{
    Error, Result, download::transfer::MediaUrl, playback::engine::Cancellation,
    protocol::api::ApiFuture,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::CString,
    fs::{self, OpenOptions, Permissions},
    io::{self, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

const REQUEST: &str = "cdn-authority-request.json";
const ARMED: &str = "cdn-authority-armed.json";
const MAX_BYTES: u64 = 256;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Authority {
    authority: String,
}

#[derive(Default)]
enum State {
    #[default]
    Fresh,
    Waiting,
    Armed(MediaUrl),
    Consumed(String),
    Closed,
}

pub struct CdnArm {
    directory: PathBuf,
    state: Mutex<State>,
}

fn refused() -> Error {
    Error::State("signed window CDN authorization refused")
}

fn host(url: &MediaUrl) -> Result<String> {
    let parsed = url::Url::parse(url.as_str()).map_err(|_| refused())?;
    parsed
        .host_str()
        .filter(|host| parsed.scheme() == "https" && valid_media_host(host))
        .map(str::to_owned)
        .ok_or_else(refused)
}

impl CdnArm {
    pub fn new() -> Self {
        Self::at(Path::new(PROOF).to_owned())
    }

    fn at(directory: PathBuf) -> Self {
        Self {
            directory,
            state: Mutex::default(),
        }
    }

    /// A media client cannot dispatch until the exact receipt URL was armed.
    /// A wrong URL, racing read, or second read closes the capability.
    pub fn consume(&self, url: &MediaUrl) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| refused())?;
        if matches!(&*state, State::Consumed(_)) {
            return Err(refused());
        }
        if let State::Armed(expected) = std::mem::replace(&mut *state, State::Closed)
            && expected == *url
        {
            *state = State::Consumed(host(url)?);
            return Ok(());
        }
        Err(refused())
    }

    pub fn consumed_authority(&self) -> Option<String> {
        match &*self.state.lock().ok()? {
            State::Consumed(authority) => Some(authority.clone()),
            _ => None,
        }
    }
}

impl MediaArm for CdnArm {
    fn arm<'a>(&'a self, url: &'a MediaUrl, cancel: &'a Cancellation) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            {
                let mut state = self.state.lock().map_err(|_| refused())?;
                if !matches!(&*state, State::Fresh) {
                    return Err(refused());
                }
                *state = State::Waiting;
            }
            cancel.check()?;
            let authority = host(url)?;
            let acknowledgement = self.directory.join(ARMED);
            if !private_dir(&self.directory).map_err(|_| refused())?
                || !matches!(
                    fs::symlink_metadata(&acknowledgement),
                    Err(error) if error.kind() == io::ErrorKind::NotFound
                )
            {
                return Err(refused());
            }
            publish(&self.directory, REQUEST, &authority).map_err(|_| refused())?;
            // The gate puts the whole wait under the response's bounded deadline.
            loop {
                cancel.check()?;
                if let Some(accepted) = read_ack(&acknowledgement)? {
                    if accepted.authority != authority {
                        return Err(refused());
                    }
                    let mut state = self.state.lock().map_err(|_| refused())?;
                    if !matches!(&*state, State::Waiting) {
                        return Err(refused());
                    }
                    *state = State::Armed(url.clone());
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
    }
}

fn publish(directory: &Path, name: &str, authority: &str) -> io::Result<()> {
    let bytes = serde_json::to_vec(&Authority {
        authority: authority.to_owned(),
    })?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversize authority",
        ));
    }
    let mut pending = tempfile::NamedTempFile::new_in(directory)?;
    pending
        .as_file()
        .set_permissions(Permissions::from_mode(0o600))?;
    pending.write_all(&bytes)?;
    pending.as_file().sync_all()?;
    let from = CString::new(pending.path().as_os_str().as_bytes())?;
    let target = directory.join(name);
    let to = CString::new(target.as_os_str().as_bytes())?;
    // SAFETY: both paths are valid NUL-terminated strings; the exclusive
    // rename publishes a fully written regular file with one hard link.
    let status = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn read_ack(path: &Path) -> Result<Option<Authority>> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(refused()),
    };
    let meta = file.metadata().map_err(|_| refused())?;
    if !meta.is_file()
        || meta.uid() != own_uid()
        || meta.mode() & 0o7777 != 0o600
        || meta.nlink() != 1
        || !(1..=MAX_BYTES).contains(&meta.len())
    {
        return Err(refused());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| refused())?;
    if bytes.len() as u64 != meta.len() {
        return Err(refused());
    }
    let accepted: Authority = serde_json::from_slice(&bytes).map_err(|_| refused())?;
    if !valid_media_host(&accepted.authority) {
        return Err(refused());
    }
    Ok(Some(accepted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const CDN: &str = "m2.music.126.net";

    fn media_url() -> MediaUrl {
        MediaUrl::parse("https://m2.music.126.net/native-only.m4a?synthetic=secret").unwrap()
    }

    fn proof_directory() -> tempfile::TempDir {
        tempfile::Builder::new()
            .permissions(Permissions::from_mode(0o700))
            .tempdir()
            .unwrap()
    }

    #[test]
    fn only_exact_same_url_can_use_an_acknowledgement_once() {
        for same_url in [true, false] {
            let directory = proof_directory();
            let arm = CdnArm::at(directory.path().into());
            let url = media_url();
            let cancel = Cancellation::default();
            tauri::async_runtime::block_on(async {
                let wait = arm.arm(&url, &cancel);
                tokio::pin!(wait);
                assert!(
                    tokio::time::timeout(Duration::from_millis(50), &mut wait)
                        .await
                        .is_err()
                );
                let request = directory.path().join(REQUEST);
                assert_eq!(
                    fs::read(&request).unwrap(),
                    br#"{"authority":"m2.music.126.net"}"#
                );
                let meta = fs::symlink_metadata(request).unwrap();
                assert!(meta.is_file() && meta.nlink() == 1 && meta.mode() & 0o7777 == 0o600);
                assert!(arm.consumed_authority().is_none());
                assert!(arm.arm(&url, &cancel).await.is_err());
                publish(directory.path(), ARMED, CDN).unwrap();
                wait.await.unwrap();
                let different_query =
                    MediaUrl::parse("https://m2.music.126.net/native-only.m4a?synthetic=forged")
                        .unwrap();
                assert_eq!(
                    arm.consume(if same_url { &url } else { &different_query })
                        .is_ok(),
                    same_url
                );
                assert!(arm.consume(&url).is_err());
                assert_eq!(arm.consumed_authority().as_deref(), same_url.then_some(CDN));
            });
        }
    }

    #[test]
    fn stale_wrong_or_unsafe_acknowledgement_never_arms_media() {
        for fault in [
            "prior",
            "wrong",
            "duplicate",
            "extra",
            "noncanonical",
            "symlink",
            "mode",
            "hardlink",
            "oversize",
        ] {
            let directory = proof_directory();
            let path = directory.path().join(ARMED);
            let arm = CdnArm::at(directory.path().into());
            let url = media_url();
            let cancel = Cancellation::default();
            if fault == "prior" {
                publish(directory.path(), ARMED, CDN).unwrap();
            }
            tauri::async_runtime::block_on(async {
                let wait = arm.arm(&url, &cancel);
                tokio::pin!(wait);
                if fault == "prior" {
                    assert!(wait.await.is_err());
                    assert!(!directory.path().join(REQUEST).exists());
                } else {
                    assert!(
                        tokio::time::timeout(Duration::from_millis(50), &mut wait)
                            .await
                            .is_err()
                    );
                    match fault {
                        "wrong" => publish(directory.path(), ARMED, "m3.music.126.net").unwrap(),
                        "duplicate" | "extra" | "noncanonical" | "oversize" => {
                            let content = match fault {
                                "duplicate" => br#"{"authority":"m2.music.126.net","authority":"m2.music.126.net"}"#.to_vec(),
                                "extra" => br#"{"authority":"m2.music.126.net","port":443}"#.to_vec(),
                                "noncanonical" => br#"{"authority":"M2.MUSIC.126.NET"}"#.to_vec(),
                                _ => vec![b' '; MAX_BYTES as usize + 1],
                            };
                            fs::write(&path, content).unwrap();
                            fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
                        }
                        "symlink" | "hardlink" => {
                            publish(directory.path(), "other", CDN).unwrap();
                            let source = directory.path().join("other");
                            if fault == "symlink" {
                                symlink(source, &path).unwrap();
                            } else {
                                fs::hard_link(source, &path).unwrap();
                            }
                        }
                        "mode" => {
                            publish(directory.path(), ARMED, CDN).unwrap();
                            fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
                        }
                        _ => unreachable!(),
                    }
                    assert!(
                        tokio::time::timeout(Duration::from_secs(1), wait)
                            .await
                            .unwrap()
                            .is_err()
                    );
                }
                assert!(arm.consume(&url).is_err());
                assert!(arm.arm(&url, &cancel).await.is_err());
                assert!(arm.consumed_authority().is_none());
            });
        }
    }

    #[test]
    fn preexisting_request_cancel_and_get_before_ack_never_grant_a_retry() {
        for case in ["prior", "cancelled", "early_get"] {
            let directory = proof_directory();
            let arm = CdnArm::at(directory.path().into());
            let url = media_url();
            let cancel = Cancellation::default();
            let path = directory.path().join(REQUEST);
            if case == "prior" {
                publish(directory.path(), REQUEST, "m3.music.126.net").unwrap();
            }
            tauri::async_runtime::block_on(async {
                let wait = arm.arm(&url, &cancel);
                tokio::pin!(wait);
                if case == "prior" {
                    assert!(wait.await.is_err());
                    assert_eq!(
                        read_ack(&path).unwrap().unwrap().authority,
                        "m3.music.126.net"
                    );
                } else {
                    assert!(
                        tokio::time::timeout(Duration::from_millis(50), &mut wait)
                            .await
                            .is_err()
                    );
                    if case == "cancelled" {
                        cancel.cancel();
                    } else {
                        assert!(arm.consume(&url).is_err());
                    }
                    publish(directory.path(), ARMED, CDN).unwrap();
                    assert!(
                        tokio::time::timeout(Duration::from_secs(1), wait)
                            .await
                            .unwrap()
                            .is_err()
                    );
                }
                assert!(arm.consume(&url).is_err());
                assert!(arm.arm(&url, &Cancellation::default()).await.is_err());
            });
        }
    }
}
