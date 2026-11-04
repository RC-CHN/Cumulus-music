use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_PROFILE_RELATIONS_PROOF").is_some()
}

pub fn report() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let handled = matches!(
        path,
        "/api/user/personal/page/info"
            | "/api/user/getfollows/3"
            | "/api/user/getfolloweds/"
            | "/api/user/follow/users/mixed/get/v2"
            | "/api/w/v1/user/detail/2"
    ) || (path == "/api/user/playlist" && args["uid"] == "2");
    if !handled {
        return None;
    }
    let mut calls = CALLS.lock().unwrap();
    calls.push(json!({"path":path,"args":args}));
    let times = calls
        .iter()
        .filter(|item| item["path"] == path && item["args"] == *args)
        .count();
    Some(Ok(match path {
        "/api/w/v1/user/detail/2" => {
            json!({"code":200,"profile":{"userId":2,"nickname":"合成自己","follows":3,"followeds":1}})
        }
        "/api/user/playlist" => json!({"code":200,"more":false,"playlist":[]}),
        "/api/user/personal/page/info" if args == &json!({"userId":"3"}) => {
            let notice = if times == 6 {
                "合成作者取消了查看权限"
            } else {
                "合成作者限制了查看"
            };
            json!({"code":200,"data":{"userRelationDTO":{"canSeeFansOrFollows":!matches!(times,4|6),"fansOrFollowsToastText":notice}}})
        }
        "/api/user/getfollows/3" if args == &json!({"offset":0,"limit":20,"getcounts":true}) => {
            let mut rows = vec![
                json!({"userId":2,"nickname":"合成自己","remarkName":"合成备注","signature":"合成签名","followed":true,"mutual":true}),
            ];
            rows.extend(
                (101..120).map(|id| json!({"userId":id,"nickname":format!("合成关注 {id}"),"followed":id==101,"mutual":id==101})),
            );
            json!({"code":200,"follow":rows,"size":21,"more":true})
        }
        "/api/user/getfollows/3" if args == &json!({"offset":20,"limit":20,"getcounts":true}) => {
            if times == 1 {
                return Some(Err(Error::Transport("合成续页失败".into())));
            }
            json!({"code":200,"follow":[{"userId":120,"nickname":"合成续页"}],"size":21,"more":false})
        }
        "/api/user/getfolloweds/"
            if args == &json!({"userId":"3","time":0,"offset":0,"limit":20,"getcounts":true}) =>
        {
            json!({"code":200,"followeds":[{"userId":90,"nickname":"合成粉丝"}],"size":1,"more":false})
        }
        "/api/user/getfolloweds/"
            if args == &json!({"userId":"2","time":0,"offset":0,"limit":20,"getcounts":true}) =>
        {
            json!({"code":200,"followeds":[{"userId":91,"nickname":"我的合成粉丝"}],"size":1,"more":false})
        }
        "/api/user/follow/users/mixed/get/v2" => {
            let Some(scene) = args["scene"].as_u64().filter(|value| *value <= 2) else {
                return Some(Err(Error::Response("invalid synthetic follow scene")));
            };
            if args
                != &json!({"page":{"size":19,"cursor":"0"},"size":19,"scene":scene,"sortType":0,"authority":false})
            {
                return Some(Err(Error::Response("invalid synthetic own-follow request")));
            }
            let row = match scene {
                0 => {
                    json!({"artistInfo":{"id":9,"name":"我的全部关注"},"userProfile":{"userId":3,"nickname":"合成作者 3","signature":"绑定用户签名"}})
                }
                1 => json!({"artistInfo":{"id":10,"name":"我的歌手关注"}}),
                _ => json!({"userProfile":{"userId":3,"nickname":"我的用户关注"}}),
            };
            json!({"code":200,"data":{"records":[row],"page":{"more":false}}})
        }
        _ => return Some(Err(Error::Response("unexpected profile relation request"))),
    }))
}
