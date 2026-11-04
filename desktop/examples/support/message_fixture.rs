use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

const T: i64 = 1_700_000_100_000;
static REQUESTS: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());
static FAILED_COMMENT_PAGE: AtomicBool = AtomicBool::new(false);
static HOLD_NOTICE: AtomicBool = AtomicBool::new(false);
static NOTICE_HELD: AtomicBool = AtomicBool::new(false);
static WRITE_ATTEMPT: AtomicBool = AtomicBool::new(false);
static REMOVED_SINGLE: AtomicBool = AtomicBool::new(false);
static REMOVED_CONVERSATION: AtomicBool = AtomicBool::new(false);
static OMIT_UNKNOWN: AtomicBool = AtomicBool::new(false);

pub fn enabled() -> bool {
    std::env::var("CUMULUS_MESSAGE_DRAWER_PROOF").as_deref() == Ok("1")
}

fn attachments() -> bool {
    std::env::var("CUMULUS_MESSAGE_ATTACHMENTS_PROOF").as_deref() == Ok("1")
}
fn deletion() -> bool {
    std::env::var("CUMULUS_MESSAGE_DELETION_PROOF").as_deref() == Ok("1")
}
fn target_comment(id: u64) -> Value {
    json!({"code":200,"comments":[],"currentComment":{"commentId":id,
        "content":format!("合成定位评论 {id}"),"time":T,"likedCount":0,
        "user":{"userId":501,"nickname":"合成评论者"}},
        "total":1,"more":false})
}

pub fn control(mode: &str) -> Value {
    match mode {
        "hold" => HOLD_NOTICE.store(true, Ordering::SeqCst),
        "release" => HOLD_NOTICE.store(false, Ordering::SeqCst),
        "deletion-omit" if deletion() => OMIT_UNKNOWN.store(true, Ordering::SeqCst),
        "read" => {}
        _ => panic!("unknown message fixture command"),
    }
    report()
}

pub fn report() -> Value {
    json!({"requests":*REQUESTS.lock().unwrap(),"blockedWrite":WRITE_ATTEMPT.load(Ordering::SeqCst),
        "noticeHeld":NOTICE_HELD.load(Ordering::SeqCst)})
}

