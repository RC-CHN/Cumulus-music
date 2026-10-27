#[path = "support/nos.rs"]
mod nos_model;
mod support;
use cumulus_core::{
    Error,
    account::AccountSession,
    cloud::nos::{NosBackend, NosNode, NosRequest, NosRunOptions, NosStatus},
    playlist::{PlaylistService, cover::*},
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use image::{DynamicImage, ImageFormat, Pixel, Rgba, RgbaImage};
use serde_json::{Value, json};
use std::{
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

fn artwork(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_fn(width, height, |x, y| {
        if x < width / 2 {
            Rgba([220, 30, 20, 255])
        } else {
            Rgba([20, 80, 220, 255])
        }
        .map(|v| if y < height / 2 { v } else { v / 2 })
    });
    let mut bytes = Cursor::new(Vec::new());
    let image = DynamicImage::ImageRgba8(image);
    if format == ImageFormat::Jpeg {
        image.to_rgb8().write_to(&mut bytes, format).unwrap();
    } else {
        image.write_to(&mut bytes, format).unwrap();
    }
    bytes.into_inner()
}
struct ApiModel {
    row: Mutex<Value>,
    calls: Mutex<Vec<(String, Value)>>,
    uid: AtomicU64,
    lose_allocation: AtomicBool,
    reject_allocation: AtomicBool,
    lose_bind: AtomicBool,
    fail_read: AtomicBool,
    allocation_gate: Mutex<Option<Arc<nos_model::Gate>>>,
}
impl Default for ApiModel {
    fn default() -> Self {
        Self {
            row: Mutex::new(json!({"id":10,"userId":1,"name":"cover test","privacy":10,
            "specialType":0,"description":"","tags":[],"coverImgId":7,"trackIds":[{"id":20,"v":0}]})),
            calls: Mutex::new(vec![]),
            uid: AtomicU64::new(1),
            lose_allocation: AtomicBool::new(false),
            reject_allocation: AtomicBool::new(false),
            lose_bind: AtomicBool::new(false),
            fail_read: AtomicBool::new(false),
            allocation_gate: Mutex::new(None),
        }
    }
}
impl ApiModel {
    fn count(&self, path: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == path)
            .count()
    }
}
impl Api for ApiModel {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            match path {
                "/api/w/nuser/account/get" => {
                    Ok(json!({"code":200,"profile":{"userId":self.uid.load(Ordering::SeqCst)}}))
                }
                "/api/v6/playlist/detail" => {
                    if self.fail_read.load(Ordering::SeqCst) {
                        return Err(Error::Transport("read unavailable".into()));
                    }
                    Ok(json!({"code":200,"playlist":self.row.lock().unwrap().clone()}))
                }
                "/api/nos/token/alloc" => {
                    assert_eq!(
                        payload,
                        json!({"filename":"cover.jpg","ext":"jpg","type":"image","bucket":"yyimgs"})
                    );
                    let gate = self.allocation_gate.lock().unwrap().take();
                    if let Some(gate) = gate {
                        gate.entered.notify_one();
                        gate.release.acquire().await.unwrap().forget();
                    }
                    if self.lose_allocation.swap(false, Ordering::SeqCst) {
                        return Err(Error::Transport("lost allocation".into()));
                    }
                    if self.reject_allocation.swap(false, Ordering::SeqCst) {
                        return Ok(json!({"code":405}));
                    }
                    Ok(
                        json!({"code":200,"result":{"bucket":"yyimgs","objectKey":"cover-test.jpg","token":"PRIVATE_SCOPED_TOKEN","docId":"9007199254740995"}}),
                    )
                }
                "/api/playlist/cover/update" => {
                    assert_eq!(payload["id"], "10");
                    self.row.lock().unwrap()["coverImgId_str"] = payload["coverImgId"].clone();
                    self.row.lock().unwrap()["coverImgUrl"] =
                        json!("http://p1.music.126.net/cover-test.jpg?private=SECRET");
                    if self.lose_bind.swap(false, Ordering::SeqCst) {
                        self.fail_read.store(true, Ordering::SeqCst);
                        return Err(Error::Transport("binding applied but reply lost".into()));
                    }
                    Ok(json!({"code":200,"url":"https://p1.music.126.net/cover-test.jpg"}))
                }
                "/api/nos/token/refresh" => Ok(json!({"code":200,"result":{"token":"renewed"}})),
                other => panic!("unexpected API {other}"),
            }
        })
    }
}

