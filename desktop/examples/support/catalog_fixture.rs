//! Synthetic endpoints only. Never connects to a real account or music service.
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
#[path = "rankings_fixture.rs"]
mod rankings;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[derive(Default)]
pub struct Library {
    artist_collected: AtomicBool,
    album_collected: AtomicBool,
    mode: Mutex<String>,
    pub comment_reads: AtomicUsize,
    pub artist_song_reads: AtomicUsize,
    pub artist_album_reads: AtomicUsize,
    calls: Mutex<Vec<Value>>,
    album_cards: AtomicBool,
    album_duration_sort: AtomicBool,
    queue_song_liked: AtomicBool,
    artist_cards: AtomicBool,
}
impl Library {
    pub fn album_cards(&self) {
        self.album_cards.store(true, Ordering::SeqCst);
    }
    pub fn album_duration_sort(&self) {
        self.album_duration_sort.store(true, Ordering::SeqCst);
    }
    pub fn artist_cards(&self) {
        self.artist_cards.store(true, Ordering::SeqCst);
    }
    pub fn mode(&self, mode: &str) {
        *self.mode.lock().unwrap() = mode.into();
    }
    pub fn report(&self) -> Value {
        json!({"albumCollected":self.album_collected.load(Ordering::SeqCst),"artistCollected":self.artist_collected.load(Ordering::SeqCst),"calls":self.calls.lock().unwrap().clone()})
    }
}
fn id(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_str()?.parse().ok()).unwrap_or(0)
}
fn song(n: u64) -> Value {
    json!({"id":n,"name":format!("合成歌曲 {n}"),"ar":[{"id":12,"name":"测试歌手"}],"al":{"id":7,"name":"测试专辑"},"dt":235000,"fee":n%3})
}
fn queue_credit_song(id: u64) -> Value {
    let mut item = song(id);
    match id {
        14 => {
            item["ar"] = json!([
                {"id":12,"name":"测试歌手"},
                {"id":13,"name":"相似歌手 13"},
                {"name":"仅有署名"}
            ]);
        }
        901 => item["ar"] = json!([{"name":"无编号歌手"}]),
        _ => {}
    }
    item
}
fn duration_album_songs() -> Vec<Value> {
    [180_000, 90_000, 180_000, 120_000]
        .into_iter()
        .enumerate()
        .map(|(index, duration)| {
            let id = 11 + index as u64;
            let mut item = queue_credit_song(id);
            item["dt"] = json!(duration);
            item["name"] = json!(if index == 3 {
                "另一首歌".into()
            } else {
                format!("合成候选 {id}")
            });
            item
        })
        .collect()
}
fn artist(n: u64) -> Value {
    json!({"id":n,"name":if n==12 {"测试歌手".into()}else{format!("相似歌手 {n}")},"musicSize":55,"albumSize":62,"mvSize":2,"alias":["A synthetic artist"],"briefDesc":"用于检查歌手详情的合成简介。"})
}
fn action_artist(n: u64) -> Value {
    let mut value = artist(n);
    if n == 13 {
        value["name"] =
            json!("长名称歌手 · AnArtistWithAnUnbrokenDisplayNameForLayoutVerification");
    }
    if n == 15 {
        value["musicSize"] = json!(0);
    }
    value["picUrl"] = json!(format!(
        "https://p1.music.126.net/cumulus-synthetic-{}.png",
        if n == 14 { "missing" } else { "cover" }
    ));
    value
}
fn album(n: u64) -> Value {
    let mut value = json!({"id":n,"name":if n==7 {"测试专辑".into()}else{format!("合成专辑 {n}")},"size":11,"publishTime":1700000000000_i64,"artists":[{"id":12,"name":"测试歌手"}],"description":"这是合成的专辑介绍。\n用于检查多行文本、深色配色和长内容布局。"});
    if n == 8 {
        value["name"] = json!("漫长的午后与未完的旋律 · A quiet afternoon / Extended edition");
        value["transNames"] = json!(["UnbrokenSyntheticAlbumTranslation".repeat(6)]);
        value["artists"][0]["name"] =
            json!("SyntheticArtistWithAnUnbrokenLongDisplayName".repeat(3));
    }
    if n == 9 {
        value["description"] = json!("");
        value["artists"] = json!([]);
        value["artist"] = json!({"id":12,"name":"测试歌手"});
    }
    value
}
fn action_album(n: u64) -> Value {
    let mut value = album(n);
    value["picUrl"] = json!(format!(
        "https://p1.music.126.net/cumulus-synthetic-{}.png",
        if n == 9 { "missing" } else { "cover" }
    ));
    if n == 7 {
        value["artists"] = json!([{"id":12,"name":"测试歌手"},{"id":13,"name":"主要署名"}]);
        value["artist"] = json!({"id":13,"name":"主要署名"});
    }
    if n == 9 {
        value["artists"] = json!([]);
        value["artist"] = Value::Null;
        value["publishTime"] = json!(0);
        value["mark"] = json!(1 << 20);
    }
    value["size"] = json!(if n == 9 { 0 } else { 3 });
    value
}
fn comment(n: u64) -> Value {
    json!({"commentId":n,"content":format!("第 {n} 条合成评论：这首歌适合慢慢听。\n用于验证长页面滚动、文字换行和底部翻页。"),"time":1700000000000_u64-n,"timeStr":"合成评论时间","liked":false,"likedCount":1234,"user":{"userId":3,"nickname":if n==1 {"LongUnbrokenSyntheticListenerName".repeat(5)}else{format!("合成听友 {n}")}},"beReplied":[],"parentCommentId":"0"})
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let mode = self.mode.lock().unwrap().clone();
            let album_cards = self.album_cards.load(Ordering::SeqCst);
            let artist_cards = self.artist_cards.load(Ordering::SeqCst);
            let album_duration_sort = self.album_duration_sort.load(Ordering::SeqCst);
            self.calls
                .lock()
                .unwrap()
                .push(json!({"path":path,"args":args,"mode":mode}));
            if let Some(response) = rankings::response(path, &args, &mode) {
                return response;
            }
            if mode == "catalog-hold"
                && matches!(path, "/api/artist/v3/detail" | "/api/artist/detail/dynamic")
            {
                let released = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while self.mode.lock().unwrap().as_str() == "catalog-hold" {
                        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    }
                })
                .await;
                if released.is_err() {
                    return Err(Error::Transport("synthetic header hold timed out".into()));
                }
            }
            if mode == "album-offline"
                && matches!(path, "/api/album/v3/detail" | "/api/album/v4/detail")
            {
                return Err(Error::Transport("synthetic album metadata offline".into()));
            }
            if mode == "artist-state-offline" && path == "/api/artist/detail/dynamic" {
                return Err(Error::Transport("synthetic artist state offline".into()));
            }
            if path.starts_with("/api/artist/albums/") {
                self.artist_album_reads.fetch_add(1, Ordering::SeqCst);
            }
            if mode == "artist-albums-offline" && path.starts_with("/api/artist/albums/") {
                return Err(Error::Transport("synthetic artist albums offline".into()));
            }
            let offset = args["offset"].as_u64().unwrap_or(0);
            let limit = args["limit"].as_u64().unwrap_or(20);
            Ok(match path {
                "/api/rtrs/abt/front/expinfo/list" => json!({"code":200,"data":[]}),
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"profile":{"userId":2,"nickname":"目录界面验证"}})
                }
                "/api/w/v1/user/detail/501" if args["userId"] == "501" => {
                    json!({"code":200,"profile":{"userId":501,"nickname":"合成歌手个人页"}})
                }
                "/api/user/playlist" => json!({"code":200,"more":false,"playlist":[]}),
                "/api/song/like/get" => {
                    let ids = if self.queue_song_liked.load(Ordering::SeqCst) {
                        vec![3, 7, 14]
                    } else {
                        vec![3, 7]
                    };
                    json!({"code":200,"ids":ids})
                }
                "/api/song/like" if album_duration_sort => {
                    let liked = args["like"]
                        .as_bool()
                        .ok_or(Error::InvalidInput("missing synthetic like operation"))?;
                    if args != json!({"trackId":"14","userid":"2","like":liked}) {
                        return Err(Error::InvalidInput("unexpected queue like target"));
                    }
                    self.queue_song_liked.store(liked, Ordering::SeqCst);
                    if liked {
                        return Err(Error::Transport("synthetic like response lost".into()));
                    }
                    json!({"code":200})
                }
                "/api/resource/commentInfo/list" if album_duration_sort => {
                    let ids: Vec<String> =
                        serde_json::from_str(args["resourceIds"].as_str().unwrap_or_default())?;
                    if args["resourceType"] != 4 || ids.len() != 1 {
                        return Err(Error::InvalidInput(
                            "expected a single queue song comment read",
                        ));
                    }
                    let id = &ids[0];
                    if id == "900" {
                        return Err(Error::Transport(
                            "synthetic comment count unavailable".into(),
                        ));
                    }
                    json!({"code":200,"data":[{"threadId":format!("R_SO_4_{id}"),"commentCount":36}]})
                }
                "/api/v3/discovery/recommend/songs" => {
                    json!({"code":200,"data":{"dailySongs":(1..=40).map(song).collect::<Vec<_>>()}})
                }
                "/api/v3/song/detail" => {
                    let rows: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    let project = if album_duration_sort {
                        queue_credit_song
                    } else {
                        song
                    };
                    json!({"code":200,"songs":rows.iter().map(|v|project(id(&v["id"]))).collect::<Vec<_>>()})
                }
                "/api/artist/v3/detail" => {
                    let n = id(&args["id"]);
                    let mut value = if artist_cards {
                        action_artist(n)
                    } else {
                        artist(n)
                    };
                    if n == 12 {
                        value["accountId"] = json!(501);
                    }
                    if mode == "artist-long" {
                        value["name"] =
                            json!("长名称测试歌手 · An artist with a very long display name");
                        value["alias"] = json!(["LongSyntheticArtistAliasWithoutBreaks".repeat(5)]);
                    }
                    if mode == "artist-zero" {
                        value["musicSize"] = json!(0);
                        value["albumSize"] = json!(0);
                        value["mvSize"] = json!(0);
                    }
                    json!({"code":200,"artist":value,"hotSongs":if mode=="artist-zero" || artist_cards && n==15 {vec![]}else{(1..=50).map(song).collect::<Vec<_>>()},"more":false})
                }
                "/api/song/enhance/privilege" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(args["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":ids.iter().map(|v|json!({"id":v,"pl":320000})).collect::<Vec<_>>()})
                }
                "/api/artist/detail/dynamic" => {
                    json!({"code":200,"followed":self.artist_collected.load(Ordering::SeqCst),"concert":{"onlineCount":if mode=="artist-zero"{0}else{1}}})
                }
                "/api/v2/artist/songs" => {
                    self.artist_song_reads.fetch_add(1, Ordering::SeqCst);
                    json!({"code":200,"total":55,"more":offset+limit<55,"songs":((offset+1)..=(offset+limit).min(55)).map(song).collect::<Vec<_>>()})
                }
                "/api/artist/introduction" => {
                    json!({"code":200,"briefDesc":"合成概述","introduction":[{"ti":"音乐经历","txt":"这段内容是合成测试文本。\n没有真实账号信息。"}]})
                }
                "/api/discovery/simiArtist" => {
                    json!({"code":200,"artists":if artist_cards{(13..=55).map(action_artist).collect::<Vec<_>>()}else{(13..=32).map(artist).collect::<Vec<_>>()},"more":false})
                }
                "/api/mlog/artist/video" => {
                    json!({"code":200,"data":{"records":[{"id":"21","resource":{"mlogBaseData":{"type":3,"text":"合成 MV"}}},{"id":"opaque","resource":{"mlogBaseData":{"type":2,"text":"普通视频类型"}}}],"page":{"cursor":"20","size":20,"more":false}}})
                }
                "/api/concert/list/byartist" => {
                    json!({"code":200,"simpleConcerts":[{"id":1,"title":"合成演出","time":[1800000000000_i64,1800086400000_i64],"simpleAddress":{"address":"合成场馆"},"price":"88起","url":"https://invalid.example/private"}],"total":1})
                }
                "/api/album/v3/detail" => {
                    let n = id(&args["id"]);
                    if album_duration_sort && n == 8 {
                        let mut info = album(n);
                        info["size"] = json!(4);
                        return Ok(json!({"code":200,"album":info,"songs":duration_album_songs()}));
                    }
                    json!({"code":200,"album":if album_cards{action_album(n)}else{album(n)},"songs":if !album_cards{(1..=11).map(song).collect::<Vec<_>>()}else if n==9{vec![]}else{((n-7)*3+1..=(n-7)*3+3).map(song).collect::<Vec<_>>()}})
                }
                "/api/album/detail/dynamic" => {
                    json!({"code":200,"isSub":self.album_collected.load(Ordering::SeqCst)})
                }
                "/api/album/privilege" => {
                    let n = id(&args["id"]);
                    let ids = if album_duration_sort && n == 8 {
                        (11..=14).collect::<Vec<_>>()
                    } else if !album_cards {
                        (1..=11).collect::<Vec<_>>()
                    } else if n == 9 {
                        vec![]
                    } else {
                        ((n - 7) * 3 + 1..=(n - 7) * 3 + 3).collect::<Vec<_>>()
                    };
                    json!({"code":200,"data":ids.into_iter().map(|id|json!({"id":id,"pl":320000})).collect::<Vec<_>>()})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().unwrap())?;
                    json!({"code":200,"data":[{"id":ids[0],"url":"https://m1.music.126.net/cumulus-relay-fixture.wav","code":200,"md5":"fixture","level":"standard","expi":120}]})
                }
                "/api/song/lyric/v1" => json!({"code":200,"lrc":{"lyric":"[00:00.00]合成歌词"}}),
                _ if path.starts_with("/api/v1/resource/comments/R_AL_3_") => {
                    self.comment_reads.fetch_add(1, Ordering::SeqCst);
                    if mode == "comments-offline" {
                        return Err(Error::Transport("synthetic album comments offline".into()));
                    }

                    json!({"code":200,"comments":((offset+1)..=(offset+limit).min(25)).map(comment).collect::<Vec<_>>(),"hotComments":[],"topComments":[],"total":25,"more":offset+limit<25})
                }
                "/api/resource-exposure/config" => {
                    json!({"code":200,"data":{"resourceType":"other"}})
                }
                "/api/artist/sublist" => {
                    json!({"code":200,"more":false,"count":usize::from(self.artist_collected.load(Ordering::SeqCst)),"data":if self.artist_collected.load(Ordering::SeqCst){vec![if artist_cards{action_artist(13)}else{artist(12)}]}else{vec![]}})
                }
                "/api/album/sublist" => {
                    json!({"code":200,"more":false,"hasMore":false,"count":usize::from(self.album_collected.load(Ordering::SeqCst)),"data":if self.album_collected.load(Ordering::SeqCst){vec![if album_cards{action_album(7)}else{album(7)}]}else{vec![]}})
                }
                "/api/artist/sub" | "/api/artist/unsub" => {
                    if mode == "artist-sub-reject" {
                        return Ok(json!({"code":403}));
                    }
                    self.artist_collected
                        .store(path.ends_with("/sub"), Ordering::SeqCst);
                    if mode == "artist-sub-lost" {
                        return Err(Error::Transport(
                            "synthetic lost artist write response".into(),
                        ));
                    }
                    json!({"code":200})
                }
                "/api/user/follow/users/mixed/get/v2" if artist_cards => {
                    let mut records = vec![];
                    if self.artist_collected.load(Ordering::SeqCst) {
                        records.push(json!({"artistInfo":action_artist(13),"userProfile":{"userId":113,"nickname":"关联用户","signature":"合成签名 · LongSignatureWithoutBreaksForOverflowVerification","avatarUrl":"https://p1.music.126.net/cumulus-synthetic-cover.png"}}));
                    }
                    records.push(json!({"artistInfo":action_artist(14),"userProfile":Value::Null}));
                    records.push(json!({"userProfile":{"userId":114,"nickname":"已注销的合成用户","accountStatus":30}}));
                    json!({"code":200,"data":{"records":records,"page":{"cursor":"0","size":19,"more":false}}})
                }
                "/api/album/sub" | "/api/album/unsub" => {
                    if mode == "album-sub-reject" {
                        return Ok(json!({"code":403}));
                    }
                    self.album_collected
                        .store(path.ends_with("/sub"), Ordering::SeqCst);
                    if mode == "album-sub-lost" {
                        return Err(Error::Transport(
                            "synthetic lost album write response".into(),
                        ));
                    }
                    json!({"code":200})
                }
                _ if path.starts_with("/api/artist/albums/") => {
                    if album_cards {
                        json!({"code":200,"artist":{"albumSize":3},"more":false,"hotAlbums":(7..=9).map(action_album).collect::<Vec<_>>()})
                    } else {
                        json!({"code":200,"artist":{"albumSize":62},"more":offset+limit<62,"hotAlbums":((offset+7)..(offset+limit+7).min(69)).map(album).collect::<Vec<_>>()})
                    }
                }
                _ => return Err(Error::Response("unsupported synthetic catalog endpoint")),
            })
        })
    }
}
