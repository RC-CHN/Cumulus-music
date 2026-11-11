//! Exact native read schedule for continuous Songs query replacement.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const ORIGINAL: &str = "合成改写原词";
const SONGS: &str = "/api/search/song/list/page";
const FIRST: &str = "合成改写乙";
const NEXT: &str = "合成改写丙";
static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn enabled() -> bool {
    super::search_fixture::enabled()
        && std::env::var_os("CUMULUS_SEARCH_SONG_REWRITE_PROOF").is_some()
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

fn song(id: u64, name: String) -> Value {
    json!({"type":"song","baseInfo":{"simpleSongData":{
        "id":id,"name":name,"dt":186000,"ar":[{"id":12,"name":"合成艺人"}],
        "al":{"id":7,"name":"合成专辑"},
        "privilege":{"id":id,"fee":0,"st":0,"pl":320000,"dl":320000}
    }}})
}

fn page(rows: Vec<Value>, total: usize, more: bool, correction: Option<&str>) -> Value {
    let mut data = json!({"resources":rows,"hasMore":more,"totalCount":total});
    if let Some(term) = correction {
        data["queryRewriteInfo"] = json!({"rewriteQuery":term});
    }
    json!({"code":200,"data":data})
}

pub fn response(path: &str, args: &Value, cached: bool) -> Option<Result<Value>> {
    if !enabled() || path != SONGS {
        return None;
    }
    let mut calls = CALLS.lock().unwrap();
    let step = calls.len();
    let expected = [
        (ORIGINAL, 0, "rewrite-first"),
        (FIRST, 0, "first-head"),
        (FIRST, 10, "rewrite-continuation"),
        (NEXT, 0, "refused-head"),
        (NEXT, 0, "retried-head"),
        (NEXT, 10, "next-tail"),
    ]
    .get(step)
    .copied()
    .filter(|(term, offset, _)| {
        !cached
            && *args
                == json!({"keyword":term,"offset":offset,"limit":10,"scene":"normal",
                          "needCorrect":true,"channel":"typing"})
    });
    calls.push(json!({"path":path,"args":args,"cached":cached,
        "outcome":expected.map_or("unexpected", |(_, _, outcome)| outcome)
    }));
    if expected.is_none() {
        return Some(Err(Error::State(
            "unexpected synthetic Songs rewrite request",
        )));
    }
    Some(match step {
        0 => Ok(page(
            vec![song(31790, "不应显示的首读结果".into())],
            30,
            true,
            Some(FIRST),
        )),
        1 => Ok(page(
            (31701..=31710)
                .map(|id| song(id, format!("{FIRST} {id}")))
                .collect(),
            30,
            true,
            None,
        )),
        2 => Ok(page(
            vec![song(31791, "不应显示的续页结果".into())],
            11,
            false,
            Some(NEXT),
        )),
        3 => Err(Error::Transport("合成纠错首批读取暂不可用".into())),
        4 => Ok(page(
            std::iter::once(31701)
                .chain(31722..=31730)
                .map(|id| song(id, format!("{NEXT} {id}")))
                .collect(),
            11,
            true,
            None,
        )),
        5 => Ok(page(
            vec![song(31731, format!("{NEXT} 31731"))],
            11,
            false,
            None,
        )),
        _ => unreachable!("only the six expected reads are admitted"),
    })
}
