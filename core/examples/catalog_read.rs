//! Explicit, read-only catalog evidence. Raw responses stay in a new private
//! directory; this executable is never run by public CI.
use cumulus_core::{
    Error,
    account::{AccountSession, credentials::Credentials, profiles::ProfileRegistry},
    catalog::details::{CatalogDetails, CatalogQuery},
    client::{ClientError, MusicClient},
    protocol::{
        api::{Api, ApiFuture},
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::{StateStore, private_directory_builder},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct Reads {
    api: Arc<dyn Api>,
    artist: String,
    responses: Mutex<Vec<(String, Value)>>,
}
impl Api for Reads {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let allowed = [
                "/api/w/nuser/account/get",
                "/api/artist/v3/detail",
                "/api/artist/detail/dynamic",
                "/api/song/enhance/privilege",
                "/api/v2/artist/songs",
                "/api/artist/introduction",
                "/api/discovery/simiArtist",
                "/api/mlog/artist/video",
                "/api/concert/list/byartist",
                "/api/album/v3/detail",
                "/api/album/v4/detail",
                "/api/album/detail/dynamic",
                "/api/album/privilege",
                "/api/resource-exposure/config",
            ];
            if !allowed.contains(&path) && path != format!("/api/artist/albums/{}", self.artist) {
                return Err(Error::InvalidInput("catalog proof only allows known reads"));
            }
            let response = self.api.request(path, payload, cache).await?;
            if path != "/api/w/nuser/account/get" {
                self.responses
                    .lock()
                    .map_err(|_| Error::State("read recorder poisoned"))?
                    .push((path.into(), response.clone()));
            }
            Ok(response)
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let profile = PathBuf::from(args.next().ok_or("explicit PC profile required")?);
    let root = PathBuf::from(
        args.next()
            .ok_or("new private evidence directory required")?,
    );
    let artist = args
        .next()
        .and_then(|s| s.into_string().ok())
        .ok_or("artist ID required")?;
    if args.next().is_some() || !profile.join("core.sqlite3").is_file() {
        return Err("unexpected arguments or missing profile".into());
    }
    CatalogQuery::Artist { id: artist.clone() }.validate()?;
    private_directory_builder().create(&root)?;
    let source = Arc::new(StateStore::open(profile)?);
    let credentials = Arc::new(Credentials::open(source, ApiOrigin::Pc)?);
    let reads = Arc::new(Reads {
        api: Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?),
        artist: artist.clone(),
        responses: Mutex::default(),
    });
    let registry = ProfileRegistry::open(&root.join("state"), ApiOrigin::Pc)?;
    let session = Arc::new(AccountSession::new(reads.clone(), Some(registry.store())));
    let client = MusicClient::with_session(session.clone(), registry);
    if let Err(error) = session.refresh().await {
        let error = ClientError::from(error);
        println!("{}", json!({"ok":false,"stage":"account","error":error}));
        return Err("existing account could not be restored".into());
    }
    let token = session.token()?;
    let service = CatalogDetails::new(session)?;
    let mut queries = vec![
        CatalogQuery::Artist { id: artist.clone() },
        CatalogQuery::ArtistDynamic { id: artist.clone() },
        CatalogQuery::ArtistSongs {
            id: artist.clone(),
            offset: 0,
            limit: 20,
        },
        CatalogQuery::ArtistAlbums {
            id: artist.clone(),
            offset: 0,
            limit: 20,
        },
        CatalogQuery::ArtistIntroduction { id: artist.clone() },
        CatalogQuery::SimilarArtists {
            id: artist.clone(),
            offset: 0,
            limit: 20,
        },
        CatalogQuery::ArtistVideos {
            id: artist.clone(),
            cursor: "0".into(),
            size: 20,
        },
        CatalogQuery::ArtistConcerts {
            id: artist.clone(),
            offset: 0,
            limit: 20,
        },
    ];
    let mut results = Vec::new();
    let mut index = 0;
    let mut album_queued = false;
    while index < queries.len() {
        let query = queries[index].clone();
        match service.page_for(&token, &query).await {
            Ok(saved) => {
                let cached = service
                    .cached_for(&token, &query)?
                    .ok_or("snapshot missing")?;
                assert_eq!(saved.response, cached.response);
                assert_eq!(saved.revision, cached.revision);
                let projected = client
                    .catalog_page(&token.generation.to_string(), query.clone(), false)
                    .await?
                    .value
                    .ok_or("projected snapshot missing")?;
                fs::write(
                    root.join(format!("view-{index}.json")),
                    serde_json::to_vec_pretty(&projected)?,
                )?;
                if !album_queued {
                    let album = saved.response["hotAlbums"]
                        .as_array()
                        .and_then(|rows| rows.first())
                        .or_else(|| saved.response["hotSongs"].as_array()?.first()?.get("al"));
                    if let Some(album) = album {
                        let id =
                            cumulus_core::catalog::metadata::AlbumMetadata::from_official(album)?
                                .id;
                        queries.push(CatalogQuery::Album { id });
                        album_queued = true;
                    }
                }
                if saved.response["more"] == true {
                    match &query {
                        CatalogQuery::ArtistSongs {
                            id,
                            offset: 0,
                            limit,
                        } => {
                            queries.push(CatalogQuery::ArtistSongs {
                                id: id.clone(),
                                offset: *limit,
                                limit: *limit,
                            });
                        }
                        CatalogQuery::ArtistAlbums {
                            id,
                            offset: 0,
                            limit,
                        } => {
                            queries.push(CatalogQuery::ArtistAlbums {
                                id: id.clone(),
                                offset: *limit,
                                limit: *limit,
                            });
                        }
                        _ => {}
                    }
                }
                fs::write(
                    root.join(format!("page-{index}.json")),
                    serde_json::to_vec_pretty(&saved)?,
                )?;
                results.push(json!({"query":query,"ok":true,"snapshot_equal":true,"typed_projection":true,"album_read":saved.album_read,"tracks":saved.songs().map(<[_]>::len)}));
            }
            Err(error) => {
                results.push(json!({"query":query,"ok":false,"error":ClientError::from(error)}))
            }
        }
        index += 1;
    }
    fs::write(
        root.join("native-responses.json"),
        serde_json::to_vec_pretty(&*reads.responses.lock().unwrap())?,
    )?;
    let report = json!({"implementation":"rust","explicit_remote_writes":0,"checks":results});
    fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    if results.iter().any(|result| result["ok"] != true) {
        return Err("one or more catalog reads failed; inspect the private report".into());
    }
    Ok(())
}
