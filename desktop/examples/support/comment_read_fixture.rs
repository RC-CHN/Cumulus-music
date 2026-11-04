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
        let emoji = cumulus_core::protocol::wire::serialization::encode_component(
            r#"{"text":"[大笑]","url":"never-render-fixture-private"}"#,
        );
        value["richContent"] = json!(format!(
            r#"提到<song id="private-song-key">合成曲目</song><c0m_cvt j="{emoji}" href="private-attribute"/>"#
        ));
        value["user"]["avatarUrl"] =
            json!("https://p1.music.126.net/cumulus-synthetic-missing.png");
    }
    if id == 102 {
        value["user"]["avatarUrl"] = json!("https://p1.music.126.net/cumulus-synthetic-cover.png");
        value["beReplied"] = json!([{"beRepliedCommentId":99,"content":"这是一条合成的原评论。","richContent":"引用<song id=\"private-reply-key\">合成引用曲目</song>","user":{"userId":4,"nickname":"被回复的听友"}}]);
    }
    if id == 103 {
        value["user"]["accountStatus"] = json!(40);
    }
    value
}
fn radio() -> Value {
    json!({"id":71,"name":"合成播单 · A radio with a long title","programCount":1,"dj":{"userId":3,"nickname":"合成主播"}})
}
fn voice() -> Value {
    json!({"id":81,"name":"合成声音 · An episode with a long title","mainTrackId":181,"duration":30000,"coverUrl":"https://p1.music.126.net/cumulus-synthetic-cover.png","radio":radio(),"dj":{"userId":3,"nickname":"合成主播"}})
}
fn video() -> Value {
    json!({"id":"123","isMv":true,"resource":{"threadId":"R_MV_5_123","mlogBaseData":{"type":3,"text":"合成MV · LongSyntheticVideoTitleWithoutBreaksLongSyntheticVideoTitleWithoutBreaks","coverUrl":"https://p1.music.126.net/cumulus-synthetic-cover.png","duration":30000},"mlogExtVO":{"artists":[{"id":12,"name":"测试歌手"}]}}})
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let mode = self.mode.lock().unwrap().clone();
            let metadata = matches!(
                path,
                "/api/v3/song/detail" | "/api/dj/program/detail" | "/api/mlog/detail/v1"
            );
            if metadata {
                self.calls
                    .lock()
                    .unwrap()
                    .push(json!({"path":path,"args":args,"mode":mode}));
                if mode == "resource-offline" {
                    return Err(Error::Transport(
                        "synthetic resource information offline".into(),
                    ));
                }
                if mode == "hold-resource" {
                    self.pending.store(true, Ordering::SeqCst);
                    let released = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                        while self.mode.lock().unwrap().as_str() == "hold-resource" {
                            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        }
                    })
                    .await;
                    self.pending.store(false, Ordering::SeqCst);
                    self.delayed.fetch_add(1, Ordering::SeqCst);
                    if released.is_err() {
                        return Err(Error::Transport("synthetic resource hold timed out".into()));
                    }
                }
                if path == "/api/v3/song/detail" {
                    let rows: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    return Ok(
                        json!({"code":200,"songs":rows.into_iter().map(|row|json!({"id":row["id"],"name":format!("合成歌曲 {} · A song for comment resource navigation",row["id"].as_str().unwrap()),"dt":30000,"ar":[{"id":12,"name":"测试歌手"},{"id":0,"name":"未关联署名"}],"al":{"id":7,"name":"合成专辑 · A long album name","picUrl":if row["id"]=="2" {"https://p1.music.126.net/cumulus-synthetic-missing.png"}else{"https://p1.music.126.net/cumulus-synthetic-cover.png"}}})).collect::<Vec<_>>()}),
                    );
                }
                return Ok(if path == "/api/dj/program/detail" {
                    json!({"code":200,"program":voice()})
                } else {
                    json!({"code":200,"data":video()})
                });
            }
            match path {
                "/api/w/v1/user/detail/3" if args["userId"] == "3" => {
                    self.calls
                        .lock()
                        .unwrap()
                        .push(json!({"path":path,"args":args,"mode":mode}));
                    return Ok(
                        json!({"code":200,"profile":{"userId":3,"nickname":"合成主播主页"}}),
                    );
                }
                "/api/user/playlist" if args["uid"] == "3" => {
                    return Ok(json!({"code":200,"playlist":[],"more":false}));
                }
                "/api/my/podcast/page/list" => {
                    return Ok(
                        json!({"code":200,"data":{"radioIdList":[71],"myPodcastVOList":[radio()]}}),
                    );
                }
                "/api/content/my/liked/voice" => {
                    return Ok(json!({"code":200,"data":{"data":[],"hasMore":false}}));
                }
                "/api/djradio/v3/get" => return Ok(json!({"code":200,"data":radio()})),
                "/api/v6/dj/program/byradio" => {
                    return Ok(
                        json!({"code":200,"data":{"programs":[voice()],"count":1,"more":false,"asc":args["asc"]}}),
                    );
                }
                "/api/dj/user/setting/resumeplay/get" => {
                    return Ok(json!({"code":200,"data":{"resumePlayValue":0}}));
                }
                "/api/dj/play/url/check" => {
                    return Ok(
                        json!({"code":200,"data":{"voiceId":"81","songId":"181","canPlay":true}}),
                    );
                }
                "/api/mlog/mycollect/bytime/get" => {
                    return Ok(
                        json!({"code":200,"data":{"hasMore":false,"mlogs":[video()],"time":"1"}}),
                    );
                }
                "/api/song/enhance/play/mv/url" => {
                    return Err(Error::State("comment resource proof does not play video"));
                }
                _ => {}
            }
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
