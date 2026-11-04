//! Closed artist MV pages; the media decoder reuses the synthetic search file.
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const LIST: &str = "/api/mlog/artist/video";
const FIRST: &str = r#"{"size":20,"cursor":0}"#;
const NEXT: &str = r#"{"size":20,"cursor":90071992547409931234}"#;
const COMMENTS: &str = "/api/v1/resource/comments/R_MV_5_601";

pub struct Library {
    base: Arc<super::fixture::Library>,
    calls: Mutex<Vec<Value>>,
}
impl Library {
    pub fn new(base: Arc<super::fixture::Library>) -> Self {
        Self {
            base,
            calls: Mutex::default(),
        }
    }
    pub fn report(&self) -> Vec<Value> {
        self.calls.lock().unwrap().clone()
    }
}
fn card(id: &str, title: &str, kind: u8) -> Value {
    json!({"id":id,"resource":{"mlogBaseData":{"type":kind,"text":title,"duration":125000},
        "mlogExtVO":{"playCount":345678,"artists":[{"id":12,"name":"不得重复展示的合成歌手"}]}}})
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            if matches!(
                path,
                LIST | COMMENTS | "/api/mlog/detail/v1" | "/api/song/enhance/play/mv/url"
            ) {
                assert!(!cache);
                let attempt = {
                    let mut calls = self.calls.lock().unwrap();
                    calls.push(json!({"path":path,"args":args}));
                    calls
                        .iter()
                        .filter(|entry| entry["path"] == path && entry["args"] == args)
                        .count()
                };
                return match path {
                    LIST if args == json!({"artistId":"12","tab":1,"order":0,"page":FIRST})
                        && attempt == 1 =>
                    {
                        let mut records = (0..20)
                            .map(|i| card(&(800 + i).to_string(), &format!("合成首批 MV {i}"), 3))
                            .collect::<Vec<_>>();
                        records[1] = card("opaque-video", "合成普通视频", 2);
                        records[16] = card("601", "合成原生 MV", 3);
                        Ok(
                            json!({"code":200,"data":{"records":records,"page":{"cursor":"90071992547409931234","size":20,"more":true}}}),
                        )
                    }
                    LIST if args == json!({"artistId":"12","tab":1,"order":0,"page":NEXT})
                        && attempt == 1 =>
                    {
                        Err(Error::Transport(
                            "synthetic artist MV continuation offline".into(),
                        ))
                    }
                    LIST if args == json!({"artistId":"12","tab":1,"order":0,"page":NEXT})
                        && attempt == 2 =>
                    {
                        let mut records = (0..20)
                            .map(|i| card(&(950 + i).to_string(), &format!("合成续页 MV {i}"), 3))
                            .collect::<Vec<_>>();
                        records[0] = card("9521", "合成替换交叠 MV", 3);
                        Ok(
                            json!({"code":200,"data":{"records":records,"page":{"cursor":"90071992547409931253","size":20,"more":false}}}),
                        )
                    }
                    COMMENTS => Ok(
                        json!({"code":200,"total":1,"more":false,"hotComments":[],"comments":[{
                            "commentId":765,"likedCount":0,"content":"合成 MV 评论读回","time":1700000000000_i64,
                            "user":{"userId":73,"nickname":"合成作者"}
                        }]}),
                    ),
                    "/api/mlog/detail/v1" | "/api/song/enhance/play/mv/url" => {
                        let mut reply = super::search_mv_fixture::response(path, &args)
                            .ok_or(Error::State("artist MV fixture transport missing"))??;
                        if path == "/api/mlog/detail/v1" {
                            reply["data"]["resource"]["threadId"] = json!("R_MV_5_601");
                        }
                        Ok(reply)
                    }
                    _ => Err(Error::InvalidInput("unexpected artist MV fixture call")),
                };
            }
            if path.starts_with("/api/v1/resource/hotcomments/") {
                assert!(!cache);
                if path == "/api/v1/resource/hotcomments/R_MV_5_601" {
                    return Ok(json!({"code":200,"hotComments":[],"total":0,"more":false}));
                }
                return Err(Error::InvalidInput("unexpected artist MV comment target"));
            }
            if matches!(
                path,
                "/api/artist/sub" | "/api/artist/unsub" | "/api/mv/sub" | "/api/mv/unsub"
            ) {
                return Err(Error::State("artist MV fixture rejects mutations"));
            }
            let unknown = path == "/api/artist/v3/detail" && args["id"] == "13";
            let mut reply = self.base.request(path, args, cache).await?;
            if unknown {
                reply["artist"].as_object_mut().unwrap().remove("mvSize");
            }
            Ok(reply)
        })
    }
}
