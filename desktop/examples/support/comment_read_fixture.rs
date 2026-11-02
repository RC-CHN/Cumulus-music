//! Read-only comment paging faults; unrelated startup/catalog calls use the base fixture.
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
pub struct Library {
    base: super::fixture::Library,
    mode: Mutex<String>,
    calls: Mutex<Vec<Value>>,
    pending: AtomicBool,
    delayed: AtomicUsize,
}
impl Library {
    pub fn mode(&self, mode: &str) {
        *self.mode.lock().unwrap() = mode.into();
    }
    pub fn report(&self) -> Value {
        json!({"calls":self.calls.lock().unwrap().clone(),"pending":self.pending.load(Ordering::SeqCst),"delayedCompleted":self.delayed.load(Ordering::SeqCst)})
    }
}
fn comment(id: u64, parent: u64) -> Value {
    let mut value = json!({"commentId":id,"parentCommentId":parent.to_string(),"content":format!("合成评论 {id}：一段用于检查读取恢复与阅读位置的内容。\n第二行文本。"),"time":1700000000000_u64-id,"timeStr":"合成评论时间","liked":false,"likedCount":id%99,"user":{"userId":3,"nickname":if id==101 {"LongSyntheticListenerWithoutSpaces".repeat(4)}else{format!("合成听友 {id}")}},"beReplied":[]});
    if id == 101 {
        value["user"]["avatarUrl"] =
            json!("https://p1.music.126.net/cumulus-synthetic-missing.png");
    }
    if id == 102 {
        value["user"]["avatarUrl"] = json!("https://p1.music.126.net/cumulus-synthetic-cover.png");
        value["beReplied"] = json!([{"beRepliedCommentId":99,"content":"这是一条合成的原评论。","user":{"userId":4,"nickname":"被回复的听友"}}]);
    }
    value
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let floor = path == "/api/resource/comment/floor/get";
            let latest = path.starts_with("/api/v1/resource/comments/");
            let hot = path.starts_with("/api/v1/resource/hotcomments/");
            if !(floor || latest || hot) {
                if path.contains("/comments/add")
                    || path.contains("/comments/reply")
                    || path.contains("/comments/delete")
                    || path.ends_with("/comment/like")
                    || path.ends_with("/comment/unlike")
                {
                    return Err(Error::State("read-only comment proof rejects mutations"));
                }
                return self.base.request(path, args, cache).await;
            }
            let mode = self.mode.lock().unwrap().clone();
            self.calls
                .lock()
                .unwrap()
                .push(json!({"path":path,"args":args,"mode":mode}));
            let offset = args["offset"].as_u64().unwrap_or(0);
            let time = args["time"].as_i64().unwrap_or(-1);
            let targeted = args["commentId"].as_str().is_some_and(|id| id != "0");
            if mode == "expire" {
                return Ok(json!({"code":301}));
            }
            if mode == "hold-next" && latest && offset > 0 {
                self.pending.store(true, Ordering::SeqCst);
                let released = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while self.mode.lock().unwrap().as_str() == "hold-next" {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                })
                .await;
                self.pending.store(false, Ordering::SeqCst);
                self.delayed.fetch_add(1, Ordering::SeqCst);
                if released.is_err() {
                    return Err(Error::Transport("synthetic comment hold timed out".into()));
                }
            }
            if mode == "fail-next" && latest && offset > 0
                || mode == "fail-hot" && hot
                || mode == "fail-target" && targeted
                || mode == "fail-floor-first" && floor && time == -1
                || mode == "fail-floor-next" && floor && time > 0
            {
                return Err(Error::Transport("synthetic comment read offline".into()));
            }
            let limit = args["limit"].as_u64().unwrap_or(20);
            if floor {
                let parent = args["parentCommentId"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .ok_or(Error::InvalidInput("synthetic floor parent"))?;
                let start = if time == -1 { 0 } else { 20 };
                return Ok(
                    json!({"code":200,"data":{"ownerComment":comment(parent,0),"currentComment":if start==0 {Some(comment(602,parent))}else{None},"comments":(start..(start+limit).min(40)).map(|n|comment(501+n,parent)).collect::<Vec<_>>(),"bestComments":if start==0 {vec![comment(601,parent)]}else{vec![]},"totalCount":40,"hasMore":start+limit<40,"time":if start==0 {1000}else{2000}}}),
                );
            }
            let base = if hot { 301 } else { 101 };
            let rows = (offset..(offset + limit).min(45))
                .map(|n| comment(base + n, 0))
                .collect::<Vec<_>>();
            let mut result = json!({"code":200,"total":45,"topComments":[],"hotComments":[],"comments":[],"more":offset+limit<45,"hasMore":offset+limit<45});
            result[if hot { "hotComments" } else { "comments" }] = json!(rows);
            if mode == "empty" {
                result["comments"] = json!([]);
                result["hotComments"] = json!([]);
                result["total"] = json!(0);
                result["more"] = json!(false);
                result["hasMore"] = json!(false);
            }
            if targeted && mode != "missing-target" {
                let mut row = comment(999, 0);
                row["commentId"] = args["commentId"].clone();
                result["currentComment"] = row;
            }
            Ok(result)
        })
    }
}
