use cumulus_core::{
    Result,
    protocol::{
        api::{Api, ApiFuture},
        pc_experiments::PcExperimentGroups,
    },
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

use tokio::sync::Notify;

pub const SETTING: &str = "/api/user/setting";
pub const SETTING_UPDATE: &str = "/api/user/setting/update";
pub const BATCH: &str = "/api/batch";
pub const MIX: &str = "/api/pc/daily/rcmd/block";
pub const RECENT: &str = "/api/pc/recent/listen/list";
pub const GENERAL: &str = "/api/homepage/block/page";
pub const CUSTOM: &str = "/api/pc/customize/block/page";
pub const BOOKS: &str = "/api/podcast/common/single/block/get";
pub const VOICES: &str = "/api/pc/voicelist/rcmd/list";
pub const AB: &str = "/api/rtrs/abt/front/expinfo/list";
pub const RANK_MAIN: &str = "/api/toplist/detail/v2";
pub const RANK_ALBUM: &str = "/api/feealbum/songsaleboard/daily/type";
pub const RANK_SONG: &str = "/api/single/sales-ranking/get";

#[derive(Default)]
pub struct Fixture {
    pub mode: AtomicU8,
    pub recall: AtomicU8,
    pub now: AtomicU64,
    pub hold_batch: AtomicBool,
    pub batch_started: Notify,
    pub release_batch: Notify,
    pub hold_opt_out: AtomicBool,
    pub opt_out_started: Notify,
    pub release_opt_out: Notify,
    pub calls: Mutex<Vec<(String, Value, bool)>>,
}

fn playlist(id: u64) -> Value {
    json!({
        "uiElement":{"mainTitle":{"title":format!("合成歌单{id}")},"image":{"imageUrl":"https://private.invalid/PRIVATE_ART"}},
        "resources":[{"resourceType":"playlist","resourceId":id}],
        "unsupported":{"url":"https://private.invalid/PRIVATE_LINK"}
    })
}
fn song(id: u64) -> Value {
    json!({
        "resourceId":id,
        "resourceExtInfo":{"songData":{"id":id,"name":format!("合成歌曲{id}"),
            "ar":[{"name":"合成歌手"}],"al":{"picUrl":"https://private.invalid/PRIVATE_ART"},
            "url":"https://private.invalid/PRIVATE_MEDIA"}}
    })
}
fn general(recall: u8) -> Value {
    let mut response = json!({"code":200,"data":{"blocks":[
        {"blockCode":"HOMEPAGE_BLOCK_PLAYLIST_RCMD","creatives":(701..712).map(playlist).collect::<Vec<_>>()},
        {"blockCode":"HOMEPAGE_BLOCK_ALL_LISTEN","creatives":[{"resources":[song(811),song(812),song(811),
            {"resourceId":999,"resourceExtInfo":{"songData":{"id":813,"name":"错配歌曲"}}}
        ]}]},
        {"blockCode":"HOMEPAGE_BLOCK_RED_SIMILAR_SONG","creatives":[{"resources":[song(815)]}]},
        {"blockCode":"HOMPAGE_BLOCK_VIP_RCMD","creatives":[{"resources":[song(816)]}]},
        {"blockCode":"HOMEPAGE_BLOCK_STYLE_RCMD","creatives":[{"resources":[song(817)]}]},
        {"blockCode":"HOMEPAGE_MUSIC_PODCAST_RCMD_BLOCK","creatives":[{"resources":[
            {"resourceId":820,"resourceExtInfo":{"djProgram":{"id":820,"name":"合成单集","mainTrackId":901}}}
        ]}]}
    ]}});
    if recall == 5 {
        response["data"]["blocks"].as_array_mut().unwrap().push(json!({
            "blockCode":"HOMEPAGE_BLOCK_HOT_RESOURCE",
            "creatives":[{"resources":[{"resourceId":1,"resourceExtInfo":{"songData":{"id":2,"name":"错配召回"}}}]}]
        }));
    }
    if recall == 1 || recall == 3 {
        response["data"]["blocks"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "blockCode":"HOMEPAGE_BLOCK_HOT_RESOURCE",
                "creatives":[{"resources":(911..919).map(song).collect::<Vec<_>>()},
                             {"resources":(919..928).map(song).collect::<Vec<_>>()}]
            }));
    }
    response
}
pub const SEED: &str = "/api/homepage/category/daily/song/list";
fn mix() -> Value {
    json!({"code":200,"data":{"items":[
        {"moduleType":"radar","resourceType":"playList","resourceId":741,"title":"合成精选雷达"},
        {"moduleType":"mood","resourceType":"playList","resourceId":742,"title":"合成精选心情"},
        {"moduleType":"rank_board","resourceType":"toplist","resourceId":743,"title":"合成精选榜单"},
        {"moduleType":"daily_podcast","resourceType":"podcast","resourceId":744,"title":"合成精选播客"},
        {"moduleType":"tag_daily_rec","resourceType":"dailySongs","subResourceType":"style_dailySong",
            "title":"合成首页风格","extData":{"categoryId":18,"tagId":21},
            "targetUrl":"https://music.163.com/style?songId=81%2C82&secret=PRIVATE_SEED","songIds":[999]},
        {"moduleType":"unsupported","resourceType":"url","resourceId":745,"title":"未知链接","url":"https://private.invalid/PRIVATE_DEEP"}
    ],"extInfo":{"pcRcmdDemoteItems":[
        {"moduleType":"tag_daily_rec","resourceType":"dailySongs","subResourceType":"style_dailySong",
            "title":"合成首页风格","extData":{"categoryId":18,"tagId":21},"songIds":[81,82]},
        {"moduleType":"unknown","resourceType":"dailySongs","subResourceType":"style_dailySong",
            "title":"合成降级风格","extData":{"categoryId":19,"tagId":22},"songIds":[83,84]},
        {"resourceType":"dailySongs","subResourceType":"other","title":"不可路由降级风格",
            "extData":{"categoryId":19,"tagId":23},"songIds":[85]}
    ]}}})
}
impl Api for Fixture {
    fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
        Ok((self.recall.load(Ordering::SeqCst) != 0).then(|| "fixture-bound".into()))
    }

    fn verified_pc_experiment_groups<'a>(
        &'a self,
        revision: &'a str,
    ) -> ApiFuture<'a, PcExperimentGroups> {
        Box::pin(async move {
            assert_eq!(revision, "fixture-bound");
            let recall = self.recall.load(Ordering::SeqCst);
            PcExperimentGroups::refreshed(&json!({"data":[
                {"expName":"PH-PC-recallAndNewPlaylist","expGroupName":if recall == 3 { "c" } else { "t1" }},
                {"expName":"PH-PC-RecallHomeContent","expGroupName":if recall == 4 { "c" } else { "t1" }},
                {"expName":"PH-PC-newEcpm","expGroupName":"c"}
            ]}))
        })
    }

    fn observed_server_time_ms(&self) -> Option<u64> {
        let now = self.now.load(Ordering::SeqCst);
        (now > 0).then_some(now)
    }

    fn request<'a>(&'a self, path: &'a str, args: Value, sensitive: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), args.clone(), sensitive));
            if path == BATCH && self.hold_batch.load(Ordering::SeqCst) {
                self.batch_started.notify_one();
                self.release_batch.notified().await;
            }
            let mode = self.mode.load(Ordering::SeqCst);
            Ok(match path {
                SETTING => {
                    json!({"code":200,"setting":{"personalServiceSetting":if mode == 2 { 2 } else { 0 }}})
                }
                SETTING_UPDATE if self.hold_opt_out.load(Ordering::SeqCst) => {
                    assert_eq!(args, json!({"personalServiceSetting":2}));
                    self.opt_out_started.notify_one();
                    self.release_opt_out.notified().await;
                    self.mode.store(2, Ordering::SeqCst);
                    json!({"code":200})
                }
                BATCH if mode == 1 => json!({"code":500}),
                BATCH => json!({
                    "code":200,
                    (GENERAL):general(self.recall.load(Ordering::SeqCst)),
                    (CUSTOM):{"code":200,"data":{"blocks":[{"creatives":[playlist(721)]}]}},
                    (BOOKS):{"code":200,"data":{"blockVOS":[{"creatives":[
                        {"creativeType":"voiceList","creativeId":837,"uiElement":{"mainTitle":{"title":"合成有声书"}}}
                    ]}]}},
                    (VOICES):{"code":200,"data":{"recommendVoiceVOS":[
                        {"djProgram":{"id":831,"name":"合成播客单集 A"}},
                        {"djProgram":{"id":832,"name":"合成播客单集 B"}},
                        {"djProgram":{"id":833,"name":"合成播客单集 C"}},
                        {"djProgram":{"id":834,"name":"不足一组"}}
                    ]}}
                }),
                MIX if mode == 3 => json!({"code":500}),
                MIX => mix(),
                SEED if args["source"] == "homepage" => {
                    let (song_id, title, category_id, tag_id) = match &args {
                        args if *args
                            == json!({"source":"homepage","categoryId":"18","tagId":"21","songId":"81,82"}) =>
                        {
                            (81, "合成首页种子歌曲", 18, 21)
                        }
                        args if *args
                            == json!({"source":"homepage","categoryId":"19","tagId":"22","songId":"83,84"}) =>
                        {
                            (83, "合成降级种子歌曲", 19, 22)
                        }
                        _ => {
                            return Err(cumulus_core::Error::Response(
                                "unexpected discovery style seed",
                            ));
                        }
                    };
                    json!({"code":200,"data":{"dailySongs":[{"id":song_id,"name":title,"dt":30000}],
                        "tags":{"categoryId":category_id,"categoryName":"合成首页分类",
                            "tagVOList":[{"tagId":tag_id,"tagName":"合成首页标签"}]}}})
                }
                RECENT if mode == 3 => json!({"code":500}),
                RECENT => json!({"code":200,"data":{"resources":[
                    {"resourceType":"album","resourceId":851,"title":"合成最近专辑"},
                    {"resourceType":"dailySongsTag","resourceId":"PRIVATE_STYLE_ROW_ID","title":"合成最近风格","tag":"风格日推",
                     "targetUrl":"https://private.invalid/PRIVATE_STYLE_TARGET"},
                    {"resourceType":"dailySongsTag","resourceId":"PRIVATE_STYLE_ROW_ID","title":"重复行"},
                    {"resourceType":"dailySongsTag","resourceId":"https://private.invalid/PRIVATE_STYLE_BAD","title":"错误身份"}
                ]}}),
                AB if args["expNames"] == "PH-PC-tagT,PH-PC-Home-Ranklist" => {
                    let group = if mode == 4 { "t2" } else { "t1" };
                    let data = if mode == 5 {
                        json!([])
                    } else if mode == 6 {
                        json!([{"expName":"PH-PC-tagT","expGroupName":"c"},
                            {"expName":"PH-PC-tagT","expGroupName":"t1"}])
                    } else {
                        json!([{"expName":"PH-PC-tagT","expGroupName":group},
                            {"expName":"PH-PC-Home-Ranklist","expGroupName":"t1"}])
                    };
                    json!({"code":200,"data":data})
                }
                RANK_MAIN => json!({"code":200,"data":[
                    {"name":"全球榜","categoryCode":"WORLD","displayType":"TOP_3","list":[
                        {"id":788,"name":"不应出现的全球榜","nameShowStyle":"singleRow","targetType":"PLAYLIST"}
                    ]},
                    {"name":"官方榜","categoryCode":"OFFICIAL","displayType":"ONLY_COVER","list":[
                        {"id":701,"name":"合成榜单","nameShowStyle":"singleRow","targetType":"PLAYLIST","canPlay":true,
                         "updateFrequency":"每天更新","firstCoverUrl":"https://private.invalid/PRIVATE_RANK_ART",
                         "trackRankList":[{"trackId":811,"rank":1,"lastRank":2},{"trackId":812,"rank":2,"lastRank":2},
                                          {"trackId":813,"rank":3,"lastRank":0},{"trackId":814,"rank":4,"lastRank":0}],
                         "tracks":[{"first":"合成榜首","second":"合成歌手"},{"first":"合成亚军","second":"合成歌手"},
                                   {"first":"合成第三","second":"合成歌手"},{"first":"不可显示的第四","second":"合成歌手"}]}
                    ]}
                ]}),
                RANK_ALBUM => {
                    json!({"code":200,"products":[{"albumId":851,"albumName":"合成畅销专辑","artistName":"合成歌手"}]})
                }
                RANK_SONG => {
                    json!({"code":200,"data":{"itemList":[{"songId":811,"songName":"合成畅销歌","artistName":"合成歌手"}]}})
                }
                "/api/w/nuser/account/get" | "/api/nuser/account/get" | "/api/login/status" => {
                    json!({"code":200,"account":{"id":42},"profile":{"userId":42,"nickname":"发现测试账号"}})
                }
                "/api/v3/song/detail" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    json!({"code":200,"songs":ids.iter().map(|row| json!({"id":row["id"],"name":"普通歌","dt":120_000})).collect::<Vec<_>>(),"privileges":[]})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(args["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":[{"id":ids[0],"code":200,"url":"https://m1.music.126.net/cumulus-relay-fixture.wav","md5":"fixture","expi":300,"level":"standard"}]})
                }
                "/api/user/playlist" => json!({"code":200,"playlist":[],"more":false}),
                "/api/v6/playlist/detail" | "/api/v3/playlist/detail" => {
                    json!({"code":200,"playlist":{"id":701,"name":"合成歌单701","trackIds":[],"tracks":[],"creator":{"userId":42,"nickname":"发现测试账号"}},"privileges":[]})
                }
                "/api/homepage/category/daily/song/list" => json!({"code":200,"data":{
                    "dailySongs":[{"id":841,"name":"合成风格歌曲","ar":[{"id":881,"name":"合成歌手"}],
                                   "al":{"id":891,"name":"合成专辑"},"dt":180000}]
                }}),
                "/api/homepage/daily/song/config/get" => {
                    json!({"code":200,"data":{"categorys":[]}})
                }
                "/api/v3/discovery/recommend/songs" => json!({"code":200,"data":{"dailySongs":[]}}),
                _ => json!({"code":200,"data":[]}),
            })
        })
    }
}