#[tokio::test]
async fn client_picker_freezes_input_replacement_is_atomic_and_native_save_cleans_up() {
    use cumulus_core::{
        account::profiles::ProfileRegistry,
        client::{MusicClient, dto::CoverCommand},
        protocol::http::ApiOrigin,
    };
    let w = World::new(256 * 1024).await;
    let registry = ProfileRegistry::open(w.dir.path(), ApiOrigin::Pc).unwrap();
    let client = MusicClient::with_session(w.session.clone(), registry)
        .with_playlist_cover_backend(w.backend.clone());
    let epoch = w.session.token().unwrap().generation.to_string();
    let path = w.dir.path().join("picked.png");
    std::fs::write(&path, artwork(ImageFormat::Png, 640, 480)).unwrap();
    let task = client
        .select_playlist_cover(&epoch, "10", &w.revision, &path)
        .await
        .unwrap()
        .value;
    let before = client
        .playlist_cover_image(&epoch, &task.id)
        .await
        .unwrap()
        .value
        .preview;
    std::fs::write(&path, b"invalid replacement").unwrap();
    assert!(
        client
            .replace_playlist_cover(&epoch, "10", &w.revision, &task.id, &path)
            .await
            .is_err()
    );
    assert_eq!(
        client
            .playlist_cover_image(&epoch, &task.id)
            .await
            .unwrap()
            .value
            .preview,
        before
    );
    std::fs::write(&path, artwork(ImageFormat::Bmp, 800, 640)).unwrap();
    let next = client
        .replace_playlist_cover(&epoch, "10", &w.revision, &task.id, &path)
        .await
        .unwrap()
        .value;
    assert_eq!(next.id, task.id);
    assert_eq!(next.width, 800);
    std::fs::write(&path, b"changed after selection").unwrap();
    let result = client
        .playlist_cover_command(
            &epoch,
            &task.id,
            CoverCommand::Save {
                crop: CoverCrop {
                    x: 400,
                    y: 0,
                    size: 320,
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(result.value.task.phase, CoverPhase::Confirmed);
    assert!(!result.value.task.cleanup_pending);
    assert_eq!(
        result.value.editor.as_ref().unwrap().cover_url.as_deref(),
        Some("https://p1.music.126.net/cover-test.jpg?param=320y320")
    );
    let json = serde_json::to_string(&result).unwrap();
    for forbidden in [
        "SECRET",
        "PRIVATE_SCOPED_TOKEN",
        "objectKey",
        "document_id",
        "source_sha256",
    ] {
        assert!(!json.contains(forbidden));
    }
    let output = w.backend.0.state.lock().unwrap().data.clone();
    let rgb = image::load_from_memory(&output).unwrap().to_rgb8();
    assert!(rgb.get_pixel(20, 20)[2] > 200);
    assert_eq!(std::fs::read(&path).unwrap(), b"changed after selection");
}

#[tokio::test]
async fn client_serializes_complete_saves_and_rejects_arbitrary_document_bindings() {
    use cumulus_core::{
        account::profiles::ProfileRegistry,
        client::{MusicClient, dto::CoverCommand},
        playlist::PlaylistCommand,
        protocol::http::ApiOrigin,
    };
    let w = World::new(256 * 1024).await;
    let registry = ProfileRegistry::open(w.dir.path(), ApiOrigin::Pc).unwrap();
    let client = Arc::new(
        MusicClient::with_session(w.session.clone(), registry)
            .with_playlist_cover_backend(w.backend.clone()),
    );
    let epoch = w.session.token().unwrap().generation.to_string();
    assert!(
        client
            .mutate_playlist(
                &epoch,
                PlaylistCommand::Cover {
                    id: "10".into(),
                    revision: w.revision.clone(),
                    document_id: "123".into()
                }
            )
            .await
            .is_err()
    );
    let path = w.dir.path().join("selected.png");
    std::fs::write(&path, artwork(ImageFormat::Png, 640, 480)).unwrap();
    assert!(matches!(
        client
            .select_playlist_cover("old", "10", &w.revision, &path)
            .await,
        Err(Error::StaleResponse)
    ));
    let id = client
        .select_playlist_cover(&epoch, "10", &w.revision, &path)
        .await
        .unwrap()
        .value
        .id;
    let gate = nos_model::Gate::new();
    *w.api.allocation_gate.lock().unwrap() = Some(gate.clone());
    let saving = tokio::spawn({
        let c = client.clone();
        let e = epoch.clone();
        let id = id.clone();
        async move {
            c.playlist_cover_command(
                &e,
                &id,
                CoverCommand::Save {
                    crop: CoverCrop {
                        x: 320,
                        y: 0,
                        size: 320,
                    },
                },
            )
            .await
        }
    });
    gate.entered.notified().await;
    assert!(matches!(
        client
            .playlist_cover_command(
                &epoch,
                &id,
                CoverCommand::Save {
                    crop: CoverCrop {
                        x: 0,
                        y: 0,
                        size: 320
                    }
                }
            )
            .await,
        Err(Error::MutationBusy)
    ));
    assert!(matches!(
        client
            .replace_playlist_cover(&epoch, "10", &w.revision, &id, &path)
            .await,
        Err(Error::MutationBusy)
    ));
    gate.release.add_permits(1);
    assert_eq!(
        saving.await.unwrap().unwrap().value.task.phase,
        CoverPhase::Confirmed
    );
    assert_eq!(w.api.count("/api/nos/token/alloc"), 1);
    assert_eq!(w.api.count("/api/playlist/cover/update"), 1);
    let output = w.backend.0.state.lock().unwrap().data.clone();
    assert!(
        image::load_from_memory(&output)
            .unwrap()
            .to_rgb8()
            .get_pixel(10, 10)[2]
            > 200
    );
}
#[derive(Default)]
struct Backend(nos_model::Backend);
impl NosBackend for Backend {
    fn discover<'a>(&'a self, bucket: &'a str) -> ApiFuture<'a, NosNode> {
        Box::pin(async move {
            assert_eq!(bucket, "yyimgs");
            NosNode::parse("https://nosup-test.127.net")
        })
    }
    fn request(&self, request: NosRequest) -> ApiFuture<'_, Value> {
        self.0.request(request)
    }
}
struct World {
    dir: tempfile::TempDir,
    store: Arc<StateStore>,
    api: Arc<ApiModel>,
    session: Arc<AccountSession>,
    backend: Arc<Backend>,
    covers: Arc<PlaylistCovers>,
    revision: String,
}
impl World {
    async fn new(chunk: usize) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(dir.path()).unwrap());
        let api = Arc::new(ApiModel::default());
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session.refresh().await.unwrap();
        let revision = PlaylistService::new(session.clone())
            .unwrap()
            .detail(&session.token().unwrap(), "10")
            .await
            .unwrap()
            .revision;
        let backend = Arc::new(Backend::default());
        let covers = Arc::new(
            PlaylistCovers::with_backend(session.clone(), backend.clone(), chunk).unwrap(),
        );
        Self {
            dir,
            store,
            api,
            session,
            backend,
            covers,
            revision,
        }
    }
    async fn selected(&self) -> String {
        self.covers
            .select(
                &self.session.token().unwrap(),
                "10",
                &self.revision,
                artwork(ImageFormat::Png, 640, 480),
            )
            .await
            .unwrap()
            .id
    }
    async fn prepared(&self) -> String {
        let id = self.selected().await;
        self.covers
            .prepare(
                &self.session.token().unwrap(),
                &id,
                CoverCrop {
                    x: 320,
                    y: 0,
                    size: 320,
                },
            )
            .unwrap();
        id
    }
    async fn uploaded(&self) -> String {
        let id = self.prepared().await;
        let v = self
            .covers
            .upload(
                &self.session.token().unwrap(),
                &id,
                NosRunOptions::default(),
            )
            .await
            .unwrap();
        assert_eq!(v.phase, CoverPhase::Uploaded);
        id
    }
    fn reopen(&self, chunk: usize) -> PlaylistCovers {
        PlaylistCovers::with_backend(self.session.clone(), self.backend.clone(), chunk).unwrap()
    }
    async fn restart(&self, chunk: usize) -> PlaylistCovers {
        let store = Arc::new(StateStore::open(self.dir.path()).unwrap());
        let session = Arc::new(AccountSession::new(self.api.clone(), Some(store)));
        session.refresh().await.unwrap();
        assert_eq!(session.token().unwrap(), self.session.token().unwrap());
        PlaylistCovers::with_backend(session, self.backend.clone(), chunk).unwrap()
    }
}

