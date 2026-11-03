use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_STYLE_HOME_PROOF").is_some()
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let value = match path {
        "/api/pc/daily/rcmd/block" if args["clientTime"].as_str().is_some() => {
            json!({"code":200,"data":{"items":[{
                "resourceType":"dailySongs","subResourceType":"style_dailySong","moduleType":"tag_daily_rec",
                "title":"合成首页风格","extData":{"categoryId":18,"tagId":21},
                "targetUrl":"https://music.163.com/style?songId=81%2C82&secret=synthetic","songIds":[93]
            }]}})
        }
        "/api/homepage/category/daily/song/list" if args["source"] == "homepage" => {
            if *args != json!({"source":"homepage","categoryId":"18","tagId":"21","songId":"81,82"})
            {
                return Some(Err(Error::Response("unexpected synthetic home style seed")));
            }
            json!({"code":200,"data":{"dailySongs":[{"id":81,"name":"合成首页歌曲","dt":30000}],"tags":{
                "categoryId":18,"categoryName":"合成首页分类","tagVOList":[{"tagId":21,"tagName":"合成首页标签"}]
            }}})
        }
        _ => return None,
    };
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    Some(Ok(value))
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}
