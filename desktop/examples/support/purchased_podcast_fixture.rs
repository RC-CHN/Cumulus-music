//! Closed read and media adapter for actual My Purchases WebView actions.
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const AUDIO: &[u8] = include_bytes!("../../../core/tests/fixtures/tag_audio/seed.mp3");
const MEDIA: &str = "https://m1.music.126.net/cumulus-purchased-episode-fixture.mp3";
pub const COHORT_REVISION: &str = "synthetic-purchases-presentation";

#[derive(Default)]
pub struct Fixture {
    calls: Mutex<Vec<Value>>,
    gets: AtomicUsize,
    playback: bool,
    compact: AtomicBool,
}

fn radio(id: u64, name: &str) -> Value {
    json!({"id":id,"name":name,"programCount":0,"subed":false})
}

fn episode(id: u64, playback: bool) -> Value {
    let duration = if playback { 30_000 } else { 1_000 };
    json!({
        "productType":1,"id":id,"name":format!("合成已购单集 {id}"),"programFeeType":10,"buyed":true,
        "mainSong":{"id":100+id,"duration":duration},"duration":duration,"displayStatus":"NORMAL",
        "playCount":999,"programCount":99,
        "radio":radio(71,"合成所属播单 71")
    })
}

impl Fixture {
    pub fn new(playback: bool) -> Self {
        Self {
            playback,
            ..Self::default()
        }
    }

    pub fn set_compact(&self, value: bool) {
        self.compact.store(value, Ordering::SeqCst);
    }

    pub fn groups(
        &self,
        revision: &str,
    ) -> Result<cumulus_core::protocol::pc_experiments::PcExperimentGroups> {
        assert_eq!(revision, COHORT_REVISION);
        let group = if self.compact.load(Ordering::SeqCst) {
            "t1"
        } else {
            "c"
        };
        cumulus_core::protocol::pc_experiments::PcExperimentGroups::refreshed(&json!({
            "data":[{"expName":"PH-PC-listPage-showMore","expGroupName":group}]
        }))
    }