#[test]
fn official_crop_geometry_and_four_formats_are_preserved() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/playlist-cover-official.json")).unwrap();
    for row in fixture["centers"].as_array().unwrap() {
        let crop = CoverCrop::centered(
            row["width"].as_u64().unwrap() as u32,
            row["height"].as_u64().unwrap() as u32,
        );
        assert_eq!(
            json!({"x":crop.x,"y":crop.y,"width":crop.size,"height":crop.size}),
            row["result"]
        );
    }
    assert_eq!(fixture["output"]["width"], 320);
    assert_eq!(fixture["output"]["mime"], "image/jpeg");
    for format in [
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Bmp,
        ImageFormat::Gif,
    ] {
        let info = inspect_image(&artwork(format, 640, 480)).unwrap();
        assert_eq!((info.width, info.height), (640, 480));
        assert!(info.preview.starts_with("data:image/jpeg;base64,"));
    }
    assert!(inspect_image(&artwork(ImageFormat::Png, 319, 480)).is_err());
    let mut boundary = artwork(ImageFormat::Png, 320, 320);
    boundary.resize(5 * 1024 * 1024, 0); // Valid PNG followed by allowed file padding.
    assert!(inspect_image(&boundary).is_ok());
    assert!(inspect_image(&vec![0; 5 * 1024 * 1024 + 1]).is_err());
    // Browser Image applies camera orientation before crop coordinates. Keep
    // native previews and native crop coordinates in that same oriented space.
    let jpeg = artwork(ImageFormat::Jpeg, 320, 640);
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let mut oriented = jpeg[..2].to_vec();
    oriented.extend_from_slice(&[0xff, 0xe1]);
    oriented.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    oriented.extend_from_slice(exif);
    oriented.extend_from_slice(&jpeg[2..]);
    let info = inspect_image(&oriented).unwrap();
    assert_eq!((info.width, info.height), (640, 320));
}

