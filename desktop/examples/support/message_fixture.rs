use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const T: i64 = 1_700_000_100_000;
static REQUESTS: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());
static FAILED_COMMENT_PAGE: AtomicBool = AtomicBool::new(false);
static HOLD_NOTICE: AtomicBool = AtomicBool::new(false);
static NOTICE_HELD: AtomicBool = AtomicBool::new(false);
static HOLD_HISTORY: AtomicBool = AtomicBool::new(false);
static HISTORY_HELD: AtomicBool = AtomicBool::new(false);
static WRITE_ATTEMPT: AtomicBool = AtomicBool::new(false);
static REMOVED_SINGLE: AtomicBool = AtomicBool::new(false);
static REMOVED_CONVERSATION: AtomicBool = AtomicBool::new(false);
static OMIT_UNKNOWN: AtomicBool = AtomicBool::new(false);
static REPLY_BINDINGS: AtomicUsize = AtomicUsize::new(0);
static REPLY_SENDS: AtomicUsize = AtomicUsize::new(0);
static HOLD_REPLY: AtomicBool = AtomicBool::new(false);
static REPLY_HELD: AtomicBool = AtomicBool::new(false);
static COUNT_MODE: AtomicUsize = AtomicUsize::new(0);
static READ_ALL_CALLS: AtomicUsize = AtomicUsize::new(0);
static HOLD_COUNT: AtomicBool = AtomicBool::new(false);
static COUNT_HELD: AtomicBool = AtomicBool::new(false);

const REPLY_PATH: &str = "/api/msg/private/send";
const REPLY_GATE: &str = "/api/w/v1/user/bindings/2";
const REPLY_TEXT: &str = "  合成发送正文\n";
const REPLY_DENIED: &str = "  合成拦截正文\n";
const REPLY_LOST: &str = "合成未知正文";
const REPLY_LATE: &str = "合成迟到发送正文";

fn reply_mode() -> bool {
    std::env::var("CUMULUS_MESSAGE_REPLY_PROOF").as_deref() == Ok("1")
}

fn sent_row() -> Value {
    json!({"id":"9007199254741997","time":T+10,"fromUser":{"userId":2,"nickname":"合成自己"},
        "toUser":{"userId":505},"msg":{"type":6,"msg":"服务端返回的合成已发送正文"}})
}

pub fn enabled() -> bool {
    std::env::var("CUMULUS_MESSAGE_DRAWER_PROOF").as_deref() == Ok("1")
}
pub fn polling() -> bool {
    std::env::var("CUMULUS_MESSAGE_POLL_PROOF").as_deref() == Ok("1")
}

pub fn read_all() -> bool {
    enabled() && std::env::var("CUMULUS_MESSAGE_READ_ALL_PROOF").as_deref() == Ok("1")
}

pub fn clear_all() -> Result<Value> {
    if !read_all() || std::env::var_os("CUMULUS_PROOF_REOPEN").is_some() {
        WRITE_ATTEMPT.store(true, Ordering::SeqCst);
        return Err(Error::State("unexpected synthetic message clear"));
    }
    REQUESTS
        .lock()
        .unwrap()
        .push(("/api/msg/count/clear".into(), json!({})));
    match READ_ALL_CALLS.fetch_add(1, Ordering::SeqCst) {
        0 => {
            COUNT_MODE.store(1, Ordering::SeqCst);
            Ok(json!({"code":200}))
        }
        1 => Err(Error::Transport(
            "synthetic lost read-all acknowledgement".into(),
        )),
        _ => {
            WRITE_ATTEMPT.store(true, Ordering::SeqCst);
            Err(Error::State("unexpected synthetic message clear replay"))
        }
    }
}

pub fn count_calls() -> usize {
    REQUESTS
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path == "/api/pl/count")
        .count()
}