pub async fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if attachments() && path.starts_with("/api/v1/resource/comments/") {
        REQUESTS.lock().unwrap().push((path.into(), args.clone()));
        let (id, wire_id) = match path {
            "/api/v1/resource/comments/R_SO_4_3" => (411, "411"),
            "/api/v1/resource/comments/R_AL_3_7" => (701, "701"),
            "/api/v1/resource/comments/A_PL_0_2" => (702, "702"),
            _ => return Some(Err(Error::State("unexpected synthetic comment resource"))),
        };
        return Some(if args["commentId"] == wire_id && args["offset"] == 0 {
            Ok(target_comment(id))
        } else {
            Err(Error::State("wrong synthetic comment anchor"))
        });
    }
    if deletion()
        && matches!(
            path,
            "/api/msg/private/delete" | "/api/msg/private/delete/single"
        )
    {
        REQUESTS.lock().unwrap().push((path.into(), args.clone()));
        return Some(match (path, args) {
            ("/api/msg/private/delete", args) if args == &json!({"userId":501}) => {
                assert!(!REMOVED_CONVERSATION.swap(true, Ordering::SeqCst));
                Ok(json!({"code":200}))
            }
            ("/api/msg/private/delete", args) if args == &json!({"userId":502}) => {
                Ok(json!({"code":403}))
            }
            ("/api/msg/private/delete/single", args) if args == &json!({"id":301}) => {
                assert!(!REMOVED_SINGLE.swap(true, Ordering::SeqCst));
                Ok(json!({"code":200}))
            }
            ("/api/msg/private/delete/single", args)
                if args == &json!({"id":"9007199254740997"}) =>
            {
                Err(Error::Transport(
                    "synthetic private message reply lost".into(),
                ))
            }
            _ => {
                WRITE_ATTEMPT.store(true, Ordering::SeqCst);
                Err(Error::InvalidInput("wrong synthetic deletion identity"))
            }
        });
    }
    if !matches!(
        path,
        "/api/pl/count"
            | "/api/msg/private/users"
            | "/api/msg/private/history"
            | "/api/forwards/get"
            | "/api/msg/notices"
            | "/api/v1/user/comments/2"
    ) {
        if path.starts_with("/api/msg/")
            || path.starts_with("/api/v1/user/comments/")
            || path.starts_with("/api/user/comments/reply")
            || path.starts_with("/api/forwards/")
        {
            WRITE_ATTEMPT.store(true, Ordering::SeqCst);
            return Some(Err(Error::State("unexpected message drawer request")));
        }
        return None;
    }
    REQUESTS.lock().unwrap().push((path.into(), args.clone()));
    let res = match path {
        "/api/pl/count" if args == &json!({}) => {
            json!({"code":200,"msg":3,"comment":2,"forward":1,"notice":4})
        }
        "/api/msg/private/users" if args == &json!({"limit":15,"offset":0,"time":-1}) => {
            let rows = (0..15).filter(|idx| !deletion() || *idx != 0 || !REMOVED_CONVERSATION.load(Ordering::SeqCst)).map(|idx| json!({"fromUser":{"userId":501+idx,"nickname":format!("合成联系人 {:02}",idx+1)},
                "lastMsgTime":T-idx,"newMsgCount":if idx==0 {3} else {0},
                "lastMsg":json!({"type":6,"msg":format!("私信预览 {}",idx+1)}).to_string()})).collect::<Vec<_>>();
            json!({"code":200,"more":true,"msgs":rows})
        }
        "/api/msg/private/users" if args == &json!({"limit":15,"offset":0,"time":T-14}) => {
            json!({"code":200,"more":false,"msgs":[{"fromUser":{"userId":599,"nickname":"合成较早联系人"},
                "lastMsgTime":T-15,"lastMsg":{"type":4,"playlist":{"name":"只读分享"}}}]})
        }
        "/api/msg/private/history" if args["limit"] == 10 && args["offset"] == 0 => {
            let Some(peer) = args["userId"]
                .as_str()
                .and_then(|id| id.parse::<i64>().ok())
            else {
                return Some(Err(Error::InvalidInput("invalid synthetic history peer")));
            };
            if !(501..516).contains(&peer) {
                return Some(Err(Error::InvalidInput("history picked another user")));
            }
            if args["time"] == -1 && deletion() {
                let mut rows = Vec::new();
                if !REMOVED_SINGLE.load(Ordering::SeqCst) {
                    rows.push(
                        json!({"id":301,"time":T+3,"fromUser":{"userId":2,"nickname":"合成自己"},
                        "toUser":{"userId":peer},"msg":{"type":6,"msg":"可确认删除的私信"}}),
                    );
                }
                if !OMIT_UNKNOWN.load(Ordering::SeqCst) {
                    rows.push(json!({"id":"9007199254740997","time":T+2,"fromUser":{"userId":peer,"nickname":"合成对方"},
                        "toUser":{"userId":2},"msg":{"type":6,"msg":"回执丢失的私信"}}));
                }
                rows.push(json!({"commentId":303,"time":T+1,"fromUser":{"userId":peer,"nickname":"合成对方"},
                    "msg":{"type":6,"msg":"没有原生消息 ID 的私信"}}));
                json!({"code":200,"more":false,"msgs":rows})
            } else if args["time"] == -1 && attachments() {
                let rows = [
                    (360, json!({"type":2,"album":{"id":7,"name":"合成分享专辑"}})),
                    (359, json!({"type":1,"song":{"id":3,"name":"合成分享单曲","url":"https://private.invalid/song"}})),
                    (358, json!({"type":15,"comment":{"resourceType":3,"commentId":701,"threadId":"R_AL_3_7","resourceJson":"{\"id\":7}"}})),
                    (357, json!({"type":14,"song":{"id":3,"name":"合成私有云歌曲"}})),
                    (356, json!({"type":12,"promotionUrl":{"text":"合成外链","url":"https://private.invalid/link"}})),
                    (355, json!({"type":15,"comment":{"resourceType":3,"commentId":701,"threadId":"R_AL_3_7","resourceJson":"{\"id\":8}"}})),
                ].into_iter().map(|(id,msg)|json!({"id":id,"time":T+id,"fromUser":{"userId":peer,"nickname":"合成对方"},"msg":msg})).collect::<Vec<_>>();
                json!({"code":200,"more":false,"msgs":rows})
            } else if args["time"] == -1 {
                json!({"code":200,"more":true,"hint":"合成提示","msgs":[
                    {"id":301,"time":T+1,"fromUser":{"userId":2,"nickname":"合成自己"},
                        "msg":{"type":6,"msg":"较新的自己消息"}},
                    {"id":300,"time":T,"fromUser":{"userId":peer,"nickname":"合成对方"},
                        "msg":json!({"type":6,"msg":"第一屏中的较早消息"}).to_string()}]})
            } else if args["time"] == T {
                json!({"code":200,"more":false,"msgs":[{"id":299,"time":T-1,
                    "fromUser":{"userId":peer,"nickname":"合成对方"},
                    "msg":{"type":12,"promotionUrl":{"text":"不打开的附件","url":"https://private.invalid"}}}]})
            } else {
                return Some(Err(Error::InvalidInput("unexpected history cursor")));
            }
        }
        "/api/v1/user/comments/2"
            if attachments() && args == &json!({"limit":10,"beforeTime":-1}) =>
        {
            json!({"code":200,"more":false,"total":1,"comments":[{"id":411,"time":T,
                "user":{"nickname":"合成评论者"},"content":"合成收到评论","combindId":"2:411:R_SO_4_3",
                "resourceType":4,"resourceJson":"{\"id\":3}"}]})
        }
        "/api/v1/user/comments/2" if args == &json!({"limit":10,"beforeTime":-1}) => {
            json!({"code":200,"more":true,"total":2,"comments":[{"id":411,"time":T,
              "user":{"nickname":"合成评论者"},"content":"评论正文","beRepliedContent":"引用原文",
              "beRepliedUser":{"nickname":"合成自己"}}]})
        }
        "/api/v1/user/comments/2" if args == &json!({"limit":10,"beforeTime":T}) => {
            if !FAILED_COMMENT_PAGE.swap(true, Ordering::SeqCst) {
                return Some(Err(Error::Transport(
                    "synthetic second comment page unavailable".into(),
                )));
            }
            json!({"code":200,"more":false,"total":2,"comments":[{"id":410,"time":T-1,
                "user":{"nickname":"合成评论者"},"content":"较早评论"}]})
        }
        "/api/forwards/get" if args == &json!({"total":true,"limit":10,"offset":0,"time":-1}) => {
            json!({"code":200,"more":true,"lasttime":T-2,"newCount":1,"forwards":[{"id":51,"type":2,
                "json":{"comment":{"user":{"nickname":"合成提及者"},"time":T,"content":"@合成自己"},
                "resource":{"name":"引用的笔记"}}}]})
        }
        "/api/forwards/get" if args == &json!({"total":true,"limit":10,"offset":0,"time":T-2}) => {
            json!({"code":200,"more":false,"lasttime":T-3,"newCount":1,"forwards":[]})
        }
        "/api/msg/notices"
            if attachments() && args == &json!({"total":true,"limit":10,"offset":0,"time":-1}) =>
        {
            json!({"code":200,"more":false,"notices":[
                {"id":61,"time":T,"notice":{"type":2,"user":{"nickname":"合成通知者"},"playlist":{"id":2,"name":"合成歌单"}}},
                {"id":62,"time":T-1,"notice":{"type":6,"comment":{"commentId":702,"threadId":"A_PL_0_2","content":"合成喜欢评论"}}},
                {"id":63,"time":T-2,"notice":{"type":6,"comment":{"commentId":703,"threadId":"R_MV_5_42","content":"移动端查看的评论"}}}
            ]})
        }
        "/api/msg/notices" if args == &json!({"total":true,"limit":10,"offset":0,"time":-1}) => {
            if HOLD_NOTICE.load(Ordering::SeqCst) {
                NOTICE_HELD.store(true, Ordering::SeqCst);
                let end = std::time::Instant::now() + std::time::Duration::from_secs(12);
                while HOLD_NOTICE.load(Ordering::SeqCst) && std::time::Instant::now() < end {
                    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                }
                NOTICE_HELD.store(false, Ordering::SeqCst);
                json!({"code":200,"more":false,"notices":[{"id":62,"time":T+50,"notice":
                    {"type":10,"generalNotice":{"actionDesc":"过期回复","content":"旧面板数据不能显示"}}}]})
            } else {
                json!({"code":200,"more":true,"lastTime":T-3,"notices":[{"id":61,"time":T,"notice":
                    json!({"type":2,"user":{"nickname":"合成通知者"},"playlist":{"name":"合成歌单"}}).to_string()}]})
            }
        }
        "/api/msg/notices" if args == &json!({"total":true,"limit":10,"offset":0,"time":T-3}) => {
            json!({"code":200,"more":false,"notices":[]})
        }
        _ => {
            return Some(Err(Error::InvalidInput(
                "incorrect synthetic message request",
            )));
        }
    };
    Some(Ok(res))
}
