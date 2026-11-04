use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const CURSOR: &str = "{ \"next\" : \"第一页/第二页\" }";
const CURSOR_2: &str = " continuation 2 ";
const CURSOR_3: &str = "{\"page\":4}";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static TOP_CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static SETTINGS: AtomicUsize = AtomicUsize::new(0);
static DISABLED: AtomicBool = AtomicBool::new(false);

pub fn disable() {
    DISABLED.store(true, Ordering::SeqCst);
}

pub fn report() -> Value {
    json!({"calls":*CALLS.lock().unwrap(),"settings":SETTINGS.load(Ordering::SeqCst)})
}

pub fn top_calls() -> Vec<Value> {
    TOP_CALLS.lock().unwrap().clone()
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

fn filler(range: std::ops::Range<u32>) -> Vec<Value> {
    range
        .map(|id| {
            group(
                "PLAYLIST_SQUARE_SINGLE_SLID",
                &format!("合成后续分组 {id}"),
                &[id],
            )
        })
        .collect()
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
        "/api/personalized/playlist/tag/rcmd" => {
            assert!(
                !DISABLED.load(Ordering::SeqCst),
                "disabled settings must fence top tags"
            );
            assert_eq!(*args, json!({}));
            let mut calls = TOP_CALLS.lock().unwrap();
            calls.push(args.clone());
            if calls.len() == 1 {
                Err(Error::Transport("合成顶部标签中断".into()))
            } else {
                assert_eq!(calls.len(), 2);
                Ok(
                    json!({"code":200,"data":[{"tag":"民谣","displayName":"不可代替原标签"},
                    {"tag":"官方"},{"tag":"民谣"},{"tag":"推荐"},{"tag":"纯音乐"},{"tag":"\t"}]}),
                )
            }
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
                            group("PLAYLIST_SQUARE_SINGLE_SLID","首批分组 1",&[21]),
                            group("PLAYLIST_SQUARE_SINGLE_SLID","首批分组 2",&[22]),
                            group("unknown","不显示活动",[999].as_slice())
                        ]}}),
                    )
                }
                2 | 3 => {
                    assert_eq!(*args, json!({"cursor":CURSOR,"newStyle":true}));
                    if calls.len() == 2 {
                        Err(Error::Transport("合成推荐续页中断".into()))
                    } else {
                        let mut groups = vec![group(
                            "PLAYLIST_SQUARE_MULTI_THREE_GRID",
                            "合成下一分组",
                            &[4, 3],
                        )];
                        groups.extend(filler(101..108));
                        Ok(
                            json!({"code":200,"data":{"hasMore":true,"cursor":CURSOR_2,"blocks":groups}}),
                        )
                    }
                }
                4 => {
                    assert_eq!(*args, json!({"cursor":CURSOR_2,"newStyle":true}));
                    Ok(
                        json!({"code":200,"data":{"hasMore":true,"cursor":CURSOR_3,"blocks":filler(108..116)}}),
                    )
                }
                5 => {
                    assert_eq!(*args, json!({"cursor":CURSOR_3,"newStyle":true}));
                    Ok(json!({"code":200,"data":{"hasMore":false,"blocks":filler(116..124)}}))
                }
                _ => panic!("unexpected playlist square read"),
            }
        }
        _ => return None,
    })
}
