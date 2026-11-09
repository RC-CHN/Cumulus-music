//! Native-only profile preparation for the bounded signed desktop research proof.
//! No network client, source path, or printable credential is returned.
#[path = "signed_window_read_profile/export.rs"]
pub mod export;
#[path = "signed_window_read_profile/sqlite.rs"]
mod sqlite;

use cumulus_core::{
    Error, Result,
    account::{
        credentials::Credentials,
        profiles::{SavedProfile, Selection},
    },
    identity::{
        HardwareFacts, IdentityService, WINDOWS_COMPATIBILITY_DESCRIPTION, collect_linux_identity,
    },
    protocol::{
        api::ApiFuture,
        context::{RequestContext, TokenProvider},
        http::{APP_VERSION, ApiOrigin, ClientProfile, WireRoute},
        mconfig::{ConfigBackend, ConfigUrl, PcConfig, Snapshot},
    },
    storage::StateStore,
};
use rusqlite::params;
use serde_json::{Value, json};
use std::{
    fs::Permissions,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::TempDir;

const TRANSPORT: &str = "transport";
const PC_CONFIG: &str = "pc-mconfig-v1";
const PROFILE: &str = "profiles";
const SAVED: &str = "saved-profiles-v1";
const REMOVAL: &str = "saved-profile-removals-v1";
const CREDENTIAL: &str = "credentials";
const SESSION: &str = "session";
const DEVICE: &str = "device";
const IDENTITY: &str = "identity";
const CLIENT_SIGN: &str = "client_sign";
const MAX_SMALL_RECORD: i64 = 32 * 1024;
const MAX_CREDENTIAL_RECORD: i64 = 512 * 1024;
const MAX_CONFIG_RECORD: i64 = 16 * 1024 * 1024;

/// Persisted identity is always preferred. Local derivation is explicitly
/// enabled only as a fallback for an absent installation identity.
#[derive(Clone, Copy)]
pub enum IdentityMode {
    RequireSaved,
    DeriveIfMissing,
}

struct DenyAuxiliary;
impl TokenProvider for DenyAuxiliary {
    fn token<'a>(&'a self, _: &'a str) -> ApiFuture<'a, String> {
        Box::pin(async { Err(refused("SDK access is disabled in the read proof")) })
    }
}
impl ConfigBackend for DenyAuxiliary {
    fn get<'a>(&'a self, _: &'a ConfigUrl) -> ApiFuture<'a, Value> {
        Box::pin(async {
            Err(refused(
                "configuration downloads are disabled in the read proof",
            ))
        })
    }
}

/// Keep the value alive until the window, transport, and context are stopped.
/// This type has no Debug/Serialize implementation. Only the scratch paths may
/// be bound into the isolated child; the original source is never retained.
pub struct PreparedProfile {
    state: PathBuf,
    credential: PathBuf,
    context_dir: PathBuf,
    native_profile: ClientProfile,
    credentials: Arc<Credentials>,
    config: Arc<PcConfig>,
    context: Arc<RequestContext>,
    identity_derived: bool,
    scratch: TempDir,
}

impl PreparedProfile {
    pub fn state_dir(&self) -> &Path {
        &self.state
    }
    pub fn credential_dir(&self) -> &Path {
        &self.credential
    }
    pub fn config_dir(&self) -> &Path {
        &self.context_dir
    }
    pub fn native_profile(&self) -> ClientProfile {
        self.native_profile.clone()
    }
    pub fn credentials(&self) -> Arc<Credentials> {
        self.credentials.clone()
    }
    pub fn request_context(&self) -> Arc<RequestContext> {
        self.context.clone()
    }
    pub fn config_snapshot(&self) -> Result<Snapshot> {
        safe(self.config.snapshot(), "invalid saved PC configuration")
    }
    pub fn identity_was_derived(&self) -> bool {
        self.identity_derived
    }

    /// Use after dropping the proof's native client and gate. Stop the config
    /// observer before closing stores and unlinking the private scratch tree.
    pub fn close(self) -> Result<()> {
        let Self {
            config,
            context,
            credentials,
            scratch,
            ..
        } = self;
        let stopped = safe(config.close(), "could not stop the private PC context");
        drop((context, config, credentials));
        let removed = safe(scratch.close(), "could not remove the private proof state");
        stopped.and(removed)
    }
}

fn refused(message: &'static str) -> Error {
    Error::State(message)
}

fn safe<T, E>(value: std::result::Result<T, E>, message: &'static str) -> Result<T> {
    value.map_err(|_| refused(message))
}

fn canonical_id(value: &str) -> bool {
    (1..=20).contains(&value.len())
        && !value.starts_with('0')
        && value.bytes().all(|b| b.is_ascii_digit())
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn required(
    source: &sqlite::Snapshot,
    owner: &str,
    kind: &str,
    key: &str,
    limit: i64,
) -> Result<Value> {
    source
        .row(owner, kind, key, limit)?
        .ok_or_else(|| refused("required private source state is missing"))
}

fn selected(source: &sqlite::Snapshot, uid: &str) -> Result<Selection> {
    if required(
        source,
        TRANSPORT,
        PROFILE,
        "configuration",
        MAX_SMALL_RECORD,
    )? != json!({"schema":1,"origin":ApiOrigin::Pc})
    {
        return Err(refused("source is not a PC profile registry"));
    }
    let selection: Selection = safe(
        serde_json::from_value(required(
            source,
            TRANSPORT,
            PROFILE,
            "active",
            MAX_SMALL_RECORD,
        )?),
        "invalid active profile binding",
    )?;
    if !selection.auto_login
        || !lower_hex(&selection.session_id, 32)
        || !lower_hex(&selection.revision, 32)
    {
        return Err(refused("selected profile is not restorable"));
    }
    let matches: i64 = safe(source.database.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM state WHERE owner=?1 AND kind=?2
          AND CASE WHEN json_valid(value) THEN json_extract(value,'$.session_id')=?3 ELSE 0 END LIMIT 2)",
        params![TRANSPORT, SAVED, selection.session_id], |row| row.get(0),
    ), "invalid saved account binding")?;
    let saved: SavedProfile = safe(
        serde_json::from_value(required(source, TRANSPORT, SAVED, uid, MAX_SMALL_RECORD)?),
        "invalid saved account binding",
    )?;
    if matches != 1
        || saved.uid != uid
        || saved.session_id != selection.session_id
        || !saved.auto_login
    {
        return Err(refused(
            "selected account does not match the independently expected UID",
        ));
    }
    if source
        .row(TRANSPORT, REMOVAL, &selection.session_id, MAX_SMALL_RECORD)?
        .is_some()
    {
        return Err(refused("selected profile has an account removal receipt"));
    }
    let pending_jsk = source
        .row(uid, "login_refresh", "jsk", MAX_SMALL_RECORD)?
        .is_some_and(|journal| journal["pending"].as_bool() != Some(false));
    let pending_legacy = source
        .row(uid, "login_refresh", "legacy-periodic", MAX_SMALL_RECORD)?
        .is_some_and(|journal| journal.get("pending") != Some(&Value::Null));
    if pending_jsk || pending_legacy {
        return Err(refused(
            "selected profile has unresolved credential renewal",
        ));
    }
    Ok(selection)
}

