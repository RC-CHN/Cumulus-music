//! Synthetic publications use the native journal and offline ledger, without HTTP.
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    download::{
        offline::OfflineLibrary,
        service::{
            DownloadDestination, DownloadOptions, DownloadOrigin, DownloadRunOptions,
            DownloadService, DownloadStatus,
        },
        transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    },
    protocol::api::{Api, ApiFuture, identifier},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const URL: &str = "https://m1.music.126.net/offline-playlist-synthetic.wav";

pub struct Downloads {
    directory: PathBuf,
    own: PathBuf,
    foreign: PathBuf,
    bytes: Arc<[u8]>,
}

impl Downloads {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        let directory = root.join("completed-playlist-files");
        fs::create_dir(&directory)?;
        Ok(Self {
            own: directory.join("same-account.wav"),
            foreign: directory.join("other-account.wav"),
            directory,
            bytes: super::super::native_lyric_wave::native_stereo_wave(6).into(),
        })
    }

    pub async fn prepare(&self, store: Arc<StateStore>) -> Result<()> {
        self.publish(store.clone(), 2, "701", &self.own).await?;
        self.publish(store, 3, "702", &self.foreign).await
    }

    async fn publish(
        &self,
        store: Arc<StateStore>,
        owner: u64,
        song: &'static str,
        path: &Path,
    ) -> Result<()> {
        let api = Arc::new(PublicationApi {
            song,
            size: self.bytes.len(),
        });
        let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
        session.adopt(api, json!({"userId":owner}), &session.token()?)?;
        let service = DownloadService::with_backend(
            session.clone(),
            Arc::new(Media(self.bytes.clone())),
            2 * 1024 * 1024,
        )?;
        let task = service.create(
            &json!(song),
            DownloadDestination::Path {
                path: path.to_path_buf(),
            },
            DownloadOptions {
                tags: false,
                cover: false,
                lyrics: false,
                ..Default::default()
            },
        )?;
        let complete = service.run(&task.id, DownloadRunOptions::default()).await?;
        assert_eq!(complete.status, DownloadStatus::Complete);
        assert_eq!(complete.origin, DownloadOrigin::Song);
        assert_eq!(complete.song_id, song);
        assert_eq!(complete.authorization_count, 1);
        let path = path.canonicalize()?;
        assert_eq!(complete.target.as_deref(), Some(path.as_path()));
        assert_eq!(fs::read(&path)?.as_slice(), self.bytes.as_ref());
        let ledger = OfflineLibrary::new(session.clone())?;
        let record = ledger
            .record(&session.token()?, &path)?
            .expect("publication created the exact owner's ledger record");
        assert_eq!(record.task_id, complete.id);
        assert_eq!(record.song_id, song);
        assert_eq!(Some(&record.integrity), complete.output_integrity.as_ref());
        assert!(!record.encrypted);
        Ok(())
    }

    pub fn change_file(&self, action: &str) -> std::io::Result<()> {
        let path = self.own.canonicalize()?;
        assert_eq!(
            path.parent(),
            Some(self.directory.canonicalize()?.as_path())
        );
        assert!(fs::symlink_metadata(&self.own)?.file_type().is_file());
        let mut damaged = self.bytes.to_vec();
        // Keep the native WAV parseable; only the publication digest changes.
        damaged[100] ^= 1;
        match action {
            "damage-digest" => {
                assert_eq!(fs::read(&path)?.as_slice(), self.bytes.as_ref());
                fs::write(path, damaged)
            }
            "damage-length" => {
                assert_eq!(fs::read(&path)?, damaged);
                fs::write(path, &self.bytes[..self.bytes.len() - 8])
            }
            "restore" => {
                assert_eq!(fs::read(&path)?, &self.bytes[..self.bytes.len() - 8]);
                fs::write(path, &*self.bytes)
            }
            _ => unreachable!("unknown fixture-only file change"),
        }
    }

    pub fn receipt(&self, store: &StateStore) -> Result<Value> {
        let own = store.values("2", "offline_music_v1")?;
        let foreign = store.values("3", "offline_music_v1")?;
        let songs = |records: &BTreeMap<String, Value>| {
            records
                .values()
                .map(|record| record["song_id"].clone())
                .collect::<Vec<_>>()
        };
        Ok(json!({
            "sameAccountLedgerSongs": songs(&own),
            "foreignAccountLedgerSongs": songs(&foreign),
            "sameAccountIntact": fs::read(&self.own).is_ok_and(|bytes| bytes.as_slice() == self.bytes.as_ref()),
            "sameAccountSizeIntact": fs::metadata(&self.own).is_ok_and(|metadata| metadata.len() == self.bytes.len() as u64),
            "foreignAccountIntact": fs::read(&self.foreign).is_ok_and(|bytes| bytes.as_slice() == self.bytes.as_ref()),
        }))
    }
}

struct PublicationApi {
    song: &'static str,
    size: usize,
}
impl Api for PublicationApi {
    fn request<'a>(&'a self, path: &'a str, args: Value, cached: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cached);
            let id = self.song.parse::<u64>().unwrap();
            Ok(match path {
                "/api/song/enhance/download/url/v1" => {
                    assert_eq!(identifier(&args["id"])?, self.song);
                    assert_eq!(args["level"], "standard");
                    json!({"code":200,"data":{"id":id,"code":200,"url":URL,"size":self.size,
                          "type":"wav","level":"standard","flag":0,"payed":0}})
                }
                "/api/v3/song/detail" => {
                    let ids: Vec<Value> = serde_json::from_str(
                        args["c"].as_str().expect("exact synthetic song detail c"),
                    )?;
                    assert_eq!(ids.len(), 1);
                    assert_eq!(identifier(&ids[0]["id"])?, self.song);
                    json!({"code":200,"songs":[{"id":id,"name":format!("合成线上曲目 {id}"),
                        "ar":[{"name":"Cumulus"}],"al":{"name":"合成专辑"},"dt":6000,"fee":0}],
                        "privileges":[{"id":id,"fee":0,"st":0,"pl":128000,"dl":128000}]})
                }
                _ => {
                    return Err(Error::Response(
                        "unexpected synthetic song publication request",
                    ));
                }
            })
        })
    }
}

struct Media(Arc<[u8]>);
impl MediaBackend for Media {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            assert_eq!(request.url.as_str(), URL);
            assert_eq!(request.offset, 0);
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([
                    ("content-length".into(), self.0.len().to_string()),
                    ("etag".into(), "\"offline-playlist-synthetic\"".into()),
                ]),
                body: Box::new(Bytes(self.0.clone(), 0)),
            })
        })
    }
}

struct Bytes(Arc<[u8]>, usize);
impl MediaBody for Bytes {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let length = buffer.len().min(self.0.len() - self.1);
            buffer[..length].copy_from_slice(&self.0[self.1..self.1 + length]);
            self.1 += length;
            Ok(length)
        })
    }
}
