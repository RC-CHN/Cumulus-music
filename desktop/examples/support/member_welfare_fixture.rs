//! Closed fixture for the official read-only welfare menu.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

const EXPERIMENT: &str = "PH-PC-activityCenter";
const LIST: &str = "/api/music/multi/terminal/pc/welfare/center/list";
static OWNER: AtomicU64 = AtomicU64::new(2);
static BEHAVIOR: Mutex<&str> = Mutex::new("list");
static TRACE: Mutex<Vec<(&str, u64)>> = Mutex::new(Vec::new());
static RELEASED: AtomicBool = AtomicBool::new(false);
static COMPLETED: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_MEMBER_WELFARE_PROOF").is_some()
}
fn mode() -> String {
    std::env::var("CUMULUS_MEMBER_WELFARE_PROOF").unwrap_or_default()
}
pub fn report() -> Value {
    json!({"trace":*TRACE.lock().unwrap(),"oldReplyReturned":COMPLETED.load(Ordering::Acquire)})
}
pub fn action(action: &str) {
    match action {
        "status" => (),
        "empty" => *BEHAVIOR.lock().unwrap() = "empty",
        "failure" => *BEHAVIOR.lock().unwrap() = "failure",
        "hold" => *BEHAVIOR.lock().unwrap() = "hold",
        "switch" => OWNER.store(3, Ordering::Release),
        "release" => RELEASED.store(true, Ordering::Release),
        _ => panic!("unexpected welfare fixture action"),
    }
}
fn list(label: &str) -> Value {
    json!({"code":200,"data":{"rightTips":"查看合成活动","unreadMsgCot":2,"welfareList":[
        {"id":"fixture-a","name":format!("{label}活动"),"desc":"仅用于离线界面验证","clicked":false,
         "buttonTxt":"立即领取","buttonJumpUrl":"https://example.invalid/?token=must-not-leak",
         "img":"https://example.invalid/?secret=must-not-leak"},
        {"id":"fixture-b","name":format!("{label}已读活动"),"desc":"不含可执行操作","clicked":true}
    ]}})
}
pub async fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let owner = OWNER.load(Ordering::Acquire);
    match path {
        "/api/w/nuser/account/get" => Some(if mode() == "guest" {
            Err(Error::LoginDeferred)
        } else {
            Ok(
                json!({"code":200,"profile":{"userId":owner,"nickname":format!("合成账号 {owner}")}}),
            )
        }),
        "/api/rtrs/abt/front/expinfo/list" if args["expNames"] == EXPERIMENT => {
            assert_eq!(args, &json!({"expNames":EXPERIMENT}));
            assert!(!cached);
            assert_ne!(mode(), "guest");
            TRACE.lock().unwrap().push(("assignment", owner));
            let group = if mode() == "control" { "c" } else { "t1" };
            Some(Ok(
                json!({"code":200,"data":[{"expName":EXPERIMENT,"expGroupName":group}]}),
            ))
        }
        LIST => {
            assert_eq!(args, &json!({"clickedIds":[]}));
            assert!(!cached);
            assert_ne!(mode(), "guest");
            assert_ne!(mode(), "control");
            TRACE.lock().unwrap().push(("list", owner));
            let state = *BEHAVIOR.lock().unwrap();
            Some(match (owner, state) {
                (3, _) => Ok(list("新账号")),
                (_, "empty") => Ok(json!({"code":200,"data":{"welfareList":[]}})),
                (_, "failure") => Err(Error::Transport("closed fixture read interrupted".into())),
                (_, "hold") => {
                    while !RELEASED.load(Ordering::Acquire) {
                        tokio::time::sleep(Duration::from_millis(15)).await;
                    }
                    COMPLETED.store(true, Ordering::Release);
                    Ok(list("已离开账号的迟到"))
                }
                (_, "list") => Ok(list("原账号")),
                _ => unreachable!(),
            })
        }
        _ => None,
    }
}