fn check_credential(record: &Value) -> Result<()> {
    if record["schema"].as_u64() != Some(1)
        || record["origin"] != ApiOrigin::Pc.as_str()
        || record["cookies"].as_array().is_none_or(Vec::is_empty)
        || !record["login_notice"].is_null()
        || !record["queue_reset"].is_null()
    {
        return Err(refused(
            "selected PC credential is empty or has pending auth or cleanup state",
        ));
    }
    Ok(())
}

fn private_temp(parent: &Path, prefix: &str) -> Result<TempDir> {
    let scratch = safe(
        tempfile::Builder::new()
            .prefix(prefix)
            .permissions(Permissions::from_mode(0o700))
            .tempdir_in(parent),
        "could not allocate private proof state",
    )?;
    let dir = sqlite::open_absolute(scratch.path())?;
    sqlite::require_private(&safe(
        dir.metadata(),
        "private proof directory is unavailable",
    )?)?;
    Ok(scratch)
}

fn build_identity(
    scratch: &Path,
    persisted: Option<Value>,
    mode: IdentityMode,
    facts: impl FnOnce() -> Result<HardwareFacts>,
) -> Result<(ClientProfile, bool)> {
    let derived = persisted.is_none();
    if derived && matches!(mode, IdentityMode::RequireSaved) {
        return Err(refused("saved installation identity is unavailable"));
    }
    let identity_dir = private_temp(scratch, "identity-")?;
    let store = Arc::new(safe(
        StateStore::open(identity_dir.path()),
        "could not initialize private identity state",
    )?);
    if let Some(record) = persisted {
        let id = record["persistent_id"]
            .as_str()
            .filter(|id| lower_hex(id, 64))
            .ok_or_else(|| refused("saved installation identity is invalid"))?;
        safe(
            store.put(DEVICE, IDENTITY, CLIENT_SIGN, &json!({"persistent_id":id})),
            "could not stage saved installation identity",
        )?;
    }
    let service = IdentityService::new(store);
    let identity = safe(
        service.resolve(&safe(facts(), "could not collect local hardware identity")?),
        "could not resolve installation identity",
    )?;
    let base = safe(
        ClientProfile::windows_compatibility(WINDOWS_COMPATIBILITY_DESCRIPTION),
        "invalid desktop compatibility profile",
    )?;
    let profile = safe(
        identity.profile(base),
        "could not build signed desktop compatibility profile",
    )?;
    drop((identity, service));
    safe(
        identity_dir.close(),
        "could not remove transient identity observations",
    )?;
    Ok((profile, derived))
}

