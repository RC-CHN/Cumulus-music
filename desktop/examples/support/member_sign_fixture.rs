//! Official check-in shapes with closed synthetic dates and receipts.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const ROOT: &str = "/api/vipnewcenter/app/";
const PAST: u64 = 1_793_318_400_000;
const TODAY: u64 = PAST + 2 * 86_400_000;
const PAST_RECORD: &str = "009007199254740993";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static CARD_READS: AtomicUsize = AtomicUsize::new(0);
static PAST_READS: AtomicUsize = AtomicUsize::new(0);
static WRITE_CALLS: AtomicUsize = AtomicUsize::new(0);
static REVEALED: AtomicBool = AtomicBool::new(false);
static OFFER_READS: AtomicUsize = AtomicUsize::new(0);
static SIGN_SONG_LIKED: AtomicBool = AtomicBool::new(false);
static SIGN_SONG_LIKE_WRITES: Mutex<Vec<bool>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_MEMBER_SIGN_PROOF").is_some()
}

fn write_mode() -> Option<String> {
    if enabled() {
        std::env::var("CUMULUS_MEMBER_SIGN_WRITE_PROOF").ok()
    } else {
        None
    }
}

pub fn observed_server_time_ms() -> Option<u64> {
    write_mode().map(|_| TODAY)
}

pub fn reveal_write() -> bool {
    if write_mode().as_deref() != Some("lost-reopen") {
        return false;
    }
    REVEALED.store(true, Ordering::Release);
    true
}

fn today_detail() -> Value {
    json!({"time":TODAY,"songInfo":{"songId":1,"songName":"合成今日乐签歌曲"},"monthCheckInTotalDay":6,
        "monthCheckInPrizList":[{"day":3,"prizeShowName":"合成已领奖励","userPrizeRecordId":"PRIVATE"}]})
}

async fn write_response(mode: &str, suffix: &str, args: &Value) -> Option<Result<Value>> {
    assert!(matches!(mode, "confirmed" | "lost" | "lost-reopen"));
    let signed = REVEALED.load(Ordering::Acquire);
    let data = match suffix {
        "/api/vip-center-bff/task/sign" if mode != "lost-reopen" && *args == json!({}) => {
            assert_eq!(WRITE_CALLS.fetch_add(1, Ordering::AcqRel), 0);
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            if mode == "lost" {
                return Some(Err(Error::Transport(
                    "synthetic lost check-in receipt".into(),
                )));
            }
            REVEALED.store(true, Ordering::Release);
            json!(true)
        }
        "/api/vip-center-bff/task/sign" => panic!("check-in write must not be replayed"),
        "minidesk/music/sign/pc" if *args == json!({"type":1}) => {
            json!({"text":"黑胶乐签","subText":"每月打卡得VIP体验","btnText":if signed {"查看乐签"} else {"立即打卡"},
                "signInfoList":[{"dayText":"30日","sign":true,"today":false,"signTime":PAST},
                  {"dayText":"1日","today":true,"sign":signed,"signTime":signed.then_some(TODAY)}]})
        }
        "level/user/checkin/history/list" if *args == json!({}) => {
            if signed {
                json!([{"dayTime":TODAY,"recordId":82}])
            } else {
                json!([])
            }
        }
        "level/user/checkin/history/detail"
            if signed
                && (*args == json!({"type":1,"signDayTime":TODAY})
                    || *args == json!({"type":2,"recordId":82})) =>
        {
            today_detail()
        }
        suffix
            if suffix.starts_with("level/user/checkin/") || suffix == "minidesk/music/sign/pc" =>
        {
            return Some(Err(Error::Response(
                "unexpected synthetic check-in contract",
            )));
        }
        _ => return None,
    };
    Some(Ok(json!({"code":200,"data":data})))
}