#[tokio::test]
async fn native_crop_upload_binding_and_cleanup_keep_exact_document_identity() {
    let w = World::new(1024 * 1024).await;
    let token = w.session.token().unwrap();
    let id = w.uploaded().await;
    let output = w.backend.0.state.lock().unwrap().data.clone();
    let decoded = image::load_from_memory(&output).unwrap().to_rgb8();
    assert_eq!(decoded.dimensions(), (320, 320)); // Not resized to the 200px crop viewport.
    let pixel = decoded.get_pixel(40, 40);
    assert!(pixel[2] > 200 && pixel[0] < 30); // Selected the right (blue) region.
    let view = w.covers.bind(&token, &id).await.unwrap();
    assert_eq!(view.phase, CoverPhase::Confirmed);
    assert!(!view.may_leave_object);
    assert_eq!(
        w.api.row.lock().unwrap()["coverImgId_str"],
        "9007199254740995"
    );
    assert_eq!(
        w.covers.bind(&token, &id).await.unwrap().phase,
        CoverPhase::Confirmed
    );
    assert_eq!(w.api.count("/api/playlist/cover/update"), 1);
    let json = serde_json::to_string(&view).unwrap();
    for secret in [
        "PRIVATE_SCOPED_TOKEN",
        "docId",
        "source",
        "objectKey",
        "9007199254740995",
    ] {
        assert!(!json.contains(secret));
    }
    w.covers.cleanup(&token, &id).unwrap();
    w.covers.cleanup(&token, &id).unwrap();
    assert!(
        !w.dir
            .path()
            .join(format!("playlist-cover-{id}.jpg"))
            .exists()
    );
    assert_eq!(
        w.store
            .get("1", "playlist-covers-v1", &id)
            .unwrap()
            .unwrap()["encoded"],
        ""
    );
}