fn attachments() -> bool {
    std::env::var("CUMULUS_MESSAGE_ATTACHMENTS_PROOF").as_deref() == Ok("1")
}
fn deletion() -> bool {
    std::env::var("CUMULUS_MESSAGE_DELETION_PROOF").as_deref() == Ok("1")
}
fn late_reply_row() -> Value {
    json!({"id":"9007199254742997","time":T+11,"fromUser":{"userId":2,"nickname":"合成自己"},
        "toUserId":505,"msg":{"type":6,"msg":"服务端返回的合成迟到正文"}})
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
        "history-hold" => HOLD_HISTORY.store(true, Ordering::SeqCst),
        "history-release" => HOLD_HISTORY.store(false, Ordering::SeqCst),
        "deletion-omit" if deletion() => OMIT_UNKNOWN.store(true, Ordering::SeqCst),
        "reply-hold" if reply_mode() => HOLD_REPLY.store(true, Ordering::SeqCst),
        "reply-release" if reply_mode() => HOLD_REPLY.store(false, Ordering::SeqCst),
        "poll-positive" if polling() => COUNT_MODE.store(0, Ordering::SeqCst),
        "poll-zero" if polling() => COUNT_MODE.store(1, Ordering::SeqCst),
        "poll-fail" if polling() => COUNT_MODE.store(2, Ordering::SeqCst),
        "poll-hold" if polling() => HOLD_COUNT.store(true, Ordering::SeqCst),
        "poll-release" if polling() => HOLD_COUNT.store(false, Ordering::SeqCst),
        "count-hold" if !read_all() => HOLD_COUNT.store(true, Ordering::SeqCst),
        "count-release" if !read_all() => HOLD_COUNT.store(false, Ordering::SeqCst),
        "inbox-delivery" if !read_all() => COUNT_MODE.store(4, Ordering::SeqCst),
        "inbox-reset" if !read_all() => COUNT_MODE.store(0, Ordering::SeqCst),
        "notice-refresh" if !read_all() => COUNT_MODE.store(5, Ordering::SeqCst),
        "read-all-delivery" if read_all() => COUNT_MODE.store(3, Ordering::SeqCst),
        "read-all-zero" if read_all() => COUNT_MODE.store(1, Ordering::SeqCst),
        "read" => {}
        _ => panic!("unknown message fixture command"),
    }
    report()
}

pub fn report() -> Value {
    json!({"requests":*REQUESTS.lock().unwrap(),"blockedWrite":WRITE_ATTEMPT.load(Ordering::SeqCst),
        "noticeHeld":NOTICE_HELD.load(Ordering::SeqCst),"replyHeld":REPLY_HELD.load(Ordering::SeqCst),
        "countHeld":COUNT_HELD.load(Ordering::SeqCst),"historyHeld":HISTORY_HELD.load(Ordering::SeqCst)})
}