pub async fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    if path == "/api/song/like/get" {
        let ids = if SIGN_SONG_LIKED.load(Ordering::Acquire) {
            vec![2, 3, 7, 10]
        } else {
            vec![3, 7, 10]
        };
        return Some(Ok(json!({"code":200,"ids":ids})));
    }
    if path == "/api/song/like" {
        assert!(write_mode().is_none());
        assert_eq!(args["trackId"], "2");
        assert_eq!(args["userid"], "2");
        let desired = args["like"].as_bool().unwrap();
        SIGN_SONG_LIKE_WRITES.lock().unwrap().push(desired);
        SIGN_SONG_LIKED.store(desired, Ordering::Release);
        return Some(Ok(json!({"code":200})));
    }
    if path == "/api/delivery/deliver" {
        assert_eq!(*args, json!({"positionId":65}));
        let refresh = OFFER_READS.fetch_add(1, Ordering::AcqRel) > 0;
        let portal = "https://music.163.com/g/vip-portal?nm_style=sbt&scene=pc_viptab_scan";
        return Some(Ok(json!({"code":200,"data":{"resourceData":{
            "text": if refresh {"不能显示的合成优惠"} else {"查看合成会员优惠"},
            "pcSideDisplay": if refresh {"4"} else {"3"},
            "jumpUrl": if refresh {format!("{portal}&sso=PRIVATE")} else {portal.into()}
        },"logContext":"PRIVATE"}})));
    }
    let suffix = path.strip_prefix(ROOT).unwrap_or(path);
    if suffix.starts_with("level/user/checkin/")
        || suffix == "minidesk/music/sign/pc"
        || path == "/api/vip-center-bff/task/sign"
    {
        CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
    }
    if let Some(mode) = write_mode()
        && let Some(response) = write_response(&mode, suffix, args).await
    {
        return Some(response);
    }
    let data = match suffix {
        "/api/vip-center-bff/task/sign" => panic!("unexpected sign write"),
        "viptab/module/get" | "resource/centertop/carousel/list" => json!([]),
        "/api/music-vip-membership/client/vip/info" => json!({"userId":2,"now":TODAY}),
        "minidesk/music/sign/pc" if *args == json!({"type":1}) => {
            let attempt = CARD_READS.fetch_add(1, Ordering::Relaxed);
            if attempt == 0 {
                return Some(Err(Error::Transport("synthetic sign card outage".into())));
            }
            if attempt == 2 {
                return Some(Err(Error::Transport(
                    "synthetic sign refresh outage".into(),
                )));
            }
            let today_signed = attempt < 3;
            json!({"text":"黑胶乐签","subText":"每月打卡得VIP体验","btnText":"查看乐签",
                   "bgTexture":"https://outside.invalid/?private=PRIVATE",
                   "signInfoList":[{"dayText":"30日","sign":true,"today":false,"signTime":PAST},
                       {"dayText":"31日","sign":false,"today":false},
                       {"dayText":"1日","sign":today_signed,"today":true,"signTime":TODAY}]})
        }
        "level/user/checkin/history/list" if *args == json!({}) => json!([
            {"dayTime":TODAY,"recordId":82},
            {"dayTime":PAST,"recordId":PAST_RECORD},
        ]),
        "level/user/checkin/history/detail"
            if *args == json!({"type":2,"recordId":PAST_RECORD}) =>
        {
            match PAST_READS.fetch_add(1, Ordering::Relaxed) {
                0 => return Some(Err(Error::Transport("synthetic sign detail outage".into()))),
                2 => tokio::time::sleep(std::time::Duration::from_millis(700)).await,
                _ => {}
            }
            json!({"time":PAST.to_string(),"songInfo":{"songId":2,"songName":"合成历史乐签歌曲","artistName":"合成歌手","cover":"https://outside.invalid/PRIVATE"},
                "wishWords":"保持好奇，继续听歌。","wishUserNickname":"合成评论者","monthCheckInTotalDay":5,
                "monthCheckInPrizList":[{"day":14,"prizeShowName":"第二档"},{"day":7,"prizeShowName":"黑胶体验","showSubTitle":"1天"},{"day":3,"prizeShowName":"首档"}]})
        }
        "level/user/checkin/history/detail" if *args == json!({"type":2,"recordId":82}) => {
            today_detail()
        }
        path if path.starts_with("level/user/checkin/") || path == "minidesk/music/sign/pc" => {
            return Some(Err(Error::Response("unexpected member sign request")));
        }
        _ => return None,
    };
    Some(Ok(json!({"code":200,"data":data})))
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

pub fn offer_reads() -> usize {
    OFFER_READS.load(Ordering::Acquire)
}

pub fn sign_song_like_writes() -> Vec<bool> {
    SIGN_SONG_LIKE_WRITES.lock().unwrap().clone()
}
