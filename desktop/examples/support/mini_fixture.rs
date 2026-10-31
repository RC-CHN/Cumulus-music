use cumulus_core::protocol::api::{Api, ApiFuture};
#[path = "mini_room_fixture.rs"]
mod room;
use serde_json::{Value, json};
#[derive(Default)]
pub struct Library {
    pub likes: std::sync::Mutex<Vec<String>>,
    pub room: room::RoomFixture,
}
fn song(id: u64) -> Value {
    json!({"id":id,"name":format!("合成歌曲 {id} · A quiet afternoon"),"ar":[{"name":"Cumulus"}],"al":{"name":"界面体验验证"},"dt":if id==1 {6_000_000} else {235_000},"fee":id%3})
}
fn playlist(id: u64) -> Value {
    json!({"id":id,"userId":if id<20 {2} else {3},"name":if id==1 {"我喜欢的音乐".into()} else if id==2 {"午后散步 · 一份用来检查较长名称与编辑入口的歌单".into()} else {format!("合成歌单 {id}")},"creator":{"userId":if id<20 {2} else {3},"nickname":"Cumulus"},"specialType":if id==1 {5} else {0},"privacy":10,"trackCount":40,"description":"沿着熟悉的小路，留一点时间给音乐。这是一段较长的合成简介，用来检查窄窗口里的文字、封面和操作入口。","tags":["轻音乐"],"trackIds":(1..=40).map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>()})
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if let Some(response) = self.room.response(path, &args) {
                return response;
            }
            Ok(match path {
                "/api/rtrs/abt/front/expinfo/list" => json!({"code":200,"data":[]}),
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"profile":{"userId":2,"nickname":"界面体验验证"}})
                }
                "/api/user/playlist" => {
                    json!({"code":200,"more":false,"playlist":(1..=24).map(playlist).collect::<Vec<_>>()})
                }
                "/api/song/like/get" => {
                    json!({"code":200,"ids":self.likes.lock().unwrap().clone()})
                }
                "/api/song/like" => {
                    let id = args["trackId"].as_str().unwrap().to_owned();
                    let mut likes = self.likes.lock().unwrap();
                    likes.retain(|v| v != &id);
                    if args["like"] == true {
                        likes.push(id);
                    }
                    json!({"code":200})
                }
                "/api/v3/discovery/recommend/songs" => {
                    json!({"code":200,"data":{"dailySongs":(1..=40).map(song).collect::<Vec<_>>()}})
                }
                "/api/v6/playlist/detail" => {
                    json!({"code":200,"playlist":playlist(args["id"].as_str().and_then(|s|s.parse().ok()).or_else(||args["id"].as_u64()).unwrap_or(2))})
                }
                "/api/v3/song/detail" => {
                    let rows: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    json!({"code":200,"songs":rows.iter().map(|row|song(row["id"].as_str().and_then(|s|s.parse().ok()).or_else(||row["id"].as_u64()).unwrap())).collect::<Vec<_>>()})
                }
                "/api/playlist/catalogue" => {
                    json!({"code":200,"categories":{"0":"风格"},"sub":[{"name":"轻音乐","category":0},{"name":"摇滚","category":0}]})
                }
                "/api/cloudsearch/pc" => {
                    json!({"code":200,"result":{"songs":(1..=12).map(song).collect::<Vec<_>>(),"songCount":12}})
                }
                "/api/song/enhance/player/url/v1" => {
                    let ids: Vec<Value> = serde_json::from_str(args["ids"].as_str().unwrap())?;
                    json!({"code":200,"data":[{"id":ids[0],"url":"https://m1.music.126.net/cumulus-relay-fixture.wav","code":200,"md5":"fixture","level":"standard","expi":120}]})
                }
                "/api/song/lyric/v1" => {
                    json!({"code":200,"lrc":{"lyric":(0..30).map(|i|format!("[00:{i:02}.00]合成歌词 {i}\n")).collect::<String>()}})
                }
                _ => {
                    return Err(cumulus_core::Error::Response(
                        "unsupported UX fixture endpoint",
                    ));
                }
            })
        })
    }
}