    pub fn response(&self, path: &str, args: &Value, cache: bool) -> Option<Result<Value>> {
        let value = match path {
            "/api/voicelist/bought/list" => {
                if args == &json!({"limit":20,"offset":0}) {
                    let mut own = radio(81, "合成已购播单 81");
                    own["productType"] = json!(0);
                    own["playCount"] = json!(145_000);
                    own["programCount"] = json!(20);
                    json!({"code":200,"data":{"count":5,"hasMore":true,"list":[
                        own,episode(81,self.playback),{"productType":7,"id":95,"name":"未知产品 95"},
                        episode(82,self.playback)
                    ]}})
                } else {
                    assert_eq!(args, &json!({"limit":20,"offset":20}));
                    let mut more = radio(90, "合成已购播单 90");
                    more["productType"] = json!(0);
                    json!({"code":200,"data":{"count":5,"hasMore":false,"list":[more]}})
                }
            }
            "/api/djradio/v3/get" => {
                let data = match args["id"].as_str() {
                    Some("81") => radio(81, "合成已购播单 81"),
                    Some("71") => radio(71, "合成所属播单 71"),
                    _ => panic!("synthetic purchase routed an unrelated radio"),
                };
                json!({"code":200,"data":data})
            }
            "/api/v6/dj/program/byradio" => {
                assert!(matches!(args["radioId"].as_str(), Some("81" | "71")));
                assert!(args["asc"].is_boolean());
                json!({"code":200,"data":{"programs":[],"count":0,"more":false,"asc":args["asc"]}})
            }
            "/api/dj/user/setting/resumeplay/get" => {
                assert!(matches!(args["radioId"].as_str(), Some("81" | "71")));
                json!({"code":200,"data":{"resumePlayValue":0}})
            }
            "/api/dj/program/detail" if self.playback => {
                let id = match args["id"].as_str() {
                    Some("81") => 81,
                    Some("82") => 82,
                    _ => panic!("synthetic purchase resolved an unrelated program"),
                };
                assert_eq!(args, &json!({"id":id.to_string()}));
                json!({"code":200,"program":episode(id,true)})
            }
            "/api/dj/play/url/check" if self.playback => match args["voiceId"].as_str() {
                Some("81") => {
                    assert_eq!(args, &json!({"voiceId":"81"}));
                    json!({"code":200,"data":{"voiceId":81,"canPlay":false}})
                }
                Some("82") => {
                    assert_eq!(args, &json!({"voiceId":"82"}));
                    json!({"code":200,"data":{"voiceId":82,"canPlay":true,"songId":182}})
                }
                _ => panic!("synthetic purchase authorized an unrelated program"),
            },
            "/api/song/enhance/player/url/v1" if self.playback => {
                let id = match args["ids"].as_str() {
                    Some(r#"["3"]"#) => "3",
                    Some(r#"["182"]"#) => "182",
                    _ => panic!("synthetic purchase requested media for an unrelated track"),
                };
                assert_eq!(
                    args,
                    &json!({"ids":format!(r#"["{id}"]"#),"level":"standard","encodeType":"aac",
                        "immerseType":"c51","trialMode":-1})
                );
                json!({"code":200,"data":[{"id":id,"url":super::MEDIA,"code":200,
                    "md5":"fixture","level":"standard","expi":120}]})
            }
            "/api/dj/voices/download/check"
            | "/api/dj/program/song/musics"
            | "/api/song/enhance/download/url/v1"
                if self.playback =>
            {
                panic!("purchased playback must not trigger download: {path}");
            }
            "/api/dj/voices/download/check" => match args["voiceIdsStr"][0].as_str() {
                Some("81") => {
                    assert_eq!(args, &json!({"voiceIdsStr":["81"]}));
                    json!({"code":200,"data":{
                        "allowDownloadVoiceIds":[],"forbidDownloadVoiceIds":[81],
                        "downloadCheckResultList":[{"voiceId":81,"canDownload":false,
                            "feeConfigs":[{"feeType":1,"guideText":"合成付费提示"}]}]
                    }})
                }
                Some("82") => {
                    assert_eq!(args, &json!({"voiceIdsStr":["82"]}));
                    json!({"code":200,"data":{
                        "allowDownloadVoiceIds":[82],"forbidDownloadVoiceIds":[],
                        "downloadCheckResultList":[{"voiceId":82,"canDownload":true}]
                    }})
                }
                _ => panic!("synthetic purchase queried another program's download rights"),
            },
            "/api/dj/program/song/musics" => {
                assert_eq!(args, &json!({"ids":["82"]}));
                json!({"code":200,"programs":[{"musics":[
                    {"bitrate":192000,"size":2097152},{"bitrate":128000,"size":1048576}
                ]}]})
            }
            "/api/dj/program/detail" => {
                assert_eq!(args, &json!({"id":"82"}));
                json!({"code":200,"program":episode(82,false)})
            }
            "/api/song/enhance/download/url/v1" => {
                assert_eq!(
                    args,
                    &json!({"id":"182","level":"higher","immerseType":"c51"})
                );
                json!({"code":200,"data":{"id":182,"code":200,"url":MEDIA,"type":"mp3",
                    "md5":"485aa2633a203a9ea13fd7416caeb9f9","size":AUDIO.len(),"level":"higher"}})
            }
            path if path.starts_with("/api/order/") || path.starts_with("/api/pay/") => {
                panic!("sealed purchase proof must not start checkout: {path}");
            }
            _ => return None,
        };
        assert!(!cache);
        self.calls.lock().unwrap().push(json!([path, args]));
        Some(Ok(value))
    }

    pub fn trace(&self) -> Value {
        json!({"calls":self.calls.lock().unwrap().clone(),"mediaGets":self.gets.load(Ordering::Acquire)})
    }
}

impl MediaBackend for Fixture {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            if self.playback
                || request.url.as_str() != MEDIA
                || request.offset != 0
                || request.if_range.is_some()
            {
                return Err(Error::State("purchased episode proof forbids other media"));
            }
            self.gets.fetch_add(1, Ordering::AcqRel);
            Ok(MediaReply {
                status: 200,
                headers: [("content-length".into(), AUDIO.len().to_string())].into(),
                body: Box::new(Body(AUDIO)),
            })
        })
    }
}

struct Body(&'static [u8]);
impl MediaBody for Body {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let len = self.0.len().min(buffer.len());
            buffer[..len].copy_from_slice(&self.0[..len]);
            self.0 = &self.0[len..];
            Ok(len)
        })
    }
}
