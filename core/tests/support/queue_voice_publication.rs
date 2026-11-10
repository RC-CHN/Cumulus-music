//! Closed native queue fixture. Use the real journal/ledger and no HTTP transport.
use cumulus_core::{
    Error, Result,
    account::AccountSession,
    download::{
        service::{
            DownloadDestination, DownloadOptions, DownloadRunOptions, DownloadService, DownloadTask,
        },
        transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    },
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

pub const AUDIO: &[u8] = include_bytes!("../fixtures/tag_audio/seed.flac");
const URL: &str = "https://m1.music.126.net/queue-episode-synthetic.flac";

struct ApiFixture {
    voice: String,
    media: String,
}
impl Api for ApiFixture {
    fn request<'a>(&'a self, path: &'a str, args: Value, cached: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cached);
            let voice = self.voice.parse::<u64>().unwrap();
            let media = self.media.parse::<u64>().unwrap();
            Ok(match path {
                "/api/dj/program/detail" => {
                    assert_eq!(args["id"], self.voice);
                    json!({"code":200,"program":{
                        "id":voice,"name":"Synthetic episode","duration":120000,
                        "mainSong":{"id":media,"duration":120000},
                        "radio":{"id":7,"name":"Synthetic radio"},"dj":{"userId":7,"nickname":"Synthetic host"}
                    }})
                }
                "/api/dj/voices/download/check" => {
                    assert_eq!(args["voiceIdsStr"], json!([self.voice]));
                    json!({"code":200,"data":{"allowDownloadVoiceIds":[voice],"forbidDownloadVoiceIds":[],
                          "downloadCheckResultList":[{"voiceId":voice,"canDownload":true}]}})
                }
                "/api/song/enhance/download/url/v1" => {
                    assert_eq!(args["id"], self.media);
                    json!({"code":200,"data":{"id":media,"code":200,"url":URL,"size":AUDIO.len(),
                          "type":"flac","level":"standard","flag":0,"payed":0}})
                }
                _ => {
                    return Err(Error::Response(
                        "unexpected synthetic queue publication request",
                    ));
                }
            })
        })
    }
}
struct Media;
impl MediaBackend for Media {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            assert_eq!(request.url.as_str(), URL);
            assert_eq!(request.offset, 0);
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([
                    ("content-length".into(), AUDIO.len().to_string()),
                    ("etag".into(), "\"queue-episode\"".into()),
                ]),
                body: Box::new(Bytes(0)),
            })
        })
    }
}
struct Bytes(usize);
impl MediaBody for Bytes {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let length = buffer.len().min(AUDIO.len() - self.0);
            buffer[..length].copy_from_slice(&AUDIO[self.0..self.0 + length]);
            self.0 += length;
            Ok(length)
        })
    }
}

pub async fn publish(
    store: Arc<StateStore>,
    owner: u64,
    voice: &str,
    media: &str,
    path: PathBuf,
) -> Result<DownloadTask> {
    let api = Arc::new(ApiFixture {
        voice: voice.into(),
        media: media.into(),
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
    session.adopt(api, json!({"userId":owner}), &session.token()?)?;
    let service = DownloadService::with_backend(session, Arc::new(Media), 1024 * 1024)?;
    let task = service
        .create_voice(
            &json!(voice),
            DownloadDestination::Path { path },
            DownloadOptions {
                tags: false,
                cover: false,
                lyrics: false,
                ..Default::default()
            },
        )
        .await?;
    service.run(&task.id, DownloadRunOptions::default()).await
}
