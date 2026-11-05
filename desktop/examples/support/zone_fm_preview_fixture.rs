use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const RECOMMENDS: &str = "/api/tag/tab/fm/recommends";
const DETAILS: &str = "/api/v3/song/detail";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct InFlight;
impl InFlight {
    fn start() -> Self {
        let current = ACTIVE.fetch_add(1, Ordering::AcqRel) + 1;
        PEAK.fetch_max(current, Ordering::AcqRel);
        Self
    }
}
impl Drop for InFlight {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn enabled() -> bool {
    std::env::var("CUMULUS_VERTICAL_ZONE_PROOF").as_deref() == Ok("fm-preview")
}

pub fn report() -> Value {
    json!({"calls":*CALLS.lock().unwrap(),"active":ACTIVE.load(Ordering::Acquire),
        "peak":PEAK.load(Ordering::Acquire)})
}

fn card(rule: u32, title: &str, personalized: bool) -> Value {
    json!({"resourceType":"fm","resourceId":rule,"name":title,"hideAfterClosePersonalized":personalized,
        "coverUrl":format!("https://p1.music.126.net/synthetic-zone-card-{rule}.png?token=private")})
}

fn track(id: u32, cover: &str) -> Value {
    json!({"id":id,"name":format!("合成漫游歌曲 {id}"),"dt":30000,"fee":0,
        "al":{"id":7,"name":"合成专区漫游专辑","picUrl":cover},"ar":[{"id":9,"name":"合成歌手"}]})
}

pub async fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if matches!(
        path,
        RECOMMENDS
            | DETAILS
            | "/api/music/dislike/get"
            | "/api/v3/radio/trash/get"
            | "/api/music-vip-membership/client/vip/info"
            | "/api/song/enhance/player/url/v1"
            | "/api/v1/radio/get"
            | "/api/v1/radio/skip"
            | "/api/radio/trash/add"
            | "/api/radio/trash/del/batch"
            | "/api/music-blacklist/add"
            | "/api/music-blacklist/delete"
    ) {
        assert!(!cached);
        CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    }
    let result = match path {
        "/api/zone/tab/get" if *args == json!({"pageCode":"ROCK","tabKey":"default"}) => {
            json!({"code":200,"data":{"currentTabContent":{"tabKey":"default","tabKeyName":"全部","blocks":[{
                "componentName":"RecommendHorizontal","title":"合成漫游栏","children":[
                    card(11,"合成漫游首曲",true),
                    card(11,"同规则漫游",false),
                    card(12,"保留旧封面",false),
                    card(13,"另一首曲封面",false),
                    card(14,"最后一组首曲",false),
                    {"resourceType":"playlist","id":208,"name":"合成普通歌单"}
                ]
            }]},"tabMetas":[{"tabKey":"default","tabKeyName":"全部"}]}})
        }
        RECOMMENDS => {
            let rule = args["ruleId"].as_str().unwrap();
            assert_eq!(
                *args,
                json!({"ruleType":1,"limit":3,"ruleId":rule,
                "firstSongId":"","firstSongAlg":"","tabTagId":""})
            );
            let _current = InFlight::start();
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let data = match rule {
                "11" => json!([{"id":81,"v":5},{"id":"84"}]),
                "12" => json!([{"id":91}]),
                "13" => json!([{"id":92}]),
                "14" => json!([{"id":93}]),
                _ => return Some(Err(Error::InvalidInput("unexpected synthetic FM rule"))),
            };
            json!({"code":200,"data":data})
        }
        DETAILS => {
            if args["trialMode"] != -1 {
                return Some(Err(Error::InvalidInput(
                    "explicit playback did not reuse first batch",
                )));
            }
            let songs = match args["c"].as_str() {
                Some(r#"[{"id":81,"v":5},{"id":"84","v":0}]"#) => vec![
                    track(84, "https://p1.music.126.net/synthetic-zone-album-84.png"),
                    track(
                        81,
                        "http://p1.music.126.net/synthetic-zone-album-81.png?token=private#frag",
                    ),
                ],
                Some(r#"[{"id":91,"v":0}]"#) => {
                    vec![track(91, "https://outside.invalid/rejected.png")]
                }
                Some(r#"[{"id":92,"v":0}]"#) => vec![track(
                    92,
                    "https://p1.music.126.net/synthetic-zone-album-92.png",
                )],
                Some(r#"[{"id":93,"v":0}]"#) => vec![track(
                    93,
                    "https://p1.music.126.net/synthetic-zone-album-93.png",
                )],
                _ => return Some(Err(Error::InvalidInput("unexpected synthetic FM details"))),
            };
            json!({"code":200,"songs":songs})
        }
        "/api/music/dislike/get" => json!({"code":200,"data":{"songIdList":[],"artistIdList":[]}}),
        "/api/v3/radio/trash/get" => json!({"code":200,"count":0,"data":[],"addTime":""}),
        "/api/music-vip-membership/client/vip/info" => {
            json!({"code":200,"data":{"now":1793030000000_u64,"userId":2}})
        }
        "/api/v1/radio/get"
        | "/api/v1/radio/skip"
        | "/api/radio/trash/add"
        | "/api/radio/trash/del/batch"
        | "/api/music-blacklist/add"
        | "/api/music-blacklist/delete" => {
            return Some(Err(Error::InvalidInput(
                "forbidden in zone FM preview proof",
            )));
        }
        _ => return None,
    };
    Some(Ok(result))
}
