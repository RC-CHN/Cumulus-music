//! Synthetic official PC search payloads for actual WebView UI inspection.
use cumulus_core::{Error, Result, storage::StateStore};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
static MODE: Mutex<String> = Mutex::new(String::new());
static CALLS: AtomicUsize = AtomicUsize::new(0);
static V3_CALLS: AtomicUsize = AtomicUsize::new(0);
const SEARCH_EXPERIMENTS: &str = "PH-PC-searchHot,PH-PC-searchMore2,PH-PC-searchPageEfficiency";
static HOT_PLAYBACK_CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static CAROUSEL_CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static VOICE_QUEUE_CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PLAYLIST_SUBSCRIPTIONS: AtomicUsize = AtomicUsize::new(0);
static ALBUM_SUBSCRIPTIONS: AtomicUsize = AtomicUsize::new(0);
static AUDIOBOOK_MORE_READS: AtomicUsize = AtomicUsize::new(0);
static PODCAST_HOME_MORE_READS: AtomicUsize = AtomicUsize::new(0);
static PODCAST_FOLLOWED: AtomicBool = AtomicBool::new(false);
static PODCAST_FOLLOW_MAP_STALE: AtomicBool = AtomicBool::new(false);
static PODCAST_FOLLOW_CALLS: Mutex<Vec<(String, Value)>> = Mutex::new(Vec::new());
static ARTIST_GALLERY_CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static ARTIST_GALLERY_FAILED: AtomicBool = AtomicBool::new(false);
static ARTIST_GALLERY_BATCH_FAILED: AtomicBool = AtomicBool::new(false);
pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_SEARCH_UI_PROOF").is_some()
}
pub fn v3_calls() -> usize {
    V3_CALLS.load(Ordering::SeqCst)
}
pub fn v3_enabled() -> bool {
    enabled() && std::env::var_os("CUMULUS_SEARCH_V3_PROOF").is_some()
}
pub fn carousel_enabled() -> bool {
    enabled() && std::env::var_os("CUMULUS_SEARCH_CAROUSEL_PROOF").is_some()
}
pub fn carousel_calls() -> Vec<Value> {
    CAROUSEL_CALLS.lock().unwrap().clone()
}
fn hot_card_enabled() -> bool {
    v3_enabled() && std::env::var_os("CUMULUS_SEARCH_HOT_CARD_PROOF").is_some()
}
pub fn hot_playback_enabled() -> bool {
    v3_enabled() && std::env::var_os("CUMULUS_SEARCH_HOT_PLAYBACK_PROOF").is_some()
}
pub fn hot_playback_calls() -> Vec<Value> {
    HOT_PLAYBACK_CALLS.lock().unwrap().clone()
}
pub fn control(value: &Value) {
    let mode = value["mode"].as_str().unwrap();
    assert!(
        [
            "good",
            "home_error",
            "chart_play",
            "hot_source",
            "selection_source",
            "audio_ranks",
            "podcast_top_classic",
            "podcast_home_disabled",
            "suggest_error",
            "result_error",
            "result_play_trial",
            "result_play_default",
            "unavailable",
            "missing_song",
            "mixed_results",
            "highlight",
            "short_pages",
            "correction",
            "correction_parts",
            "card_error",
            "cover",
            "cover_error",
            "scroll",
            "scroll_error",
            "scroll_duplicates",
            "song_waterfall",
            "song_waterfall_error",
            "voice_queue",
            "podcast_follow",
            "podcast_follow_incomplete",
            "podcast_follow_failed",
            "podcast_follow_lost",
            "playlist_creator_search",
            "playlist_playability",
            "album_artist_search",
            "artist_gallery",
            "preview",
            "lyrics",
            "empty",
            "expired"
        ]
        .contains(&mode)
    );
    *MODE.lock().unwrap() = mode.into();
}
pub fn podcast_follow_calls() -> Option<Vec<(String, Value)>> {
    MODE.lock()
        .unwrap()
        .starts_with("podcast_follow")
        .then(|| PODCAST_FOLLOW_CALLS.lock().unwrap().clone())
}
pub fn artist_gallery_calls() -> Option<Vec<Value>> {
    (MODE.lock().unwrap().as_str() == "artist_gallery")
        .then(|| ARTIST_GALLERY_CALLS.lock().unwrap().clone())
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
pub fn missing_song() -> bool {
    enabled() && *MODE.lock().unwrap() == "missing_song"
}
pub fn card_failed() -> bool {
    *MODE.lock().unwrap() == "card_error"
}
pub fn result_play_trial() -> Option<i64> {
    if !enabled() {
        return None;
    }
    match MODE.lock().unwrap().as_str() {
        "result_play_trial" | "song_waterfall" | "song_waterfall_error" => Some(42),
        "result_play_default" => Some(-1),
        _ => None,
    }
}
static AUDIO_RANK_DETAILS: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());
pub fn audio_rank_details() -> Vec<u64> {
    AUDIO_RANK_DETAILS.lock().unwrap().clone()
}
pub fn voice_queue_calls() -> Option<Vec<String>> {
    (MODE.lock().unwrap().as_str() == "voice_queue")
        .then(|| VOICE_QUEUE_CALLS.lock().unwrap().clone())
}

