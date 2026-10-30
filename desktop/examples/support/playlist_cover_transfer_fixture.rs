//! In-memory service/NOS faults for production cover UI inspection. No HTTP.
use cumulus_core::{
    Error, Result,
    client::MusicClient,
    cloud::nos::{NosBackend, NosMethod, NosNode, NosRequest},
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_COVER_TRANSFER_PROOF").is_some()
}
#[derive(Default)]
struct State {
    mode: String,
    hold: bool,
    unreadable: bool,
    cover: Option<Value>,
    changes: u64,
    allocations: u64,
    bindings: u64,
    renewals: u64,
    chunks: u64,
    queries: u64,
    objects: BTreeMap<String, Vec<u8>>,
    task: String,
    backup: Option<(PathBuf, Vec<u8>)>,
}
pub struct Fixture {
    inner: Arc<dyn Api>,
    state: Mutex<State>,
    store: Arc<StateStore>,
    root: PathBuf,
}
impl Fixture {
    pub fn new(inner: Arc<dyn Api>, store: Arc<StateStore>, root: PathBuf) -> Self {
        assert!(enabled() && store.directory().starts_with(&root));
        Self {
            inner,
            state: Mutex::new(State::default()),
            store,
            root,
        }
    }
    pub fn report(&self) -> Value {
        let s = self.state.lock().unwrap();
        json!({"allocations":s.allocations,"bindings":s.bindings,"renewals":s.renewals,"chunks":s.chunks,"queries":s.queries,"objects":s.objects.len(),"realAccountWrites":0,"networkUploads":0})
    }
    pub async fn control(&self, client: &MusicClient, value: &Value) -> Result<()> {
        match value["kind"].as_str().unwrap() {
            "seed" => {
                let epoch = client.account()?.epoch;
                let editor = client.playlist_editor(&epoch, "3").await?.value;
                let file = self.root.join("synthetic-transfer.bmp");
                std::fs::write(&file, artwork())?;
                let task = client
                    .select_playlist_cover(&epoch, "3", &editor.revision, &file)
                    .await?
                    .value;
                self.state.lock().unwrap().task = task.id;
            }
            "mode" => {
                let mode = value["value"].as_str().unwrap();
                assert!(
                    [
                        "success",
                        "hold",
                        "allocation_unknown",
                        "upload_unknown",
                        "auth_error",
                        "binding_unknown",
                        "binding_rejected",
                        "conflict",
                        "cleanup_failure",
                        "expired"
                    ]
                    .contains(&mode)
                );
                let mut s = self.state.lock().unwrap();
                s.mode = mode.into();
                s.hold = mode == "hold";
            }
            "release" => {
                let mut s = self.state.lock().unwrap();
                s.hold = false;
                s.mode = "success".into();
            }
            "read_ready" => self.state.lock().unwrap().unreadable = false,
            "restore_cache" => {
                let backup = self
                    .state
                    .lock()
                    .unwrap()
                    .backup
                    .take()
                    .expect("synthetic cache backup");
                assert!(backup.0.starts_with(self.store.directory()));
                std::fs::write(backup.0, backup.1)?;
            }
            _ => panic!("unsupported cover UI fixture control"),
        }
        Ok(())
    }
}
impl Api for Fixture {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let mode = self.state.lock().unwrap().mode.clone();
            match path {
                "/api/w/nuser/account/get" | "/api/v6/playlist/detail" if mode == "expired" => {
                    Ok(json!({"code":301}))
                }
                "/api/v6/playlist/detail" => {
                    if self.state.lock().unwrap().unreadable {
                        return Err(Error::Transport("synthetic cover read unavailable".into()));
                    }
                    let mut value = self.inner.request(path, args, cache).await?;
                    if value["playlist"]["id"] == 3 {
                        let s = self.state.lock().unwrap();
                        if let Some(cover) = &s.cover {
                            value["playlist"]["coverImgId_str"] = cover.clone();
                        }
                        if s.changes > 0 {
                            value["playlist"]["name"] = json!("其他设备更新后的歌单");
                            value["playlist"]["description"] = json!(format!(
                                "来自另一设备的第{}次简介更新。{}",
                                s.changes,
                                "LongDescriptionWithoutWordBreaks".repeat(12)
                            ));
                        }
                    }
                    Ok(value)
                }
                "/api/nos/token/alloc" => {
                    assert_eq!(
                        args,
                        json!({"filename":"cover.jpg","ext":"jpg","type":"image","bucket":"yyimgs"})
                    );
                    let mut s = self.state.lock().unwrap();
                    s.allocations += 1;
                    if mode == "allocation_unknown" {
                        return Err(Error::Transport("synthetic allocation reply lost".into()));
                    }
                    Ok(
                        json!({"code":200,"result":{"bucket":"yyimgs","objectKey":format!("cover-ui-{}.jpg",s.allocations),"token":"synthetic-scoped-token","docId":(10000+s.allocations).to_string()}}),
                    )
                }
                "/api/nos/token/refresh" => {
                    self.state.lock().unwrap().renewals += 1;
                    Ok(if mode == "auth_error" {
                        json!({"code":403})
                    } else {
                        json!({"code":200,"result":{"token":"synthetic-renewed-token"}})
                    })
                }
                "/api/playlist/cover/update" => {
                    assert_eq!(args["id"], "3");
                    let mut s = self.state.lock().unwrap();
                    s.bindings += 1;
                    if mode == "binding_rejected" {
                        return Ok(json!({"code":403}));
                    }
                    s.cover = Some(args["coverImgId"].clone());
                    if mode == "binding_unknown" {
                        s.unreadable = true;
                        return Err(Error::Transport(
                            "synthetic binding acknowledgment lost".into(),
                        ));
                    }
                    if mode == "cleanup_failure" {
                        let path = self
                            .store
                            .directory()
                            .join(format!("playlist-cover-{}.jpg", s.task));
                        let bytes = std::fs::read(&path)?;
                        std::fs::write(&path, b"synthetic changed cache")?;
                        s.backup = Some((path, bytes));
                    }
                    Ok(json!({"code":200}))
                }
                _ => self.inner.request(path, args, cache).await,
            }
        })
    }
}
impl NosBackend for Fixture {
    fn discover<'a>(&'a self, bucket: &'a str) -> ApiFuture<'a, NosNode> {
        Box::pin(async move {
            assert_eq!(bucket, "yyimgs");
            NosNode::parse("https://nosup-ui-test.127.net")
        })
    }
    fn request(&self, request: NosRequest) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            let url = tauri::Url::parse(request.url()).unwrap();
            assert_eq!(url.host_str(), Some("nosup-ui-test.127.net"));
            let query = request.method() == NosMethod::Query;
            let params = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
            let offset: usize = params
                .get("offset")
                .map(|v| v.parse().unwrap())
                .unwrap_or(0);
            let complete = params.get("complete").is_some_and(|v| v == "true");
            let key = url.path().to_owned();
            {
                let mut s = self.state.lock().unwrap();
                if query {
                    s.queries += 1;
                } else {
                    s.chunks += 1;
                }
            }
            for i in 0..800 {
                if !self.state.lock().unwrap().hold {
                    break;
                }
                if i == 799 {
                    return Err(Error::Response("synthetic upload hold timed out"));
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            let mut s = self.state.lock().unwrap();
            if s.mode == "auth_error" {
                return Err(Error::HttpStatus {
                    endpoint: None,
                    status: 403,
                    body: None,
                });
            }
            let bytes = s.objects.entry(key).or_default();
            if !query {
                assert_eq!(offset, bytes.len());
                bytes.extend_from_slice(request.body());
            }
            let length = bytes.len();
            if complete && s.mode == "upload_unknown" {
                return Err(Error::Transport("synthetic final upload reply lost".into()));
            }
            if complete && s.mode == "conflict" {
                s.changes += 1;
            }
            Ok(json!({"offset":length,"context":"synthetic-cover-context"}))
        })
    }
}
fn artwork() -> Vec<u8> {
    let width = 1024_u32;
    let height = 1024_u32;
    let size = 54 + width * height * 3;
    let mut bytes = Vec::with_capacity(size as usize);
    bytes.extend_from_slice(b"BM");
    for n in [size, 0, 54, 40, width, height] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(24_u16.to_le_bytes());
    for n in [0_u32, size - 54, 2835, 2835, 0, 0] {
        bytes.extend(n.to_le_bytes());
    }
    let mut rng = 0x39af57bd_u32;
    for _ in 0..width * height * 3 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        bytes.push(rng as u8);
    }
    bytes
}
