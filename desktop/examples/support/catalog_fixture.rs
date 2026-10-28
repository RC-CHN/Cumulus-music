//! Synthetic endpoints only. Never connects to a real account or music service.
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Default)]
pub struct Library {
    artist_collected: AtomicBool,
    album_collected: AtomicBool,
}
fn id(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_str()?.parse().ok()).unwrap_or(0)
}
fn song(n: u64) -> Value {
    json!({"id":n,"name":format!("合成歌曲 {n}"),"ar":[{"id":12,"name":"测试歌手"}],"al":{"id":7,"name":"测试专辑"},"dt":235000,"fee":n%3})
}
fn artist(n: u64) -> Value {
    json!({"id":n,"name":if n==12 {"测试歌手".into()}else{format!("相似歌手 {n}")},"musicSize":55,"albumSize":62,"alias":["A synthetic artist"],"briefDesc":"用于检查歌手详情的合成简介。"})
}
fn album(n: u64) -> Value {
    json!({"id":n,"name":if n==7 {"测试专辑".into()}else{format!("合成专辑 {n}")},"size":11,"publishTime":1700000000000_i64,"artists":[{"id":12,"name":"测试歌手"}],"description":"这是合成的专辑介绍。\n用于检查多行文本、深色配色和长内容布局。"})
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let offset = args["offset"].as_u64().unwrap_or(0);
            let limit = args["limit"].as_u64().unwrap_or(20);
            Ok(match path {
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"profile":{"userId":2,"nickname":"目录界面验证"}})
                }
                "/api/user/playlist" => json!({"code":200,"more":false,"playlist":[]}),
                "/api/song/like/get" => json!({"code":200,"ids":[3,7]}),
                "/api/v3/discovery/recommend/songs" => {
                    json!({"code":200,"data":{"dailySongs":(1..=40).map(song).collect::<Vec<_>>()}})
                }
                "/api/v3/song/detail" => {
                    let rows: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    json!({"code":200,"songs":rows.iter().map(|v|song(id(&v["id"]))).collect::<Vec<_>>()})
                }
                "/api/artist/v3/detail" => {
                    json!({"code":200,"artist":artist(id(&args["id"])),"hotSongs":(1..=50).map(song).collect::<Vec<_>>(),"more":false})
                }
                "/api/song/enhance/privilege" => {
                    let ids: Vec<Value> =
                        serde_json::from_str(args["ids"].as_str().unwrap()).unwrap();
                    json!({"code":200,"data":ids.iter().map(|v|json!({"id":v,"pl":320000})).collect::<Vec<_>>()})
                }
                "/api/artist/detail/dynamic" => {
                    json!({"code":200,"followed":self.artist_collected.load(Ordering::SeqCst),"concert":{"onlineCount":1}})
                }
                "/api/v2/artist/songs" => {
                    json!({"code":200,"total":55,"more":offset+limit<55,"songs":((offset+1)..=(offset+limit).min(55)).map(song).collect::<Vec<_>>()})
                }
                "/api/artist/introduction" => {
                    json!({"code":200,"briefDesc":"合成概述","introduction":[{"ti":"音乐经历","txt":"这段内容是合成测试文本。\n没有真实账号信息。"}]})
                }
                "/api/discovery/simiArtist" => {
                    json!({"code":200,"artists":(13..=32).map(artist).collect::<Vec<_>>(),"more":false})
                }
                "/api/mlog/artist/video" => {
                    json!({"code":200,"data":{"records":[{"id":"21","resource":{"mlogBaseData":{"type":3,"text":"合成 MV"}}},{"id":"opaque","resource":{"mlogBaseData":{"type":2,"text":"普通视频类型"}}}],"page":{"cursor":"20","size":20,"more":false}}})
                }
                "/api/concert/list/byartist" => {
                    json!({"code":200,"simpleConcerts":[{"id":1,"title":"合成演出","time":[1800000000000_i64,1800086400000_i64],"simpleAddress":{"address":"合成场馆"},"price":"88起","url":"https://invalid.example/private"}],"total":1})
                }
                "/api/album/v3/detail" => {
                    json!({"code":200,"album":album(id(&args["id"])),"songs":(1..=11).map(song).collect::<Vec<_>>()})
                }
                "/api/album/detail/dynamic" => {
                    json!({"code":200,"isSub":self.album_collected.load(Ordering::SeqCst)})
                }
                "/api/album/privilege" => {
                    json!({"code":200,"data":(1..=11).map(|id|json!({"id":id,"pl":320000})).collect::<Vec<_>>()})
                }
                "/api/resource-exposure/config" => {
                    json!({"code":200,"data":{"resourceType":"other"}})
                }
                "/api/artist/sublist" => {
                    json!({"code":200,"more":false,"count":usize::from(self.artist_collected.load(Ordering::SeqCst)),"data":if self.artist_collected.load(Ordering::SeqCst){vec![artist(12)]}else{vec![]}})
                }
                "/api/album/sublist" => {
                    json!({"code":200,"more":false,"count":usize::from(self.album_collected.load(Ordering::SeqCst)),"data":if self.album_collected.load(Ordering::SeqCst){vec![album(7)]}else{vec![]}})
                }
                "/api/artist/sub" | "/api/artist/unsub" => {
                    self.artist_collected
                        .store(path.ends_with("/sub"), Ordering::SeqCst);
                    json!({"code":200})
                }
                "/api/album/sub" | "/api/album/unsub" => {
                    self.album_collected
                        .store(path.ends_with("/sub"), Ordering::SeqCst);
                    json!({"code":200})
                }
                _ if path.starts_with("/api/artist/albums/") => {
                    json!({"code":200,"artist":{"albumSize":62},"more":offset+limit<62,"hotAlbums":((offset+7)..(offset+limit+7).min(69)).map(album).collect::<Vec<_>>()})
                }
                _ => return Err(Error::Response("unsupported synthetic catalog endpoint")),
            })
        })
    }
}
