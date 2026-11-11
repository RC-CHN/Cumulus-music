//! Closed synthetic comment and deletion receipts for the history row menu.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

#[derive(Default)]
pub struct Fixture {
    calls: Mutex<Vec<Value>>,
}

impl Fixture {
    pub fn report(&self) -> Value {
        json!(*self.calls.lock().unwrap())
    }

    pub fn authorize_delete(&self, path: &str, args: &Value, cached: bool) -> Result<()> {
        if cached
            || path != "/api/play-record/batch/delete"
            || *args
                != json!({"type":"SONG","resourceIds":"[\"10\",\"7\"]",
                    "lastResourceId":"300","lastPlayTime":1793179700000_u64})
        {
            return Err(Error::Response("unexpected synthetic row menu delete"));
        }
        self.record(path, args, cached);
        Ok(())
    }

    pub fn response(&self, path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
        if path == "/api/resource/commentInfo/list" {
            self.record(path, args, cached);
            return Some(Self::count(args, cached));
        }
        let (hot, thread) = path
            .strip_prefix("/api/v1/resource/comments/")
            .map(|thread| (false, thread))
            .or_else(|| {
                path.strip_prefix("/api/v1/resource/hotcomments/")
                    .map(|thread| (true, thread))
            })?;
        self.record(path, args, cached);
        Some(Self::page(hot, thread, args, cached))
    }

    fn record(&self, path: &str, args: &Value, cached: bool) {
        self.calls
            .lock()
            .unwrap()
            .push(json!({"path":path,"args":args,"cached":cached}));
    }

    fn count(args: &Value, cached: bool) -> Result<Value> {
        let invalid = || Error::Response("unexpected synthetic row comment count");
        let ids: Vec<String> =
            serde_json::from_str(args["resourceIds"].as_str().ok_or_else(invalid)?)?;
        let [id] = ids.as_slice() else {
            return Err(invalid());
        };
        let id_number = id.parse::<u64>().map_err(|_| invalid())?;
        if cached
            || id != &id_number.to_string()
            || args.as_object().is_none_or(|args| args.len() != 2)
        {
            return Err(invalid());
        }
        let (prefix, total) = match (args["resourceType"].as_u64(), id_number) {
            (Some(4), 11) => {
                return Err(Error::Transport(
                    "synthetic song comment count offline".into(),
                ));
            }
            (Some(4), 10) => ("R_SO_4", 36),
            (Some(4), 1..=300) => ("R_SO_4", 0),
            (Some(1), 10) => ("A_DJ_1", 47),
            (Some(1), 1..=100) => ("A_DJ_1", 0),
            _ => return Err(invalid()),
        };
        Ok(json!({"code":200,"data":[{"threadId":format!("{prefix}_{id}"),"commentCount":total}]}))
    }

    fn page(hot: bool, thread: &str, args: &Value, cached: bool) -> Result<Value> {
        if cached || args["offset"] != 0 {
            return Err(Error::Response("unexpected synthetic row comment page"));
        }
        let (id, content, total) = match thread {
            "R_SO_4_10" => (17001, "合成历史歌曲评论", 36),
            "A_DJ_1_10" => (17002, "合成历史声音评论", 47),
            _ => return Err(Error::Response("unexpected synthetic row comment thread")),
        };
        let mut result = json!({"code":200,"total":total,"more":false,
            "comments":[],"hotComments":[],"topComments":[]});
        result[if hot { "hotComments" } else { "comments" }] = json!([{
            "commentId":id,"content":content,"time":1700000000000_u64,
            "liked":false,"likedCount":0,"user":{"userId":3,"nickname":"合成听友"},"beReplied":[]
        }]);
        Ok(result)
    }
}
