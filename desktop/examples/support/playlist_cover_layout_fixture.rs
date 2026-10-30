//! Synthetic local artwork and rejected allocation for cropper UI inspection.
//! Image decoding, crop geometry, journals and IPC still use the real core.
use cumulus_core::{
    Error, Result,
    client::MusicClient,
    cloud::nos::{NosBackend, NosNode, NosRequest},
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Mutex, OnceLock},
};

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_COVER_LAYOUT_PROOF").is_some()
}
fn mode() -> &'static Mutex<String> {
    static MODE: OnceLock<Mutex<String>> = OnceLock::new();
    MODE.get_or_init(|| Mutex::new(String::new()))
}
pub async fn allocation() -> Result<Value> {
    for _ in 0..800 {
        if *mode().lock().unwrap() != "hold" {
            return Ok(json!({"code":405}));
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    Err(Error::Response("synthetic allocation hold timed out"))
}
pub struct NoNetwork;
impl NosBackend for NoNetwork {
    fn discover<'a>(&'a self, _: &'a str) -> ApiFuture<'a, NosNode> {
        Box::pin(async { Err(Error::State("cover layout proof never uploads")) })
    }
    fn request(&self, _: NosRequest) -> ApiFuture<'_, Value> {
        Box::pin(async { Err(Error::State("cover layout proof never uploads")) })
    }
}

// A lossless 24-bit BMP generated from coordinates, with no external asset or
// image library. Widths are multiples of four, so rows need no extra padding.
fn artwork(width: u32, height: u32) -> Vec<u8> {
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
    for y in 0..height {
        for x in 0..width {
            bytes.extend([
                (70 + 150 * x / width) as u8,
                (60 + 150 * y / height) as u8,
                if (x / 80 + y / 80) % 2 == 0 { 230 } else { 120 },
            ]);
        }
    }
    bytes
}

pub async fn control(client: &MusicClient, root: &Path, value: &Value) -> Result<()> {
    assert!(enabled());
    let kind = value["kind"].as_str().unwrap();
    if kind == "files" {
        std::fs::write(root.join("synthetic-cover.bmp"), artwork(640, 480))?;
        std::fs::write(root.join("invalid-cover.bmp"), artwork(100, 100))?;
        return Ok(());
    }
    if ["hold", "reject"].contains(&kind) {
        *mode().lock().unwrap() = kind.into();
        return Ok(());
    }
    let (width, height) = match kind {
        "landscape" => (640, 480),
        "portrait" => (320, 960),
        "wide" => (1280, 320),
        _ => panic!("unsupported cover fixture shape"),
    };
    let epoch = client.account()?.epoch;
    let editor = client.playlist_editor(&epoch, "3").await?.value;
    let file = root.join("synthetic-cover.bmp");
    std::fs::write(&file, artwork(width, height))?;
    client
        .select_playlist_cover(&epoch, "3", &editor.revision, &file)
        .await?;
    Ok(())
}
