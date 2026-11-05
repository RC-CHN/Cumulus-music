use super::*;
use crate::{
    account::{AccountSession, profiles::ProfileRegistry},
    download::transfer::{MediaBody, MediaReply, MediaRequest},
    podcast::{
        Episode,
        scene::{SceneFmCategory, SceneFmSnapshot},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Cursor,
    sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
};
use tokio::sync::Notify;
#[derive(Default)]
struct Backend {
    calls: Mutex<Vec<MediaRequest>>,
    mode: AtomicU8,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
    hold_image: AtomicBool,
    image_started: Notify,
    image_release: Notify,
    reads: Arc<AtomicUsize>,
}
struct Body {
    bytes: Vec<u8>,
    at: usize,
    reads: Arc<AtomicUsize>,
}
impl MediaBody for Body {
    fn read<'a>(&'a mut self, target: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let n = target.len().min(self.bytes.len() - self.at);
            target[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
            self.at += n;
            Ok(n)
        })
    }
}
fn png() -> Vec<u8> {
    let mut data = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(16, 16, image::Rgb([128, 64, 32]))
        .write_to(&mut data, ImageFormat::Png)
        .unwrap();
    data.into_inner()
}
use image::ImageFormat;
impl MediaBackend for Backend {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let header = request.url.as_str().contains("xnos=1");
            self.calls.lock().unwrap().push(request);
            if self.hold.swap(false, Ordering::SeqCst) {
                self.started.notify_one();
                self.release.notified().await;
            }
            if !header && self.hold_image.swap(false, Ordering::SeqCst) {
                self.image_started.notify_one();
                self.image_release.notified().await;
            }
            let mut headers = BTreeMap::new();
            let bytes = if header { vec![0x42; 16] } else { png() };
            if header {
                let value = match self.mode.load(Ordering::SeqCst) {
                    0 => Some("M:0A141E"),
                    1 => Some("T:102030|B:304050|V:1"),
                    2 => Some("V:1|X:noop"),
                    3 => None,
                    4 => Some("M"),
                    5 => Some("M:GGHHII"),
                    _ => panic!("unexpected image mode"),
                };
                if let Some(value) = value {
                    headers.insert("x-nos-meta-n-m-cl".into(), value.into());
                }
            } else {
                headers.insert("content-type".into(), "image/png".into());
                headers.insert("content-length".into(), bytes.len().to_string());
            }
            Ok(MediaReply {
                status: 200,
                headers,
                body: Box::new(Body {
                    bytes,
                    at: 0,
                    reads: self.reads.clone(),
                }),
            })
        })
    }
}
struct NoApi;
impl Api for NoApi {
    fn request<'a>(
        &'a self,
        _path: &'a str,
        _payload: Value,
        _retried: bool,
    ) -> ApiFuture<'a, Value> {
        Box::pin(async { panic!("palette must not call account service") })
    }
}
struct Fixture {
    _directory: tempfile::TempDir,
    client: Arc<MusicClient>,
    backend: Arc<Backend>,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(directory.path(), ApiOrigin::Pc).unwrap();
        let api = Arc::new(NoApi);
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session
            .adopt(api, json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let voices=(1..=2).map(|id| Episode::from_official(&json!({"id":id,"name":"synthetic","coverUrl":format!("http://p1.music.126.net/path/{id}.jpg?auth=PRIVATE"),"mainSong":{"id":id+100}})).unwrap()).collect();
        let state = SceneFmSnapshot {
            revision: 1,
            active_id: Some("OP_1".into()),
            pending: false,
            scenes: vec![SceneFmCategory {
                id: "OP_1".into(),
                name: "sealed".into(),
                label: String::new(),
                algorithm: String::new(),
                voices,
                selected: Some("1".into()),
            }],
        };
        registry
            .store()
            .put(
                "9",
                "scene-fm-v1",
                "current",
                &serde_json::to_value(state).unwrap(),
            )
            .unwrap();
        let backend = Arc::new(Backend::default());
        let client = Arc::new(
            MusicClient::with_session(session, registry)
                .with_scene_fm_palette_backend(backend.clone()),
        );
        Self {
            _directory: directory,
            client,
            backend,
        }
    }
    fn change(&self, selected: &str) {
        let token = self.client.session.token().unwrap();
        let mut state = SceneFmService::new(self.client.session.clone())
            .unwrap()
            .cached_for(&token)
            .unwrap()
            .unwrap();
        state.revision += 1;
        state.scenes[0].selected = Some(selected.into());
        self.client
            .registry
            .store()
            .put(
                "9",
                "scene-fm-v1",
                "current",
                &serde_json::to_value(state).unwrap(),
            )
            .unwrap();
    }
    fn change_shared_cover(&self, selected: &str) {
        let token = self.client.session.token().unwrap();
        let mut state = SceneFmService::new(self.client.session.clone())
            .unwrap()
            .cached_for(&token)
            .unwrap()
            .unwrap();
        let cover = state.selected().unwrap().cover_url.clone();
        let voice = state.scenes[0]
            .voices
            .iter_mut()
            .find(|voice| voice.id == selected)
            .unwrap();
        voice.cover_url = cover.clone();
        voice.raw["coverUrl"] = json!(cover);
        state.revision += 1;
        state.scenes[0].selected = Some(selected.into());
        self.client
            .registry
            .store()
            .put(
                "9",
                "scene-fm-v1",
                "current",
                &serde_json::to_value(state).unwrap(),
            )
            .unwrap();
    }
    async fn read(&self, revision: &str, voice: &str) -> Result<SceneFmPaletteRead> {
        Ok(self
            .client
            .scene_fm_palette_read("1", revision, voice, 1.25)
            .await?
            .value)
    }
}
fn background_json(value: SceneFmPaletteRead) -> Value {
    serde_json::to_value(value).unwrap()["background"].clone()
}
#[tokio::test]
async fn native_header_truthiness_and_exact_credential_free_targets() {
    for mode in [0, 1, 2, 5] {
        let f = Fixture::new();
        f.backend.mode.store(mode, Ordering::SeqCst);
        let value = background_json(f.read("1", "1").await.unwrap());
        assert_eq!(
            value["kind"],
            if mode == 0 {
                "gradient"
            } else {
                "theme_fallback"
            }
        );
        assert_eq!(
            f.backend.reads.load(Ordering::SeqCst),
            0,
            "header hit must not read image body"
        );
        let calls = f.backend.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].url.as_str(),
            "https://p1.music.126.net/path/1.jpg?xnos=1&imageView=&thumbnail=375y375"
        );
        assert_eq!(calls[0].offset, 0);
        assert!(calls[0].if_range.is_none());
        assert!(!serde_json::to_string(&value).unwrap().contains("PRIVATE"));
    }
}

