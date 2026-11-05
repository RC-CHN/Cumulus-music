//! Sealed backend for the real search membership/privilege WebView path.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const QUERY: &str = "合成会员刷新";
const COMPLEX: &str = "/api/search/pc/complex/page/v3";
const TABS: &str = "/api/search/pc/result/tab";
const MEMBERSHIP: &str = "/api/music-vip-membership/client/vip/info";
const PRIVILEGE: &str = "/api/song/enhance/privilege";
const PREVIEW_IDS: &str = r#"["601","602","603","604","605","606"]"#;

static STATE: Mutex<State> = Mutex::new(State {
    calls: Vec::new(),
    membership_reads: 0,
    complex_reads: 0,
    privilege_reads: 0,
    privilege_failures: 0,
});

struct State {
    calls: Vec<Value>,
    membership_reads: u32,
    complex_reads: u32,
    privilege_reads: u32,
    privilege_failures: u32,
}

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_SEARCH_PRIVILEGE_PROOF").is_some()
}

pub fn report() -> Value {
    let state = STATE.lock().unwrap();
    json!({
        "calls": state.calls,
        "membershipReads": state.membership_reads,
        "complexReads": state.complex_reads,
        "privilegeReads": state.privilege_reads,
        "privilegeFailures": state.privilege_failures,
    })
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    let mut state = STATE.lock().unwrap();
    let (outcome, response) = match path {
        TABS if args["keyword"] == QUERY => {
            assert_eq!(*args, json!({"keyword":QUERY}));
            (
                "tabs",
                Ok(
                    json!({"code":200,"data":{"algTabs":[{"title":"综合"},{"title":"单曲"},{"title":"歌手"}]}}),
                ),
            )
        }
        COMPLEX if args["keyword"] == QUERY => {
            assert_eq!(
                *args,
                json!({"keyword":QUERY,"scene":"normal","needCorrect":true,
                "channel":"typing","bizQueryInfo":""})
            );
            state.complex_reads += 1;
            ("comprehensive", Ok(comprehensive()))
        }
        MEMBERSHIP => {
            assert!(!cached);
            assert_eq!(*args, json!({"userId":"2"}));
            state.membership_reads += 1;
            let (label, vinyl, level, plus) = match state.membership_reads {
                1 => ("baseline", false, 0, false),
                2 => ("changed", true, 1, false),
                3 => ("same_tuple", true, 2, false),
                4 => ("changed_for_failure", true, 2, true),
                _ => panic!("unexpected fifth membership read"),
            };
            let now = 1_900_000_000_000_u64 + u64::from(state.membership_reads) * 1000;
            let future = now + 7 * 86_400_000;
            (
                label,
                Ok(json!({"code":200,"data":{"userId":2,"now":now,
                    "associator":{"expireTime":if vinyl {future} else {0},"vipLevel":level},
                    "redplus":{"expireTime":if plus {future} else {0},"vipLevel":u8::from(plus)}
                }})),
            )
        }
        PRIVILEGE => {
            assert!(!cached);
            assert_eq!(*args, json!({"ids":PREVIEW_IDS}));
            state.privilege_reads += 1;
            if state.membership_reads >= 4 {
                state.privilege_failures += 1;
                (
                    "failed",
                    Err(Error::Transport("合成搜索权限回读中断".into())),
                )
            } else {
                assert!(
                    state.membership_reads >= 2,
                    "privilege refresh before verified change"
                );
                ("refreshed", Ok(privileges()))
            }
        }
        _ => return None,
    };
    let membership_read = state.membership_reads;
    state
        .calls
        .push(json!({"path":path,"args":args,"cached":cached,
        "outcome":outcome,"membershipRead":membership_read}));
    Some(response)
}

fn comprehensive() -> Value {
    let songs = (601..=607)
        .map(|id| {
            let extension = match id {
                601 => json!({"privilege":{"st":-200,"flag":4096},
                    "noCopyrightRcmd":{"type":3,"expInfo":{"mvPlay":true},"mvInfo":{"id":955}}}),
                602 => json!({"privilege":{"st":0,"maxbr":320001}}),
                603 => json!({"privilege":{"st":0,"flag":462848}}),
                604 => json!({"privilege":{"st":0,"flag":393216}}),
                605 => json!({"privilege":{"st":0,"flag":2097152,"maxbr":6999000}}),
                606 => json!({"privilege":{"st":0,"fee":1,"flag":131072}}),
                _ => json!({"privilege":{"st":0}}),
            };
            json!({"type":"song",
                "baseInfo":{"simpleSongData":{"id":id,"name":format!("合成会员歌曲 {id}"),
                    "mv":if id == 601 {951} else {0},
                    "dt":180_000,"fee":0,"ar":[{"name":"合成会员歌手"}],
                    "al":{"id":741,"name":"合成会员专辑"}}},
                "extInfo":extension})
        })
        .collect::<Vec<_>>();
    json!({"code":200,"data":{"blocks":[
        {"blockCode":"search_block_song","uiElement":{"mainTitle":{"title":"会员刷新单曲"}},"resources":songs},
        {"blockCode":"search_block_artist","uiElement":{"mainTitle":{"title":"无关区块"}},"resources":[
            {"type":"artist","baseInfo":{"artistDTO":{"id":731,"name":"合成无关歌手"}}}
        ]}
    ]}})
}

fn privileges() -> Value {
    let rows = (601..=606)
        .map(|id| {
            let mut row = json!({"id":id,"st":0,"fee":0});
            match id {
                601 => {
                    row["fee"] = json!(1);
                    row["flag"] = json!(65536);
                    row["freeTrialPrivilege"] = json!({"userConsumable":true,"resConsumable":true,
                        "freeLimitTagType":"VIP_AND_FREE_TAG"});
                }
                602 => row["flag"] = json!(262144),
                604 => row["maxbr"] = json!(1998000),
                605 => row["flag"] = json!(4096),
                606 => row["flag"] = json!(131072),
                _ => {}
            }
            row
        })
        .collect::<Vec<_>>();
    json!({"code":200,"data":rows})
}
