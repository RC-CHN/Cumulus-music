use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

const LIST: &str = "/api/music-vip-center/app/pc/skin/list";
static LISTS: AtomicUsize = AtomicUsize::new(0);
static COVERS: AtomicUsize = AtomicUsize::new(0);
static PREVIEWS: AtomicUsize = AtomicUsize::new(0);

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_REMOTE_SKIN_PROOF").is_some()
}
pub fn report() -> Value {
    json!({
        "lists": LISTS.load(Ordering::Relaxed),
        "covers": COVERS.load(Ordering::Relaxed),
        "previews": PREVIEWS.load(Ordering::Relaxed),
    })
}
pub async fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    assert!(
        !matches!(
            path,
            "/api/vipauth/app/auth/set" | "/api/vipauth/app/auth/stop"
        ),
        "skin reading cannot write entitlement"
    );
    if path != LIST {
        return None;
    }
    assert_eq!(args, &json!({}));
    assert!(!cached);
    let reads = LISTS.fetch_add(1, Ordering::Relaxed);
    Some(match reads {
        0 => {
            tokio::time::sleep(Duration::from_millis(1450)).await;
            Ok(json!({"code":503}))
        }
        1 => Ok(json!({"code":200,"data":{"tabs":[
            {"name":"合成新品","detailVoList":[
                {"name":"碧色云间","type":15,"cornerType":3,
                 "url":"https://p6.music.126.net/obj/fixture/card.bmp?private=not-forwarded",
                 "previewUrl":"https://p6.music.126.net/obj/fixture/preview.bmp?ticket=not-forwarded",
                 "fileUrl":"https://invalid.example/private-package.zip?token=never-read"},
                {"name":"无公开预览","type":6,"cornerType":4,
                 "previewUrl":"https://interface.music.163.com/api/private"}
            ]},
            {"name":"合成精选","detailVoList":[{"name":"云层","cornerType":2}]},
            {"name":"隐藏空组","detailVoList":[]}
        ]}})),
        2 => Ok(json!({"code":200,"data":{"tabs":[]}})),
        _ => panic!("unexpected catalogue request"),
    })
}

pub struct Media;
struct Body(Cursor<Vec<u8>>);
impl MediaBody for Body {
    fn read<'a>(&'a mut self, output: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move { Ok(self.0.read(output)?) })
    }
}

fn bmp(rgb: [u8; 3]) -> Vec<u8> {
    let size = 128 * 72 * 3;
    let mut bytes = vec![0; 54 + size];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&((54 + size) as u32).to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&128_u32.to_le_bytes());
    bytes[22..26].copy_from_slice(&72_u32.to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    bytes[34..38].copy_from_slice(&(size as u32).to_le_bytes());
    for pixel in bytes[54..].chunks_exact_mut(3) {
        pixel.copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
    }
    bytes
}
impl MediaBackend for Media {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            assert_eq!((request.offset, request.if_range), (0, None));
            let bytes = match request.url.as_str() {
                "https://p6.music.126.net/obj/fixture/card.bmp" => {
                    COVERS.fetch_add(1, Ordering::Relaxed);
                    bmp([56, 134, 186])
                }
                "https://p6.music.126.net/obj/fixture/preview.bmp" => {
                    let attempt = PREVIEWS.fetch_add(1, Ordering::Relaxed);
                    if attempt == 0 {
                        tokio::time::sleep(Duration::from_millis(1450)).await;
                        return Err(Error::Media("synthetic preview interruption"));
                    }
                    bmp([99, 173, 110])
                }
                _ => panic!("unexpected skin image URL"),
            };
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([("content-length".into(), bytes.len().to_string())]),
                body: Box::new(Body(Cursor::new(bytes))),
            })
        })
    }
}
