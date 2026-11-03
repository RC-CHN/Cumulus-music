use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const PATH: &str = "/api/homepage/category/daily/song/list";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_STYLE_RECOMMENDATIONS_PROOF").is_some()
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled()
        || !matches!(
            path,
            PATH | "/api/homepage/daily/song/tag/save" | "/api/homepage/daily/song/config/get"
        )
    {
        return None;
    }
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    if path == "/api/homepage/daily/song/config/get" && *args == json!({"limit":20}) {
        return Some(Ok(json!({"code":200,"data":{"categorys":[
            {"categoryId":"c1","categoryName":"合成曲风","tagVOList":[{"tagId":"t1","tagName":"合成轻音乐"}]},
            {"categoryId":"c2","categoryName":"合成空分类","tagVOList":[]}
        ]}})));
    }
    if path != PATH || args != &json!({"source":"","categoryId":"","tagId":"","songId":""}) {
        return Some(Err(Error::Response(
            "unexpected synthetic style recommendation request",
        )));
    }
    Some(Ok(json!({"code":200,"data":{
        "dailySongs":[
            {"id":71,"name":"合成风格歌曲 71","ar":[{"id":9,"name":"合成风格歌手"}],"al":{"id":7,"name":"合成风格专辑"},"dt":190000},
            {"id":72,"name":"合成风格歌曲 72","ar":[{"id":9,"name":"合成风格歌手"}],"al":{"id":7,"name":"合成风格专辑"},"dt":200000}
        ],
        "tags":{"privateSentinel":"synthetic-style-tag-sentinel"}
    }})))
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}
