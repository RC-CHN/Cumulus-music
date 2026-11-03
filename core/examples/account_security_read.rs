//! Explicit online read proof for an existing authorized, isolated PC profile.
//! No arguments means no local profile access and no requests.
use cumulus_core::{
    Error, Result,
    account::SessionStatus,
    client::MusicClient,
    identity::WINDOWS_COMPATIBILITY_DESCRIPTION,
    protocol::{
        api::ApiFuture,
        http::{
            ApiOrigin, ClientProfile, HttpBackend, HttpReply, RequestPlan, ReqwestBackend,
            WireRoute,
        },
        wire::eapi,
    },
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

struct ReadOnly {
    backend: ReqwestBackend,
    reads: AtomicUsize,
    denied: AtomicUsize,
}

fn permitted(plan: &RequestPlan) -> bool {
    if plan.route != WireRoute::Eapi || ApiOrigin::Pc.require_url(&plan.url).is_err() {
        return false;
    }
    let mut form = url::form_urlencoded::parse(&plan.body);
    let Some((key, encrypted)) = form.next() else {
        return false;
    };
    if key != "params" || form.next().is_some() {
        return false;
    }
    let Ok((path, _)) = eapi::unwrap(&encrypted) else {
        return false;
    };
    if !ApiOrigin::Pc
        .request_url(WireRoute::Eapi, &path)
        .is_ok_and(|url| url == plan.url)
    {
        return false;
    }
    matches!(
        path.as_str(),
        "/api/w/nuser/account/get" | "/api/middle/user/device/list"
    ) || ["/api/w/v1/user/detail/", "/api/w/v1/user/bindings/"]
        .into_iter()
        .any(|prefix| {
            path.strip_prefix(prefix).is_some_and(|uid| {
                uid.bytes().all(|byte| byte.is_ascii_digit())
                    && uid.bytes().any(|byte| byte != b'0')
            })
        })
}

impl HttpBackend for ReadOnly {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            if !permitted(&plan) {
                self.denied.fetch_add(1, Ordering::Relaxed);
                return Err(Error::RequestNotSent(Box::new(Error::State(
                    "outside read-only account proof",
                ))));
            }
            self.reads.fetch_add(1, Ordering::Relaxed);
            self.backend.post(plan).await
        })
    }
}

fn copy_closed_profile(source: &Path, target: &Path) -> Result<()> {
    if !fs::symlink_metadata(source)?.is_dir() {
        return Err(Error::InvalidInput(
            "expected an isolated profile directory",
        ));
    }
    let database = source.join("core.sqlite3");
    if !fs::symlink_metadata(&database)?.is_file() {
        return Err(Error::InvalidInput("expected an existing profile database"));
    }
    // Require a closed, standalone database copy. Never open the supplied
    // SQLite file in place or guess which sidecars belong to its snapshot.
    for name in [
        "core.sqlite3-journal",
        "core.sqlite3-wal",
        "core.sqlite3-shm",
    ] {
        match fs::symlink_metadata(source.join(name)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
            Ok(_) => return Err(Error::InvalidInput("profile copy has SQLite sidecars")),
        }
    }
    cumulus_core::storage::private_directory_builder().create(target)?;
    let copied = target.join("core.sqlite3");
    fs::copy(database, &copied)?;
    let connection = Connection::open_with_flags(copied, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let existing: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM state WHERE owner = 'transport' AND kind = 'credentials' AND key = 'session')",
        [],
        |row| row.get(0),
    )?;
    if !existing {
        return Err(Error::InvalidInput(
            "profile copy has no native credential record",
        ));
    }
    Ok(())
}

fn failure(error: &Error) -> &'static str {
    if error.is_service_code(301) {
        return "session_unavailable";
    }
    match error.root() {
        Error::StaleResponse => "stale_session",
        Error::Response(_) | Error::Protocol(_) | Error::Json(_) => "invalid_response",
        Error::Service { .. } | Error::HttpStatus { .. } => "service_rejected",
        Error::Transport(_) => "transport_failed",
        _ => "failed",
    }
}

async fn verify(source: &Path) -> Result<Value> {
    let root = tempfile::tempdir()?;
    let profile_path = root.path().join("profile-copy");
    copy_closed_profile(source, &profile_path)?;
    let transport = Arc::new(ReadOnly {
        backend: ReqwestBackend::new(Duration::from_secs(20))?,
        reads: AtomicUsize::new(0),
        denied: AtomicUsize::new(0),
    });
    let client = MusicClient::open_with_native_profile(
        &root.path().join("state"),
        Some(&profile_path),
        ClientProfile::windows_compatibility(WINDOWS_COMPATIBILITY_DESCRIPTION)?,
        Some(transport.clone()),
    )?;
    let mut report =
        json!({"status":"failed","profileStatus":"not_read","devicesStatus":"not_read"});
    match client.restore_account().await {
        Ok(account) => {
            report["accountStatus"] = json!(account.status);
            if account.status == SessionStatus::Authenticated {
                match client.account_profile_detail(&account.epoch).await {
                    Ok(detail) => {
                        report["profileStatus"] = json!("ok");
                        report["bindingsCount"] = json!(detail.value.bindings.len());
                    }
                    Err(error) => report["profileStatus"] = json!(failure(&error)),
                }
                match client.login_devices(&account.epoch).await {
                    Ok(result) => {
                        let devices = &result.value.devices;
                        report["devicesStatus"] = json!("ok");
                        report["devicesCount"] = json!(devices.len());
                        report["signedInDevicesCount"] =
                            json!(devices.iter().filter(|d| d.signed_in).count());
                    }
                    Err(error) => report["devicesStatus"] = json!(failure(&error)),
                }
            }
        }
        Err(error) => report["accountStatus"] = json!(failure(&error)),
    }
    let denied = transport.denied.load(Ordering::Relaxed);
    if denied == 0 && report["profileStatus"] == "ok" && report["devicesStatus"] == "ok" {
        report["status"] = json!("ok");
    }
    report["readRequests"] = json!(transport.reads.load(Ordering::Relaxed));
    report["deniedRequests"] = json!(denied);
    report["remoteWrites"] = json!(0);
    Ok(report)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let report = match (args.next(), args.next(), args.next()) {
        (None, None, None) => json!({"status":"skipped","readRequests":0,"remoteWrites":0}),
        (Some(flag), Some(source), None) if flag == "--isolated-profile-copy" => {
            match verify(&PathBuf::from(source)).await {
                Ok(report) => report,
                Err(_) => {
                    json!({"status":"profile_setup_failed","readRequests":0,"remoteWrites":0})
                }
            }
        }
        _ => json!({"status":"invalid_arguments","readRequests":0,"remoteWrites":0}),
    };
    println!("{report}");
    if report["status"] == "ok" || report["status"] == "skipped" {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
