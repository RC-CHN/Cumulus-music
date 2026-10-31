//! Synthetic voice selection for queue collection; no network fallback.
use cumulus_core::Result;
use serde_json::{Value, json};

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    std::env::var_os("CUMULUS_QUEUE_COLLECTION_UI_PROOF")?;
    let radio = || json!({"id":77,"name":"合成声音栏目","programCount":2});
    let episode = |id: u64| json!({"id":id,"name":format!("合成声音 {id}"),"mainSong":{"id":id+1000,"name":"节目音频"},"duration":30000,"radio":radio(),"dj":{"id":9,"nickname":"Cumulus"}});
    Some(Ok(match path {
        "/api/djradio/v3/get" => json!({"code":200,"data":radio()}),
        "/api/v6/dj/program/byradio" => {
            json!({"code":200,"data":{"programs":[episode(701),episode(702)],"count":2,"more":false,"asc":args["asc"]}})
        }
        "/api/dj/program/detail" => {
            let id = args["id"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap();
            assert!([701, 702].contains(&id));
            json!({"code":200,"program":episode(id)})
        }
        "/api/dj/play/url/check" => {
            let id = args["voiceId"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap();
            json!({"code":200,"data":{"voiceId":id,"songId":id+1000,"canPlay":true}})
        }
        "/api/dj/user/setting/resumeplay/get" => json!({"code":200,"data":{"resumePlayValue":0}}),
        "/api/content/my/liked/voice" => json!({"code":200,"data":{"data":[],"hasMore":false}}),
        _ => return None,
    }))
}
