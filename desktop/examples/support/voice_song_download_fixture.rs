//! Sealed SongCard download transport. No production account or media hosts.
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest},
    protocol::api::ApiFuture,
};
use serde_json::{Value, json};
use std::sync::Mutex;

const AUDIO: &[u8] = include_bytes!("../../../core/tests/fixtures/tag_audio/seed.mp3");
const MEDIA: &str = "https://m1.music.126.net/cumulus-song-card-proof.mp3";
const LIKED: &str = "/api/song/like/get";
const COLLECT: &str = "/api/user/playlist/v2";
const LIST: &str = "/api/user/playlist";
const DETAIL: &str = "/api/v6/playlist/detail";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";
const CREATE: &str = "/api/playlist/create";
const META: &str = "/api/v3/song/detail";
const AUTH: &str = "/api/song/enhance/download/url/v1";
const NAME: &str = "合成私密下载歌单";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Liked,
    Cancel,
    ExistingRejected,
    CreateRejected,
    Recover,
    PurchasedLiked,
    PurchasedCancel,
    PurchasedExisting,
    GroupLiked,
    GroupDirectory,
    GroupPurchased,
}

#[derive(Default)]
struct State {
    calls: Vec<Value>,
    liked_reads: usize,
    add_sent: bool,
    created: bool,
    details_after_send: usize,
    membership_visible: bool,
}

pub struct Fixture {
    case: Case,
    state: Mutex<State>,
}
impl Fixture {
    pub fn new(case: &str) -> Self {
        Self {
            case: match case {
                "liked" => Case::Liked,
                "cancel" => Case::Cancel,
                "existing_rejected" => Case::ExistingRejected,
                "create_rejected" => Case::CreateRejected,
                "recover" => Case::Recover,
                "purchased_liked" => Case::PurchasedLiked,
                "purchased_cancel" => Case::PurchasedCancel,
                "purchased_existing" => Case::PurchasedExisting,
                "group_liked" => Case::GroupLiked,
                "group_directory" => Case::GroupDirectory,
                "group_purchased" => Case::GroupPurchased,
                _ => panic!("unknown isolated SongCard download proof"),
            },
            state: Mutex::default(),
        }
    }

