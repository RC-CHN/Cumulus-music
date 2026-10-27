//! Authorized read-only service proof. Creates synthetic media in a new caller
//! directory; no existing library files or account resources are changed.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    catalog::{CatalogApi, SearchOptions},
    library::{
        ListOptions, LocalLibrary,
        catalog::LocalQuery,
        matching::{LocalMatching, MatchAction},
    },
    media::{
        ncm::{NcmWriteOptions, write_file},
        tags::{track_plan, write_tagged_copy},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};
struct Reads {
    inner: Arc<dyn Api>,
    calls: Mutex<BTreeMap<String, usize>>,
    matching_calls: Mutex<Vec<Value>>,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if ![
                "/api/w/nuser/account/get",
                "/api/v3/song/detail",
                "/api/search/match/new",
                "/api/cloudsearch/pc",
            ]
            .contains(&path)
            {
                return Err(Error::InvalidInput("unexpected matching proof endpoint"));
            }
            *self.calls.lock().unwrap().entry(path.into()).or_default() += 1;
            let response = self.inner.request(path, payload.clone(), cache).await?;
            if path == "/api/search/match/new" {
                self.matching_calls
                    .lock()
                    .unwrap()
                    .push(json!({"payload":payload,"response":response}));
            }
            Ok(response)
        })
    }
}
fn save(root: &Path, name: &str, value: Value) -> Result<()> {
    fs::write(root.join(name), serde_json::to_vec_pretty(&value)?)?;
    Ok(())
}
async fn run(profile: &Path, root: &Path) -> Result<()> {
    fs::create_dir(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(Reads {
        inner: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
        calls: Mutex::new(BTreeMap::new()),
        matching_calls: Mutex::new(Vec::new()),
    });
    let store = Arc::new(StateStore::open(root.join("state"))?);
    let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
    session.refresh().await?;
    let found = CatalogApi::new(api.clone())
        .search("海阔天空", SearchOptions::default())
        .await?;
    let selected = found["result"]["songs"]
        .as_array()
        .and_then(|songs| {
            songs.iter().find(|s| {
                s["name"] == "海阔天空"
                    && s["ar"].as_array().is_some_and(|artists| {
                        artists.iter().any(|a| {
                            a["name"]
                                .as_str()
                                .is_some_and(|n| n.eq_ignore_ascii_case("Beyond"))
                        })
                    })
            })
        })
        .ok_or(Error::State(
            "public proof song not present in search results",
        ))?;
    let song = CatalogApi::new(api.clone())
        .songs(&[selected["id"].clone()], 1)
        .await?
        .songs
        .remove(0)
        .ok_or(Error::State("proof song unavailable"))?;
    save(
        root,
        "proof-song.json",
        json!({"id":cumulus_core::protocol::api::identifier(&song["id"])?}),
    )?;
    let media = root.join("media");
    fs::create_dir(&media)?;
    let duration = song["dt"]
        .as_f64()
        .ok_or(Error::Response("missing proof song duration"))?
        / 1000.0;
    if !(1.0..=900.0).contains(&duration) {
        return Err(Error::State("proof duration outside bounds"));
    }
    let source = media.join("plain.flac");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=8000:cl=mono",
            "-t",
        ])
        .arg(duration.to_string())
        .args(["-c:a", "flac", "-metadata"])
        .arg(format!("title={}", song["name"].as_str().unwrap_or("")))
        .arg("-metadata")
        .arg(format!(
            "artist={}",
            song["ar"][0]["name"].as_str().unwrap_or("")
        ))
        .arg("-metadata")
        .arg(format!(
            "album={}",
            song["al"]["name"].as_str().unwrap_or("")
        ))
        .arg(&source)
        .status()?;
    if !status.success() {
        return Err(Error::State("cannot generate proof media"));
    }
    let plan = track_plan(
        &song,
        &json!({"type":"flac","br":128000,"md5":"ab".repeat(16)}),
        false,
    )?;
    write_tagged_copy(&source, media.join("tagged.flac"), &plan, None)?;
    let metadata: Value = serde_json::from_str(
        plan.comment_plain
            .strip_prefix("music:")
            .ok_or(Error::State("unexpected proof tag"))?,
    )?;
    write_file(
        &source,
        media.join("tagged.ncm"),
        NcmWriteOptions {
            metadata: Some(&metadata),
            ..Default::default()
        },
    )?;
    let hashes = fs::read_dir(&media)?
        .map(|e| {
            let path = e?.path();
            Ok((path.clone(), Sha256::digest(fs::read(path)?)))
        })
        .collect::<Result<Vec<_>>>()?;
    let library = LocalLibrary::new(store)?;
    library.scan(&media)?;
    let service = LocalMatching::new(session.clone(), library.clone())?;
    let job = service.start(&LocalQuery::default())?;
    let automatic = service.step().await;
    let automatic_error = automatic.as_ref().err().map(ToString::to_string);
    let automatic_job = service.state()?;
    save(
        root,
        "matching-calls.json",
        serde_json::to_value(&*api.matching_calls.lock().unwrap())?,
    )?;
    if automatic.is_err() {
        service.control(&job.id, MatchAction::Cancel)?;
        service.start(&LocalQuery {
            keyword: "tagged.".into(),
            ..Default::default()
        })?;
        service.step().await?;
    }
    let items = library.list(ListOptions::default())?;
    let ids = items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let mappings = library.online_mappings(&session, &session.token()?, &ids)?;
    let automatic_links=mappings.iter().map(|(id,m)| json!({"file":items.iter().find(|i| &i.id==id).unwrap().path.file_name().unwrap().to_string_lossy(),"source":m.source,"song_id":m.song_id})).collect::<Vec<_>>();
    let plain = items
        .iter()
        .find(|i| i.path == source.canonicalize().unwrap())
        .unwrap();
    service.clear(&plain.id, &plain.revision)?;
    service
        .bind(&plain.id, &plain.revision, &song["id"])
        .await?;
    let reopened = LocalMatching::new(session.clone(), library.clone())?;
    let recovered = reopened.recover()?;
    let final_maps = library.online_mappings(&session, &session.token()?, &ids)?;
    let expected_id = cumulus_core::protocol::api::identifier(&song["id"])?;
    let source_unchanged = hashes
        .into_iter()
        .all(|(p, h)| fs::read(p).is_ok_and(|bytes| Sha256::digest(bytes) == h));
    save(
        root,
        "report.json",
        json!({"automatic_job":automatic_job,"automatic_error":automatic_error,
        "automatic_links":automatic_links,"recovered_job":recovered,"final_links":final_maps.len(),
        "manual_bind_clear":final_maps.get(&plain.id).is_some_and(|m| m.song_id==expected_id),
        "source_unchanged":source_unchanged,"api_calls":*api.calls.lock().unwrap(),"remote_mutations":0,
        "synthetic_audio":true,"goal_complete":false}),
    )?;
    if !source_unchanged || final_maps.len() != 3 {
        return Err(Error::State(
            "matching proof did not preserve all mappings and sources",
        ));
    }
    println!("local matching proof saved; remote mutations: 0");
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: local_matching_roundtrip AUTHORIZED_PC_PROFILE NEW_PROOF_DIRECTORY",
        ));
    }
    run(&args[0], &args[1]).await
}
