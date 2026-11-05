use cumulus_core::{Error, Result, protocol::pc_experiments::PcExperimentGroups};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
const THREAD: &str = "R_SO_4_1";
const ZERO_THREAD: &str = "R_SO_4_2";
const ADD: &str = "/api/resource/comments/add";
const REPLY: &str = "/api/v1/resource/comments/reply";
const LIKE: &str = "/api/v1/comment/like";
const UNLIKE: &str = "/api/v1/comment/unlike";
const LOST: &str = "合成回复已送达，回包丢失";
static WRITTEN: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static LIKED: AtomicBool = AtomicBool::new(false);

pub fn group() -> Option<String> {
    std::env::var("CUMULUS_VINYL_COMMENTS_PROOF")
        .ok()
        .filter(|group| matches!(group.as_str(), "t1" | "c"))
}
pub fn composer() -> bool {
    std::env::var_os("CUMULUS_VINYL_COMPOSER_PROOF").is_some() && group().as_deref() == Some("t1")
}
pub fn likes() -> bool {
    std::env::var_os("CUMULUS_VINYL_LIKES_PROOF").is_some() && group().as_deref() == Some("t1")
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
    if composer()
        && let Some(response) = composer_response(path, args)
    {
        return Some(response);
    }
    if likes()
        && let Some(response) = likes_response(path, args, "unbound")
    {
        return Some(response);
    }
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

fn row(id: &str, thread: &str, content: &str, owner: u64) -> Value {
    json!({"commentId":id,"threadId":thread,"content":content,"time":1700000000123_u64,
        "timeStr":"合成时间","liked":false,"likedCount":2,"beReplied":[],
        "user":{"userId":owner,"nickname":if owner==2 {"合成自己"} else {"合成作者"}}})
}

pub fn bound_like_read(path: &str, args: &Value) -> Result<Value> {
    likes_response(path, args, "bound_read")
        .unwrap_or(Err(Error::InvalidInput("unexpected vinyl like read")))
}

pub fn bound_like_write(path: &str, args: &Value) -> Result<Value> {
    likes_response(path, args, "bound_write")
        .unwrap_or(Err(Error::InvalidInput("unexpected vinyl like write")))
}

fn likes_response(path: &str, args: &Value, lane: &str) -> Option<Result<Value>> {
    let liked = LIKED.load(Ordering::SeqCst);
    let mut target = row("1", THREAD, "合成点赞评论", 9);
    target["liked"] = json!(liked);
    target["likedCount"] = json!(u8::from(liked));
    let reply = match path {
        LIKE | UNLIKE
            if lane == "bound_write" && *args == json!({"threadId":THREAD,"commentId":"1"}) =>
        {
            LIKED.store(path == LIKE, Ordering::SeqCst);
            if path == UNLIKE {
                Err(Error::Transport(
                    "synthetic accepted unlike with no receipt".into(),
                ))
            } else {
                Ok(json!({"code":200}))
            }
        }
        "/api/v1/resource/comments/R_SO_4_1"
            if lane == "bound_read" && args["commentId"] == "1" && args["limit"] == 1 =>
        {
            Ok(json!({"code":200,"comments":[],"total":63,"more":false,"currentComment":target}))
        }
        "/api/v1/resource/comments/R_SO_4_1"
            if lane == "bound_read" && args["offset"] == 0 && args["limit"] == 60 =>
        {
            let mut latest = vec![target.clone()];
            latest.extend((2..=60).map(|id| row(&id.to_string(), THREAD, "合成评论", 9)));
            latest[1]["liked"] = Value::Null;
            Ok(
                json!({"code":200,"comments":latest,"hotComments":[target],"total":63,
                "more":true,"moreHot":false}),
            )
        }
        "/api/v1/resource/comments/R_SO_4_1"
        | LIKE
        | UNLIKE
        | ADD
        | REPLY
        | "/api/resource/comments/delete"
        | "/api/report/reportcomment" => Err(Error::InvalidInput(
            "vinyl like proof refused this dispatch",
        )),
        _ => return None,
    };
    CALLS
        .lock()
        .unwrap()
        .push(json!({"path":path,"args":args,"lane":lane}));
    Some(reply)
}

fn track_rows(written: &[Value], thread: &str) -> Vec<Value> {
    let mut rows = written
        .iter()
        .rev()
        .filter(|row| row["threadId"] == thread)
        .cloned()
        .collect::<Vec<_>>();
    if thread == THREAD {
        rows.extend((1..=63).map(|id| row(&id.to_string(), thread, &format!("合成评论 {id}"), 9)));
    }
    rows
}

fn composer_response(path: &str, args: &Value) -> Option<Result<Value>> {
    let mut written = WRITTEN.lock().unwrap();
    let reply = match path {
        "/api/resource/commentInfo/list" => {
            let thread = match args["resourceIds"].as_str() {
                Some(r#"["1"]"#) => THREAD,
                Some(r#"["2"]"#) => ZERO_THREAD,
                _ => return Some(Err(Error::InvalidInput("unexpected composer count ID"))),
            };
            Ok(
                json!({"code":200,"data":[{"threadId":thread,"commentCount":track_rows(&written, thread).len()}]}),
            )
        }
        "/api/v1/resource/comments/R_SO_4_1" | "/api/v1/resource/comments/R_SO_4_2" => {
            let thread = path.rsplit('/').next().unwrap();
            let rows = track_rows(&written, thread);
            let total = rows.len();
            let selected = args["commentId"].as_str().unwrap_or("0");
            if selected != "0" {
                assert_eq!(args["limit"], 1);
                let target = rows.iter().find(|row| row["commentId"] == selected);
                Ok(
                    json!({"code":200,"comments":[],"total":total,"more":false,"currentComment":target}),
                )
            } else {
                let limit = args["limit"].as_u64().unwrap() as usize;
                let offset = args["offset"].as_u64().unwrap() as usize;
                let more = offset.saturating_add(limit) < total;
                let page = rows
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .collect::<Vec<_>>();
                Ok(json!({"code":200,"comments":page,"total":total,"more":more,"moreHot":false}))
            }
        }
        ADD | REPLY => {
            let thread = args["threadId"].as_str().unwrap();
            let content = args["content"].as_str().unwrap();
            let id = (9001 + written.len()).to_string();
            let mut new = row(&id, thread, content, 2);
            if path == REPLY {
                let target = args["commentId"].as_str().unwrap();
                new["parentCommentId"] = json!(target);
                new["beReplied"] = json!([{"beRepliedCommentId":target,
                    "content":format!("合成评论 {target}"),"user":{"userId":9,"nickname":"合成作者"}}]);
            }
            written.push(new.clone());
            if content == LOST {
                Err(Error::Transport(
                    "synthetic accepted reply with no receipt".into(),
                ))
            } else {
                Ok(json!({"code":200,"comment":new}))
            }
        }
        "/api/resource/comments/delete"
        | "/api/v1/comment/like"
        | "/api/v1/comment/unlike"
        | "/api/report/reportcomment" => Err(Error::InvalidInput(
            "composer proof forbids other comment writes",
        )),
        _ => return None,
    };
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    Some(reply)
}
