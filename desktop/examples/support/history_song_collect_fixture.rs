//! Sealed recent-song and owned-playlist transport for the real history picker.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Mutex};

pub const REVISION: &str = "sealed-history-song-collection";
const HISTORY: &str = "/api/play-record/song/list";
const RIGHTS: &str = "/api/song/enhance/privilege";
const DESTINATIONS: &str = "/api/user/playlist/v2";
const DETAIL: &str = "/api/v6/playlist/detail";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";

#[derive(Default)]
struct State {
    calls: Vec<Value>,
    members: BTreeMap<String, Vec<String>>,
}

#[derive(Default)]
pub struct Fixture(Mutex<State>);

fn playlist(id: &str, owner: u64, special: u8, name: &str, songs: &[String]) -> Value {
    json!({"id":id,"userId":owner,"creator":{"userId":owner,"nickname":"合成创作者"},
        "name":name,"specialType":special,"privacy":if owner==2 {10}else{0},"tags":[],
        "trackCount":songs.len(),"trackIds":songs.iter().map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>()})
}

fn owned(state: &State, id: &str) -> Value {
    let name = match id {
        "81" => "合成历史目标甲",
        "82" => "合成历史目标乙",
        _ => unreachable!("unknown history collection destination"),
    };
    playlist(
        id,
        2,
        0,
        name,
        state.members.get(id).map_or(&[], Vec::as_slice),
    )
}

fn song(id: u64) -> Value {
    let name = if id == 2 {
        "合成筛选目标 · 02".to_owned()
    } else {
        format!("合成历史歌曲 · {id:02}")
    };
    json!({"data":{"id":id,"name":name,"ar":[{"id":12,"name":"合成歌手"}],
        "al":{"id":13,"name":"合成历史专辑"},"dt":235000},"playTime":1793180000000_u64-id*1000,
        "multiTerminalInfo":{"os":"windows"}})
}

impl Fixture {
    pub fn report(&self) -> Value {
        let state = self.0.lock().unwrap();
        json!({"calls":state.calls,"members":state.members})
    }

    pub fn response(&self, path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
        if !matches!(
            path,
            HISTORY | RIGHTS | DESTINATIONS | DETAIL | ADD | "/api/playlist/create"
        ) {
            return None;
        }
        if path == DETAIL && !matches!(args["id"].as_str(), Some("81" | "82" | "83" | "91")) {
            return None;
        }
        let mut state = self.0.lock().unwrap();
        let bad = || Error::Response("unexpected history song collection fixture request");
        Some((|| {
            if cached {
                return Err(bad());
            }
            Ok(match path {
                HISTORY if args == &json!({"limit":300}) => {
                    json!({"code":200,"data":{"list":(1..=6).map(song).collect::<Vec<_>>()}})
                }
                RIGHTS => {
                    let ids: Vec<String> =
                        serde_json::from_str(args["ids"].as_str().ok_or_else(bad)?)?;
                    if args.as_object().is_none_or(|values| values.len() != 1)
                        || ids
                            .iter()
                            .any(|id| !matches!(id.as_str(), "1" | "2" | "3" | "4" | "5" | "6"))
                    {
                        return Err(bad());
                    }
                    let rows: Vec<_> = ids
                        .iter()
                        .map(|id| {
                            let mut row = json!({"id":id,"pl":320000,"dl":320000,"st":0,"fee":0});
                            if id != "4" {
                                row["subp"] = json!(match id.as_str() {
                                    "1" => 1,
                                    "2" | "5" => 0,
                                    _ => -1,
                                });
                            }
                            row
                        })
                        .collect();
                    json!({"code":200,"data":rows})
                }
                DESTINATIONS
                    if matches!(args["sortType"].as_str(), Some("default" | "recently"))
                        && args
                            == &json!({"uid":"2","offset":0,"limit":10,"sortType":args["sortType"]}) =>
                {
                    state.calls.push(json!({"kind":"destinations","args":args}));
                    json!({"code":200,"more":false,"playlist":[owned(&state,"81"),
                        playlist("91",7,0,"不属于当前账号的歌单",&[]),owned(&state,"82"),
                        playlist("83",2,5,"系统歌单不能作为普通目标",&[])]})
                }
                DETAIL => {
                    let id = args["id"].as_str().ok_or_else(bad)?;
                    if args != &json!({"id":id,"n":100000,"s":0}) {
                        return Err(bad());
                    }
                    let row = match id {
                        "81" | "82" => owned(&state, id),
                        "83" => playlist("83", 2, 5, "系统歌单不能作为普通目标", &[]),
                        "91" => playlist("91", 7, 0, "不属于当前账号的歌单", &[]),
                        _ => return Err(bad()),
                    };
                    state
                        .calls
                        .push(json!({"kind":"detail","pid":id,"trackIds":row["trackIds"]}));
                    json!({"code":200,"playlist":row})
                }
                ADD | "/api/playlist/create" => {
                    state
                        .calls
                        .push(json!({"kind":"unguarded_write","path":path,"args":args}));
                    return Err(bad());
                }
                _ => return Err(bad()),
            })
        })())
    }

    pub fn add(&self, path: &str, args: Value) -> Result<Value> {
        let mut state = self.0.lock().unwrap();
        state
            .calls
            .push(json!({"kind":"guarded_add","path":path,"args":args}));
        let bad = || Error::Response("unexpected ordered history collection add");
        let id = args["pid"].as_str().ok_or_else(bad)?;
        let songs = match id {
            "81" => vec!["5", "4", "2", "1"],
            "82" => vec!["1", "4"],
            _ => return Err(bad()),
        };
        if path != ADD
            || args != json!({"pid":id,"op":"add","trackIds":serde_json::to_string(&songs)?})
            || state.members.contains_key(id)
        {
            return Err(bad());
        }
        state
            .members
            .insert(id.into(), songs.into_iter().map(str::to_owned).collect());
        Ok(json!({"code":200}))
    }
}
