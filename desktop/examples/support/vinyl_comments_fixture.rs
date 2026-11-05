use cumulus_core::{Error, Result, protocol::pc_experiments::PcExperimentGroups};
use serde_json::{Value, json};
use std::sync::Mutex;

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
const THREAD: &str = "R_SO_4_1";

pub fn group() -> Option<String> {
    std::env::var("CUMULUS_VINYL_COMMENTS_PROOF")
        .ok()
        .filter(|group| matches!(group.as_str(), "t1" | "c"))
}

pub fn assignment(revision: &str) -> Result<PcExperimentGroups> {
    let group = group().ok_or(Error::State("not a vinyl comments proof"))?;
    if revision != group {
        return Err(Error::CredentialChanged);
    }
    PcExperimentGroups::refreshed(&json!({
        "data":[{"expName":"PH-PC-optimizeComment","expGroupName":group}]
    }))
}

pub fn report() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    group()?;
    if !matches!(
        path,
        "/api/resource/commentInfo/list"
            | "/api/v1/resource/comments/R_SO_4_1"
            | "/api/v1/resource/hotcomments/R_SO_4_1"
    ) {
        return None;
    }
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    let comment = |id: u32| {
        json!({"commentId":id,"content":format!("合成评论 {id}"),"time":1700000000123_u64,
            "timeStr":"合成时间","liked":false,"likedCount":2,"user":{"userId":9,"nickname":"合成作者"}})
    };
    Some(Ok(match path {
        "/api/resource/commentInfo/list" => match args["resourceIds"].as_str() {
            Some(r#"["1"]"#) => json!({"code":200,"data":[{"threadId":THREAD,"commentCount":63}]}),
            Some(r#"["2"]"#) => {
                json!({"code":200,"data":[{"threadId":"R_SO_4_2","commentCount":0}]})
            }
            _ => return Some(Err(Error::InvalidInput("unexpected vinyl count ID"))),
        },
        "/api/v1/resource/hotcomments/R_SO_4_1" => {
            json!({"code":200,"hotComments":[comment(101)],"total":1,"hasMore":false})
        }
        _ => {
            let limit = args["limit"].as_u64().unwrap_or(0) as u32;
            let offset = args["offset"].as_u64().unwrap_or(0) as u32;
            let end = (offset + limit).min(63);
            json!({"code":200,"comments":((offset+1)..=end).map(comment).collect::<Vec<_>>(),
                "hotComments":(101..107).map(comment).collect::<Vec<_>>(),"total":63,
                "more":end<63,"moreHot":true})
        }
    }))
}
