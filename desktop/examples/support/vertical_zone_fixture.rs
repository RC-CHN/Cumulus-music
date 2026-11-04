use cumulus_core::{Error, Result, protocol::pc_experiments::PcExperimentGroups};
use serde_json::{Value, json};
use std::sync::Mutex;

static CALLS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
const HOT_WORD: &str = "合成专区热点";

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_VERTICAL_ZONE_PROOF").is_some()
}

pub fn hotspot() -> bool {
    std::env::var("CUMULUS_VERTICAL_ZONE_PROOF").as_deref() == Ok("hotspot")
}

pub fn groups(revision: &str) -> Result<PcExperimentGroups> {
    if revision != "synthetic-vertical-zone" || !enabled() {
        return Err(Error::CredentialChanged);
    }
    PcExperimentGroups::refreshed(&json!({"data":[
        {"expName":"PH-PC-zhuanqu","expGroupName":"t1"},
        {"expName":"PH-PC-zhuanquTab","expGroupName":"t1"}
    ]}))
}

pub fn calls() -> Vec<Value> {
    CALLS.lock().unwrap().clone()
}

fn record(path: &str, args: &Value) {
    CALLS.lock().unwrap().push(json!({"path":path,"args":args}));
}

pub fn observe(path: &str, args: &Value) {
    if enabled()
        && matches!(
            path,
            "/api/album/v3/detail"
                | "/api/album/v4/detail"
                | "/api/v6/playlist/detail"
                | "/api/song/enhance/player/url/v1"
                | "/api/user/setting/update"
        )
    {
        record(path, args);
    }
}

fn content(key: &str, title: &str, blocks: Vec<Value>) -> Value {
    json!({"tabKey":key,"tabKeyName":title,"blocks":blocks})
}

fn blocks() -> Vec<Value> {
    let mut albums = vec![json!({"resourceType":"album","resourceId":7,"name":"专区合成专辑"})];
    albums.extend(
        (204..=212).map(
            |id| json!({"resourceType":"playlist","id":id,"name":format!("专区合成歌单 {id}")}),
        ),
    );
    vec![
        json!({"componentName":"RecommendTabCarousel","children":[
            {"targetType":10,"targetId":7,"title":"专区合成横幅"},
            {"targetType":99,"targetId":77,"title":"禁止出现的远程动作"}
        ]}),
        json!({"componentName":"RecommendHorizontal","title":"专区推荐栏","hideAfterClosePersonalized":true,"children":albums}),
        json!({"componentName":"MusiclibraryRank","title":"专区榜单","children":[{
            "targetType":"PLAYLIST","nameShowStyle":"doubleRow","id":205,"name":"专区合成榜单","canPlay":true,
            "trackRankList":[{"songName":"专区榜单歌曲","artistName":"专区榜单歌手"}],
            "tracks":[{"first":"专区榜单歌曲","second":"专区榜单歌手"}]
        }]}),
        json!({"componentName":"RecommendGrid","title":"专区常规网格","children":[
            {"resourceType":"playlist","id":206,"name":"专区常规歌单"},
            {"resourceType":"playlist","id":207,"name":"仅个性化网格卡片","hideAfterClosePersonalized":true}
        ]}),
    ]
}