fn subscribable_playlist() -> Value {
    let mut playlist = super::playlist(31);
    playlist["privacy"] = json!(0);
    playlist["name"] = json!("可收藏的合成歌单");
    playlist
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if hot_playback_enabled()
        && matches!(
            path,
            "/api/v3/song/detail"
                | "/api/dj/program/detail"
                | "/api/dj/play/url/check"
                | "/api/djradio/v3/get"
                | "/api/v6/dj/program/byradio"
                | "/api/song/enhance/player/url/v1"
        )
    {
        HOT_PLAYBACK_CALLS
            .lock()
            .unwrap()
            .push(json!({"path":path,"args":args}));
    }
    if enabled()
        && MODE.lock().unwrap().as_str() == "artist_gallery"
        && path == "/api/v1/artist/list"
    {
        let filters = [
            args["area"].as_i64(),
            args["type"].as_i64(),
            args["initial"].as_i64(),
        ];
        let offset = args["offset"].as_u64().unwrap();
        assert_eq!(args["limit"], 20);
        ARTIST_GALLERY_CALLS.lock().unwrap().push(args.clone());
        let (start, count, more) = match (filters, offset) {
            ([Some(-1), Some(-1), Some(-1)], 0) => (201, 20, false),
            ([Some(7), Some(-1), Some(-1)], 0) => (401, 4, false),
            ([Some(7), Some(2), Some(-1)], 0) => (501, 4, false),
            ([Some(7), Some(2), Some(65)], 0) => (101, 20, true),
            ([Some(7), Some(2), Some(65)], 20) => {
                if !ARTIST_GALLERY_FAILED.swap(true, Ordering::SeqCst) {
                    return Some(Err(Error::Transport("合成歌手续页中断".into())));
                }
                (7, 1, false)
            }
            ([Some(7), Some(2), Some(0)], 0) => (0, 0, false),
            ([Some(7), Some(2), Some(66)], 0..=180) => (1000 + offset, 20, true),
            ([Some(7), Some(2), Some(66)], 200) => {
                if !ARTIST_GALLERY_BATCH_FAILED.swap(true, Ordering::SeqCst) {
                    return Some(Err(Error::Transport("合成歌手下一批中断".into())));
                }
                (1200, 20, false)
            }
            _ => {
                return Some(Err(Error::InvalidInput(
                    "unexpected synthetic artist filters",
                )));
            }
        };
        let artists = (start..start + count)
            .map(|id| json!({"id":id,"name":format!("合成歌手{id}"),"musicSize":id}))
            .collect::<Vec<_>>();
        return Some(Ok(json!({"code":200,"artists":artists,"more":more})));
    }
    if enabled() && MODE.lock().unwrap().as_str() == "album_artist_search" {
        match path {
            "/api/album/sub" => {
                assert_eq!(*args, json!({"id":"201"}));
                assert_eq!(ALBUM_SUBSCRIPTIONS.fetch_add(1, Ordering::SeqCst), 0);
                return Some(Ok(json!({"code":200})));
            }
            "/api/album/detail/dynamic" if args["id"] == "201" => {
                return Some(Ok(json!({
                    "code":200,"isSub":ALBUM_SUBSCRIPTIONS.load(Ordering::SeqCst) != 0
                })));
            }
            "/api/album/sublist" => {
                let rows = if ALBUM_SUBSCRIPTIONS.load(Ordering::SeqCst) == 0 {
                    vec![]
                } else {
                    vec![json!({"id":201,"name":"合成专辑"})]
                };
                return Some(Ok(json!({"code":200,"data":rows,"more":false})));
            }
            _ => {}
        }
    }
    if enabled() && MODE.lock().unwrap().as_str() == "playlist_creator_search" {
        match path {
            "/api/playlist/subscribe" => {
                assert_eq!(*args, json!({"id":"31"}));
                assert_eq!(PLAYLIST_SUBSCRIPTIONS.fetch_add(1, Ordering::SeqCst), 0);
                return Some(Ok(json!({"code":200})));
            }
            "/api/v6/playlist/detail" if args["id"] == "31" => {
                return Some(Ok(json!({"code":200,"playlist":subscribable_playlist()})));
            }
            "/api/user/playlist"
                if args["uid"] == "2" && PLAYLIST_SUBSCRIPTIONS.load(Ordering::SeqCst) != 0 =>
            {
                let mut rows = (1..=24).map(super::playlist).collect::<Vec<_>>();
                rows.push(subscribable_playlist());
                return Some(Ok(json!({"code":200,"more":false,"playlist":rows})));
            }
            _ => {}
        }
    }
    if enabled()
        && let Some(result) = podcast_response(path, args)
    {
        return Some(result);
    }
    if let Some(result) = result_response(path, args) {
        return Some(result);
    }
    if !enabled()
        || !(path.starts_with("/api/search/") || path == "/api/hot/search/relate/song/get")
    {
        return None;
    }
    CALLS.fetch_add(1, Ordering::SeqCst);
    let mode = match MODE.lock().unwrap().as_str() {
        "podcast_top_classic" => "audio_ranks".into(),
        other => other.to_owned(),
    };
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
        "/api/hot/search/relate/song/get" if mode == "chart_play" => {
            let expected = (1..=20)
                .map(|n| format!("热搜关键词{n}"))
                .collect::<Vec<_>>();
            let received = args["keywords"]
                .as_str()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());
            if received.as_ref() != Some(&expected) {
                return Some(Err(Error::InvalidInput(
                    "wrong synthetic chart play keywords",
                )));
            }
            json!({"songs":[{"song":{"id":178}},{"song":{"id":177}}]})
        }
        "/api/search/default/keyword/get" => {
            json!({"refreshTime":60000,"keywords":[{"realkeyword":"合成默认关键词","showKeyword":"发现合成好声音"}]})
        }
        "/api/search/pc/rcmd/keyword/get" => json!({"operateWords":{},"algWords":words(18)}),
        "/api/search/pc/chart/list" if mode == "audio_ranks" => {
            json!({"tabInfo":{"list":[
                {"id":"HOT_SEARCH_DJ#@#","name":"播客榜","button":{"text":"更多","url":"https://ignored.synthetic.invalid"}},
                {"id":"HOT_SEARCH_DJ_VOICE_BOOK#@#","name":"听书榜","button":{"text":"更多","url":"https://ignored.synthetic.invalid"}}]},
                "items":{"HOT_SEARCH_DJ#@#":[
                    {"toUserWord":"合成播客榜条目","url":"orpheus://audio/detail?id=301&private=omit"},
                    {"toUserWord":"外链不可打开","url":"https://synthetic.invalid/?id=999"},
                    {"toUserWord":"无目标不可打开"}]}})
        }
        "/api/search/pc/chart/detail" if mode == "audio_ranks" => {
            if args["id"] != "HOT_SEARCH_DJ_VOICE_BOOK#@#" {
                return Some(Err(Error::InvalidInput("unexpected synthetic audio chart")));
            }
            json!({"itemList":[{"toUserWord":"合成听书榜条目","url":"orpheus://audio/detail?id=302"},
                {"toUserWord":"重复编号不可打开","url":"orpheus://audio/detail?id=999&id=1000"}]})
        }
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
                    .map(|(id, name)| json!({"id":id,"name":name,"needPlay":mode=="chart_play" && *id=="HOT_SEARCH_SONG#@#"}))
                    .collect()
            };
            json!({"tabInfo":{"list":tabs},"items":{}})
        }
        "/api/search/pc/chart/detail" if mode == "hot_source" => {
            let hot = args["id"] == "HOT_SEARCH_SONG#@#";
            json!({"itemList":[{"searchWord":if hot {"合成热搜来源"} else {"合成普通来源"},
                "toUserWord":"合成榜单条目","tabId":args["id"]}]})
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

fn podcast_response(path: &str, args: &Value) -> Option<Result<Value>> {
    if MODE.lock().unwrap().as_str() == "voice_queue"
        && matches!(
            path,
            "/api/djradio/v3/get" | "/api/v6/dj/program/byradio" | "/api/dj/program/detail"
        )
    {
        VOICE_QUEUE_CALLS.lock().unwrap().push(path.into());
    }
    let number = |key: &str, fallback| {
        args[key]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .or_else(|| args[key].as_u64())
            .unwrap_or(fallback)
    };
    let radio = |id| json!({"id":id,"name":format!("合成播客 {id}"),"programCount":3,"dj":{"nickname":"合成主播"}});
    let program = |id, radio_id| json!({"id":id,"name":format!("合成声音 {id}"),"mainSong":{"id":id-400,"name":format!("合成声音 {id}"),"duration":360000},"duration":360000,"radio":radio(radio_id),"dj":{"nickname":"合成主播"}});
    let configured = MODE.lock().unwrap().clone();
    if configured.starts_with("podcast_follow")
        && matches!(
            path,
            "/api/djradio/get/subed"
                | "/api/djradio/v3/get"
                | "/api/djradio/sub"
                | "/api/djradio/unsub"
        )
    {
        PODCAST_FOLLOW_CALLS
            .lock()
            .unwrap()
            .push((path.into(), args.clone()));
        return Some(match path {
            "/api/djradio/get/subed" => {
                assert_eq!(args, &json!({"targetUserId":"2","limit":1000}));
                if configured == "podcast_follow_failed" {
                    Err(Error::Transport("synthetic membership read failed".into()))
                } else {
                    let stale = PODCAST_FOLLOW_MAP_STALE.swap(false, Ordering::SeqCst);
                    let ids = if PODCAST_FOLLOWED.load(Ordering::SeqCst) && !stale {
                        vec![json!({"id":301})]
                    } else {
                        vec![]
                    };
                    Ok(json!({"code":200,"djRadios":ids,"count":ids.len(),
                        "hasMore":configured == "podcast_follow_incomplete"}))
                }
            }
            "/api/djradio/v3/get" => {
                assert_eq!(args, &json!({"id":"301"}));
                let mut value = radio(301);
                value["dj"]["userId"] = json!(501);
                value["subed"] = json!(PODCAST_FOLLOWED.load(Ordering::SeqCst));
                Ok(json!({"code":200,"data":value}))
            }
            "/api/djradio/sub" | "/api/djradio/unsub" => {
                assert_eq!(args, &json!({"id":"301"}));
                PODCAST_FOLLOWED.store(path == "/api/djradio/sub", Ordering::SeqCst);
                if path == "/api/djradio/sub" {
                    PODCAST_FOLLOW_MAP_STALE.store(true, Ordering::SeqCst);
                }
                if configured == "podcast_follow_lost" {
                    Err(Error::Transport(
                        "synthetic subscription acknowledgement lost".into(),
                    ))
                } else {
                    Ok(json!({"code":200}))
                }
            }
            _ => unreachable!(),
        });
    }
    let mode = if configured == "podcast_top_classic" {
        "audio_ranks".into()
    } else {
        configured.clone()
    };
    let data = match path {
        "/api/rtrs/abt/front/expinfo/list" if args == &json!({"expNames":"podcast_tab_better"}) => {
            if configured == "podcast_top_classic" {
                return Some(Err(Error::Transport(
                    "synthetic experiment lookup unavailable".into(),
                )));
            }
            json!({"code":200,"data":[{"expName":"podcast_tab_better","expGroupName":"t1"}]})
        }
        "/api/podcast/ranklist/square/ranklist/get/v2"
            if matches!(mode.as_str(), "audio_ranks" | "podcast_home_disabled") =>
        {
            let rows = match args["ranklistSquareId"].as_u64() {
                Some(2) => json!([
                    {"resourceId":"daily","name":"今日最热","chartType":3,"showRankOffset":true},
                    {"resourceId":"2","name":"音乐声音","chartType":0}]),
                Some(1) => json!([
                    {"resourceId":"score","name":"高分必听","chartType":7,"showScore":true},
                    {"resourceId":"2","name":"音乐播单","chartType":0}]),
                _ => {
                    return Some(Err(Error::InvalidInput(
                        "unexpected synthetic podcast rank menu",
                    )));
                }
            };
            json!({"code":200,"data":rows})
        }
        "/api/charts/voice" if matches!(mode.as_str(), "audio_ranks" | "podcast_home_disabled") => {
            assert_eq!(*args, json!({"chartType":"3"}));
            json!({"code":200,"data":[{"voiceMetaVO":program(407,305),"chartMetaVO":{"rank":1,"lastRank":3}}]})
        }
        "/api/charts/voice/category" if mode == "podcast_home_disabled" => {
            assert_eq!(*args, json!({"categoryId":"2"}));
            json!({"code":200,"data":[{"voiceMetaVO":program(408,305),"chartMetaVO":{"rank":1}}]})
        }
        "/api/charts/voicelist"
            if matches!(mode.as_str(), "audio_ranks" | "podcast_home_disabled") =>
        {
            assert_eq!(*args, json!({"chartType":"7"}));
            let rows = (361..=370)
                .map(|id| {
                    let mut item = radio(id);
                    item["score"] = json!("9.3分");
                    json!({"voicelistMetaVO":item,"chartMetaVO":{"rank":id-359}})
                })
                .collect::<Vec<_>>();
            json!({"code":200,"data":rows})
        }
        "/api/charts/voicelist/category" if mode == "podcast_home_disabled" => {
            assert_eq!(*args, json!({"categoryId":"2"}));
            json!({"code":200,"data":[{"voicelistMetaVO":radio(364),"chartMetaVO":{"rank":1}}]})
        }
        "/api/dj/toplist/hours" if mode == "podcast_home_disabled" => {
            assert_eq!(*args, json!({}));
            json!({"code":200,"data":{"list":[{"id":29,"nickname":"合成主播榜","rank":1,"lastRank":4}]}})
        }
        "/api/voice/book/chart/config"
            if matches!(mode.as_str(), "audio_ranks" | "podcast_home_disabled") =>
        {
            assert_eq!(*args, json!({}));
            json!({"code":200,"data":[
                {"chartNameCode":"FREE_CHARTS","subList":[{"code":"VOICE_BOOK_FREE_CHARTS_ALL","name":"免费总榜"}]},
                {"chartNameCode":"OVERALL_CHARTS","subList":[{"code":"RECOMMEND_CHARTS","name":"个性听书榜"},{"code":"VOICE_BOOK_OVERALL","name":"听书热门榜"}]}
            ]})
        }
        "/api/voice/book/chart/get"
            if matches!(mode.as_str(), "audio_ranks" | "podcast_home_disabled") =>
        {
            let id = match args["subCode"].as_str() {
                Some("VOICE_BOOK_OVERALL") => {
                    assert_eq!(args["chartNameCode"], "OVERALL_CHARTS");
                    362
                }
                Some("VOICE_BOOK_FREE_CHARTS_ALL") => {
                    assert_eq!(args["chartNameCode"], "FREE_CHARTS");
                    363
                }
                Some("RECOMMEND_CHARTS") => 365,
                _ => {
                    return Some(Err(Error::InvalidInput(
                        "unexpected synthetic audiobook ranking",
                    )));
                }
            };
            assert_eq!(args["type"], "0");
            json!({"code":200,"data":[{"voicelistMetaVO":radio(id),"chartMetaVO":{"rank":1}}]})
        }
        "/api/user/setting" if matches!(mode.as_str(), "audio_ranks" | "podcast_home_disabled") => {
            assert_eq!(*args, json!({}));
            json!({"code":200,"setting":{"personalServiceSetting":if mode == "podcast_home_disabled" {2} else {0}}})
        }
        "/api/voice/homepage/block/page" if mode == "podcast_home_disabled" => {
            return Some(Err(Error::InvalidInput(
                "disabled podcast recommendations must not be requested",
            )));
        }
        "/api/voice/homepage/block/page" if mode == "audio_ranks" => {
            let radios = |title, id| {
                json!({"blockId":7,"blockCode":"same_block",
                "showType":"VOICE_HOMEPAGE_FIXED_VOICELIST","uiElement":{"mainTitle":{"title":title}},
                "creatives":[{"creativeExtInfoVO":{"radio":radio(id)}}]})
            };
            let voice = |id: u64, resource_id: u64| {
                json!({"resourceType":"voice","resourceId":resource_id.to_string(),
                "valid":true,"resourceUrl":"https://ignored.synthetic.invalid","resourceExtInfo":{"djProgram":program(id,305)}})
            };
            let programs = json!({"blockId":9,"showType":"VOICE_HOMEPAGE_WEEKNEW_VOICE",
                "uiElement":{"mainTitle":{"title":"合成新节目"}},"creatives":[
                    {"resources":[voice(404,404),voice(405,999)]},
                    {"resources":[voice(406,406)]}]});
            match args["cursor"].as_str() {
                Some("") => {
                    json!({"code":200,"data":{"cursor":"{\"homePrivate\":\"next\"}","hasMore":true,
                    "blocks":[radios("合成首页首批",305),programs,
                        {"showType":"VOICE_HOMEPAGE_BLOCK_BANNER","uiElement":{"mainTitle":{"title":"不显示推广"}},"creatives":[{}]}]}})
                }
                Some("{\"homePrivate\":\"next\"}") => {
                    if PODCAST_HOME_MORE_READS.fetch_add(1, Ordering::SeqCst) == 0 {
                        return Some(Err(Error::Transport(
                            "synthetic podcast page unavailable".into(),
                        )));
                    }
                    json!({"code":200,"data":{"cursor":null,"hasMore":false,
                        "blocks":[radios("合成首页后续",306)]}})
                }
                _ => {
                    return Some(Err(Error::InvalidInput(
                        "unexpected synthetic podcast page",
                    )));
                }
            }
        }
        "/api/dj/radio/category/list"
            if matches!(mode.as_str(), "audio_ranks" | "podcast_home_disabled") =>
        {
            assert_eq!(*args, json!({}));
            let mut categories = vec![
                json!({"id":"2","name":"音乐播客","subCategoryVO":[{"id":"000","name":"全部"},{"id":"3074096","name":"乐评"}]}),
                json!({"id":"0020141111","name":"精品播客","subCategoryVO":[{"id":"000","name":"全部"}]}),
                json!({"id":"3","name":"情感","subCategoryVO":[{"id":"000","name":"全部"}]}),
                json!({"id":"10001","name":"有声书","subCategoryVO":[{"id":"000","name":"全部"},{"id":"3076096","name":"文学"}]}),
            ];
            categories.extend([("8","脱口秀"),("2001","创作翻唱"),("10002","电音"),("11","知识"),("3001","二次元")]
                .map(|(id,name)| json!({"id":id,"name":name,"subCategoryVO":[{"id":"000","name":"全部"}]})));
            json!({"code":200,"data":{"parentCategoryVoList":categories}})
        }
        "/api/dj/radio/category/radio/list"
            if matches!(mode.as_str(), "audio_ranks" | "podcast_home_disabled") =>
        {
            assert_eq!(args["limit"], 20);
            let cursor = args["cursor"].as_str();
            let sub = args["subId"].as_str();
            let parent = args["parentId"].as_str();
            if parent != Some("10001") {
                let (id, next) = match (parent, sub, cursor) {
                    (Some("2"), Some("000"), Some("")) => (351, Some("music-next")),
                    (Some("2"), Some("000"), Some("music-next")) => (352, None),
                    (Some("2"), Some("3074096"), Some("")) => (353, None),
                    (Some("0020141111"), Some("000"), Some("")) => (354, None),
                    (Some("3" | "8" | "2001" | "10002" | "11" | "3001"), Some("000"), Some("")) => {
                        (355, None)
                    }
                    _ => {
                        return Some(Err(Error::InvalidInput(
                            "unexpected synthetic podcast category",
                        )));
                    }
                };
                let mut item = radio(id);
                item["score"] = match id {
                    351 => json!(8.75),
                    352 => json!(0),
                    353 => json!("0"),
                    354 => json!("https://ignored.synthetic.invalid/category-private"),
                    _ => Value::Null,
                };
                return Some(Ok(json!({"code":200,"data":{"radioList":[item],
                    "page":{"cursor":next,"more":next.is_some()}}})));
            }
            match (sub, cursor) {
                (Some("000"), Some("")) => json!({"code":200,"data":{
                    "radioList":[{"id":302,"name":"合成有声书首批","programCount":3,"score":"9.50"}],
                    "page":{"cursor":"{\"internal\":\"next\"}","more":true}}}),
                (Some("000"), Some("{\"internal\":\"next\"}")) => {
                    if AUDIOBOOK_MORE_READS.fetch_add(1, Ordering::SeqCst) == 0 {
                        return Some(Err(Error::Transport(
                            "synthetic book page unavailable".into(),
                        )));
                    }
                    json!({"code":200,"data":{"radioList":[
                        {"id":302,"name":"合成有声书重复项"}, {"id":303,"name":"合成有声书续页"}],
                        "page":{"cursor":null,"more":false}}})
                }
                (Some("3076096"), Some("")) => json!({"code":200,"data":{
                    "radioList":[{"id":304,"name":"合成文学有声书"}],
                    "page":{"cursor":null,"more":false}}}),
                _ => return Some(Err(Error::InvalidInput("unexpected synthetic book page"))),
            }
        }
        "/api/djradio/v3/get" => {
            if card_failed() {
                return Some(Err(Error::Transport("合成播客读取失败".into())));
            }
            let id = number("id", 301);
            if MODE.lock().unwrap().as_str() == "audio_ranks" {
                AUDIO_RANK_DETAILS.lock().unwrap().push(id);
            }
            json!({"code":200,"data":radio(id)})
        }
        "/api/v6/dj/program/byradio" => {
            json!({"code":200,"data":{"programs":(401..=403).map(|id|program(id,number("radioId",301))).collect::<Vec<_>>(),"count":3,"more":false,"asc":args["asc"]}})
        }
        "/api/content/my/liked/voice" => json!({"code":200,"data":{"data":[],"hasMore":false}}),
        "/api/dj/user/setting/resumeplay/get" => json!({"code":200,"data":{"resumePlayValue":0}}),
        "/api/dj/program/detail" => json!({"code":200,"program":program(number("id",401),301)}),
        "/api/dj/play/url/check" => {
            let id = number("voiceId", 401);
            json!({"code":200,"data":{"voiceId":id,"songId":id-400,"canPlay":true}})
        }
        _ => return None,
    };
    Some(Ok(data))
}

fn hot_playback_word(args: &Value) -> Option<&str> {
    args["keyword"].as_str().filter(|word| {
        matches!(
            *word,
            "合成热点歌曲"
                | "合成热点声音"
                | "合成热点仅封面"
                | "合成热点延迟"
                | "合成热点禁止外链"
        )
    })
}

fn result_response(path: &str, args: &Value) -> Option<Result<Value>> {
    if carousel_enabled() {
        let response = match path {
            "/api/rtrs/abt/front/expinfo/list"
                if *args == json!({"expNames":SEARCH_EXPERIMENTS}) =>
            {
                Some(json!({"code":200,"data":[
                    {"expName":"PH-PC-searchHot","expGroupName":"c"},
                    {"expName":"PH-PC-searchMore2","expGroupName":"t1"},
                    {"expName":"PH-PC-searchPageEfficiency","expGroupName":"t1"}
                ]}))
            }
            "/api/search/pc/complex/page"
                if *args
                    == json!({"keyword":"合成轮播","scene":"normal","needCorrect":true,"channel":"typing","cursor":"{\"page\":0,\"ignoreBlocks\":[],\"traceId\":\"\"}","bizQueryInfo":""}) =>
            {
                let playlist = |id| json!({"type":"playlist","baseInfo":{"id":id,"userId":3,"name":format!("合成轮播歌单 {id}"),"trackCount":40,"creator":{"userId":3,"nickname":"合成作者"}}});
                let artist = |id| json!({"type":"artist","baseInfo":{"id":id,"name":format!("合成轮播歌手 {id}")}});
                Some(json!({"code":200,"data":{"blocks":[
                    {"blockCode":"search_block_best_match","resources":[artist(102),playlist(28),playlist(29),playlist(30)]},
                    {"blockCode":"search_block_playlist","uiElement":{"mainTitle":{"title":"歌单"}},"resources":(20..32).map(playlist).collect::<Vec<_>>()},
                    {"blockCode":"search_block_artist","uiElement":{"mainTitle":{"title":"歌手"}},"resources":(100..110).map(artist).collect::<Vec<_>>()}
                ]}}))
            }
            _ => None,
        };
        if let Some(response) = response {
            CAROUSEL_CALLS
                .lock()
                .unwrap()
                .push(json!({"path":path,"args":args}));
            return Some(Ok(response));
        }
    }
    if v3_enabled() {
        let tag = match path {
            "/api/homepage/rcmd/tag/songs"
                if args
                    == &json!({"reasonId":"reason-41","reasonName":"合成推荐","reasonType":1,"songId":"3"}) =>
            {
                Some(
                    json!({"code":200,"data":{"title":"合成推荐详情","playCount":123,"reasonRcmdSongCommonVOS":[{"songData":{"id":18,"name":"合成推荐曲目"}}]}}),
                )
            }
            "/api/chart/detail"
                if args == &json!({"chartCode":"city-chart","targetId":"77","targetType":"4"}) =>
            {
                Some(json!({"code":200,"data":{"name":"合成城市详情","playCount":"456"}}))
            }
            "/api/chart/song/detail"
                if args == &json!({"chartCode":"city-chart","targetId":"77","targetType":"4"}) =>
            {
                Some(
                    json!({"code":200,"data":{"charts":[{"songData":{"id":19,"name":"合成城市曲目"}}]}}),
                )
            }
            _ => None,
        };
        if let Some(reply) = tag {
            return Some(Ok(reply));
        }
        let response = match path {
            "/api/rtrs/abt/front/expinfo/list"
                if args == &json!({"expNames":SEARCH_EXPERIMENTS}) =>
            {
                Some(json!({"code":200,"data":[{"expName":"PH-PC-searchHot","expGroupName":"t1"}]}))
            }
            "/api/search/pc/result/tab"
                if super::search_mv_fixture::fallback_enabled()
                    && args == &json!({"keyword":"合成替代"}) =>
            {
                Some(json!({"code":200,"data":{"algTabs":[{"title":"综合"},{"title":"单曲"}]}}))
            }
            "/api/search/pc/complex/page/v3"
                if super::search_mv_fixture::fallback_enabled()
                    && args
                        == &json!({"keyword":"合成替代","scene":"normal","needCorrect":true,"channel":"typing","bizQueryInfo":""}) =>
            {
                Some(super::search_mv_fixture::fallback_results())
            }
            "/api/search/pc/result/tab" if args == &json!({"keyword":"合成 V3"}) => Some(
                json!({"code":200,"data":{"algTabs":[{"title":"综合"},{"title":"视频"},{"title":"单曲"}]}}),
            ),
            "/api/search/pc/result/tab"
                if hot_playback_enabled() && hot_playback_word(args).is_some() =>
            {
                Some(
                    json!({"code":200,"data":{"algTabs":[{"title":"综合"},{"title":"单曲"},{"title":"声音"}]}}),
                )
            }
            "/api/search/pc/complex/page/v3"
                if hot_playback_enabled() && hot_playback_word(args).is_some() =>
            {
                let word = hot_playback_word(args).unwrap();
                assert_eq!(
                    *args,
                    json!({"keyword":word,"scene":"normal","needCorrect":true,"channel":"typing","bizQueryInfo":""})
                );
                let action = match word {
                    "合成热点歌曲" => json!("orpheus://song/1?trace=PRIVATE"),
                    "合成热点延迟" => json!("orpheus://song/2?trace=PRIVATE"),
                    "合成热点禁止外链" => json!("https://example.invalid/PRIVATE"),
                    _ => Value::Null,
                };
                Some(json!({"code":200,"data":{"blocks":[
                    {"blockCode":"search_block_hot_result_hot_explain","resources":[{
                        "baseInfo":{"hotExplainModuleData":{"title":word,"text":"热点播放操作验证。","textAction":action,
                            "urlText":"查看播放详情","canClick":word!="合成热点仅封面"}},
                        "relatedResources":[{"resourceType":"voice","baseInfo":{"pubDJProgramData":{
                            "id":401,"name":"合成热点单集","mainSong":{"id":1},"radio":{"id":301}
                        }}}]
                    }]},
                    {"blockCode":"search_block_song","resources":(10..16).map(|id| json!({
                        "baseInfo":{"simpleSongData":{"id":id,"name":format!("合成搜索结果 {id}")}}
                    })).collect::<Vec<_>>()}
                ]}}))
            }
            "/api/search/pc/result/tab"
                if hot_card_enabled()
                    && matches!(
                        args["keyword"].as_str(),
                        Some("合成热点链接" | "合成热点资源" | "合成热点外链")
                    ) =>
            {
                Some(json!({"code":200,"data":{"algTabs":[{"title":"综合"},{"title":"歌单"}]}}))
            }
            "/api/search/pc/complex/page/v3"
                if hot_card_enabled()
                    && matches!(
                        args["keyword"].as_str(),
                        Some("合成热点链接" | "合成热点资源" | "合成热点外链")
                    ) =>
            {
                let word = args["keyword"].as_str().unwrap();
                if *args
                    != json!({"keyword":word,"scene":"normal","needCorrect":true,"channel":"typing","bizQueryInfo":""})
                {
                    return Some(Err(Error::InvalidInput("wrong synthetic hotspot query")));
                }
                let action = match word {
                    "合成热点链接" => json!("orpheus://playlist/28?trace=PRIVATE"),
                    "合成热点外链" => json!("https://example.invalid/PRIVATE"),
                    _ => Value::Null,
                };
                Some(json!({"code":200,"data":{"blocks":[
                    {"blockCode":"search_block_hot_result_hot_explain","resources":[{
                        "baseInfo":{"hotExplainModuleData":{"title":word,"text":"热点说明的合成资料，仅供界面验证。","textAction":action,
                            "urlText":"查看相关歌单","canClick":true,"hotValue":253567,"pictureCornerMarker":"热点"}},
                        "relatedResources":[{"resourceType":"playlist","baseInfo":{"pubPlaylistData":{"id":29,"name":"关联歌单"}}}]
                    }]}
                ]}}))
            }
            "/api/search/pc/complex/page/v3"
                if args
                    == &json!({"keyword":"合成 V3","scene":"normal","needCorrect":true,"channel":"typing","bizQueryInfo":""}) =>
            {
                Some(json!({"code":200,"data":{"blocks":[
                    {"blockCode":"search_block_hot_result_hot_explain","resources":[{"baseInfo":{"hotExplainModuleData":{"title":"热点说明","text":"合成热点内容","textAction":"https://example.invalid/private"}}}]},
                    {"blockCode":"search_block_hot_result_core_resources","resources":(100..104).map(|id|
                        json!({"type":"artist","baseInfo":{"artistDTO":{"id":id,"name":format!("合成热点歌手 {id}")}}})
                    ).collect::<Vec<_>>()},
                    {"blockCode":"search_block_song","resources":[{"type":"song","baseInfo":{"simpleSongData":{"id":1,"name":"综合 V3 合成歌曲","mark":1048576,"originCoverType":1,"dt":60000,"ar":[],"al":{"id":1,"name":"合成专辑"},"newSongExt":{"songAlias":"歌词存在时隐藏别名"}}},"extInfo":{"mv":601,"lyrics":{"txt":"🎵合成歌词片段","range":[{"first":2,"second":4}]}}},
                    {"type":"song","baseInfo":{"simpleSongData":{"id":2,"name":"合成备用曲目","mark":1,"originCoverType":"1","album":{"mark":1048576},"newSongExt":{"songAlias":"不会展示的旧别名"}}},"extInfo":{"songAlias":"前缀<em>合成 V3</em>别名","lyrics":{"txt":"🎵合成","range":[{"first":1,"second":4}]}}},
                    {"type":"song","baseInfo":{"simpleSongData":{"id":3,"name":"合成标签曲目"}},"extInfo":{"algClickableTags":[
                        {"text":"合成推荐","clickable":true,"reasonType":1,"reasonId":"reason-41","resourceId":3},
                        {"text":"合成城市","clickable":true,"reasonType":5,"url":"https://example.invalid/private?chartCode=city-chart&targetId=77&targetType=4"},
                        {"text":"不可跳转","clickable":true,"reasonType":42}
                    ]}}]}
                ]}}))
            }
            _ => None,
        };
        if let Some(response) = response {
            V3_CALLS.fetch_add(1, Ordering::SeqCst);
            CALLS.fetch_add(1, Ordering::SeqCst);
            return Some(Ok(response));
        }
        if matches!(
            path,
            "/api/rtrs/abt/front/expinfo/list"
                | "/api/search/pc/result/tab"
                | "/api/search/pc/complex/page/v3"
                | "/api/search/pc/complex/page"
        ) {
            return Some(Err(Error::InvalidInput(
                "unexpected synthetic search experiment request",
            )));
        }
    }
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
    if mode == "scroll_error" && args["offset"].as_u64().unwrap_or(0) > 0 {
        return Some(Err(Error::Transport("合成续页失败".into())));
    }
    if mode == "song_waterfall_error" && category == "songs" && args["offset"] == 10 {
        return Some(Err(Error::Transport("合成单曲续页失败".into())));
    }
    if mode == "expired" {
        return Some(Ok(json!({"code":301})));
    }
    if category == "all" && args["keyword"] == "合成搜索受限" {
        return Some(Ok(json!({
            "code":405,"msg":"部分结果暂不可展示","ignored":"synthetic private field"
        })));
    }
    if mode == "result_error" {
        return Some(Err(Error::Transport("合成搜索服务暂不可用".into())));
    }
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../core/tests/fixtures/search-results.json"
    ))
    .unwrap();
    let mut response = fixtures[category].clone();
    if v3_enabled() && category == "songs" && args["keyword"] == "合成 V3" {
        response["data"]["resources"][0]["baseInfo"]["simpleSongData"]["originCoverType"] =
            json!(1);
        response["data"]["resources"][1]["baseInfo"]["simpleSongData"]["originCoverType"] =
            json!(2);
    }

    if super::search_mv_fixture::enabled() && category == "all" {
        let rows = response["data"]["blocks"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|block| block["blockCode"] == "search_block_song")
            .unwrap()["resources"]
            .as_array_mut()
            .unwrap();
        rows[0]["baseInfo"]["mv"] = json!(601);
        rows[1]["baseInfo"]["mvid"] = json!("601");
        rows[1]["baseInfo"]["privilege"] = json!({"st": -200});
        rows[2]["baseInfo"]["mv"] = json!("https://synthetic.invalid/private");
    }
    if mode == "album_artist_search" && matches!(category, "all" | "albums") {
        let album = if category == "all" {
            &mut response["data"]["blocks"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|block| block["blockCode"] == "search_block_album")
                .unwrap()["resources"][0]["baseInfo"]
        } else {
            &mut response["result"]["albums"][0]
        };
        album["artist"] = json!({"id":100,"name":"合成主艺人"});
        album["artists"] = json!([
            {"id":101,"name":"合成合作艺人"},
            {"name":"合成未标识艺人"},
            {"id":100,"name":"合成主艺人"}
        ]);
    }
    if mode == "voice_queue" && category == "podcasts" {
        let rows = response["data"]["resources"].as_array_mut().unwrap();
        rows[0]["baseInfo"]["dj"] = json!({"userId":501,"nickname":"合成主播"});
        rows[0]["baseInfo"]["playCount"] = json!(12345);
        rows[0]["extInfo"] = json!({"scoreDto":{"score":9.7}});
        rows[0]["uiElement"] = json!({"desc":{"title":"合成播客推荐"}});
        rows[1]["baseInfo"]["dj"] =
            json!({"userId":502,"nickname":"合成不可用主播","accountStatus":30});
        rows[1]["baseInfo"]["playCount"] = json!(0);
    }
    if mode.starts_with("podcast_follow") && category == "podcasts" {
        let rows = response["data"]["resources"].as_array_mut().unwrap();
        rows[0]["baseInfo"]["dj"] = json!({"userId":501,"nickname":"合成主播"});
        rows[0]["baseInfo"]["subed"] = json!(false); // Search preview state is not authoritative.
        rows[1]["baseInfo"]["dj"] = json!({"userId":2,"nickname":"当前账号主播"});
    }
    if mode == "playlist_creator_search" && matches!(category, "all" | "playlists") {
        fn creators(rows: &mut [Value]) {
            for row in rows {
                let entry = if row["type"] == "playlist" {
                    &mut row["baseInfo"]
                } else {
                    row
                };
                match entry["id"].as_u64() {
                    Some(3) => {
                        entry["userId"] = json!(501);
                        entry["creator"] = json!({"userId":501,"nickname":"合成匹配作者"});
                        entry["recommendText"] = json!("  <em>合成作者入口</em>  推荐理由 <片段> ");
                    }
                    Some(4) => {
                        entry["userId"] = json!(501);
                        entry["creator"] = json!({"userId":502,"nickname":"合成不匹配作者"});
                        entry["privacy"] = json!(10);
                        entry["recommendText"] = json!("  <b></b> \t ");
                        entry["description"] = json!("详情描述不是搜索推荐理由");
                    }
                    Some(5) => {
                        entry["id"] = json!(31);
                        entry["name"] = json!("可收藏的合成歌单");
                        entry["userId"] = json!(3);
                        entry["creator"] = json!({"userId":3,"nickname":"合成歌单作者"});
                    }
                    _ => {}
                }
            }
        }
        if category == "all" {
            for block in response["data"]["blocks"].as_array_mut().unwrap() {
                if block["blockCode"] == "search_block_playlist" {
                    creators(block["resources"].as_array_mut().unwrap());
                }
            }
        } else {
            creators(response["result"]["playlists"].as_array_mut().unwrap());
        }
    }
    if mode == "playlist_playability" && matches!(category, "all" | "playlists") {
        let entries = if category == "all" {
            response["data"]["blocks"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|block| block["blockCode"] == "search_block_playlist")
                .unwrap()["resources"]
                .as_array_mut()
                .unwrap()
        } else {
            response["result"]["playlists"].as_array_mut().unwrap()
        };
        for row in entries {
            let entry = if category == "all" {
                &mut row["baseInfo"]
            } else {
                row
            };
            match entry["id"].as_u64() {
                Some(4) => entry["trackCount"] = json!(0),
                Some(5) => {
                    entry.as_object_mut().unwrap().remove("trackCount");
                }
                _ => {}
            }
        }
    }
    if matches!(mode.as_str(), "hot_source" | "selection_source") {
        let keyword = args["keyword"].as_str().or_else(|| args["s"].as_str());
        let expected = match keyword {
            Some("合成热搜来源") => "hotquery",
            Some("合成普通来源") => "suggest",
            Some("合成输入来源") if mode == "selection_source" => "history",
            Some("合成输入来源") => "typing",
            Some("合成推荐词1") => "history",
            Some("合成输入推荐来源推荐") => "rec_query",
            Some("合成默认关键词") => "defaultquery",
            _ => return Some(Err(Error::InvalidInput("wrong synthetic chart keyword"))),
        };
        if (matches!(category, "all" | "songs" | "mvs") && args["channel"] != expected)
            || (!matches!(category, "all" | "songs" | "mvs") && !args["channel"].is_null())
        {
            return Some(Err(Error::InvalidInput(
                "wrong synthetic chart search channel",
            )));
        }
    }
    if mode == "highlight" {
        let track = match category {
            "all" => Some(&mut response["data"]["blocks"][1]["resources"][0]["baseInfo"]),
            "songs" => Some(&mut response["data"]["resources"][0]["baseInfo"]["simpleSongData"]),
            "lyrics" => Some(&mut response["data"]["resources"][0]["song"]),
            _ => None,
        };
        if let Some(track) = track {
            track["name"] = json!("A+B 和 a+b <plain>");
            track["ar"][0]["name"] = json!("歌手 A+B");
            track["al"]["name"] = json!("a+b 专辑");
        }
        if category == "lyrics" {
            response["data"]["resources"][0]["lyrics"] = json!("A+B 原歌词");
        }
    }
    if matches!(mode.as_str(), "correction" | "correction_parts") {
        let valid = match category {
            "all" => args["keyword"] == "合成错词",
            "songs" => args["keyword"] == "合成正词" && args["needCorrect"] == true,
            "playlists" => args["s"] == "合成正词" && args["queryCorrect"] == true,
            _ => false,
        };
        if !valid {
            return Some(Err(Error::InvalidInput("wrong synthetic correction query")));
        }
        // Even a stale upstream reminder after opting out must not re-enable correction.
        if category == "all" {
            let rewrite = if mode == "correction_parts" {
                json!({"queryRewriteDTO":{"keyword":"合成错词","rewriteQuery":"合成正词","searchQcReminder":{"qcReminders":[
                    {"qcReminderPart":"正在显示合成正词。<b>仍查</b> "},
                    {"qcReminderPart":"合成错词","highLight":true},
                    {"qcReminderPart":"。","highLight":false}
                ]}}})
            } else {
                json!({"keyword":"合成错词","rewriteQuery":"合成正词"})
            };
            response["data"]["blocks"].as_array_mut().unwrap().insert(
                0,
                json!({"blockCode":"search_block_correction","resources":[{"baseInfo":rewrite}]}),
            );
            response["data"]["blocks"][2]["resources"][0]["baseInfo"]["name"] =
                if args["needCorrect"] == false {
                    json!("按原词搜索结果")
                } else {
                    json!("合成错词／合成正词")
                };
        } else if category == "songs" {
            response["data"]["resources"][0]["baseInfo"]["simpleSongData"]["name"] =
                json!("合成错词／合成正词");
        }
    }
    if mode == "mixed_results" && category == "all" {
        response["data"]["blocks"][0]["resources"]
            .as_array_mut()
            .unwrap()
            .insert(
                0,
                json!({"type":"song","baseInfo":{"name":"不可展示的合成结果"}}),
            );
    }
    if mode == "preview" && category == "all" {
        for block in response["data"]["blocks"].as_array_mut().unwrap() {
            let rows = block["resources"].as_array().unwrap();
            if rows.is_empty() || rows[0]["type"] == "related_search" {
                continue;
            }
            block["resources"] = json!(
                (0..9)
                    .map(|i| {
                        let mut row = rows[i % rows.len()].clone();
                        if row["type"] == "song" {
                            row["baseInfo"]["al"]["picUrl"] = json!(if i == 0 {
                                "https://p1.music.126.net/cumulus-preview-good"
                            } else {
                                "https://p1.music.126.net/cumulus-preview-bad"
                            });
                        }
                        let info = &mut row["baseInfo"];
                        let field = if info["userId"].is_null() {
                            "id"
                        } else {
                            "userId"
                        };
                        if let Some(id) = info[field]
                            .as_u64()
                            .or_else(|| info[field].as_str().and_then(|s| s.parse().ok()))
                        {
                            let id = id + (i / rows.len()) as u64 * 1000;
                            info[field] = if info[field].is_string() {
                                json!(id.to_string())
                            } else {
                                json!(id)
                            };
                        }
                        row
                    })
                    .collect::<Vec<_>>()
            );
        }
    }
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
    let short_playlists = mode == "short_pages" && category == "playlists";
    let limit = if short_playlists {
        match offset {
            0 => 18,
            18 => 11,
            29 => 4,
            _ => return Some(Err(Error::InvalidInput("unexpected synthetic page offset"))),
        }
    } else {
        limit
    };
    let total = if short_playlists {
        33
    } else if mode.starts_with("song_waterfall") {
        30
    } else if mode == "scroll_duplicates" {
        39
    } else if mode.starts_with("scroll") {
        600
    } else {
        60
    };
    let start = if mode == "scroll_duplicates" && offset == 13 {
        0
    } else {
        offset
    };
    container[key] = json!(if mode == "empty" {
        vec![]
    } else if category == "all" {
        rows.clone()
    } else {
        (start..(start + limit).min(total))
            .map(|i| {
                let mut row = rows[i % rows.len()].clone();
                // Additional grid rows have distinct identities, so the active
                // resource marker cannot accidentally match repeated fixtures.
                if category == "songs" && mode.starts_with("song_waterfall") {
                    let song = &mut row["baseInfo"]["simpleSongData"];
                    song["id"] = json!(i + 101);
                    song["name"] = json!(format!("合成连续单曲 {}", i + 101));
                } else if category != "songs" && category != "lyrics" {
                    let item = if legacy {
                        &mut row
                    } else {
                        &mut row["baseInfo"]
                    };
                    let field = if category == "users" { "userId" } else { "id" };
                    if let Some(id) = item[field]
                        .as_u64()
                        .or_else(|| item[field].as_str().and_then(|s| s.parse::<u64>().ok()))
                    {
                        let next = id + (i / rows.len()) as u64 * 1000;
                        item[field] = if item[field].is_string() {
                            json!(next.to_string())
                        } else {
                            json!(next)
                        };
                    }
                }
                row
            })
            .collect()
    });
    if category == "users"
        && std::env::var_os("CUMULUS_SEARCH_USER_PROFILE_PROOF").is_some()
        && let Some(rows) = container[key].as_array_mut()
    {
        for row in rows.iter_mut().filter(|row| row["userId"] == 503) {
            row["accountStatus"] = json!(30);
        }
    }
    if short_playlists {
        container[key][0]["name"] = json!(format!("合成分页 {offset}"));
        if offset == 18 {
            container[key][1] = json!({"name":"无法展示的歌单"});
        }
    }
    if mode == "mixed_results" && category == "songs" {
        container[key][0] = json!({"baseInfo":{"simpleSongData":{"name":"不可展示的合成结果"}}});
    }
    if mode == "lyrics" && category == "lyrics" {
        for (i, row) in container[key]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            row["song"]["id"] = json!(i + 1 + offset);
            row["lyrics"] = json!(match i {
                1 => "只有两行合成歌词\n短歌词的展开按钮不可用".to_owned(),
                2 => String::new(),
                3 => format!("无空格长行 {}\n第二行\n第三行\n第四行", "SyntheticLyrics🎵".repeat(40)),
                4 => (0..600).map(|n|format!("合成歌词 {n}\n")).collect(),
                _ => "第一行合成歌词\n在夜色中听见你的声音🎵\n每段旋律都是此刻的回响\n第四行 · 展开后的文字\n\n第六行 · 空行之后\n<img src=x onerror=alert(1)>\n第八行 · 合成歌词结束".to_owned(),
            });
            row["matchedLyric"] = json!([]);
        }
    }
    container["hasMore"] = json!(mode != "empty" && category != "all" && offset + limit < total);
    if mode.starts_with("scroll") || mode.starts_with("song_waterfall") {
        for key in [
            "totalCount",
            "playlistCount",
            "artistCount",
            "albumCount",
            "userprofileCount",
        ] {
            if !container[key].is_null() {
                container[key] = json!(total);
            }
        }
    }
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
    if ["cover", "cover_error"].contains(&mode.as_str()) && category == "playlists" {
        for row in response["result"]["playlists"].as_array_mut().unwrap() {
            // The UI probe intercepts this exact synthetic marker before assigning
            // img.src, using local valid/invalid bytes. It never contacts the CDN.
            row["coverImgUrl"] = json!(format!(
                "https://p1.music.126.net/cumulus-ui-test-{mode}.png"
            ));
        }
    }
    Some(Ok(response))
}