#[tokio::test]
async fn pending_header_follows_current_native_cover_but_new_old_voice_admission_stays_rejected() {
    let f = Fixture::new();
    f.backend.hold.store(true, Ordering::SeqCst);
    let pending = {
        let client = f.client.clone();
        tokio::spawn(async move { client.scene_fm_palette_read("1", "1", "1", 1.25).await })
    };
    f.backend.started.notified().await;
    f.change_shared_cover("2");
    assert!(matches!(f.read("2", "1").await, Err(Error::StaleResponse)));
    f.backend.release.notify_one();
    let value = pending.await.unwrap().unwrap().value;
    assert_eq!(background_json(value)["kind"], "gradient");
    assert_eq!(f.backend.calls.lock().unwrap().len(), 1);
    assert_eq!(f.backend.reads.load(Ordering::SeqCst), 0);
    // A newly admitted current voice may also use the completed same-art cache.
    assert_eq!(
        background_json(f.read("2", "2").await.unwrap())["kind"],
        "gradient"
    );
    assert_eq!(f.backend.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn pending_image_and_returned_canvas_receipt_survive_same_cover_voice_changes() {
    for change_during_image in [false, true] {
        let f = Fixture::new();
        f.backend.mode.store(3, Ordering::SeqCst);
        let value = if change_during_image {
            f.backend.hold_image.store(true, Ordering::SeqCst);
            let pending = {
                let client = f.client.clone();
                tokio::spawn(async move { client.scene_fm_palette_read("1", "1", "1", 1.25).await })
            };
            f.backend.image_started.notified().await;
            f.change_shared_cover("2");
            f.backend.image_release.notify_one();
            pending.await.unwrap().unwrap().value
        } else {
            let value = f.read("1", "1").await.unwrap();
            f.change_shared_cover("2");
            value
        };
        let SceneFmPaletteRead::Sample {
            receipt,
            width,
            height,
            ..
        } = value
        else {
            panic!("expected the original image receipt");
        };
        // A rejected old-voice read must not replace the already-admitted image.
        assert!(matches!(f.read("2", "1").await, Err(Error::StaleResponse)));
        let result = f
            .client
            .scene_fm_palette_sample(
                "1",
                &receipt,
                width,
                height,
                [128, 64, 32, 255].repeat((width * height) as usize),
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(result.value).unwrap()["background"]["kind"],
            "gradient"
        );
        assert!(
            f.client
                .scene_fm_palette_sample(
                    "1",
                    &receipt,
                    width,
                    height,
                    vec![0; (width * height * 4) as usize]
                )
                .is_err()
        );
        assert_eq!(f.backend.calls.lock().unwrap().len(), 2);
        assert_eq!(
            background_json(f.read("2", "2").await.unwrap())["kind"],
            "gradient"
        );
        assert_eq!(f.backend.calls.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn a_b_a_sequence_and_old_receipt_cancel_cannot_reopen_or_clear_a_new_task() {
    let f = Fixture::new();
    f.backend.mode.store(3, Ordering::SeqCst);
    let SceneFmPaletteRead::Sample { receipt: old_a, .. } = f.read("1", "1").await.unwrap() else {
        panic!()
    };
    f.change("2");
    let SceneFmPaletteRead::Sample { receipt: old_b, .. } = f.read("2", "2").await.unwrap() else {
        panic!()
    };
    f.change("1");
    let SceneFmPaletteRead::Sample {
        receipt: current_a, ..
    } = f.read("3", "1").await.unwrap()
    else {
        panic!()
    };
    assert_ne!(old_a, current_a);
    f.client.scene_fm_palette_cancel("1", &old_a).unwrap();
    f.client.scene_fm_palette_cancel("1", &old_b).unwrap();
    for receipt in [&old_a, &old_b] {
        assert!(matches!(
            f.client
                .scene_fm_palette_sample("1", receipt, 16, 16, vec![0; 1024]),
            Err(Error::StaleResponse)
        ));
    }
    assert!(
        f.client
            .scene_fm_palette_sample("1", &current_a, 16, 16, vec![0; 1024])
            .is_ok()
    );
    assert_eq!(f.backend.calls.lock().unwrap().len(), 6);
}

#[tokio::test]
async fn a_b_a_keeps_an_old_header_future_obsolete_even_after_its_cover_returns() {
    let f = Fixture::new();
    f.backend.hold.store(true, Ordering::SeqCst);
    let old_a = {
        let client = f.client.clone();
        tokio::spawn(async move { client.scene_fm_palette_read("1", "1", "1", 1.25).await })
    };
    f.backend.started.notified().await;
    f.change("2");
    f.read("2", "2").await.unwrap();
    f.change("1");
    f.read("3", "1").await.unwrap();
    f.backend.release.notify_one();
    assert!(matches!(old_a.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(f.backend.calls.lock().unwrap().len(), 3);
}
#[tokio::test]
async fn missing_or_throwing_header_uses_bounded_canvas_receipt_once_and_caches_same_source() {
    for mode in [3, 4] {
        let f = Fixture::new();
        f.backend.mode.store(mode, Ordering::SeqCst);
        let SceneFmPaletteRead::Sample {
            receipt,
            mime_type,
            bytes,
            width,
            height,
        } = f.read("1", "1").await.unwrap()
        else {
            panic!("expected local canvas");
        };
        assert_eq!((mime_type.as_str(), width, height), ("image/png", 16, 16));
        assert_eq!(bytes, png());
        assert_eq!(
            f.backend.calls.lock().unwrap()[1].url.as_str(),
            "https://p1.music.126.net/path/1.jpg?imageView=&thumbnail=375y375&type=webp&rotate=0&tostatic=0"
        );
        assert!(
            f.client
                .scene_fm_palette_sample("1", &receipt, 1, 1, vec![0; 4])
                .is_err()
        );
        let rgba = [128, 64, 32, 255].repeat(256);
        assert_eq!(
            serde_json::to_value(
                f.client
                    .scene_fm_palette_sample("1", &receipt, 16, 16, rgba.clone())
                    .unwrap()
                    .value
            )
            .unwrap()["background"]["kind"],
            "gradient"
        );
        assert!(
            f.client
                .scene_fm_palette_sample("1", &receipt, 16, 16, rgba)
                .is_err()
        );
        f.change("1"); // Pause/sync may bump revision without changing art.
        assert_eq!(
            background_json(f.read("1", "1").await.unwrap())["kind"],
            "gradient"
        );
        assert_eq!(f.backend.calls.lock().unwrap().len(), 2);
    }
}
#[tokio::test]
async fn new_read_cancels_old_read_and_late_old_source_cannot_publish() {
    let f = Fixture::new();
    f.backend.hold.store(true, Ordering::SeqCst);
    let first = {
        let c = f.client.clone();
        tokio::spawn(async move { c.scene_fm_palette_read("1", "1", "1", 1.0).await })
    };
    f.backend.started.notified().await;
    assert_eq!(
        background_json(f.read("1", "1").await.unwrap())["kind"],
        "gradient"
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(1), first)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    f.backend.hold.store(true, Ordering::SeqCst);
    f.change("2");
    let second = {
        let c = f.client.clone();
        tokio::spawn(async move { c.scene_fm_palette_read("1", "2", "2", 1.0).await })
    };
    f.backend.started.notified().await;
    f.change("1");
    f.backend.release.notify_one();
    assert!(second.await.unwrap().is_err());
    assert_eq!(
        background_json(f.read("3", "1").await.unwrap())["kind"],
        "gradient"
    );
}
#[tokio::test]
async fn receipt_is_current_source_account_bound_and_exact_cancel_does_not_revoke_newer_one() {
    let f = Fixture::new();
    f.backend.mode.store(3, Ordering::SeqCst);
    let SceneFmPaletteRead::Sample { receipt: old, .. } = f.read("1", "1").await.unwrap() else {
        panic!()
    };
    let SceneFmPaletteRead::Sample { receipt: new, .. } = f.read("1", "1").await.unwrap() else {
        panic!()
    };
    f.client.scene_fm_palette_cancel("1", &old).unwrap();
    assert!(
        f.client
            .scene_fm_palette_sample("1", &old, 16, 16, vec![0; 1024])
            .is_err()
    );
    f.change("2");
    assert!(
        f.client
            .scene_fm_palette_sample("1", &new, 16, 16, vec![0; 1024])
            .is_err()
    );
    let before = f.backend.calls.lock().unwrap().len();
    assert!(f.read("3", "2").await.is_err());
    assert!(f.read("2", "1").await.is_err());
    f.client
        .session
        .adopt(
            Arc::new(NoApi),
            json!({"userId":10}),
            &f.client.session.token().unwrap(),
        )
        .unwrap();
    assert!(
        f.client
            .scene_fm_palette_sample("1", &new, 16, 16, vec![0; 1024])
            .is_err()
    );
    assert_eq!(f.backend.calls.lock().unwrap().len(), before);
}
#[test]
fn body_format_dimensions_and_ipc_allocations_are_bounded() {
    assert!(artwork::inspect(png(), Some("image/jpeg")).is_err());
    assert!(artwork::inspect(b"<svg/>".to_vec(), None).is_err());
    assert!(artwork::inspect(vec![0; artwork::MAX_BYTES + 1], None).is_err());
    let mut wide = Cursor::new(Vec::new());
    image::RgbImage::new(4097, 1)
        .write_to(&mut wide, ImageFormat::Png)
        .unwrap();
    assert!(artwork::inspect(wide.into_inner(), None).is_err());
    assert!(serde_json::from_value::<SceneFmPalettePixels>(json!(vec![0u8; 65_537])).is_err());
    assert!(serde_json::from_value::<SceneFmPalettePixels>(json!([0, 256])).is_err());
    for ratio in [f64::NAN, f64::INFINITY, 0.49, 4.01] {
        assert!(artwork::thumbnail_size(ratio).is_err());
    }
    for source in [
        "https://localhost/test.png",
        "https://p1.music.126.net:444/a.png",
        "file:///tmp/image.png",
        "https://p1.music.126.net/iadmusicmat/a.png",
    ] {
        assert!(artwork::source(source).is_err());
    }
}

#[test]
fn jpeg_orientation_is_native_geometry_not_a_different_resampling_rule() {
    let mut encoded = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(16, 8, image::Rgb([128, 64, 32]))
        .write_to(&mut encoded, ImageFormat::Jpeg)
        .unwrap();
    // Minimal little-endian EXIF IFD0: orientation SHORT=6 (90 clockwise).
    let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
    jpeg.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
    jpeg.extend_from_slice(exif);
    jpeg.extend_from_slice(&encoded.get_ref()[2..]);
    let image = artwork::inspect(jpeg, Some("image/jpeg")).unwrap();
    assert_eq!((image.width, image.height), (8, 16));
}

#[tokio::test]
async fn prior_directory_revision_can_color_the_current_unchanged_voice() {
    let f = Fixture::new();
    f.change("1"); // automatic preparation/synchronization advances r1 to r2.
    assert_eq!(
        background_json(f.read("1", "1").await.unwrap())["kind"],
        "gradient"
    );
    assert_eq!(f.backend.calls.lock().unwrap().len(), 1);
    f.change("2");
    let before = f.backend.calls.lock().unwrap().len();
    assert!(f.read("1", "1").await.is_err());
    assert!(f.read("4", "2").await.is_err());
    assert_eq!(f.backend.calls.lock().unwrap().len(), before);
}
