use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const STYLE: &str = "/api/homepage/category/daily/song/list";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_STYLE_HOME_PROOF").is_some()
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let value = match path {
        "/api/user/setting" => json!({"code":200,"setting":{"personalServiceSetting":0}}),
        "/api/batch" if args["/api/homepage/block/page"].is_string() => {
            json!({"code":200,"/api/homepage/block/page":{"code":200,"data":{"blocks":[]}}})
        }
        "/api/pc/recent/listen/list" => json!({"code":200,"data":{"resources":[]}}),
        "/api/pc/daily/rcmd/block" if args["clientTime"].as_str().is_some() => {
            CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
            json!({"code":200,"data":{"items":[{
                "resourceType":"dailySongs","subResourceType":"style_dailySong","moduleType":"tag_daily_rec",
                "title":"合成首页风格 A","extData":{"categoryId":18,"tagId":21},
                "targetUrl":"https://music.163.com/style?songId=81%2C82&secret=synthetic","songIds":[93]
            },{
                "resourceType":"dailySongs","subResourceType":"style_dailySong","moduleType":"tag_daily_rec",
                "title":"合成首页风格 B","extData":{"categoryId":19,"tagId":22},
                "targetUrl":"https://music.163.com/style?songId=91%2C92&secret=synthetic","songIds":[94]
            },
                {"moduleType":"radar","resourceType":"playList","resourceId":741,"title":"合成精选雷达"},
                {"moduleType":"mood","resourceType":"playList","resourceId":742,"title":"合成精选心情"}
            ]}})
        }
        STYLE => {
            let mut calls = CALLS.lock().unwrap();
            let previous = calls
                .iter()
                .filter(|entry| entry["path"] == STYLE && entry["args"] == *args)
                .count();
            calls.push(json!({"path":path,"args":args}));
            drop(calls);
            let (category, tag, label, first) =
                if *args == json!({"source":"","categoryId":"","tagId":"","songId":""}) {
                    (17, 20, "默认", 71)
                } else if *args
                    == json!({"source":"homepage","categoryId":"18","tagId":"21","songId":"81,82"})
                {
                    (18, 21, "A", 81)
                } else if *args
                    == json!({"source":"homepage","categoryId":"19","tagId":"22","songId":"91,92"})
                {
                    (19, 22, "B", 91)
                } else {
                    return Some(Err(Error::Response("unexpected synthetic style request")));
                };
            let ids = match previous {
                0 => [first, first + 1],
                1 => [first + 5, first + 6],
                _ => {
                    return Some(Err(Error::Response(
                        "unexpected extra synthetic style read",
                    )));
                }
            };
            let songs =
                ids.map(|id| json!({"id":id,"name":format!("合成{label}歌曲 {id}"),"dt":30000}));
            json!({"code":200,"data":{"dailySongs":songs,"tags":{
                "categoryId":category,"categoryName":format!("合成{label}分类"),
                "tagVOList":[{"tagId":tag,"tagName":format!("合成{label}标签")}]
            }}})
        }
        _ => return None,
    };
    Some(Ok(value))
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}
