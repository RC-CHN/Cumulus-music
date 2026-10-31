//! Explicit read-only account proof for the intelligence API. It does not
//! start AI playback, activate a trial, or mutate playlists. Never run by CI.
use cumulus_core::{
    Error, Result,
    account::{AccountSession, credentials::Credentials},
    catalog::intelligence::{
        IntelligenceApi, IntelligenceRequest, IntelligenceSongSource, IntelligenceStart,
    },
    client::ClientError,
    playlist::PlaylistApi,
    protocol::{
        api::{Api, ApiFuture, identifier},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::{StateStore, private_directory_builder},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct Reads {
    api: Arc<dyn Api>,
    calls: Mutex<BTreeMap<String, usize>>,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if !matches!(
                path,
                "/api/w/nuser/account/get"
                    | "/api/user/playlist"
                    | "/api/v6/playlist/detail"
                    | "/api/v3/song/detail"
                    | "/api/playmode/intelligence/list"
            ) {
                return Err(Error::InvalidInput(
                    "intelligence proof only allows explicit reads",
                ));
            }
            if path == "/api/playmode/intelligence/list" {
                let mut fields = payload
                    .as_object()
                    .ok_or(Error::InvalidInput("invalid intelligence payload"))?
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                fields.sort_unstable();
                if fields != ["count", "playlistId", "songId", "startMusicId", "type"] || cache {
                    return Err(Error::InvalidInput(
                        "unexpected intelligence wire fields/cache",
                    ));
                }
            }
            *self
                .calls
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .entry(path.into())
                .or_default() += 1;
            self.api.request(path, payload, cache).await
        })
    }
}

async fn inspect(session: &AccountSession, stage: &mut &'static str) -> Result<Value> {
    session.refresh().await?;
    let token = session.token()?;
    *stage = "liked_playlist";
    let (playlist_id, liked) = session
        .run_for(&token, |api, uid| async move {
            let api = PlaylistApi::new(api);
            let lists = api.list(&json!(uid)).await?;
            let playlist = lists
                .iter()
                .find(|p| {
                    p["specialType"] == 5
                        && [&p["userId"], &p["creator"]["userId"]]
                            .into_iter()
                            .any(|id| identifier(id).ok().as_deref() == Some(uid.as_str()))
                })
                .ok_or(Error::State("no owned liked playlist"))?;
            let id = identifier(&playlist["id"])?;
            let liked = api
                .tracks(&json!(id))
                .await?
                .songs
                .songs
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            if liked.is_empty() {
                return Err(Error::State("liked playlist has no resolved metadata"));
            }
            Ok((id, liked))
        })
        .await?;
    let first = identifier(&liked[0]["id"])?;
    let mut batches = Vec::new();
    for start in [IntelligenceStart::PlayAll, IntelligenceStart::PlayOne] {
        *stage = if start == IntelligenceStart::PlayAll {
            "from_play_all"
        } else {
            "from_play_one"
        };
        let request = IntelligenceRequest {
            playlist_id: playlist_id.clone(),
            song_id: identifier(
                &liked[if start == IntelligenceStart::PlayOne {
                    1.min(liked.len() - 1)
                } else {
                    0
                }]["id"],
            )?,
            start_music_id: first.clone(),
            start,
            count: liked.len(),
        };
        let batch = session
            .run_for(&token, |api, _| async {
                IntelligenceApi::new(api).recommend(&request, &liked).await
            })
            .await?;
        batches.push(json!({"start":start,"entries":batch.entries.len(),"songInfo":batch.entries.iter().filter(|v|v.source==IntelligenceSongSource::SongInfo).count(),"liked":batch.entries.iter().filter(|v|v.source==IntelligenceSongSource::Liked).count(),"unresolved":batch.entries.iter().filter(|v|v.source==IntelligenceSongSource::Unresolved).count(),"recommendationKeys":batch.recommendations.len()}));
    }
    Ok(
        json!({"likedMetadataCount":liked.len(),"batches":batches,"seedPlayableVerified":false,"coordinatorVerified":false,"uiStartVerified":false,"resourceWrites":0}),
    )
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let profile = PathBuf::from(args.next().ok_or("explicit existing PC profile required")?);
    let output = PathBuf::from(args.next().ok_or("new private proof directory required")?);
    if args.next().is_some() || !profile.join("core.sqlite3").is_file() {
        return Err("invalid arguments".into());
    }
    private_directory_builder().create(&output)?;
    let source = rusqlite::Connection::open_with_flags(
        profile.join("core.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let raw: String = source.query_row(
        "SELECT value FROM state WHERE owner='transport' AND kind='credentials' AND key='session'",
        [],
        |row| row.get(0),
    )?;
    let record: Value = serde_json::from_str(&raw)?;
    drop(source);
    let store = Arc::new(StateStore::open(output.join("state"))?);
    // Preserve the complete original origin/domain/path flags and device ID in
    // an isolated profile. No Cookie widening, login, refresh or source writes.
    store.put("transport", "credentials", "session", &record)?;
    let credentials = Arc::new(Credentials::open(store.clone(), ApiOrigin::Pc)?);
    let reads = Arc::new(Reads {
        api: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
        calls: Mutex::new(BTreeMap::new()),
    });
    let session = AccountSession::new(reads.clone(), Some(store));
    let mut stage = "account";
    let result = inspect(&session, &mut stage).await;
    let ok = result.is_ok();
    let mut report = match result {
        Ok(report) => report,
        Err(error) => json!({"stage":stage,"errorCode":ClientError::from(error).code}),
    };
    report["ok"] = json!(ok);
    report["requests"] = json!(
        *reads
            .calls
            .lock()
            .map_err(|_| "read recorder unavailable")?
    );
    report["resourceWrites"] = json!(0);
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string(&report)?);
    if !ok {
        return Err("intelligence read proof did not complete".into());
    }
    Ok(())
}
