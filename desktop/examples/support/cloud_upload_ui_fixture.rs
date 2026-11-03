//! Production upload pipeline with actual synthetic FLAC/cover/text bytes and
//! ffprobe; only API/object storage and OS picker selection are isolated doubles.
#[path = "../../../core/tests/support/cloud_upload.rs"]
mod adapters;
use cumulus_core::{Result, cloud::upload::UploadAdapters};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
const AUDIO: &[u8] = include_bytes!("../../../core/tests/fixtures/tag_audio/seed.flac");
const COVER: &[u8] = include_bytes!("../../../core/tests/fixtures/tag_audio/cover.png");
const LYRIC: &[u8] = "[00:00.00]Cumulus 合成上传歌词\n[00:01.00]第二行\n".as_bytes();
const CUE: &[u8] = b"FILE \"same.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n";
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_CLOUD_UPLOAD_UI_PROOF").is_some()
}
#[derive(Default)]
struct State {
    serial: u64,
    calls: BTreeMap<String, usize>,
    inputs: BTreeMap<String, (String, u64)>,
    metadata: BTreeMap<u64, Value>,
    published: BTreeMap<u64, Value>,
    attachment_metadata: Vec<Vec<String>>,
}
#[derive(Default)]
pub struct Fixture {
    state: Mutex<State>,
    objects: Arc<adapters::Objects>,
    seeded: Mutex<Vec<PathBuf>>,
    seed_lock: tokio::sync::Mutex<()>,
    held: AtomicBool,
    hold_path: Mutex<String>,
    hold_entered: AtomicUsize,
    release: tokio::sync::Notify,
    lose_allocation: AtomicBool,
    image_only_text: AtomicBool,
    encoding_mode: AtomicUsize,
}
impl Fixture {
    pub async fn seed(
        &self,
        root: &Path,
        session: Arc<cumulus_core::account::AccountSession>,
        count: usize,
    ) -> Result<Vec<String>> {
        let _guard = self.seed_lock.lock().await;
        let start = self.seeded.lock().unwrap().len();
        if count == 0 || start + count > 500 {
            return Err(cumulus_core::Error::InvalidInput(
                "invalid synthetic task count",
            ));
        }
        let manager =
            cumulus_core::cloud::upload::CloudUploads::with_adapters(session, self.adapters())?;
        let mut ids = Vec::new();
        for index in start..start + count {
            let source = root.join("upload-inputs/many").join(format!(
                "{index:03}-用于检查长文件名和路径显示的合成音频.flac"
            ));
            let task = manager.create(&source, Default::default()).await?;
            self.seeded.lock().unwrap().push(source);
            ids.push(task.id);
        }
        Ok(ids)
    }
    pub fn configure(&self, value: &Value) {
        if let Some(mode) = value["encodingMode"].as_u64() {
            assert!(mode <= 2);
            self.encoding_mode.store(mode as usize, Ordering::Release);
        }

        if let Some(path) = value["holdPath"].as_str() {
            assert!([adapters::CHECK, adapters::ALLOC, ""].contains(&path));
            *self.hold_path.lock().unwrap() = path.into();
        }
        if let Some(held) = value["hold"].as_bool() {
            self.hold_entered.store(0, Ordering::Release);
            self.held.store(held, Ordering::Release);
            if !held {
                self.release.notify_one();
            }
        }
        if let Some(blocked) = value["imageOnlyText"].as_bool() {
            self.image_only_text.store(blocked, Ordering::Release);
        }
        if let Some(lost) = value["loseAllocation"].as_bool() {
            self.lose_allocation.store(lost, Ordering::Release);
        }
    }
    pub fn adapters(&self) -> UploadAdapters {
        UploadAdapters {
            nos: self.objects.clone(),
            oss: Arc::new(adapters::oss::Backend::default()),
            chunk_size: 4096,
            ..UploadAdapters::native().expect("native clock and ffprobe adapters")
        }
    }
    pub fn initialize(&self, root: &Path) -> Result<()> {
        let files = root.join("upload-inputs");
        for folder in ["first", "second", "many"] {
            fs::create_dir_all(files.join(folder))?;
        }
        for name in [
            "first/same.flac",
            "second/same.flac",
            "long.flac",
            "extra.flac",
        ] {
            fs::write(files.join(name), AUDIO)?;
        }
        fs::write(files.join("cover.png"), COVER)?;
        fs::write(files.join("invalid.png"), b"synthetic invalid image")?;
        fs::write(files.join("lyrics.lrc"), LYRIC)?;
        fs::write(files.join("sheet.cue"), CUE)?;
        for i in 0..500 {
            fs::write(
                files
                    .join("many")
                    .join(format!("{i:03}-用于检查长文件名和路径显示的合成音频.flac")),
                AUDIO,
            )?;
        }
        Ok(())
    }
    pub fn selection(&self, root: &Path, name: &str) -> Vec<PathBuf> {
        let files = root.join("upload-inputs");
        let paths: Vec<&str> = match name {
            "pair" => vec!["first/same.flac", "second/same.flac"],
            "single" => vec!["long.flac"],
            "extra" => vec!["extra.flac"],
            "many" => vec!["many"],
            "cover" => vec!["cover.png"],
            "invalid" => vec!["invalid.png"],
            "lyric" => vec!["lyrics.lrc"],
            "cue" => vec!["sheet.cue"],
            "cancel" => vec![],
            _ => panic!("unknown isolated upload selection"),
        };
        paths.into_iter().map(|p| files.join(p)).collect()
    }
    pub fn report(&self) -> Value {
        let state = self.state.lock().unwrap();
        let seeded = self.seeded.lock().unwrap();
        let seeded_count = seeded.len();
        let seeded_intact = seeded
            .iter()
            .all(|path| fs::read(path).is_ok_and(|data| data == AUDIO));
        drop(seeded);
        let objects = self.objects.objects.lock().unwrap().iter().map(|(key, backend)| {
            let stored = backend.state.lock().unwrap();
            json!({"key":key,"bytes":stored.data.len(),"matchesFixture": ([AUDIO,COVER,LYRIC,CUE].contains(&stored.data.as_slice())), "requests":stored.calls.len()})
        }).collect::<Vec<_>>();
        json!({"calls":state.calls,"publishedIds":state.published.keys().collect::<Vec<_>>(),"attachmentMetadata":state.attachment_metadata,"objects":objects,"realRequests":0,"probe":"ffprobe","held":self.held.load(Ordering::Acquire),"holdEntered":self.hold_entered.load(Ordering::Acquire),"seededInputs":seeded_count,"seededInputsIntact":seeded_intact})
    }
    pub async fn response(&self, path: &str, args: &Value) -> Option<Result<Value>> {
        if !enabled()
            || !matches!(
                path,
                adapters::CHECK
                    | adapters::ALLOC
                    | adapters::ATTACH
                    | adapters::META
                    | adapters::STATUS
                    | adapters::PUBLISH
                    | adapters::LIST
            )
        {
            return None;
        }
        let hold_path = self.hold_path.lock().unwrap().clone();
        if self.held.load(Ordering::Acquire) && hold_path == path {
            self.hold_entered.fetch_add(1, Ordering::AcqRel);
            while self.held.load(Ordering::Acquire) {
                self.release.notified().await;
            }
        }
        let mut state = self.state.lock().unwrap();
        *state.calls.entry(path.into()).or_default() += 1;
        if path == adapters::ALLOC && self.lose_allocation.swap(false, Ordering::AcqRel) {
            return Some(Err(cumulus_core::Error::Transport(
                "synthetic allocation reply lost".into(),
            )));
        }

        Some(Ok(match path {
            adapters::CHECK => {
                state.serial += 1;
                let ticket = format!("synthetic-ticket-{}", state.serial);
                state.inputs.insert(
                    ticket.clone(),
                    (
                        args["fileName"].as_str().unwrap().into(),
                        args["length"].as_u64().unwrap(),
                    ),
                );
                json!({"code":200,"needUpload":true,"songId":ticket})
            }
            adapters::ALLOC | adapters::ATTACH => {
                state.serial += 1;
                let key = format!("object-{}", state.serial);
                let mut data = adapters::allocation(&key, state.serial);
                if path == adapters::ATTACH
                    && args["type"] == "other"
                    && self.image_only_text.load(Ordering::Acquire)
                {
                    data["bucket"] = json!("yyimgs");
                }
                json!({"code":200,"data":data})
            }
            adapters::META => {
                let ticket = args["songid"].as_str().unwrap();
                let (name, size) = state.inputs.get(ticket).unwrap().clone();
                let id = 700
                    + ticket
                        .strip_prefix("synthetic-ticket-")
                        .unwrap()
                        .parse::<u64>()
                        .unwrap();
                let metadata = ["coverid", "lyricid", "cueid"]
                    .into_iter()
                    .filter(|k| args.get(*k).is_some())
                    .map(String::from)
                    .collect::<Vec<_>>();
                state.attachment_metadata.push(metadata);
                let row = json!({"songId":id,"fileName":name,"fileSize":size,"addTime":1700000000000u64,"bitrate":320000,"simpleSong":{"name":"合成上传音频","ar":[],"al":{"name":"上传流程验证"},"dt":2000}});
                state.metadata.insert(id, row);
                let ready = self.encoding_mode.load(Ordering::Acquire) == 0;
                json!({"code":200,"songId":id,"exists":ready,"waitTime":0,"privateCloud":{"songId":id,"simpleSong":{"mst":if ready{9}else{0}}}})
            }
            adapters::PUBLISH => {
                let id = args["songid"].as_str().unwrap().parse::<u64>().unwrap();
                let row = state.metadata.get(&id).unwrap().clone();
                state.published.insert(id, row);
                json!({"code":201,"privateCloud":{"songId":id}})
            }
            adapters::LIST => {
                let offset = args["offset"].as_u64().unwrap() as usize;
                json!({"code":200,"size":1234,"maxSize":1073741824,"hasMore":false,"data":state.published.values().skip(offset).take(200).collect::<Vec<_>>()})
            }
            adapters::STATUS => {
                let ids: Vec<Value> =
                    serde_json::from_str(args["songIds"].as_str().unwrap()).unwrap();
                let status = if self.encoding_mode.load(Ordering::Acquire) == 1 {
                    -1
                } else {
                    9
                };
                let statuses = ids
                    .into_iter()
                    .map(|id| {
                        let id = id
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| id.to_string());
                        (id, json!({"status":status,"waitTime":0}))
                    })
                    .collect::<BTreeMap<_, _>>();
                json!({"code":200,"statuses":statuses})
            }
            _ => panic!("unexpected cloud task endpoint"),
        }))
    }
}
