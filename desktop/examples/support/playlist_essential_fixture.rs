use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const LASTTIME: u64 = 1_700_000_000_123;
static PAGES: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static TAGS: AtomicUsize = AtomicUsize::new(0);

pub fn report() -> Value {
    json!({"pages":*PAGES.lock().unwrap(),"tags":TAGS.load(Ordering::SeqCst)})
}

fn row(id: u32) -> Value {
    json!({"id":id,"name":format!("精品合成歌单 {id}"),"playCount":12345})
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    Some(match path {
        "/api/playlist/highquality/tags" => {
            assert_eq!(*args, json!({}));
            if TAGS.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(Error::Transport("合成精品标签暂不可读".into()))
            } else {
                Ok(
                    json!({"code":200,"tags":[{"id":51,"name":"轻音乐","displayName":"轻音乐精选"},{"id":52,"name":"摇滚"}]}),
                )
            }
        }
        "/api/playlist/highquality/list" => {
            assert_eq!(args["limit"], 36);
            let mut pages = PAGES.lock().unwrap();
            pages.push(args.clone());
            match (args["cat"].as_str(), &args["lasttime"]) {
                (Some("轻音乐"), cursor) if cursor == "-1" => Ok(json!({
                    "code":200,"playlists":(500..536).map(row).collect::<Vec<_>>(),"more":true,"lasttime":LASTTIME,
                })),
                (Some("轻音乐"), cursor) if cursor == LASTTIME => {
                    if pages
                        .iter()
                        .filter(|page| page["lasttime"] == LASTTIME)
                        .count()
                        == 1
                    {
                        Err(Error::Transport("合成精品续页中断".into()))
                    } else {
                        Ok(json!({"code":200,"playlists":[row(3)],"more":false}))
                    }
                }
                (Some("全部"), cursor) if cursor == "-1" => {
                    Ok(json!({"code":200,"playlists":[row(4)],"more":false}))
                }
                _ => panic!("unexpected essential playlist request"),
            }
        }
        _ => return None,
    })
}