#[tokio::test]
async fn unknown_allocation_is_not_repeated_and_abandon_retains_orphan_boundary() {
    let w = World::new(1024 * 1024).await;
    let token = w.session.token().unwrap();
    let id = w.prepared().await;
    w.api.lose_allocation.store(true, Ordering::SeqCst);
    assert!(
        w.covers
            .upload(&token, &id, NosRunOptions::default())
            .await
            .is_err()
    );
    let reopened = w.restart(1024 * 1024).await;
    assert_eq!(
        reopened.get(&token, &id).unwrap().phase,
        CoverPhase::Allocating
    );
    assert!(matches!(
        reopened.upload(&token, &id, NosRunOptions::default()).await,
        Err(Error::MutationUncertain)
    ));
    assert!(reopened.retry_allocation(&token, &id).is_err());
    assert_eq!(w.api.count("/api/nos/token/alloc"), 1);
    assert_eq!(w.api.count("/api/playlist/cover/update"), 0);
    assert!(w.backend.0.calls().is_empty());
    assert!(reopened.abandon(&token, &id).unwrap().may_leave_object);
    reopened.cleanup(&token, &id).unwrap();
}

#[tokio::test]
async fn only_definitive_rejection_allows_new_allocation_attempt() {
    let w = World::new(1024 * 1024).await;
    let token = w.session.token().unwrap();
    let id = w.prepared().await;
    w.api.reject_allocation.store(true, Ordering::SeqCst);
    assert!(
        w.covers
            .upload(&token, &id, NosRunOptions::default())
            .await
            .is_err()
    );
    assert_eq!(
        w.covers.get(&token, &id).unwrap().phase,
        CoverPhase::AllocationRejected
    );
    assert!(
        w.covers
            .upload(&token, &id, NosRunOptions::default())
            .await
            .is_err()
    );
    w.covers.retry_allocation(&token, &id).unwrap();
    assert_eq!(
        w.covers
            .upload(&token, &id, NosRunOptions::default())
            .await
            .unwrap()
            .phase,
        CoverPhase::Uploaded
    );
    assert_eq!(w.api.count("/api/nos/token/alloc"), 2);
}

#[tokio::test]
async fn confirmed_transfer_is_required_and_lost_final_chunk_never_binds() {
    let w = World::new(1024 * 1024).await;
    let token = w.session.token().unwrap();
    let id = w.prepared().await;
    assert!(w.covers.bind(&token, &id).await.is_err());
    w.backend.0.fault(nos_model::Fault::LostReply);
    assert!(matches!(
        w.covers.upload(&token, &id, NosRunOptions::default()).await,
        Err(Error::MutationUncertain)
    ));
    let reopened = w.restart(1024 * 1024).await;
    assert_eq!(
        reopened.get(&token, &id).unwrap().transfer,
        Some(NosStatus::Ambiguous)
    );
    assert!(
        reopened
            .upload(&token, &id, NosRunOptions::default())
            .await
            .is_err()
    );
    assert!(reopened.bind(&token, &id).await.is_err());
    assert_eq!(w.backend.0.calls().len(), 1);
    assert_eq!(w.api.count("/api/playlist/cover/update"), 0);
}

