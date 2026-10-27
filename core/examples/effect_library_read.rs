//! Read-only account proof of native effect catalogue/rights/material recovery.
//! Usage: effect_library_read PRIVATE_PC_PROFILE NEW_PRIVATE_OUTPUT SONG_ID
//! No auth/set, auth/stop, purchase, trial-start or user-resource mutation.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    download::transfer::{MediaBackend, MediaReply, MediaRequest, ReqwestMediaBackend, RunOptions},
    effects::{
        api::EffectsApi,
        catalog::{CatalogueKind, EffectId},
        service::EffectsLibrary,
    },
    protocol::{
        api::{Api, ApiFuture, identifier},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

struct ReadOnly {
    inner: Arc<dyn Api>,
    root: PathBuf,
    count: Mutex<usize>,
}
impl Api for ReadOnly {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if ![
                "/api/w/nuser/account/get",
                "/api/usertool/sound/queryAllSound",
                "/api/usertool/sound/mobile/promote",
                "/api/usertool/sound/mobile/all",
                "/api/usertool/home/soundeffect/devicebrand/list",
                "/api/usertool/home/soundeffect/devicesound/list",
                "/api/usertool/home/sound/info",
                "/api/vipauth/app/interests/auth/query",
                "/api/vipauth/app/interests/recyclequery",
                "/api/usertool/smart/sound/get",
                "/api/vip-rights/svip/sound/effect/get",
                "/api/vip-rights/emotion/audio/effect/get",
            ]
            .contains(&path)
            {
                return Err(Error::State("effect proof attempted an unapproved API"));
            }
            let sequence = {
                let mut n = self.count.lock().map_err(|_| Error::LockPoisoned)?;
                *n += 1;
                *n
            };
            save(
                &self.root,
                &format!("call-{sequence:03}-request.json"),
                &json!({"path":path,"payload":payload,"cache":cache}),
            )?;
            match self.inner.request(path, payload, cache).await {
                Ok(value) => {
                    save(
                        &self.root,
                        &format!("call-{sequence:03}-response.json"),
                        &value,
                    )?;
                    Ok(value)
                }
                Err(error) => {
                    save(
                        &self.root,
                        &format!("call-{sequence:03}-error.json"),
                        &json!({"error":error.to_string()}),
                    )?;
                    Err(error)
                }
            }
        })
    }
}
struct Media {
    inner: ReqwestMediaBackend,
    calls: Mutex<Vec<MediaTrace>>,
}
#[derive(Clone, serde::Serialize)]
struct MediaTrace {
    offset: u64,
    status: u16,
    content_range: Option<String>,
}
impl MediaBackend for Media {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let offset = request.offset;
            let reply = self.inner.get(request).await?;
            self.calls
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .push(MediaTrace {
                    offset,
                    status: reply.status,
                    content_range: reply.headers.get("content-range").cloned(),
                });
            Ok(reply)
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
async fn run(profile: &Path, root: &Path, song: &str) -> Result<Value> {
    let song = json!(identifier(&json!(song))?);
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(ReadOnly {
        inner: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
        root: root.into(),
        count: Mutex::new(0),
    });
    let mut account = session(api.clone(), root).await?;
    let media = Arc::new(Media {
        inner: ReqwestMediaBackend::new(Duration::from_secs(30))?,
        calls: Mutex::new(vec![]),
    });
    let mut library = EffectsLibrary::with_backend(account.clone(), media.clone())?;
    let mut catalogues = vec![];
    let mut candidates = vec![];
    let mut seen = BTreeSet::new();
    for kind in [
        CatalogueKind::HighlyRecommended,
        CatalogueKind::Recommended,
        CatalogueKind::More,
    ] {
        let items = library.catalogue_for(&account.token()?, kind, true).await?;
        catalogues.push(json!({"expose_type":kind.expose_type(),"count":items.len()}));
        for sound in items {
            if sound.catalogue_authorization().is_some_and(|a| a.can_use)
                && sound.parameter_material().is_ok_and(|m| m.is_some())
                && seen.insert(sound.id)
            {
                candidates.push(sound);
            }
        }
    }
    let reads=account.run_for(&account.token()?,|api,_|async move {
        let api=EffectsApi::new(api);let legacy_recommended=api.legacy_catalogue(true).await?;let legacy_all=api.legacy_catalogue(false).await?;
        let brands=api.device_brands().await?;let device_count=if let Some(brand)=brands.first(){api.device_sounds(EffectId::parse(&brand["deviceManagerId"])?).await?.len()}else{0};
        let mut rights=vec![];for id in [200,201,202] { let auth=api.authorization(EffectId::new(id)?).await?;rights.push(json!({"effect":id,"authorization":auth})); }
        let recycle=api.recycle_query(EffectId::new(200)?).await?;
        let smart=api.smart_key(&song).await?;let parameters=api.song_parameters(&song).await?;let emotion=api.emotion_features(&song).await?;
        Ok(json!({"legacy_recommended":legacy_recommended.len(),"legacy_all":legacy_all.len(),"device_brands":brands.len(),"device_sounds":device_count,"special_rights":rights,"recycle_result_type":if recycle.is_array(){"array"}else if recycle.is_object(){"object"}else{"other"},"smart_key_present":!smart.is_empty(),"song_parameters_present":parameters.is_some(),"song_material_url_present":parameters.as_ref().and_then(|p|p["effect"]["url"].as_str()).is_some(),"emotion_features_present":emotion.is_some()}))
    }).await?;
    save(root, "reads.json", &reads)?;
    let mut materials = vec![];
    for sound in candidates.into_iter().take(6) {
        let before = media.calls.lock().map_err(|_| Error::LockPoisoned)?.len();
        let paused = library
            .acquire_for(
                &account.token()?,
                sound.id,
                RunOptions {
                    byte_budget: Some(17),
                    ..Default::default()
                },
            )
            .await?;
        if paused.material.is_some() || paused.bytes != 17 {
            return Err(Error::State(
                "material did not pause at the requested boundary",
            ));
        }
        drop(library);
        drop(account);
        account = session(api.clone(), root).await?;
        library = EffectsLibrary::with_backend(account.clone(), media.clone())?;
        let ready = library
            .acquire_for(&account.token()?, sound.id, RunOptions::default())
            .await?;
        if ready.transfer_id != paused.transfer_id {
            return Err(Error::State("reopen lost the material transfer"));
        }
        let material = ready
            .material
            .ok_or(Error::State("material did not finish after reopening"))?;
        let cached = library
            .acquire_for(&account.token()?, sound.id, RunOptions::default())
            .await?
            .material
            .ok_or(Error::State("cached material is unavailable"))?;
        if cached.plain_sha256 != material.plain_sha256 {
            return Err(Error::Media("cached material digest changed"));
        }
        let calls = media.calls.lock().map_err(|_| Error::LockPoisoned)?[before..].to_vec();
        let offsets: Vec<_> = calls.iter().map(|c| c.offset).collect();
        if offsets != [0, 17] || calls[1].status != 206 {
            return Err(Error::State(
                "material resume/cache issued unexpected media requests",
            ));
        }
        materials.push(json!({"effect":sound.id,"detail_request_type":10,"catalogue_sound_type":sound.catalogue_sound_type,"transfer_id":ready.transfer_id,"bytes":ready.bytes,"type_id":material.type_id,"plain_sha256":material.plain_sha256,"path":material.path.strip_prefix(root).map_err(|_|Error::State("material outside proof root"))?,"parameters_object":material.parameters.is_object(),"resume_offsets":offsets,"media_requests":calls,"handle":ready.handle}));
    }
    if materials.is_empty() {
        return Err(Error::State(
            "account has no authorized parameter material sample",
        ));
    }
    let reads_total = *api.count.lock().map_err(|_| Error::LockPoisoned)?;
    Ok(
        json!({"catalogues":catalogues,"reads":reads,"materials":materials,"api_requests":reads_total,"remote_writes":0,"reopened_per_material":true,"fresh_auth_for_cached_use":true,"player_dsp_connected":false,"ui_verified":false}),
    )
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err(Error::InvalidInput(
            "expected PC profile, new private output and song ID",
        ));
    }
    let root = std::path::absolute(PathBuf::from(&args[1]))?;
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&root)?;
    let song = args[2]
        .to_str()
        .ok_or(Error::InvalidInput("song ID must be UTF-8"))?;
    match run(Path::new(&args[0]), &root, song).await {
        Ok(report) => {
            save(&root, "report.json", &report)?;
            println!(
                "{}",
                json!({"materials":report["materials"].as_array().map(Vec::len),"api_requests":report["api_requests"],"remote_writes":0})
            );
            Ok(())
        }
        Err(error) => {
            save(&root, "failure.json", &json!({"error":error.to_string()}))?;
            Err(error)
        }
    }
}
