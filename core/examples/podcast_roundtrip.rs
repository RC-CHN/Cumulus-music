//! Current PC account, read-only remote proof. All downloaded files and native
//! journals live under a new private root. No payment, history upload or writes.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    download::{
        completed::{CompletedDownloads, FileAction},
        offline::OfflineLibrary,
        service::{
            DownloadDestination, DownloadOptions, DownloadRunOptions, DownloadService,
            DownloadStatus,
        },
    },
    media::integrity::inspect_file,
    playback::{
        engine::{AudioEngine, Cancellation, LoadRequest, MediaInput, MpvConfig, MpvEngine},
        source::SourceResolver,
    },
    podcast::{HistoryKind, LibraryKind, PodcastApi},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct ReadOnly {
    inner: Arc<dyn Api>,
    root: PathBuf,
    sequence: Mutex<usize>,
}
impl Api for ReadOnly {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if ![
                "/api/w/nuser/account/get",
                "/api/my/podcast/page/list",
                "/api/social/my/subscribed/voicelist/v1",
                "/api/social/my/created/voicelist/v1",
                "/api/voicelist/bought/list",
                "/api/content/my/liked/voice",
                "/api/content/my/liked/voice/count",
                "/api/djradio/v3/get",
                "/api/v6/dj/program/byradio",
                "/api/dj/program/detail",
                "/api/dj/play/url/check",
                "/api/dj/voices/download/check",
                "/api/dj/user/setting/resumeplay/get",
                "/api/voice/alg/skip/switch/get",
                "/api/song/enhance/player/url/v1",
                "/api/song/enhance/download/url/v1",
                "/api/voice/homepage/block/page",
                "/api/dj/radio/category/list",
                "/api/dj/radio/category/radio/list",
                "/api/play-record/album/list",
                "/api/play-record/djradio/list",
                "/api/play-record/voice/list",
                "/api/play-record/voice/book/timeline/list",
            ]
            .contains(&path)
            {
                return Err(Error::State("proof attempted an unapproved API"));
            }
            let sequence = {
                let mut n = self.sequence.lock().map_err(|_| Error::LockPoisoned)?;
                *n += 1;
                *n
            };
            save(
                &self.root,
                &format!("call-{sequence:03}-request.json"),
                &json!({"path":path,"payload":payload,"cache":cache}),
            )?;
            let value = self.inner.request(path, payload, cache).await?;
            save(
                &self.root,
                &format!("call-{sequence:03}-response.json"),
                &value,
            )?;
            Ok(value)
        })
    }
}
fn save(root: &Path, name: &str, v: &Value) -> Result<()> {
    use std::io::Write;
    let mut f = fs::File::create(root.join(name))?;
    f.write_all(&serde_json::to_vec_pretty(v)?)?;
    f.sync_all()?;
    Ok(())
}
async fn account(profile: &Path, root: &Path) -> Result<Arc<AccountSession>> {
    let credentials = Arc::new(Credentials::open(
        Arc::new(StateStore::open(profile)?),
        ApiOrigin::Pc,
    )?);
    let api = Arc::new(ReadOnly {
        inner: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
        root: root.into(),
        sequence: Mutex::new(0),
    });
    let session = Arc::new(AccountSession::new(
        api,
        Some(Arc::new(StateStore::open(root.join("state"))?)),
    ));
    session.refresh().await?;
    Ok(session)
}
fn play(input: MediaInput) -> Result<Value> {
    let mut engine = MpvEngine::open(MpvConfig {
        library: None,
        null_output: true,
    })?;
    let media = engine.load(LoadRequest::new(input), &Cancellation::default())?;
    engine.pause(false)?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let progress = loop {
        let s = engine.poll()?;
        if s.position.is_some_and(|p| p > 0.4) {
            break s.position.unwrap();
        }
        if Instant::now() > deadline {
            return Err(Error::Media("program decode did not progress"));
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    engine.pause(true)?;
    engine.seek(10.0)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let seek = loop {
        let s = engine.poll()?;
        if s.paused && s.position.is_some_and(|p| (p - 10.0).abs() < 0.6) {
            break s.position.unwrap();
        }
        if Instant::now() > deadline {
            return Err(Error::Media("program paused seek failed"));
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    engine.stop()?;
    Ok(
        json!({"duration":media.duration,"codec":media.codec,"progress":progress,"paused_seek":seek,"null_output":true}),
    )
}
async fn run(profile: &Path, root: &Path) -> Result<Value> {
    let session = account(profile, root).await?;
    let token = session.token()?;
    let (library,books,summary,episode,queue)=session.run_for(&token,|api,_|async move{
  let api=PodcastApi::new(api);let library=api.my_library(LibraryKind::Podcast).await?;let books=api.my_library(LibraryKind::Audiobook).await?;
  let subscribed=api.legacy_library(false).await?;let created=api.legacy_library(true).await?;let purchased=api.purchased().await?;let liked=api.liked_voices().await?;let liked_count=api.liked_count().await?;
  let home=api.home("").await?;let next_home=if home.more{Some(api.home(&home.cursor).await?)}else{None};let categories=api.categories().await?;
  let category=categories.iter().find_map(|v|v["id"].as_str()).ok_or(Error::Response("no category sample"))?;
  let page=api.category(category,"000","",5).await?;let next_category=if page.more{Some(api.category(category,"000",&page.cursor,5).await?)}else{None};
  let mut histories=json!({});for (name,kind) in [("album",HistoryKind::Album),("podcast",HistoryKind::Podcast),("voice",HistoryKind::Voice),("audiobook",HistoryKind::Audiobook)]{let value=api.history(kind).await?;histories[name]=json!(value.as_array().map(Vec::len).unwrap_or_else(||value.as_object().map_or(0,|v|v.values().filter_map(Value::as_array).map(Vec::len).sum())));}
  let radio=library.items.iter().filter(|r|r.raw["fee"].as_u64().unwrap_or(0)==0&&r.raw["grayDisplay"]!=true&&r.program_count>2).min_by_key(|r|r.program_count).ok_or(Error::State("no free subscribed sample"))?;
  let radio=api.radio(&json!(radio.id)).await?;let first=api.programs(&json!(radio.id),2,0,true).await?;let second=if first.more{Some(api.programs(&json!(radio.id),2,2,true).await?)}else{None};
  let mut queue=first.items;queue.extend(second.into_iter().flat_map(|v|v.items));let reverse=api.programs(&json!(radio.id),queue.len(),0,false).await?;
  let reversed=if first.total as usize==queue.len(){if queue.iter().rev().map(|v|&v.id).collect::<Vec<_>>()!=reverse.items.iter().map(|v|&v.id).collect::<Vec<_>>(){return Err(Error::Response("program reverse order mismatch"));}true}else{false};
  let episode=api.episode(&json!(queue.first().ok_or(Error::Response("program sample missing"))?.id)).await?;
  let play=api.play_permission(&json!(episode.id)).await?;let download=api.download_permission(&[json!(episode.id)]).await?;if !play.can_play||!download.allowed.contains(&episode.id){return Err(Error::State("free sample is not authorized"));}
  let resume=api.resume_enabled(&json!(radio.id)).await?;let skip=api.skip_enabled(&json!(radio.id)).await?;
  let summary=json!({"podcasts":library.ids.len(),"audiobooks":books.ids.len(),"legacy_subscribed":subscribed.len(),"created":created.len(),"purchased":purchased.len(),"liked_voices":liked.len(),"liked_count":liked_count,"home_blocks":home.records.len(),"next_home_blocks":next_home.map(|v|v.records.len()),"categories":categories.len(),"category_page":page.records.len(),"next_category_page":next_category.map(|v|v.records.len()),"histories":histories,"program_pages":queue.len(),"reverse_verified":reversed,"resume_enabled":resume,"skip_enabled":skip});
  Ok((library,books,summary,episode,queue))
 }).await?;
    save(root, "reads-verified.json", &summary)?;
    save(root, "program.json", &episode.normalized())?;
    save(root, "queue.json", &serde_json::to_value(queue)?)?;
    let source = SourceResolver::new(session.clone(), None)
        .resolve(&episode.playback_resource()?, &token)
        .await?;
    let input = source.input.clone();
    let online = tokio::task::spawn_blocking(move || play(input))
        .await
        .map_err(|_| Error::State("program player worker failed"))??;
    save(root, "online-playback.json", &online)?;
    let service = DownloadService::new(session.clone())?;
    let task = service
        .create_voice(
            &json!(episode.id),
            DownloadDestination::Named {
                root: root.join("media"),
                path_type: 0,
                file_type: 1,
            },
            DownloadOptions::default(),
        )
        .await?;
    let paused = service
        .run(
            &task.id,
            DownloadRunOptions {
                byte_budget: Some(131071),
                ..Default::default()
            },
        )
        .await?;
    if paused.status != DownloadStatus::Paused || paused.bytes != 131071 {
        return Err(Error::State("program pause not demonstrated"));
    }
    drop(service);
    let service = DownloadService::new(session.clone())?;
    service.enqueue(&task.id)?;
    let completed = service.run(&task.id, DownloadRunOptions::default()).await?;
    if completed.status != DownloadStatus::Complete {
        return Err(Error::State("program download incomplete"));
    }
    let path = completed
        .target
        .as_ref()
        .ok_or(Error::State("program file missing"))?;
    let digest = inspect_file(path)?;
    let output = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(Error::Media("program full decode failed"));
    }
    let input = MediaInput::File(path.clone());
    let local = tokio::task::spawn_blocking(move || play(input))
        .await
        .map_err(|_| Error::State("program local player worker failed"))??;
    let offline = OfflineLibrary::new(session.clone())?;
    let checked = offline.check(&token, path, true).await?;
    if !checked.access.can_play || !checked.integrity_verified {
        return Err(Error::State("program offline verification failed"));
    }
    let files = CompletedDownloads::new(session.clone())?;
    let op = files.rename(&task.id, "program-renamed.mp3")?;
    files.resume(&op.id)?;
    fs::create_dir(root.join("moved"))?;
    let target = root.join("moved/program.mp3");
    let op = files.start(&task.id, FileAction::Move, Some(target.clone()))?;
    files.resume(&op.id)?;
    if inspect_file(&target)? != digest {
        return Err(Error::Media("program managed move changed bytes"));
    }
    let op = files.start(&task.id, FileAction::Delete, None)?;
    files.resume(&op.id)?;
    if target.exists() || !offline.records(&token)?.is_empty() {
        return Err(Error::State("program local cleanup incomplete"));
    }
    let (after, after_books) = session
        .run_for(&token, |api, _| async move {
            let api = PodcastApi::new(api);
            Ok((
                api.my_library(LibraryKind::Podcast).await?,
                api.my_library(LibraryKind::Audiobook).await?,
            ))
        })
        .await?;
    if after.ids != library.ids || after_books.ids != books.ids {
        return Err(Error::State("podcast baseline changed"));
    }
    Ok(
        json!({"ok":true,"reads":summary,"online_mpv":online,"local_mpv":local,"pause_bytes":paused.bytes,"authorization_count":completed.authorization_count,"raw_integrity":completed.raw_integrity,"output_integrity":digest,"full_decode":true,"offline_verified":true,"rename_move_delete":true,"baseline_preserved":true,"remote_mutations":0}),
    )
}
#[tokio::main]
async fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        eprintln!("usage: podcast_roundtrip EXISTING_PC_PROFILE NEW_PRIVATE_ROOT");
        std::process::exit(2);
    }
    let profile = PathBuf::from(&args[0]);
    let root = PathBuf::from(&args[1]);
    if !profile.is_absolute()
        || !profile.join("core.sqlite3").is_file()
        || !root.is_absolute()
        || root.exists()
    {
        eprintln!("existing absolute profile and new absolute proof root required");
        std::process::exit(2);
    }
    if let Err(e) = fs::create_dir(&root) {
        eprintln!("cannot create private proof directory: {e}");
        std::process::exit(2);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("private proof permissions");
    }
    match run(&profile, &root).await {
        Ok(report) => {
            save(&root, "report.json", &report).expect("save proof");
            println!("podcast native proof passed");
        }
        Err(error) => {
            let _ = save(&root, "failed.json", &json!({"error":error.to_string()}));
            eprintln!("podcast proof failed; inspect private failed.json");
            std::process::exit(1);
        }
    }
}
