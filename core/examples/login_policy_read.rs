//! Explicit authorized-profile check. Only account/get and official login experiments;
//! refresh and every business mutation are denied even if credentials change.
use cumulus_core::{
    Error, Result,
    account::{
        login_policy::{AB_PATH, LoginExperiments},
        refresh::{DesktopDeviceToken, DeviceInfo},
    },
    client::{
        MusicClient,
        dto::{RenewalAction, RenewalState},
    },
    protocol::{
        api::ApiFuture,
        http::{HttpBackend, HttpReply, RequestPlan, ReqwestBackend},
        wire::eapi,
    },
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
struct ReadOnly {
    backend: ReqwestBackend,
    reads: AtomicUsize,
    experiments: AtomicUsize,
    flags: Mutex<Option<LoginExperiments>>,
    denied: AtomicUsize,
}
impl HttpBackend for ReadOnly {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let form: std::collections::BTreeMap<_, _> = url::form_urlencoded::parse(&plan.body)
                .into_owned()
                .collect();
            let (path, _) = eapi::unwrap(
                form.get("params")
                    .ok_or(Error::Response("expected PC EAPI"))?,
            )?;
            if path == AB_PATH {
                self.experiments.fetch_add(1, Ordering::SeqCst);
                let response = self.backend.post(plan).await?;
                let value = serde_json::from_slice(&response.body).or_else(|_| {
                    let plain = eapi::decrypt_response(&response.body).unwrap_or_default();
                    serde_json::from_slice(&plain)
                });
                if let Ok(value) = value {
                    *self.flags.lock().map_err(|_| Error::LockPoisoned)? =
                        LoginExperiments::from_response(&value).ok();
                }
                return Ok(response);
            }
            if path != "/api/w/nuser/account/get" {
                self.denied.fetch_add(1, Ordering::SeqCst);
                return Err(Error::RequestNotSent(Box::new(Error::State(
                    "read-only renewal proof",
                ))));
            }
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.backend.post(plan).await
        })
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let profile = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or(Error::InvalidInput("explicit profile required"))?,
    );
    let root = tempfile::tempdir()?;
    let transport = Arc::new(ReadOnly {
        backend: ReqwestBackend::new(Duration::from_secs(20))?,
        reads: AtomicUsize::new(0),
        experiments: AtomicUsize::new(0),
        flags: Mutex::new(None),
        denied: AtomicUsize::new(0),
    });
    let client =
        MusicClient::open_with_http_backend(root.path(), Some(&profile), transport.clone())?
            .with_official_login_policy()
            .with_session_renewal(DeviceInfo::default(), Arc::new(DesktopDeviceToken));
    let account = client.restore_account().await?;
    client.refresh_login_experiments(&account.epoch).await?;
    let status = client.session_renewal(&account.epoch)?;
    let result = if status.state == RenewalState::MissingCredentials {
        client
            .renew_session(&account.epoch, RenewalAction::Check)
            .await?
    } else {
        status
    };
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"accountStatus": account.status,"renewal":result,"identityReads":transport.reads.load(Ordering::SeqCst),"experimentReads":transport.experiments.load(Ordering::SeqCst),"observedExperiments":*transport.flags.lock().map_err(|_|Error::LockPoisoned)?,"deniedRequests":transport.denied.load(Ordering::SeqCst),"refreshRequests":0,"realBusinessMutations":0})
        )?
    );
    Ok(())
}
