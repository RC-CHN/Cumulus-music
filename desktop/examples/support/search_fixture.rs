//! Synthetic official PC search payloads for actual WebView UI inspection.
use cumulus_core::{Error, Result, storage::StateStore};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
static MODE: Mutex<String> = Mutex::new(String::new());
static CALLS: AtomicUsize = AtomicUsize::new(0);
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_SEARCH_UI_PROOF").is_some()
}
pub fn control(value: &Value) {
    let mode = value["mode"].as_str().unwrap();
    assert!(
        [
            "good",
            "home_error",
            "suggest_error",
            "result_error",
            "unavailable",
            "empty",
            "expired"
        ]
        .contains(&mode)
    );
    *MODE.lock().unwrap() = mode.into();
}
pub fn initialize(store: &StateStore) -> Result<()> {
    if enabled() {
        store.put(
            "2",
            "search-history-v1",
            "keywords",
            &json!(
                (1..=30)
                    .map(|n| format!("历史关键词{n} · 合成记录"))
                    .collect::<Vec<_>>()
            ),
        )?;
    }
    Ok(())
}
pub fn calls() -> usize {
    CALLS.load(Ordering::SeqCst)
}
pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if let Some(result) = result_response(path, args) {
        return Some(result);
    }
    if !enabled() || !path.starts_with("/api/search/") {
        return None;
    }
    CALLS.fetch_add(1, Ordering::SeqCst);
    let mode = MODE.lock().unwrap().clone();
    if mode == "expired" {
        return Some(Ok(json!({"code":301})));
    }
    if mode == "home_error" && (path.contains("/chart/") || path.contains("/rcmd/"))
        || mode == "suggest_error" && path.contains("/suggest/")
    {
        return Some(Err(Error::Transport("synthetic search unavailable".into())));
    }
    let empty = mode == "empty";
    let words = |count: usize| -> Vec<Value> {
        if empty {
            return vec![];
        }
        (1..=count).map(|n|json!({"keyword":format!("合成推荐词{n}"),"toUserWord":format!("推荐词{n} · 非常长的标题用来检查省略显示")})).collect()
    };
    let data = match path {
        "/api/search/default/keyword/get" => {
            json!({"refreshTime":60000,"keywords":[{"realkeyword":"合成默认关键词","showKeyword":"发现合成好声音"}]})
        }
        "/api/search/pc/rcmd/keyword/get" => json!({"operateWords":{},"algWords":words(18)}),
        "/api/search/pc/chart/list" => {
            let ids = [
                ("HOT_SEARCH_SONG#@#", "热搜榜"),
                ("HOT_SEARCH_STYLE_ROCK#@#", "摇滚榜"),
                ("HOT_SEARCH_STYLE_ACG#@#", "ACG榜"),
            ];
            let tabs: Vec<_> = if empty {
                vec![]
            } else {
                ids.iter()
                    .map(|(id, name)| json!({"id":id,"name":name}))
                    .collect()
            };
            json!({"tabInfo":{"list":tabs},"items":{}})
        }
        "/api/search/pc/chart/detail" => {
            json!({"itemList":if empty{vec![]}else{(1..=20).map(|n|json!({"searchWord":format!("热搜关键词{n}"),"toUserWord":format!("{n} · SyntheticLongSongTitleWithoutSpacesAndWithExtraText"),"iconType":if n==1{1}else if n==2{2}else{0}})).collect::<Vec<_>>()}})
        }
        "/api/search/pc/suggest/keyword/get" => {
            let q = args["keyword"].as_str().unwrap_or("");
            json!({"recTitle":"猜你想搜","recs":if empty{vec![]}else{vec![json!({"keyword":format!("{q}推荐"),"name":format!("{q} · 推荐结果")})]},"suggests":if empty{vec![]}else{(1..=6).map(|n|json!({"keyword":format!("{q}联想{n}"),"name":format!("{q} · 联想结果 {n}")})).collect::<Vec<_>>()}})
        }
        _ => {
            return Some(Err(Error::Response(
                "unimplemented synthetic search endpoint",
            )));
        }
    };
    Some(Ok(json!({"code":200,"data":data})))
}

fn result_response(path: &str, args: &Value) -> Option<Result<Value>> {
    let category = match path {
        "/api/search/pc/complex/page" => "all",
        "/api/search/song/list/page" => "songs",
        "/api/v1/search/playlist/get" => "playlists",
        "/api/v1/search/artist/get" => "artists",
        "/api/v1/search/album/get" => "albums",
        "/api/v1/search/user/get" => "users",
        "/api/search/voice/get" => "voices",
        "/api/search/voicelist/get" => "podcasts",
        "/api/search/resource/lyric" => "lyrics",
        "/api/search/mlog/get" => "mvs",
        _ => return None,
    };
    CALLS.fetch_add(1, Ordering::SeqCst);
    let mode = MODE.lock().unwrap().clone();
    if mode == "expired" {
        return Some(Ok(json!({"code":301})));
    }
    if mode == "result_error" {
        return Some(Err(Error::Transport("合成搜索服务暂不可用".into())));
    }
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../core/tests/fixtures/search-results.json"
    ))
    .unwrap();
    let mut response = fixtures[category].clone();
    let legacy = response["result"].is_object();
    let container = if legacy {
        &mut response["result"]
    } else {
        &mut response["data"]
    };
    let key = if category == "all" {
        "blocks"
    } else if legacy {
        if category == "users" {
            "userprofiles"
        } else {
            category
        }
    } else {
        "resources"
    };
    let rows = container[key].as_array().unwrap();
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let limit = args["limit"].as_u64().unwrap_or(30) as usize;
    container[key] = json!(if mode == "empty" {
        vec![]
    } else if category == "all" {
        rows.clone()
    } else {
        (offset..(offset + limit).min(60))
            .map(|i| rows[i % rows.len()].clone())
            .collect()
    });
    container["hasMore"] = json!(mode != "empty" && category != "all" && offset + limit < 60);
    if mode == "empty" {
        for key in [
            "totalCount",
            "playlistCount",
            "artistCount",
            "albumCount",
            "userprofileCount",
        ] {
            if !container[key].is_null() {
                container[key] = json!(0);
            }
        }
    }
    if mode == "unavailable" && ["voices", "mvs"].contains(&category) {
        for row in response["data"]["resources"].as_array_mut().unwrap() {
            if category == "voices" {
                row["baseInfo"]["radio"] = Value::Null;
            } else {
                row["baseInfo"]["id"] = json!("opaque-video");
            }
        }
    }
    Some(Ok(response))
}