#[tokio::test]
async fn partial_upload_reopens_at_confirmed_offset_without_reallocation() {
    let w = World::new(256).await;
    let token = w.session.token().unwrap();
    let id = w.prepared().await;
    let first = w
        .covers
        .upload(
            &token,
            &id,
            NosRunOptions {
                chunk_budget: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(first.bytes, 256);
    assert_eq!(first.transfer, Some(NosStatus::Paused));
    let reopened = w.restart(256).await;
    let result = reopened
        .upload(&token, &id, NosRunOptions::default())
        .await
        .unwrap();
    assert_eq!(result.bytes, result.total);
    assert_eq!(result.phase, CoverPhase::Uploaded);
    let calls = w.backend.0.calls();
    assert!(calls[1].query);
    assert_eq!(calls[2].offset, 256);
    assert_eq!(w.api.count("/api/nos/token/alloc"), 1);
}

#[tokio::test]
async fn lost_binding_reply_reconciles_once_after_reopen_and_cannot_be_abandoned() {
    let w = World::new(1024 * 1024).await;
    let token = w.session.token().unwrap();
    let id = w.uploaded().await;
    w.api.lose_bind.store(true, Ordering::SeqCst);
    assert!(w.covers.bind(&token, &id).await.is_err());
    assert_eq!(
        w.covers.get(&token, &id).unwrap().phase,
        CoverPhase::Binding
    );
    assert!(matches!(
        w.covers.abandon(&token, &id),
        Err(Error::MutationUncertain)
    ));
    let reopened = w.restart(1024 * 1024).await;
    w.api.fail_read.store(false, Ordering::SeqCst);
    assert_eq!(
        reopened.bind(&token, &id).await.unwrap().phase,
        CoverPhase::Confirmed
    );
    assert_eq!(
        reopened.bind(&token, &id).await.unwrap().phase,
        CoverPhase::Confirmed
    );
    assert_eq!(w.api.count("/api/playlist/cover/update"), 1);
}

#[tokio::test]
async fn before_child_crash_is_read_only_and_a_different_document_cannot_use_uploaded_status() {
    let w = World::new(1024 * 1024).await;
    let token = w.session.token().unwrap();
    let id = w.uploaded().await;
    let mut record = w
        .store
        .get("1", "playlist-covers-v1", &id)
        .unwrap()
        .unwrap();
    record["phase"] = json!("binding");
    w.store
        .put("1", "playlist-covers-v1", &id, &record)
        .unwrap();
    let reopened = w.restart(1024 * 1024).await;
    assert_eq!(
        reopened.reconcile(&token, &id).await.unwrap().phase,
        CoverPhase::Uploaded
    );
    assert_eq!(w.api.count("/api/playlist/cover/update"), 0);
    record["phase"] = json!("uploaded");
    record["allocation"]["resource"] = json!("12345");
    w.store
        .put("1", "playlist-covers-v1", &id, &record)
        .unwrap();
    assert!(reopened.bind(&token, &id).await.is_err());
    assert_eq!(w.api.count("/api/playlist/cover/update"), 0);
}

#[tokio::test]
async fn changed_playlist_requires_explicit_revision_acknowledgement() {
    let w = World::new(1024 * 1024).await;
    let token = w.session.token().unwrap();
    let id = w.uploaded().await;
    w.api.row.lock().unwrap()["name"] = json!("changed on another device");
    assert!(matches!(
        w.covers.bind(&token, &id).await,
        Err(Error::QueueChanged)
    ));
    assert_eq!(
        w.covers.get(&token, &id).unwrap().phase,
        CoverPhase::Uploaded
    );
    assert_eq!(w.api.count("/api/playlist/cover/update"), 0);
    let revision = PlaylistService::new(w.session.clone())
        .unwrap()
        .detail(&token, "10")
        .await
        .unwrap()
        .revision;
    w.covers.rebase(&token, &id, &revision).await.unwrap();
    assert_eq!(
        w.covers.bind(&token, &id).await.unwrap().phase,
        CoverPhase::Confirmed
    );
    assert_eq!(
        w.api.row.lock().unwrap()["name"],
        "changed on another device"
    );
}

#[tokio::test]
async fn ownership_overlap_and_late_account_reply_are_guarded() {
    let w = World::new(1024 * 1024).await;
    let token = w.session.token().unwrap();
    w.api.row.lock().unwrap()["userId"] = json!(2);
    assert!(
        w.covers
            .select(
                &token,
                "10",
                &w.revision,
                artwork(ImageFormat::Png, 640, 480)
            )
            .await
            .is_err()
    );
    w.api.row.lock().unwrap()["userId"] = json!(1);
    let id = w.prepared().await;
    assert!(matches!(
        w.covers
            .select(
                &token,
                "10",
                &w.revision,
                artwork(ImageFormat::Png, 640, 480)
            )
            .await,
        Err(Error::MutationBusy)
    ));
    assert!(
        w.covers
            .prepare(
                &token,
                &id,
                CoverCrop {
                    x: u32::MAX,
                    y: 0,
                    size: 320
                }
            )
            .is_err()
    );
    let gate = nos_model::Gate::new();
    *w.api.allocation_gate.lock().unwrap() = Some(gate.clone());
    let job = tokio::spawn({
        let covers = w.covers.clone();
        let id = id.clone();
        let token = token.clone();
        async move { covers.upload(&token, &id, NosRunOptions::default()).await }
    });
    gate.entered.notified().await;
    w.api.uid.store(2, Ordering::SeqCst);
    w.session.refresh().await.unwrap();
    gate.release.add_permits(1);
    assert!(matches!(job.await.unwrap(), Err(Error::StaleResponse)));
    let current = w.session.token().unwrap();
    assert!(w.covers.get(&current, &id).is_err());
    assert!(w.reopen(1024 * 1024).list(&current).unwrap().is_empty());
    assert!(w.backend.0.calls().is_empty());
    assert_eq!(
        w.store
            .get("1", "playlist-covers-v1", &id)
            .unwrap()
            .unwrap()["phase"],
        "allocating"
    );
}

#[tokio::test]
async fn parent_commit_failure_recovers_existing_child_and_cleanup_preserves_foreign_file() {
    let w = World::new(1024 * 1024).await;
    let token = w.session.token().unwrap();
    let id = w.uploaded().await;
    let db = rusqlite::Connection::open(w.dir.path().join("core.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_cover_commit BEFORE INSERT ON state WHEN NEW.kind='playlist-covers-v1' AND json_extract(NEW.value,'$.phase')='confirmed' BEGIN SELECT RAISE(FAIL,'cover parent failure'); END;").unwrap();
    assert!(matches!(
        w.covers.bind(&token, &id).await,
        Err(Error::Storage(_))
    ));
    assert_eq!(
        w.covers.get(&token, &id).unwrap().phase,
        CoverPhase::Binding
    );
    db.execute_batch("DROP TRIGGER fail_cover_commit;").unwrap();
    let reopened = w.restart(1024 * 1024).await;
    assert_eq!(
        reopened.reconcile(&token, &id).await.unwrap().phase,
        CoverPhase::Confirmed
    );
    assert_eq!(w.api.count("/api/playlist/cover/update"), 1);
    let path = w.dir.path().join(format!("playlist-cover-{id}.jpg"));
    std::fs::write(&path, b"foreign replacement").unwrap();
    assert!(reopened.cleanup(&token, &id).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"foreign replacement");
}