    pub fn read(&self, path: &str, body: &Value, cache: bool) -> Option<Result<Value>> {
        match path {
            LIKED => {
                assert!(!cache);
                assert_eq!(*body, json!({"uid":"2"}));
                self.state.lock().unwrap().liked_reads += 1;
                let ids = match self.case {
                    Case::Liked | Case::PurchasedLiked => vec![1710],
                    Case::GroupLiked | Case::GroupDirectory | Case::GroupPurchased => vec![1790],
                    _ => vec![],
                };
                Some(Ok(json!({"code":200,"ids":ids})))
            }
            COLLECT => {
                assert!(
                    !cache
                        && !matches!(
                            self.case,
                            Case::Liked
                                | Case::PurchasedLiked
                                | Case::PurchasedCancel
                                | Case::GroupLiked
                                | Case::GroupDirectory
                                | Case::GroupPurchased
                        )
                );
                assert_eq!(
                    *body,
                    json!({"uid":"2","offset":0,"limit":10,"sortType":"default"})
                );
                self.record(path, body);
                // Owned ordinary, owned Favorites, and followed playlists:
                // only the first can appear in the actual destination list.
                Some(Ok(json!({"code":200,"more":false,"playlist":[
                    {"id":81,"name":"合成已有下载歌单","userId":2,"privacy":10,"trackCount":3},
                    {"id":82,"name":"合成喜欢列表","userId":2,"privacy":10,"specialType":5},
                    {"id":83,"name":"合成他人歌单","userId":3,"privacy":0}
                ]})))
            }
            LIST if body["includeVideo"] == true => {
                assert!(!cache);
                assert_eq!(body["uid"], "2");
                assert_eq!(body["offset"], 0);
                assert!(body["limit"].as_u64().is_some_and(|limit| limit > 0));
                assert_eq!(body.as_object().unwrap().len(), 4);
                let mut state = self.state.lock().unwrap();
                // Also serves the ordinary signed-in bootstrap. Journal
                // reads for the create branch occur after picker discovery.
                if self.case == Case::CreateRejected
                    && state.calls.iter().any(|call| call[0] == COLLECT)
                {
                    state.calls.push(json!([path, body]));
                }
                let mut rows =
                    vec![json!({"id":81,"name":"合成已有下载歌单","userId":2,"privacy":10})];
                if state.created {
                    rows.push(json!({"id":84,"name":NAME,"userId":2,"privacy":10}));
                }
                Some(Ok(json!({"code":200,"playlist":rows,"more":false})))
            }
            DETAIL => {
                assert!(!cache);
                let id = self.destination();
                assert_eq!(*body, json!({"id":id.to_string(),"n":100000,"s":0}));
                let mut state = self.state.lock().unwrap();
                if state.add_sent {
                    state.details_after_send += 1;
                }
                let present = state.membership_visible;
                state.calls.push(json!([path, body, present]));
                let tracks = if present {
                    vec![json!({"id":1710})]
                } else {
                    Vec::new()
                };
                Some(Ok(
                    json!({"code":200,"playlist":{"id":id,"name":if id == 84 {NAME} else {"合成已有下载歌单"},
                    "userId":2,"privacy":10,"tags":[],"trackIds":tracks}}),
                ))
            }
            META if body["c"].as_str().is_some_and(|ids| {
                ["1710", "1712", "1791"]
                    .into_iter()
                    .any(|id| self.song(id) && ids.contains(id))
            }) =>
            {
                assert!(!cache && self.transfers());
                let packed: Vec<Value> = serde_json::from_str(body["c"].as_str().unwrap()).unwrap();
                let id = packed[0]["id"].as_str().unwrap();
                assert!(self.song(id));
                assert_eq!(*body, json!({"c":format!(r#"[{{"id":"{id}","v":0}}]"#)}));
                self.record(path, body);
                let id = id.parse::<u64>().unwrap();
                let privilege = if matches!(
                    self.case,
                    Case::PurchasedLiked | Case::PurchasedExisting
                ) {
                    json!({"id":id,"fee":4,"payed":2,"st":0,"dl":320000,"maxbr":999000,"flag":69632})
                } else {
                    json!({"id":id,"fee":0,"st":0,"dl":320000})
                };
                Some(Ok(
                    json!({"code":200,"songs":[{"id":id,"name":format!("合成包含歌曲 {id}"),
                    "dt":30000,"ar":[{"id":57,"name":"合成歌曲作者"}],"al":{"id":55,"name":"合成包含歌曲专辑"}}],
                    "privileges":[privilege]}),
                ))
            }
            AUTH => {
                assert!(!cache && self.transfers());
                let selected = match self.case {
                    Case::PurchasedLiked | Case::GroupLiked => "exhigh",
                    Case::PurchasedExisting => "lossless",
                    Case::GroupPurchased => "hires",
                    _ => "standard",
                };
                let id = body["id"].as_str().unwrap();
                assert!(self.song(id));
                assert_eq!(*body, json!({"id":id,"level":selected,"immerseType":"c51"}));
                // All synthetic transfers use one pinned MP3. The actual
                // service level can differ from the explicitly requested one.
                self.record(path, body);
                Some(Ok(
                    json!({"code":200,"data":{"id":id.parse::<u64>().unwrap(),"code":200,"url":MEDIA,"type":"mp3",
                    "md5":"485aa2633a203a9ea13fd7416caeb9f9","size":AUDIO.len(),"level":"standard"}}),
                ))
            }
            ADD | CREATE | "/api/dj/voices/download/check" | "/api/song/like" => {
                panic!("unexpected unguarded SongCard action {path}")
            }
            _ => None,
        }
    }

    fn record(&self, path: &str, body: &Value) {
        self.state.lock().unwrap().calls.push(json!([path, body]));
    }

    fn destination(&self) -> u64 {
        match self.case {
            Case::ExistingRejected | Case::Recover | Case::PurchasedExisting => 81,
            Case::CreateRejected => 84,
            _ => panic!("SongCard case cannot query playlist detail"),
        }
    }

    fn song(&self, id: &str) -> bool {
        id == "1710"
            || matches!(self.case, Case::GroupLiked | Case::GroupPurchased) && id == "1712"
            || self.case == Case::GroupPurchased && id == "1791"
    }

    fn transfers(&self) -> bool {
        matches!(
            self.case,
            Case::Liked
                | Case::ExistingRejected
                | Case::PurchasedLiked
                | Case::PurchasedExisting
                | Case::GroupLiked
                | Case::GroupPurchased
        )
    }

    pub fn add(&self, path: &str, body: Value) -> Result<Value> {
        assert_eq!(path, ADD);
        assert_eq!(
            body,
            json!({"pid":self.destination().to_string(),"trackIds":r#"["1710"]"#,"op":"add"})
        );
        let mut state = self.state.lock().unwrap();
        assert!(!state.add_sent, "never replay the synthetic add");
        assert_eq!(state.created, self.case == Case::CreateRejected);
        state.add_sent = true;
        state.calls.push(json!([path, body]));
        if self.case == Case::PurchasedExisting {
            state.membership_visible = true;
            Ok(json!({"code":200}))
        } else if self.case == Case::Recover {
            Err(Error::Transport(
                "synthetic lost playlist acknowledgment".into(),
            ))
        } else {
            Ok(json!({"code":403}))
        }
    }

    pub fn create(&self, path: &str, body: Value) -> Value {
        assert_eq!(path, CREATE);
        assert!(self.case == Case::CreateRejected);
        assert_eq!(body, json!({"uid":"2","name":NAME,"privacy":10}));
        let mut state = self.state.lock().unwrap();
        assert!(!state.created && !state.add_sent);
        state.created = true;
        state.calls.push(json!([path, body]));
        json!({"code":200,"id":84})
    }

    // Only the fixture's synthetic server state changes. The app must request
    // a real status read to see it, and cannot replay the write.
    pub fn reveal_recovery(&self) {
        assert!(self.case == Case::Recover);
        let mut state = self.state.lock().unwrap();
        assert!(state.add_sent && state.details_after_send == 1 && !state.membership_visible);
        state.membership_visible = true;
    }

    pub fn snapshot(&self) -> Value {
        let state = self.state.lock().unwrap();
        json!({"calls":state.calls,"likedReads":state.liked_reads})
    }
}

impl MediaBackend for Fixture {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            assert!(self.transfers());
            assert_eq!(request.url.as_str(), MEDIA);
            assert_eq!(request.offset, 0);
            self.record(
                "media_get",
                &json!({"offset":request.offset,"bytes":AUDIO.len()}),
            );
            Ok(MediaReply {
                status: 200,
                headers: [("content-length".into(), AUDIO.len().to_string())].into(),
                body: Box::new(Body(AUDIO)),
            })
        })
    }
}

struct Body(&'static [u8]);
impl MediaBody for Body {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let len = self.0.len().min(buffer.len());
            buffer[..len].copy_from_slice(&self.0[..len]);
            self.0 = &self.0[len..];
            Ok(len)
        })
    }
}
