//! Synthetic-only CDN and palette body; never makes a network request.
use cumulus_core::{
    Error,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 16, 0, 0, 0, 8, 8, 2, 0,
    0, 0, 127, 20, 232, 192, 0, 0, 0, 19, 73, 68, 65, 84, 120, 156, 99, 104, 112, 80, 32, 9, 49,
    140, 106, 160, 133, 6, 0, 246, 131, 112, 1, 96, 184, 25, 28, 0, 0, 0, 0, 73, 69, 78, 68, 174,
    66, 96, 130,
];

pub struct PaletteMedia {
    case: String,
    calls: Mutex<Vec<String>>,
    pub body_bytes: Arc<AtomicUsize>,
    held: AtomicBool,
    release: tokio::sync::watch::Sender<bool>,
}
impl PaletteMedia {
    pub fn new(case: String) -> Self {
        assert!(matches!(
            case.as_str(),
            "header"
                | "only_tb"
                | "sample"
                | "late"
                | "pending_header"
                | "pending_image"
                | "pending_canvas"
        ));
        Self {
            case,
            calls: Mutex::default(),
            body_bytes: Arc::new(AtomicUsize::new(0)),
            held: AtomicBool::new(false),
            release: tokio::sync::watch::channel(false).0,
        }
    }
    pub fn snapshot(&self) -> Value {
        json!({"urls":self.calls.lock().unwrap().clone(),
            "bodyBytes":self.body_bytes.load(Ordering::SeqCst),
            "held":self.held.load(Ordering::SeqCst),"released":*self.release.borrow()})
    }
    pub fn shared_cover(&self) -> bool {
        self.case.starts_with("pending_")
    }
    pub fn release(&self) {
        assert!(matches!(
            self.case.as_str(),
            "pending_header" | "pending_image"
        ));
        assert!(self.held.load(Ordering::SeqCst));
        self.release.send_replace(true);
    }
    async fn hold(&self) {
        let mut released = self.release.subscribe();
        self.held.store(true, Ordering::SeqCst);
        while !*released.borrow_and_update() {
            released
                .changed()
                .await
                .expect("live synthetic palette release");
        }
    }
}
impl MediaBackend for PaletteMedia {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            let url = request.url.as_str();
            assert!(url.starts_with("https://p1.music.126.net/obj/cumulus-palette/cover"));
            assert!(url.contains("601.png") || url.contains("602.png"));
            assert_eq!(request.offset, 0);
            assert!(request.if_range.is_none());
            self.calls.lock().unwrap().push(url.to_owned());
            if self.case == "pending_header" && url.contains("xnos=1")
                || self.case == "pending_image" && !url.contains("xnos=1")
            {
                self.hold().await;
            }
            if self.case == "late" && url.contains("601.png") {
                tokio::time::sleep(Duration::from_millis(1500)).await;
            }
            let mut headers = BTreeMap::from([
                ("content-type".into(), "image/png".into()),
                ("content-length".into(), PNG.len().to_string()),
            ]);
            if url.contains("xnos=1") {
                match self.case.as_str() {
                    "sample" | "pending_image" | "pending_canvas" => {}
                    "only_tb" => {
                        headers.insert("x-nos-meta-n-m-cl".into(), "T:112233|B:445566".into());
                    }
                    _ => {
                        headers.insert(
                            "x-nos-meta-n-m-cl".into(),
                            if url.contains("602.png") {
                                "M:FF0000"
                            } else {
                                "M:0A141E"
                            }
                            .into(),
                        );
                    }
                }
            }
            Ok(MediaReply {
                status: 200,
                headers,
                body: Box::new(Body {
                    offset: 0,
                    read: self.body_bytes.clone(),
                }),
            })
        })
    }
}
struct Body {
    offset: usize,
    read: Arc<AtomicUsize>,
}
impl MediaBody for Body {
    fn read<'a>(&'a mut self, output: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            if output.is_empty() {
                return Ok(0);
            }
            let remaining = PNG
                .get(self.offset..)
                .ok_or(Error::Media("invalid synthetic image offset"))?;
            let len = remaining.len().min(output.len());
            output[..len].copy_from_slice(&remaining[..len]);
            self.offset += len;
            self.read.fetch_add(len, Ordering::SeqCst);
            Ok(len)
        })
    }
}

pub fn artwork_response(mut value: Value, shared_cover: bool) -> Value {
    fn decorate(voice: &mut Value, shared_cover: bool) {
        if let Some(id) = voice["id"].as_u64() {
            assert!(matches!(id, 601 | 602));
            let artwork_id = if shared_cover { 601 } else { id };
            voice["coverUrl"] = json!(format!(
                "https://p1.music.126.net/obj/cumulus-palette/cover{artwork_id}.png"
            ));
        }
    }
    if let Some(rows) = value
        .get_mut("data")
        .and_then(Value::as_array_mut)
        .and_then(|data| data.first_mut())
        .and_then(|block| block.get_mut("creatives"))
        .and_then(Value::as_array_mut)
    {
        for row in rows {
            if let Some(voices) = row["creativeExtInfoVO"]["djPrograms"].as_array_mut() {
                for voice in voices {
                    decorate(voice, shared_cover);
                }
            }
        }
    }
    if value["program"].is_object() {
        decorate(&mut value["program"], shared_cover);
    }
    value
}
