//! One explicit non-trial PC effect registration/release with private evidence.
//! Usage: effect_authorization_roundtrip PROFILE NEW_PRIVATE_OUTPUT EFFECT_ID
//! Only one auth/set and one auth/stop are permitted, with no retry or payment.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    effects::{
        api::EffectsApi,
        authorization::{EffectAuthorizations, LeasePhase, ReleaseRetry, TrialConsent},
        catalog::EffectId,
    },
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Counts {
    requests: usize,
    starts: usize,
    stops: usize,
}
struct AuditApi {
    inner: Arc<dyn Api>,
    root: PathBuf,
    effect: EffectId,
    counts: Mutex<Counts>,
}
impl Api for AuditApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let number = {
                let mut counts = self.counts.lock().map_err(|_| Error::LockPoisoned)?;
                match path {
                    "/api/w/nuser/account/get"
                    | "/api/vipauth/app/interests/auth/query"
                    | "/api/vipauth/app/interests/recyclequery" => (),
                    "/api/vipauth/app/interests/auth/set"
                    | "/api/vipauth/app/interests/auth/stop" => {
                        let start = path.ends_with("/set");
                        if payload
                            != json!({"interestsId":self.effect.value().to_string(),"interestsType":self.effect.interests_type(),"type":if start{0}else{1}})
                        {
                            return Err(Error::State("proof attempted a different effect write"));
                        }
                        if start {
                            if counts.starts != 0 {
                                return Err(Error::State("proof refuses registration replay"));
                            }
                            counts.starts += 1;
                        } else {
                            if counts.stops != 0 || counts.starts != 1 {
                                return Err(Error::State(
                                    "proof refuses release replay or foreign release",
                                ));
                            }
                            counts.stops += 1;
                        }
                    }
                    _ => return Err(Error::State("proof attempted an unapproved API")),
                }
                counts.requests += 1;
                counts.requests
            };
            save(
                &self.root,
                &format!("call-{number:03}-request.json"),
                &json!({"path":path,"payload":payload,"cache":cache}),
            )?;
            match self.inner.request(path, payload, cache).await {
                Ok(value) => {
                    save(
                        &self.root,
                        &format!("call-{number:03}-response.json"),
                        &value,
                    )?;
                    Ok(value)
                }
                Err(error) => {
                    save(
                        &self.root,
                        &format!("call-{number:03}-error.json"),
                        &json!({"error":error.to_string()}),
                    )?;
                    Err(error)
                }
            }
        })
    }
}
fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(root.join(name))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
async fn session(api: Arc<dyn Api>, root: &Path) -> Result<Arc<AccountSession>> {
    let session = Arc::new(AccountSession::new(
        api,
        Some(Arc::new(StateStore::open(root.join("state"))?)),
    ));
    session.refresh().await?;
    Ok(session)
}
async fn run(profile: &Path, root: &Path, effect: EffectId) -> Result<Value> {
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(AuditApi {
        inner: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
        root: root.into(),
        effect,
        counts: Mutex::new(Counts::default()),
    });
    let mut account = session(api.clone(), root).await?;
    let baseline = account
        .run_for(&account.token()?, |api, _| async move {
            let api = EffectsApi::new(api);
            Ok((
                api.authorization(effect).await?,
                api.recyclable(effect).await?,
            ))
        })
        .await?;
    let auth = baseline
        .0
        .as_ref()
        .ok_or(Error::State("no effect authorization"))?;
    if !auth.can_use
        || auth.is_trial()
        || !matches!(auth.can_use_type, Some(10..=12))
        || baseline.1 != Some(false)
    {
        return Err(Error::State(
            "proof requires an existing non-trial entitlement with no recycle signal",
        ));
    }
    save(
        root,
        "baseline.json",
        &json!({"authorization":auth,"recyclable":baseline.1}),
    )?;
    let mut service = EffectAuthorizations::new(account.clone())?;
    let operation = async {
        let registered = service
            .begin_for(&account.token()?, effect, TrialConsent::DoNotStart)
            .await?;
        if !registered.can_use || !registered.lease.start_acknowledged {
            return Err(Error::State(
                "registration was not acknowledged and currently authorized",
            ));
        }
        save(
            root,
            "registered.json",
            &serde_json::to_value(&registered.lease)?,
        )?;
        Ok::<_, Error>(registered.lease.operation)
    }
    .await;
    // A distinct stop compensates even a lost start response. Never automatically
    // repeat stop if that result is itself unknown. Keep its journal for recovery.
    let cleanup = if service.record_for(&account.token()?, effect)?.is_some() {
        service
            .release_for(&account.token()?, effect, ReleaseRetry::Never)
            .await
            .map(Some)
    } else {
        Ok(None)
    };
    match &cleanup {
        Ok(lease) => save(root, "cleanup.json", &serde_json::to_value(lease)?)?,
        Err(error) => save(
            root,
            "cleanup-error.json",
            &json!({"error":error.to_string()}),
        )?,
    }
    let operation = operation?;
    let released = cleanup?.ok_or(Error::State("missing cleanup record"))?;
    if released.phase != LeasePhase::Released || !released.stop_acknowledged {
        return Err(Error::State("release was not acknowledged"));
    }
    drop(service);
    drop(account);
    account = session(api.clone(), root).await?;
    service = EffectAuthorizations::new(account.clone())?;
    let observed = service.observe_for(&account.token()?, effect).await?;
    if observed.lease.phase != LeasePhase::Released
        || observed.can_use
        || observed.lease.operation != operation
    {
        return Err(Error::State("release did not survive reopening"));
    }
    if observed.lease.authorization != baseline.0 || observed.lease.recyclable != baseline.1 {
        return Err(Error::State(
            "effect entitlement changed during the roundtrip",
        ));
    }
    save(
        root,
        "reopened.json",
        &serde_json::to_value(observed.lease)?,
    )?;
    let counts = api.counts.lock().map_err(|_| Error::LockPoisoned)?;
    if counts.starts != 1 || counts.stops != 1 {
        return Err(Error::State("unexpected effect write count"));
    }
    Ok(
        json!({"api_requests":counts.requests,"registrations":counts.starts,"releases":counts.stops,"start_acknowledged":true,"stop_acknowledged":true,"entitlement_unchanged":true,"reopened":true,"pending":0,"trial_started":false,"audio_activated":false,"ui_verified":false,"occupancy_readback_available":false}),
    )
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(Error::InvalidInput(
            "expected PC profile, new private output and effect ID",
        ));
    }
    let effect = EffectId::parse(&json!(
        args[2]
            .to_str()
            .ok_or(Error::InvalidInput("effect ID must be UTF-8"))?
    ))?;
    let root = std::path::absolute(PathBuf::from(&args[1]))?;
    let builder = cumulus_core::storage::private_directory_builder();
    builder.create(&root)?;
    match run(Path::new(&args[0]), &root, effect).await {
        Ok(report) => {
            save(&root, "report.json", &report)?;
            println!(
                "{}",
                json!({"api_requests":report["api_requests"],"registrations":1,"releases":1,"pending":0,"trial_started":false})
            );
            Ok(())
        }
        Err(error) => {
            save(&root, "failure.json", &json!({"error":error.to_string()}))?;
            Err(error)
        }
    }
}
