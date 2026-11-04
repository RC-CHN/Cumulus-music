use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

const NAME: &str = "听歌 100%";
const ACT: &str = "90071992547409999";
const CURSOR: &str = "90071992547409933331";
const NUMERIC: u64 = 9_007_199_254_740_997;
const FEED: &str = "/api/event/pc/history/feed/get";
const TOPIC: &str = "/api/act/detail";
const HOT: &str = "/api/act/event/hot";
const ALL: &str = "/api/act/event";
const NOTICE: &str = "/api/act/event/new";
const DETAIL: &str = "/api/v1/event/73/31";
const COMMENTS: &str = "/api/v1/resource/comments/A_EV_2_31_73";

#[derive(Default)]
pub struct Library(Mutex<Vec<(String, Value)>>);

impl Library {
    pub fn enabled() -> bool {
        std::env::var("CUMULUS_COMMUNITY_TOPIC_PROOF").as_deref() == Ok("1")
    }

    pub fn report(&self) -> Value {
        json!(&*self.0.lock().unwrap())
    }

    pub fn request(&self, path: &str, body: &Value, cache: bool) -> Option<Result<Value>> {
        if !Self::enabled() || ![FEED, TOPIC, HOT, ALL, NOTICE, DETAIL, COMMENTS].contains(&path) {
            return None;
        }
        assert!(!cache);
        let attempt = {
            let mut calls = self.0.lock().unwrap();
            calls.push((path.into(), body.clone()));
            calls
                .iter()
                .filter(|(target, args)| target == path && args == body)
                .count()
        };
        Some(Ok(match path {
            FEED if *body == json!({"cursor":"-1"}) => {
                let mut rows = (1..=7)
                    .map(|id| event(id, &format!("合成动态 {id}")))
                    .collect::<Vec<_>>();
                rows.push(event(8, "跟随笔记 #听歌 100%# 与 #不存在#，非法 #坏/标签#"));
                json!({"code":200,"data":{"events":rows,"more":false}})
            }
            TOPIC if *body == json!({"actname":NAME}) => json!({"code":200,"act":{
                "actId":ACT,"text":["合成话题说明","RAW_PRIVATE_SECOND"],"participateCount":27,
                "resourceJson":"RAW_PRIVATE_RESOURCE","resourceType":4}}),
            TOPIC if *body == json!({"actname":"不存在"}) => json!({"code":404}),
            HOT if *body == json!({"actid":ACT}) => match attempt {
                1 => json!({"code":503}),
                2 => {
                    json!({"code":200,"events":[event(31,"合成热门首条"),event(90,"合成热门独有")]})
                }
                _ => json!({"code":200,"events":[event(95,"合成刷新热门")]}),
            },
            ALL => {
                let cursor = &body["lasttime"];
                assert_eq!(*body, json!({"actid":ACT,"lasttime":cursor}));
                if cursor == "-1" {
                    let mut events = vec![event(31, "合成热门首条")];
                    events.extend((32..=42).map(|id| event(id, &format!("合成话题笔记 {id}"))));
                    json!({"code":200,"events":events,"more":true,"lasttime":CURSOR})
                } else if cursor == CURSOR {
                    if attempt == 1 {
                        json!({"code":503})
                    } else {
                        let events = (43..=50)
                            .map(|id| event(id, &format!("合成话题续页 {id}")))
                            .collect::<Vec<_>>();
                        json!({"code":200,"events":events,"more":true,"lasttime":NUMERIC})
                    }
                } else if cursor == NUMERIC {
                    json!({"code":200,"events":[event(51,"合成末页笔记")],"more":false})
                } else if cursor == -1 {
                    json!({"code":200,"events":[event(96,"合成刷新全部")],"more":false})
                } else {
                    return Some(Err(Error::Response("unexpected synthetic topic cursor")));
                }
            }
            NOTICE => {
                let now = body["lasttime"].as_u64().unwrap();
                assert!(now > 1_700_000_000_000);
                assert_eq!(*body, json!({"actid":ACT,"lasttime":now}));
                json!({"code":200,"new":true})
            }
            DETAIL if *body == json!({}) => json!({"code":200,"event":event(31,"合成话题详情")}),
            COMMENTS
                if *body
                    == json!({"limit":60,"offset":0,"beforeTime":0,
                "compareUserLocation":false,"commentId":"0","composeConcert":false,
                "markReplied":false,"forceFlatComment":false,"showInner":false}) =>
            {
                json!({"code":200,"total":1,"more":false,"comments":[{
                    "commentId":101,"likedCount":0,"content":"合成话题评论","time":1700000000001_u64,
                    "user":{"userId":91,"nickname":"合成评论人"}}]})
            }
            _ => return Some(Err(Error::Response("unexpected synthetic topic request"))),
        }))
    }
}

fn event(id: u64, text: &str) -> Value {
    json!({"id":id,"type":18,"eventTime":1700000000000_u64-id,
        "actId":if id>=31 {Some(ACT)} else {None},
        "user":{"userId":73,"nickname":format!("合成话题作者 {id}")},
        "info":{"commentCount":u64::from(id==31),"threadId":format!("A_EV_2_{id}_73")},
        "json":json!({"msg":text}).to_string()})
}