/// Proof entry; caller must explicitly stop source writers and pass the expected
/// account independently. Uses only hardware facts from this Linux host.
pub fn prepare(
    source: &Path,
    private_parent: &Path,
    expected_uid: &str,
    identity: IdentityMode,
) -> Result<PreparedProfile> {
    prepare_with(source, private_parent, expected_uid, identity, || {
        collect_linux_identity(Path::new("/sys"))
    })
}

fn prepare_with(
    source: &Path,
    private_parent: &Path,
    expected_uid: &str,
    identity: IdentityMode,
    facts: impl FnOnce() -> Result<HardwareFacts>,
) -> Result<PreparedProfile> {
    if !canonical_id(expected_uid)
        || source.starts_with(private_parent)
        || private_parent.starts_with(source)
    {
        return Err(refused(
            "proof requires an independent account and nonoverlapping private paths",
        ));
    }
    let source_dir = sqlite::open_absolute(source)?;
    let source_meta = safe(
        source_dir.metadata(),
        "private source directory is unavailable",
    )?;
    sqlite::require_private(&source_meta)?;
    let output_parent = sqlite::open_absolute(private_parent)?;
    let output_meta = safe(
        output_parent.metadata(),
        "private output directory is unavailable",
    )?;
    sqlite::require_private(&output_meta)?;
    if sqlite::same_inode(&source_meta, &output_meta) {
        return Err(refused("proof input and output must be distinct"));
    }
    let registry = sqlite::Snapshot::read(&source_dir)?;
    let selected = selected(&registry, expected_uid)?;
    let config = required(&registry, TRANSPORT, PC_CONFIG, "pc", MAX_CONFIG_RECORD)?;
    let persistent = registry.row(DEVICE, IDENTITY, CLIENT_SIGN, MAX_SMALL_RECORD)?;
    let sessions = sqlite::child(&source_dir, "sessions")?;
    let selected_dir = sqlite::child(&sessions, &selected.session_id)?;
    let profile = sqlite::Snapshot::read(&selected_dir)?;
    let record = required(
        &profile,
        TRANSPORT,
        CREDENTIAL,
        SESSION,
        MAX_CREDENTIAL_RECORD,
    )?;
    check_credential(&record)?;
    profile.recheck(&selected_dir)?;
    registry.recheck(&source_dir)?;
    let resolved = sqlite::open_absolute(source)?;
    if !sqlite::same_inode(
        &source_meta,
        &safe(
            resolved.metadata(),
            "private source directory is unavailable",
        )?,
    ) || !sqlite::same_inode(
        &safe(selected_dir.metadata(), "selected profile is unavailable")?,
        &safe(
            sqlite::child(&sqlite::child(&resolved, "sessions")?, &selected.session_id)?.metadata(),
            "selected profile is unavailable",
        )?,
    ) {
        return Err(refused("source selection changed during preparation"));
    }
    // All source objects are detached before any output state is materialized.
    drop((
        profile,
        registry,
        resolved,
        selected_dir,
        sessions,
        source_dir,
    ));

    let scratch = private_temp(private_parent, "signed-window-")?;
    let verified_output = sqlite::open_absolute(private_parent)?;
    if !sqlite::same_inode(
        &output_meta,
        &safe(
            verified_output.metadata(),
            "private output directory is unavailable",
        )?,
    ) {
        return Err(refused("proof output directory changed during preparation"));
    }
    let state = scratch.path().join("application-state");
    let credential = scratch.path().join("selected-credentials");
    let context_dir = scratch.path().join("pc-context");
    // Separate production StateStore databases, never the source pages/rows.
    drop(safe(
        StateStore::open(&state),
        "could not initialize private application state",
    )?);
    let store = Arc::new(safe(
        StateStore::open(&credential),
        "could not initialize private credential state",
    )?);
    safe(
        store.put(TRANSPORT, CREDENTIAL, SESSION, &record),
        "could not stage the selected credential",
    )?;
    let credentials = Arc::new(safe(
        Credentials::open(store, ApiOrigin::Pc),
        "invalid selected PC credential",
    )?);
    let account = ApiOrigin::Pc.request_url(WireRoute::Eapi, "/api/w/nuser/account/get")?;
    let scoped = safe(
        credentials.request_credentials(&account),
        "could not verify selected PC cookie scope",
    )?;
    let mut tokens = scoped.cookies.iter().filter(|(name, _)| name == "MUSIC_U");
    if !tokens.next().is_some_and(|(_, token)| {
        !token.is_empty() && token.len() <= 16_384 && !token.contains(['\r', '\n', '\0'])
    }) || tokens.next().is_some()
    {
        return Err(refused("one usable scoped MUSIC_U cookie is required"));
    }
    let config_store = Arc::new(safe(
        StateStore::open(&context_dir),
        "could not initialize private PC context",
    )?);
    safe(
        config_store.put(TRANSPORT, PC_CONFIG, "pc", &config),
        "could not stage saved PC configuration",
    )?;
    let disabled = Arc::new(DenyAuxiliary);
    let config = safe(
        PcConfig::new(config_store, disabled.clone()),
        "invalid saved PC configuration",
    )?;
    if safe(config.snapshot(), "invalid saved PC configuration")?.version == 0 {
        return Err(refused("saved native PC configuration is unavailable"));
    }
    let context = Arc::new(safe(
        RequestContext::from_pc(config.clone(), APP_VERSION, disabled),
        "invalid private PC context",
    )?);
    let (native_profile, identity_derived) =
        build_identity(scratch.path(), persistent, identity, facts)?;
    Ok(PreparedProfile {
        state,
        credential,
        context_dir,
        native_profile,
        credentials,
        config,
        context,
        identity_derived,
        scratch,
    })
}

#[cfg(test)]
#[path = "signed_window_read_profile/tests.rs"]
mod tests;
