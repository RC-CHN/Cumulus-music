use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

const PATH: &str = "/api/homepage/category/daily/song/list";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static APPLIED: AtomicBool = AtomicBool::new(false);

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
    if path == "/api/homepage/daily/song/tag/save"
        && std::env::var_os("CUMULUS_STYLE_WRITE_PROOF").is_some()
        && *args == json!({"tags":{"categoryId":"c1","tagIds":["t2"]}})
        && !APPLIED.swap(true, Ordering::SeqCst)
    {
        return Some(Err(Error::Transport(
            "synthetic lost style save reply".into(),
        )));
    }
    if path == "/api/homepage/daily/song/config/get" && *args == json!({"limit":20}) {
        return Some(Ok(json!({"code":200,"data":{"categorys":[
            {"categoryId":"c2","categoryName":"合成空分类","tagVOList":[]},
            {"categoryId":"c1","categoryName":"合成曲风","tagVOList":[
                {"tagId":"t1","tagName":"合成轻音乐"},{"tagId":"t2","tagName":"合成节拍"}
            ]}
        ]}})));
    }
    if path != PATH || args != &json!({"source":"","categoryId":"","tagId":"","songId":""}) {
        return Some(Err(Error::Response(
            "unexpected synthetic style recommendation request",
        )));
    }
    let (id, name) = if APPLIED.load(Ordering::SeqCst) {
        ("t2", "合成节拍")
    } else {
        ("t1", "合成轻音乐")
    };
    Some(Ok(json!({"code":200,"data":{
        "dailySongs":(71..=90).map(|id| json!({"id":id,"name":format!("合成风格歌曲 {id}"),"ar":[{"id":9,"name":"合成风格歌手"}],"al":{"id":7,"name":"合成风格专辑"},"dt":190000})).collect::<Vec<_>>(),
        "tags":{"categoryId":"c1","categoryName":"合成曲风","privateSentinel":"synthetic-style-tag-sentinel",
            "tagVOList":[{"tagId":id,"tagName":name}]}
    }})))
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}
