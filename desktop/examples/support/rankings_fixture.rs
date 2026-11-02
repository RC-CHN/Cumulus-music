//! Optional synthetic ranking reads for the catalog WebView proof. No HTTP.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};

fn products(period: &str, singles: bool, year: i64, empty: bool) -> Vec<Value> {
    if empty {
        return vec![];
    }
    (0..4)
        .map(|index| {
            json!({"albumId":7+index,"albumName":format!("合成{year}年{period}{} {index} · 用来核对很长的标题与返回范围",if singles {"单曲"}else{"专辑"}),"artistName":"合成歌手 · A long artist name","rank":index+1,"rankIncr":if index==0 {1}else{-1},"newest":index==2,"saleNum":123456+index})
        })
        .collect()
}
fn card(id: u64, name: &str, cover_only: bool) -> Value {
    json!({"id":id,"name":name,"nameShowStyle":if cover_only {"doubleRow"}else{"singleRow"},"targetType":"PLAYLIST","canPlay":true,"updateFrequency":"每周五更新",
        "trackRankList":(0..3).map(|i|json!({"trackId":i+1,"rank":i+1,"lastRank":if i==0 {0}else if i==1 {4}else{3}})).collect::<Vec<_>>(),
        "tracks":(0..3).map(|i|json!({"first":format!("合成榜单曲目 {i} · 标题很长时应正确省略"),"second":"合成歌手与合作者"})).collect::<Vec<_>>()})
}
pub fn response(path: &str, payload: &Value, mode: &str) -> Option<Result<Value>> {
    if std::env::var("CUMULUS_RANKINGS_PROOF").as_deref() != Ok("1") {
        return None;
    }
    if !matches!(
        path,
        "/api/toplist/detail/v2"
            | "/api/feealbum/songsaleboard/daily/type"
            | "/api/single/sales-ranking/get"
            | "/api/batch"
            | "/api/v6/playlist/detail"
            | "/api/v1/resource/comments/R_SO_4_1"
    ) {
        return None;
    }
    if mode == "rank-offline" {
        return Some(Err(Error::Transport(
            "synthetic ranking read unavailable".into(),
        )));
    }
    Some(Ok(match path {
        "/api/toplist/detail/v2" => {
            json!({"code":200,"data":if mode=="rank-empty" {vec![]}else{vec![
                json!({"name":"官方榜","categoryCode":"OFFICIAL","displayType":"TOP_3","list":[card(19723756,"合成上升榜 · 长标题检查",false),card(19723757,"合成新歌榜",false)]}),
                json!({"name":"全球榜","categoryCode":"GLOBAL","displayType":"ONLY_COVER","list":(101..105).map(|id|card(id,"合成全球音乐榜 · 两行名称检查",true)).collect::<Vec<_>>()})]}})
        }
        "/api/feealbum/songsaleboard/daily/type" => {
            json!({"code":200,"products":products("daily",false,2026,false)})
        }
        "/api/single/sales-ranking/get" => {
            json!({"code":200,"data":{"genTime":1700000000000_i64,"itemList":(1..=5).map(|id|json!({"songId":id,"songName":format!("合成畅销歌曲 {id}"),"artistName":"合成歌手","quantity":id*5,"indicator":99.5-id as f64,"rankIncr":id,"newest":id==1})).collect::<Vec<_>>()}})
        }
        "/api/batch" => {
            let mut response = json!({"code":200});
            for (key, args) in payload.as_object().expect("synthetic batch object") {
                assert!(key.starts_with("/api/feealbum/songsaleboard/"));
                let args: Value = serde_json::from_str(args.as_str().unwrap()).unwrap();
                let period = key.split('/').nth(4).unwrap();
                let singles = args["albumType"] == 1;
                let year = args["year"].as_i64().unwrap_or(2026);
                response[key] = json!({"code":200,"products":products(period,singles,year,mode=="rank-empty-period" && period=="week" && singles)});
            }
            response
        }
        "/api/v6/playlist/detail" => {
            json!({"code":200,"playlist":{"id":payload["id"],"userId":3,"name":"合成榜单详情","description":"验证从榜单详情返回原卡片。","creator":{"userId":3,"nickname":"合成作者"},"trackCount":5,"trackIds":(1..=5).map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>(),"tracks":(1..=5).map(super::song).collect::<Vec<_>>()}})
        }
        "/api/v1/resource/comments/R_SO_4_1" => {
            json!({"code":200,"comments":[super::comment(101)],"hotComments":[],"topComments":[],"total":1,"more":false})
        }
        _ => unreachable!(),
    }))
}