pub fn response(path: &str, args: &Value) -> Option<Result<Value>> {
    if !enabled() {
        return None;
    }
    observe(path, args);
    let result = match path {
        "/api/rtrs/abt/front/expinfo/list"
            if hotspot()
                && *args
                    == json!({"expNames":"PH-PC-searchHot,PH-PC-searchMore2,PH-PC-searchPageEfficiency"}) =>
        {
            json!({"code":200,"data":[{"expName":"PH-PC-searchHot","expGroupName":"t1"}]})
        }
        "/api/search/pc/result/tab" if hotspot() && *args == json!({"keyword":HOT_WORD}) => {
            json!({"code":200,"data":{"algTabs":[{"title":"综合"},{"title":"单曲"}]}})
        }
        "/api/search/pc/complex/page/v3" if hotspot() && args["keyword"] == HOT_WORD => {
            assert_eq!(
                *args,
                json!({"keyword":HOT_WORD,"scene":"normal","needCorrect":true,
                "channel":"typing","bizQueryInfo":""})
            );
            record(path, args);
            json!({"code":200,"data":{"blocks":[
                {"blockCode":"search_block_hot_result_hot_explain","resources":[{
                    "baseInfo":{"hotExplainModuleData":{"title":HOT_WORD,"text":"只用于回归的专区说明。",
                        "textAction":"orpheus://rnpage/private-id?component=cube-renderer-rn&page=ROCK&tabKey=%E7%83%AD%E9%97%A8&from=private-zone-source",
                        "canClick":true,"urlText":"查看专区"}}
                }]},
                {"blockCode":"search_block_song","resources":[
                    {"baseInfo":{"simpleSongData":{"id":81,"name":"合成热点搜索歌曲"}}}
                ]}
            ]}})
        }
        "/api/secondary/tab/list" if *args == json!({}) => {
            let rows = [
                ("feature", "精选", "STATIC"),
                ("rock", "摇滚", "ROCK"),
                ("folk", "民谣", "FOLK"),
                ("electronic", "电子", "ELECTRONIC"),
                ("hiphop", "说唱", "HIPHOP"),
                ("classical", "古典", "CLASSICAL"),
                ("jazz", "爵士", "JAZZ"),
            ]
            .map(|(code,title,page)| json!({"code":code,"title":title,"url":format!("orpheus://rnpage?page={page}&private=synthetic")}));
            json!({"code":200,"data":{"hints":[{"position":{"code":"music_top_tab_full"},
                "data":{"extra":{"nowChannelItems":rows}}}]}})
        }
        "/api/user/setting" if *args == json!({}) => {
            let disabled = std::env::var("CUMULUS_VERTICAL_ZONE_PROOF").as_deref() == Ok("privacy");
            json!({"code":200,"setting":{"personalServiceSetting":if disabled {2} else {0}}})
        }
        "/api/zone/tab/get" if *args == json!({"pageCode":"ROCK","tabKey":"default"}) => {
            json!({"code":200,"data":{"currentTabContent":content("default","全部",blocks()),
                "tabMetas":[{"tabKey":"default","tabKeyName":"全部"},{"tabKey":"1","tabKeyName":"热门"}]}})
        }
        "/api/zone/tab/get" if *args == json!({"pageCode":"ROCK","tabKey":"1"}) => {
            let module = json!({"componentName":"RecommendGrid","title":"热门精选内容","children":[
                {"resourceType":"playlist","id":208,"name":"专区热门歌单"}
            ]});
            json!({"code":200,"data":{"currentTabContent":content("1","热门",vec![module]),
                "tabMetas":[{"tabKey":"default","tabKeyName":"全部"},{"tabKey":"1","tabKeyName":"热门"}]}})
        }
        "/api/zone/tab/get" if *args == json!({"pageCode":"JAZZ","tabKey":"default"}) => {
            json!({"code":503})
        }
        "/api/zone/get" if *args == json!({"pageCode":"JAZZ"}) => {
            let module = json!({"componentName":"RecommendGrid","title":"爵士传统布局","children":[
                {"resourceType":"playlist","id":209,"name":"专区爵士歌单"}
            ]});
            json!({"code":200,"data":[content("default","全部",vec![module])]})
        }
        "/api/v6/playlist/detail" if args["id"] == "204" || args["id"] == 204 => {
            json!({"code":200,"playlist":{"id":204,"name":"专区打开后的合成歌单",
                "creator":{"userId":3,"nickname":"合成作者"},"trackCount":2,
                "trackIds":[{"id":81,"v":0},{"id":84,"v":0}]}})
        }
        "/api/batch" if args["/api/homepage/block/page"].is_string() => {
            json!({"code":200,"/api/homepage/block/page":{"code":200,"data":{"blocks":[]}}})
        }
        "/api/pc/recent/listen/list" => json!({"code":200,"data":{"resources":[]}}),
        "/api/pc/daily/rcmd/block" => json!({"code":200,"data":{"items":[]}}),
        "/api/secondary/tab/list"
        | "/api/zone/tab/get"
        | "/api/zone/get"
        | "/api/user/setting/update" => {
            return Some(Err(Error::InvalidInput(
                "unexpected sealed vertical zone request",
            )));
        }
        _ => return None,
    };
    if matches!(
        path,
        "/api/secondary/tab/list" | "/api/user/setting" | "/api/zone/tab/get" | "/api/zone/get"
    ) {
        record(path, args);
    }
    Some(Ok(result))
}
