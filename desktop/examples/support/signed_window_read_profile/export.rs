//! Host-only one-shot staging. The child only sees these three native inputs.
use super::{
    IdentityMode, PreparedProfile, canonical_id, prepare, private_temp, refused, safe, sqlite,
};
use cumulus_core::{
    Result,
    download::transfer::MediaUrl,
    protocol::{http::ApiOrigin, wire::serialization::trim_js},
    storage::private_directory_builder,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::CString,
    fs::{File, OpenOptions, Permissions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{OpenOptionsExt, PermissionsExt},
    },
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Intent {
    keyword: String,
    song_id: String,
    media_host: String,
}

impl Intent {
    fn validate(&self) -> Result<()> {
        let host = &self.media_host;
        if self.keyword.is_empty()
            || self.keyword.chars().count() > 64
            || trim_js(&self.keyword) != self.keyword
            || self.keyword.chars().any(char::is_control)
            || !canonical_id(&self.song_id)
            || host.len() > 253
            || !host.split('.').all(|label| {
                (1..=63).contains(&label.len())
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
            })
        {
            return Err(refused("invalid signed proof intent"));
        }
        let url = format!("https://{host}/");
        if [ApiOrigin::Pc, ApiOrigin::Interface]
            .iter()
            .any(|origin| origin.url().as_str() == url)
            || MediaUrl::parse(&url).is_err()
        {
            return Err(refused("signed proof requires one exact media host"));
        }
        Ok(())
    }
}

/// No Debug implementation. Serialize only into the private 0600 file.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Parameters<'a> {
    expected_uid: &'a str,
    keyword: &'a str,
    song_id: &'a str,
    client_sign: &'a str,
    os_version: &'a str,
    media_host: &'a str,
}

/// Returns only whether a missing identity was explicitly derived. The caller
/// owns the complete prepared directory and must remove it after the child.
pub fn export_stopped(
    source: &Path,
    uid_file: &Path,
    intent_file: &Path,
    output: &Path,
    identity: IdentityMode,
) -> Result<bool> {
    export_with(source, uid_file, intent_file, output, identity, prepare)
}

pub(super) fn export_with(
    source: &Path,
    uid_file: &Path,
    intent_file: &Path,
    output: &Path,
    identity: IdentityMode,
    prepare_profile: impl FnOnce(&Path, &Path, &str, IdentityMode) -> Result<PreparedProfile>,
) -> Result<bool> {
    let output_parent = output
        .parent()
        .ok_or_else(|| refused("invalid proof output path"))?;
    let output_name = output
        .file_name()
        .and_then(|part| part.to_str())
        .filter(|name| {
            (1..=64).contains(&name.len())
                && name.as_bytes()[0].is_ascii_alphanumeric()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or_else(|| refused("invalid proof output directory name"))?;
    let parent = sqlite::open_absolute(output_parent)?;
    let parent_meta = safe(parent.metadata(), "proof output parent is unavailable")?;
    sqlite::require_private(&parent_meta)?;

    let (uid, uid_meta) = sqlite::private_input(uid_file, 21)?;
    let (intent, intent_meta) = sqlite::private_input(intent_file, 1024)?;
    if sqlite::same_inode(&uid_meta, &intent_meta) {
        return Err(refused(
            "independent account and intent inputs are required",
        ));
    }
    let uid = safe(
        std::str::from_utf8(&uid),
        "invalid independent expected account input",
    )?;
    let uid = uid.strip_suffix('\n').unwrap_or(uid);
    if !canonical_id(uid) {
        return Err(refused("invalid independent expected account input"));
    }
    let intent: Intent = safe(
        serde_json::from_slice(&intent),
        "invalid signed proof intent",
    )?;
    intent.validate()?;

    let prepared = prepare_profile(source, output_parent, uid, identity)?;
    let staged = private_temp(output_parent, ".signed-input-")?;
    write_database(
        prepared.credential_dir(),
        &staged.path().join("credentials"),
    )?;
    write_database(prepared.config_dir(), &staged.path().join("config"))?;
    let profile = prepared.native_profile();
    let parameters = Parameters {
        expected_uid: uid,
        keyword: &intent.keyword,
        song_id: &intent.song_id,
        client_sign: profile
            .client_sign
            .as_deref()
            .filter(|sign| !sign.is_empty())
            .ok_or_else(|| refused("prepared signed identity is missing"))?,
        os_version: &profile.os_version,
        media_host: &intent.media_host,
    };
    let parameters = safe(
        serde_json::to_vec(&parameters),
        "could not serialize native proof parameters",
    )?;
    write_private(&staged.path().join("parameters.json"), &parameters)?;
    let derived = prepared.identity_was_derived();
    prepared.close()?;
    let stage = sqlite::open_absolute(staged.path())?;
    safe(stage.sync_all(), "could not flush private proof directory")?;
    let resolved_parent = sqlite::open_absolute(output_parent)?;
    if !sqlite::same_inode(
        &parent_meta,
        &safe(
            resolved_parent.metadata(),
            "proof output parent is unavailable",
        )?,
    ) {
        return Err(refused("proof output parent changed during preparation"));
    }
    let staged_name = staged
        .path()
        .file_name()
        .ok_or_else(|| refused("invalid proof staging name"))?;
    promote(
        &parent,
        staged_name.as_encoded_bytes(),
        output_name.as_bytes(),
    )?;
    let _ = staged.keep();
    Ok(derived)
}

fn write_database(source: &Path, target: &Path) -> Result<()> {
    let source = sqlite::open_absolute(source)?;
    let (bytes, _) = sqlite::stopped_database_bytes(&source)?;
    safe(
        private_directory_builder().create(target),
        "could not stage a private proof database",
    )?;
    write_private(&target.join("core.sqlite3"), &bytes)?;
    safe(
        sqlite::open_absolute(target)?.sync_all(),
        "could not flush private proof database directory",
    )
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = safe(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path),
        "could not create private proof input",
    )?;
    safe(
        file.set_permissions(Permissions::from_mode(0o600)),
        "could not restrict private proof input",
    )?;
    safe(file.write_all(bytes), "could not write private proof input")?;
    safe(file.sync_all(), "could not flush private proof input")
}

fn promote(parent: &File, staged: &[u8], output: &[u8]) -> Result<()> {
    let staged = safe(CString::new(staged), "invalid proof staging name")?;
    let output = safe(CString::new(output), "invalid proof output name")?;
    // SAFETY: both names are single C-string components in the pinned parent;
    // RENAME_NOREPLACE prevents replacing an existing directory or alias.
    let result = unsafe {
        libc::renameat2(
            parent.as_raw_fd(),
            staged.as_ptr(),
            parent.as_raw_fd(),
            output.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result != 0 {
        return Err(refused(
            "could not atomically publish the new private proof directory",
        ));
    }
    Ok(())
}
