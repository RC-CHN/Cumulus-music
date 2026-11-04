use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const CURSOR: &str = "{ \"next\" : \"第一页/第二页\" }";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static SETTINGS: AtomicUsize = AtomicUsize::new(0);
static DISABLED: AtomicBool = AtomicBool::new(false);

pub fn disable() {
    DISABLED.store(true, Ordering::SeqCst);
}

pub fn report() -> Value {
    json!({"calls":*CALLS.lock().unwrap(),"settings":SETTINGS.load(Ordering::SeqCst)})
}

fn group(kind: &str, title: &str, ids: &[u32]) -> Value {
    json!({"showType":kind,"uiElement":{"mainTitle":{"title":title}},"creatives":[
        {"creativeType":"list","resources":ids.iter().map(|id|json!({
            "resourceType":"playlist","resourceId":id,
            "uiElement":{"mainTitle":{"title":format!("推荐合成歌单 {id}")}},
            "resourceExt":{"playCount":12345}
        })).collect::<Vec<_>>()}
    ]})
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    Some(match path {
        "/api/user/setting" => {
            assert_eq!(*args, json!({}));
            SETTINGS.fetch_add(1, Ordering::SeqCst);
            Ok(
                json!({"code":200,"setting":{"personalServiceSetting":if DISABLED.load(Ordering::SeqCst) {2} else {0}}}),
            )
        }
        "/api/playlist/square/block/page" => {
            assert!(
                !DISABLED.load(Ordering::SeqCst),
                "disabled feed must not call the recommendation endpoint"
            );
            let mut calls = CALLS.lock().unwrap();
            calls.push(args.clone());
            match calls.len() {
                1 => {
                    assert_eq!(*args, json!({"cursor":"","newStyle":true}));
                    Ok(
                        json!({"code":200,"data":{"hasMore":true,"cursor":CURSOR,"blocks":[
                            group("PLAYLIST_SQUARE_SINGLE_SLID","合成推荐分组",[11,12,13,14,15,16,3].as_slice()),
                            group("unknown","不显示活动",[999].as_slice())
                        ]}}),
                    )
                }
                2 | 3 => {
                    assert_eq!(*args, json!({"cursor":CURSOR,"newStyle":true}));
                    if calls.len() == 2 {
                        Err(Error::Transport("合成推荐续页中断".into()))
                    } else {
                        Ok(json!({"code":200,"data":{"hasMore":false,"blocks":[
                            group("PLAYLIST_SQUARE_MULTI_THREE_GRID","合成下一分组",[4,3].as_slice())
                        ]}}))
                    }
                }
                _ => panic!("unexpected playlist square read"),
            }
        }
        _ => return None,
    })
}