pub async fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if reply_mode() && matches!(path, REPLY_PATH | REPLY_GATE) {
        REQUESTS.lock().unwrap().push((path.into(), args.clone()));
        return Some(match path {
            REPLY_GATE if args == &json!({"userId":"2"}) => {
                let attempt = REPLY_BINDINGS.fetch_add(1, Ordering::SeqCst);
                Ok(
                    json!({"code":200,"bindings":if attempt == 0 {json!([])} else {json!([{"type":1}])}}),
                )
            }
            REPLY_PATH => {
                let attempt = REPLY_SENDS.fetch_add(1, Ordering::SeqCst);
                let text = match attempt {
                    0 => REPLY_DENIED,
                    1 => REPLY_TEXT,
                    2 => REPLY_LATE,
                    3 => REPLY_LOST,
                    _ => panic!("synthetic reply replayed"),
                };
                assert_eq!(
                    args,
                    &json!({"type":"text","id":0,"userIds":["505"],"msg":text})
                );
                match attempt {
                    0 => Ok(json!({"code":2202,"msg":"SYNTHETIC_RAW_INTERCEPTION"})),
                    1 => Ok(json!({"code":200,"newMsgs":[sent_row()]})),
                    2 => {
                        REPLY_HELD.store(true, Ordering::SeqCst);
                        let end = std::time::Instant::now() + std::time::Duration::from_secs(12);
                        while HOLD_REPLY.load(Ordering::SeqCst) && std::time::Instant::now() < end {
                            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                        }
                        REPLY_HELD.store(false, Ordering::SeqCst);
                        Ok(json!({"code":200,"newMsgs":[late_reply_row()]}))
                    }
                    _ => Err(Error::Transport("synthetic message reply lost".into())),
                }
            }
            _ => Err(Error::InvalidInput("unexpected synthetic reply request")),
        });
    }
    if attachments()
        && matches!(
            path,
            "/api/djradio/sub"
                | "/api/djradio/unsub"
                | "/api/djradio/newcount/clear/byradio"
                | "/api/dj/user/setting/resumeplay/update"
                | "/api/dj/play/url/check"
                | "/api/dj/program/detail"
        )
    {
        WRITE_ATTEMPT.store(true, Ordering::SeqCst);
        return Some(Err(Error::State("a podcast notice must only open detail")));
    }
    if attachments()
        && matches!(
            path,
            "/api/djradio/v3/get"
                | "/api/v6/dj/program/byradio"
                | "/api/dj/user/setting/resumeplay/get"
        )
    {
        REQUESTS.lock().unwrap().push((path.into(), args.clone()));
        let id = match (args["id"].as_str(), args["radioId"].as_str()) {
            (Some("81"), None) | (None, Some("81")) => "81",
            (Some("82"), None) | (None, Some("82")) => "82",
            _ => return Some(Err(Error::State("unexpected podcast notice identity"))),
        };
        let radio = json!({"id":id,"name":format!("通知播客 {id}"),"programCount":1,
            "subed":true,"dj":{"userId":2,"nickname":"合成主播"}});
        return Some(match path {
            "/api/djradio/v3/get" if args == &json!({"id":id}) => {
                Ok(json!({"code":200,"data":radio}))
            }
            "/api/v6/dj/program/byradio"
                if args == &json!({"radioId":id,"limit":200,"offset":0,"asc":true}) =>
            {
                Ok(
                    json!({"code":200,"data":{"programs":[{"id":401,"name":"合成单集",
                    "mainSong":{"id":1,"name":"合成声音","duration":30000},
                    "radio":radio,"duration":30000}],"count":1,"more":false,"asc":true}}),
                )
            }
            "/api/dj/user/setting/resumeplay/get" if args == &json!({"radioId":id}) => {
                Ok(json!({"code":200,"data":{"resumePlayValue":0}}))
            }
            _ => Err(Error::State("unexpected podcast notice arguments")),
        });
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
            if HOLD_COUNT.load(Ordering::SeqCst) {
                COUNT_HELD.store(true, Ordering::SeqCst);
                while HOLD_COUNT.load(Ordering::SeqCst) {
                    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                }
                COUNT_HELD.store(false, Ordering::SeqCst);
            }
            match COUNT_MODE.load(Ordering::SeqCst) {
                1 => json!({"code":200,"msg":0,"comment":0,"forward":0,"notice":0}),
                2 => json!({"code":503}),
                3 if read_all() => json!({"code":200,"msg":6,"comment":1,"forward":2,"notice":3}),
                5 if !read_all() => json!({"code":200,"msg":3,"comment":2,"forward":1,"notice":7}),
                _ => json!({"code":200,"msg":3,"comment":2,"forward":1,"notice":4}),
            }
        }
        "/api/msg/private/users" if args == &json!({"limit":15,"offset":0,"time":-1}) => {
            let mode = COUNT_MODE.load(Ordering::SeqCst);
            let rows = (0..15).filter(|idx| !deletion() || *idx != 0 || !REMOVED_CONVERSATION.load(Ordering::SeqCst)).map(|idx| json!({"fromUser":{"userId":501+idx,"nickname":format!("合成联系人 {:02}",idx+1)},
                "lastMsgTime":match (read_all(), mode, idx) {
                    (true, 3, 0) => T+100,
                    (false, 4, 4) => T+100,
                    _ => T-idx,
                },"newMsgCount":match (read_all(), mode, idx) {
                    (true, 1, _) => 0,
                    (true, 3, 0) => 4,
                    (false, 4, 4) => 6,
                    (_, _, 0) => 1,
                    (_, _, 4) => 2,
                    _ => 0,
                },
                "lastMsg":json!({"type":6,"msg":if !read_all() && mode == 4 && idx == 4 {
                    "合成新私信".to_string()
                } else { format!("私信预览 {}",idx+1) }}).to_string()})).collect::<Vec<_>>();
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
            if HOLD_HISTORY.load(Ordering::SeqCst) {
                HISTORY_HELD.store(true, Ordering::SeqCst);
                while HOLD_HISTORY.load(Ordering::SeqCst) {
                    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                }
                HISTORY_HELD.store(false, Ordering::SeqCst);
            }
            if args["time"] == -1 && reply_mode() {
                let attempts = REPLY_SENDS.load(Ordering::SeqCst);
                let mut rows = Vec::new();
                if peer == 505 && attempts >= 4 {
                    rows.push(json!({"id":"9007199254743997","time":T+12,"fromUser":{"userId":2,"nickname":"合成自己"},
                        "toUserId":peer,"msg":{"type":6,"msg":REPLY_LOST}}));
                }
                if peer == 505 && attempts >= 3 && !REPLY_HELD.load(Ordering::SeqCst) {
                    rows.push(late_reply_row());
                }
                if peer == 505 && attempts >= 2 {
                    rows.push(sent_row());
                }
                rows.push(
                    json!({"id":301,"time":T,"fromUser":{"userId":peer,"nickname":"合成对方"},
                    "msg":{"type":6,"msg":"已有的合成私信"}}),
                );
                json!({"code":200,"more":false,"msgs":rows})
            } else if args["time"] == -1 && deletion() {
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
                {"id":63,"time":T-2,"notice":{"type":6,"comment":{"commentId":703,"threadId":"R_MV_5_42","content":"移动端查看的评论"}}},
                {"id":64,"time":T-3,"notice":{"type":8,"user":{"nickname":"合成收藏者"},"djRadio":{"id":81,"name":"通知播客 81"}}},
                {"id":65,"time":T-4,"notice":{"type":8,"djRadio":{"id":"82","name":"通知播客 82"}}},
                {"id":66,"time":T-5,"notice":{"type":8,"djRadio":{"id":"https://private.invalid/89","name":"合成不可打开播客"},"program":{"id":401}}}
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
