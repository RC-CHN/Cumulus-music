//! Entirely synthetic HTTP service. No sockets, real credentials, or public writes.
use cumulus_core::{
    Error, Result,
    protocol::{
        api::ApiFuture,
        http::{BodyStatus, HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex};

#[derive(Serialize, Deserialize)]
pub struct State {
    pub rows: BTreeMap<String, Value>,
    pub calls: Vec<Value>,
    serial: u64,
}
pub struct Service {
    pub state: Mutex<State>,
    path: PathBuf,
}
fn item(id: &str, owner: &str, content: &str) -> Value {
    json!({"commentId":id,"content":content,"time":1700000000000u64,"timeStr":"模拟服务评论","liked":false,"likedCount":0,"user":{"userId":owner,"nickname":if owner=="1" {"评论验证账号"}else{"另一位听友"}},"beReplied":[],"parentCommentId":"0"})
}
fn number(value: &Value) -> Result<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.parse().ok())
        .ok_or(Error::State("fixture numeric parameter missing"))
}
impl Service {
    pub fn open(path: PathBuf, reopen: bool) -> Result<Self> {
        let state = if reopen {
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            let mut rows = BTreeMap::from([
                ("101".into(), item("101", "1", "自己的原评论，保留不动。")),
                (
                    "102".into(),
                    item("102", "2", "听到这里，想起了夏天的海风。"),
                ),
            ]);
            for i in 0..21 {
                let id = (201 + i).to_string();
                let mut row = item(
                    &id,
                    "2",
                    &format!("楼层回复 {}：这首歌很适合慢慢听。", i + 1),
                );
                row["parentCommentId"] = json!("102");
                row["time"] = json!(1700000001000u64 + i);
                rows.insert(id, row);
            }
            State {
                rows,
                calls: vec![],
                serial: 9000,
            }
        };
        let service = Self {
            state: Mutex::new(state),
            path,
        };
        fs::write(
            &service.path,
            serde_json::to_vec_pretty(&*service.state.lock().unwrap())?,
        )?;
        Ok(service)
    }
    fn response(state: &mut State, path: &str, data: &Value) -> Result<Value> {
        match path {
            "/api/w/nuser/account/get" => {
                Ok(json!({"code":200,"profile":{"userId":"1","nickname":"评论验证账号"}}))
            }
            "/api/user/playlist" => Ok(json!({"code":200,"playlist":[],"more":false})),
            "/api/song/like/get" => Ok(json!({"code":200,"ids":[]})),
            "/api/v3/discovery/recommend/songs" => Ok(
                json!({"code":200,"data":{"dailySongs":[{"id":123,"name":"夏日回声（合成测试）","ar":[{"id":1,"name":"合成歌手"}],"al":{"id":1,"name":"验证专辑"},"dt":180000}]}}),
            ),
            "/api/resource/comments/add" | "/api/v1/resource/comments/reply" => {
                if data["threadId"] != "R_SO_4_123" {
                    return Err(Error::State("unexpected fixture thread"));
                }
                let content = data["content"]
                    .as_str()
                    .ok_or(Error::State("fixture text missing"))?;
                if content == "需要绑定手机号" {
                    return Ok(json!({"code":480}));
                }
                if content == "目标已消失" {
                    return Ok(json!({"code":404}));
                }
                let id = state.serial.to_string();
                state.serial += 1;
                let mut row = item(&id, "1", content);
                if path.ends_with("/reply") {
                    let parent = data["commentId"]
                        .as_str()
                        .ok_or(Error::State("fixture reply missing"))?;
                    let target = state
                        .rows
                        .get(parent)
                        .ok_or(Error::State("fixture target missing"))?;
                    row["parentCommentId"] = json!(parent);
                    row["beReplied"] = json!([{"beRepliedCommentId":parent,"content":target["content"],"user":target["user"]}]);
                }
                state.rows.insert(id, row.clone());
                if content == "丢失回包后保留责任" {
                    return Err(Error::Transport("synthetic applied response loss".into()));
                }
                Ok(json!({"code":200,"comment":row}))
            }
            "/api/resource/comments/delete" => {
                let id = data["commentId"]
                    .as_str()
                    .ok_or(Error::State("fixture delete missing"))?;
                if data["threadId"] != "R_SO_4_123"
                    || !id.starts_with('9')
                    || state
                        .rows
                        .get(id)
                        .is_none_or(|v| v["user"]["userId"] != "1")
                {
                    return Err(Error::State(
                        "fixture original or non-owned deletion forbidden",
                    ));
                }
                state.rows.remove(id);
                Ok(json!({"code":200}))
            }
            "/api/v1/resource/comments/R_SO_4_123" => {
                let current = data["commentId"].as_str().and_then(|id| state.rows.get(id));
                let rows: Vec<_> = state
                    .rows
                    .values()
                    .filter(|v| v["parentCommentId"] == "0")
                    .take(number(&data["limit"])? as usize)
                    .collect();
                Ok(
                    json!({"code":200,"comments":rows,"currentComment":current,"hotComments":[],"topComments":[],"total":state.rows.len(),"more":false}),
                )
            }
            "/api/resource/comment/floor/get" => {
                if data["parentCommentId"] != "102" || data["threadId"] != "R_SO_4_123" {
                    return Err(Error::State("unexpected floor identity"));
                }
                let cursor = number(&data["time"])?;
                let all: Vec<_> = state
                    .rows
                    .values()
                    .filter(|v| v["parentCommentId"] == "102")
                    .collect();
                let remaining: Vec<_> = all
                    .iter()
                    .filter(|v| cursor == -1 || v["time"].as_i64().unwrap() > cursor)
                    .copied()
                    .collect();
                let rows: Vec<_> = remaining
                    .iter()
                    .take(number(&data["limit"])? as usize)
                    .copied()
                    .collect();
                Ok(
                    json!({"code":200,"data":{"ownerComment":state.rows["102"],"currentComment":null,"comments":rows,"bestComments":[],"totalCount":all.len(),"hasMore":remaining.len()>rows.len(),"time":rows.last().map(|v|v["time"].clone()).unwrap_or(json!(cursor))}}),
                )
            }
            _ => Err(Error::State("unexpected comment fixture path")),
        }
    }
}
impl HttpBackend for Service {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let form = std::str::from_utf8(&plan.body).map_err(|_| Error::State("fixture UTF8"))?;
            let params = form
                .split('&')
                .find_map(|v| v.strip_prefix("params="))
                .ok_or(Error::State("fixture EAPI expected"))?;
            let (path, mut data) = eapi::unwrap(params)?;
            // Record semantic payloads only, never the transport identity/header.
            data.as_object_mut().unwrap().remove("header");
            let mut state = self.state.lock().unwrap();
            let response = Self::response(&mut state, &path, &data);
            state.calls.push(json!({"path":path,"payload":data,"response":response.as_ref().ok(),"lostResponse":response.is_err()}));
            fs::write(&self.path, serde_json::to_vec_pretty(&*state)?)?;
            Ok(HttpReply {
                status: 200,
                body: serde_json::to_vec(&response?)?,
                body_status: BodyStatus::Complete,
                set_cookies: vec![],
                refresh_token: None,
                metadata: BTreeMap::new(),
            })
        })
    }
}
