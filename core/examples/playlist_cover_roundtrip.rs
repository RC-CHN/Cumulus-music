//! Explicit real-account cover proof. Creates one empty PRIVATE playlist,
//! uploads synthetic artwork, binds and rereads it, then deletes only that
//! playlist and verifies the full original order. Never run from startup/tests.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, SessionToken, credentials::Credentials},
    cloud::nos::NosRunOptions,
    playlist::{cover::*, *},
    protocol::{
        api::{Api, ApiFuture, identifier},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

fn save(path: &Path, value: &Value) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
struct GuardApi {
    inner: Arc<dyn Api>,
    root: PathBuf,
    name: String,
    id: Mutex<Option<String>>,
    writes: Mutex<Vec<String>>,
    sequence: AtomicUsize,
}
impl Api for GuardApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let allowed = match path {
                "/api/w/nuser/account/get" | "/api/user/playlist" | "/api/v6/playlist/detail" => {
                    true
                }
                "/api/playlist/create" => payload["name"] == self.name && payload["privacy"] == 10,
                "/api/nos/token/alloc" => {
                    payload
                        == json!({"filename":"cover.jpg","ext":"jpg","type":"image","bucket":"yyimgs"})
                        && self.id.lock().unwrap().is_some()
                }
                "/api/playlist/cover/update" => self
                    .id
                    .lock()
                    .unwrap()
                    .as_deref()
                    .is_some_and(|id| payload["id"] == id),
                "/api/playlist/delete" => self
                    .id
                    .lock()
                    .unwrap()
                    .as_deref()
                    .is_some_and(|id| payload["pid"] == id),
                _ => false,
            };
            if !allowed {
                return Err(Error::State("request outside isolated cover proof"));
            }
            if matches!(
                path,
                "/api/playlist/create"
                    | "/api/nos/token/alloc"
                    | "/api/playlist/cover/update"
                    | "/api/playlist/delete"
            ) {
                let mut writes = self.writes.lock().unwrap();
                if writes.iter().any(|p| p == path) {
                    return Err(Error::State("proof refuses duplicate mutation"));
                }
                writes.push(path.into());
            }
            let n = self.sequence.fetch_add(1, Ordering::SeqCst);
            save(
                &self.root.join(format!("api-{n:03}-request.json")),
                &json!({"path":path,"payload":payload,"cache":cache}),
            )?;
            let result = self.inner.request(path, payload, cache).await;
            let mut evidence = match &result {
                Ok(v) => v.clone(),
                Err(_) => json!({"failed":true}),
            };
            // Preserve document/object identity for independent audit, never scoped tokens.
            for key in ["result", "data"] {
                if evidence[key].get("token").is_some() {
                    evidence[key]["token"] = json!("<redacted>");
                }
            }
            save(
                &self.root.join(format!("api-{n:03}-response.json")),
                &evidence,
            )?;
            result
        })
    }
}
fn ids(list: &PlaylistList) -> Vec<String> {
    list.playlists.iter().map(|p| p.id.clone()).collect()
}
async fn confirmed(
    service: &PlaylistService,
    token: &SessionToken,
    command: PlaylistCommand,
) -> Result<PlaylistMutation> {
    let mut record = service.execute(token, command).await?;
    for _ in 0..3 {
        if !record.phase.pending() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        record = service.reconcile(token, &record.operation_id).await?;
    }
    if !matches!(
        record.phase,
        PlaylistPhase::Confirmed | PlaylistPhase::Unchanged
    ) {
        return Err(Error::MutationUncertain);
    }
    Ok(record)
}
async fn cleanup(
    service: &PlaylistService,
    covers: &PlaylistCovers,
    token: &SessionToken,
    guard: &GuardApi,
    plan: &Value,
) -> Result<()> {
    for task in covers.list(token)? {
        let task = if task.phase == CoverPhase::Binding {
            covers.reconcile(token, &task.id).await?
        } else {
            task
        };
        if task.phase == CoverPhase::Binding {
            return Err(Error::MutationUncertain);
        }
        if !task.phase.terminal() {
            covers.abandon(token, &task.id)?;
        }
        covers.cleanup(token, &task.id)?;
    }
    for record in service
        .operations(token)?
        .into_iter()
        .filter(|r| r.phase.pending())
    {
        if service
            .reconcile(token, &record.operation_id)
            .await?
            .phase
            .pending()
        {
            return Err(Error::MutationUncertain);
        }
    }
    let baseline: Vec<String> = serde_json::from_value(plan["ids"].clone())?;
    let current = service.list(token).await?;
    let uid = token
        .uid
        .as_deref()
        .ok_or(Error::State("account missing"))?;
    let candidates = current
        .playlists
        .iter()
        .filter(|p| {
            p.owner == uid
                && p.name == plan["name"]
                && p.private
                && !p.favorite
                && !baseline.contains(&p.id)
        })
        .collect::<Vec<_>>();
    if candidates.len() > 1 {
        return Err(Error::State("multiple cleanup candidates"));
    }
    if let Some(p) = candidates.first() {
        let detail = service.detail(token, &p.id).await?;
        if detail.summary.owner != uid
            || detail.summary.name != plan["name"]
            || !detail.summary.private
            || detail.summary.favorite
            || !detail.tracks.is_empty()
        {
            return Err(Error::State("isolated cover cleanup evidence changed"));
        }
        *guard.id.lock().unwrap() = Some(p.id.clone());
        confirmed(
            service,
            token,
            PlaylistCommand::Delete {
                id: p.id.clone(),
                revision: detail.revision,
            },
        )
        .await?;
    }
    let after = service.list(token).await?;
    if ids(&after) != baseline {
        return Err(Error::State(
            "original playlist order differs after cleanup",
        ));
    }
    save(
        &guard.root.join("cleanup.json"),
        &json!({"ok":true,"full_order_restored":true,"ids":ids(&after),
        "remote_image_deleted":false,"image_orphan_possible":true}),
    )?;
    Ok(())
}
async fn run() -> Result<Value> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if !(args.len() == 2 || args.len() == 3 && args[2] == "--cleanup") {
        return Err(Error::InvalidInput(
            "usage: playlist_cover_roundtrip PC_PROFILE PRIVATE_STATE [--cleanup]",
        ));
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    let recovery = args.len() == 3;
    if !profile.join("core.sqlite3").is_file() || root.exists() != recovery {
        return Err(Error::InvalidInput(
            "explicit existing profile and new state (or cleanup state) required",
        ));
    }
    let store = Arc::new(StateStore::open(&root)?);
    let evidence = if recovery {
        root.join(format!("recovery-{}", uuid::Uuid::new_v4().simple()))
    } else {
        root.join("evidence")
    };
    fs::create_dir(&evidence)?;
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let name = if recovery {
        serde_json::from_slice::<Value>(&fs::read(root.join("plan.json"))?)?["name"]
            .as_str()
            .ok_or(Error::State("missing proof name"))?
            .to_owned()
    } else {
        format!(
            "CumulusCover{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        )
    };
    let guard = Arc::new(GuardApi {
        inner: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
        root: evidence.clone(),
        name: name.clone(),
        id: Mutex::new(None),
        writes: Mutex::new(vec![]),
        sequence: AtomicUsize::new(0),
    });
    let session = Arc::new(AccountSession::new(guard.clone(), Some(store.clone())));
    session.refresh().await?;
    let token = session.token()?;
    let uid = token
        .uid
        .as_deref()
        .ok_or(Error::State("missing account"))?;
    let service = PlaylistService::new(session.clone())?;
    let covers = PlaylistCovers::new(session.clone())?;
    let plan = if recovery {
        let plan: Value = serde_json::from_slice(&fs::read(root.join("plan.json"))?)?;
        if plan["owner"] != uid {
            return Err(Error::State("proof belongs to another account"));
        }
        plan
    } else {
        let baseline = service.list(&token).await?;
        let plan = json!({"owner":uid,"name":name,"ids":ids(&baseline)});
        save(&root.join("plan.json"), &plan)?;
        plan
    };
    if recovery {
        cleanup(&service, &covers, &token, &guard, &plan).await?;
        return Ok(json!({"cleanup_ok":true}));
    }
    let result:Result<Value>=async {
        let created=confirmed(&service,&token,PlaylistCommand::Create{name:name.clone(),private:true}).await?;
        let id=created.playlist_id.ok_or(Error::Response("created playlist missing"))?;
        *guard.id.lock().unwrap()=Some(id.clone());
        let before=service.detail(&token,&id).await?;
        if before.summary.owner!=uid || !before.summary.private || before.summary.name!=name || !before.tracks.is_empty(){return Err(Error::State("created playlist evidence differs"));}
        let image=RgbImage::from_fn(640,480,|x,y| Rgb([((x/4)%256) as u8,((y/3)%256) as u8,if x>=320 {220} else {30}]));
        let mut source=Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image).write_to(&mut source,ImageFormat::Png).map_err(|_|Error::Media("synthetic image failed"))?;
        fs::write(root.join("synthetic-input.png"),source.get_ref())?;
        let task=covers.select(&token,&id,&before.revision,source.into_inner()).await?;
        covers.prepare(&token,&task.id,CoverCrop{x:320,y:40,size:320})?;
        save(&root.join("prepared.json"),&serde_json::to_value(covers.get(&token,&task.id)?)?)?;
        let uploaded=covers.upload(&token,&task.id,NosRunOptions{chunk_budget:Some(1),..Default::default()}).await?;
        // Recreate the Rust service before binding; no archived process is used.
        let reopened=PlaylistCovers::new(session.clone())?;
        let uploaded=if uploaded.phase!=CoverPhase::Uploaded {reopened.upload(&token,&task.id,NosRunOptions::default()).await?} else {uploaded};
        if uploaded.phase!=CoverPhase::Uploaded {return Err(Error::State("cover transfer incomplete"));}
        let bytes=fs::read(root.join(format!("playlist-cover-{}.jpg",task.id)))?;
        fs::write(root.join("uploaded.jpg"),&bytes)?;
        let decoded=image::load_from_memory(&bytes).map_err(|_|Error::Media("uploaded JPEG invalid"))?;
        if decoded.width()!=320 || decoded.height()!=320 {return Err(Error::State("crop dimensions differ"));}
        let mut bound=reopened.bind(&token,&task.id).await?;
        for _ in 0..3 {if bound.phase!=CoverPhase::Binding {break;} bound=reopened.reconcile(&token,&task.id).await?;}
        if bound.phase!=CoverPhase::Confirmed {return Err(Error::MutationUncertain);}
        let after=service.detail(&token,&id).await?;
        let task_record=store.get(uid,"playlist-covers-v1",&task.id)?.ok_or(Error::State("cover journal missing"))?;
        let doc=identifier(&task_record["allocation"]["resource"])?;
        if after.cover_id.as_deref()!=Some(&doc) || before.summary.name!=after.summary.name || before.tracks!=after.tracks {
            return Err(Error::State("cover readback differs"));
        }
        save(&root.join("binding.json"),&json!({"task":bound,"document_id":doc,"before":before,"after":after}))?;
        Ok(json!({"ok":true,"task_id":task.id,"playlist_id":id,"document_id":doc,"width":decoded.width(),"height":decoded.height(),"bytes":bytes.len(),"source":"official M546/M2167/M2131/M444","ui_tested":false}))
    }.await;
    let cleanup = cleanup(&service, &covers, &token, &guard, &plan).await;
    let report = json!({"operation":result.as_ref().ok(),"operation_ok":result.is_ok(),"operation_error":result.err().map(|e|e.to_string()),
        "cleanup_ok":cleanup.is_ok(),"cleanup_error":cleanup.err().map(|e|e.to_string()),"writes":*guard.writes.lock().unwrap(),
        "remote_image_deleted":false,"image_orphan_possible":true});
    save(&root.join("report.json"), &report)?;
    Ok(report)
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
            if value["cleanup_ok"] != true || value.get("operation_ok").is_some_and(|v| v != true) {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("cover proof: {error}");
            std::process::exit(1);
        }
    }
}
