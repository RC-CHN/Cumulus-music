use cumulus_core::{
    Error, Result,
    account::AccountSession,
    comments::*,
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Copy)]
pub enum Mode {
    Ok,
    LostBefore,
    LostApplied,
    MissingId,
    WrongOwner,
    WrongThread,
    Hold,
    Hidden,
    Tombstone,
    Reject(i64),
}
pub struct Server {
    pub calls: Mutex<Vec<(String, Value)>>,
    pub rows: Mutex<BTreeMap<String, Value>>,
    pub mode: Mutex<Mode>,
    pub hidden: AtomicBool,
    pub hold_read: AtomicBool,
    pub started: tokio::sync::Notify,
    pub release: tokio::sync::Notify,
    serial: AtomicU64,
}
pub fn item(id: &str, owner: &str, content: &str) -> Value {
    json!({"commentId":id,"content":content,"time":1700000000000u64,"timeStr":"fixture","liked":false,"likedCount":0,"user":{"userId":owner,"nickname":"Example"},"beReplied":[]})
}
impl Api for Server {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            self.calls
                .lock()
                .unwrap()
                .push((path.into(), payload.clone()));
            if matches!(
                path,
                "/api/resource/comments/add"
                    | "/api/v1/resource/comments/reply"
                    | "/api/resource/comments/delete"
            ) {
                let mode = *self.mode.lock().unwrap();
                if matches!(mode, Mode::Hold) {
                    self.started.notify_one();
                    self.release.notified().await;
                }
                if let Mode::Reject(code) = mode {
                    return Ok(json!({"code":code,"message":"private service detail"}));
                }
                if matches!(mode, Mode::LostBefore) {
                    return Err(Error::Transport("secret fixture".into()));
                }
                let mut comment = Value::Null;
                if path.ends_with("/delete") {
                    let id = payload["commentId"].as_str().unwrap();
                    if matches!(mode, Mode::Tombstone) {
                        let mut c = item(id, "1", "");
                        c["status"] = json!(-5);
                        self.rows.lock().unwrap().insert(id.into(), c);
                    } else {
                        self.rows.lock().unwrap().remove(id);
                    }
                } else {
                    let id = format!(
                        "9007199254740993{}",
                        self.serial.fetch_add(1, Ordering::SeqCst)
                    );
                    comment = item(
                        &id,
                        if matches!(mode, Mode::WrongOwner) {
                            "2"
                        } else {
                            "1"
                        },
                        payload["content"].as_str().unwrap(),
                    );
                    if path.ends_with("/reply") {
                        comment["beReplied"] = json!([{"beRepliedCommentId":payload["commentId"],"content":"quoted","user":{"userId":"2","nickname":"Other"}}]);
                    }
                    self.rows.lock().unwrap().insert(id, comment.clone());
                    if matches!(mode, Mode::Hidden) {
                        self.hidden.store(true, Ordering::Release);
                    }
                    if matches!(mode, Mode::WrongThread) {
                        comment["threadId"] = json!("R_SO_4_999");
                    }
                }
                if matches!(mode, Mode::LostApplied) {
                    return Err(Error::Transport("secret fixture".into()));
                }
                return Ok(if matches!(mode, Mode::MissingId) {
                    json!({"code":200})
                } else {
                    json!({"code":200,"comment":comment})
                });
            }
            if !path.starts_with("/api/v1/resource/comments/") {
                return Err(Error::State("unexpected comment fixture route"));
            }
            let selected = payload["commentId"].as_str().filter(|id| *id != "0");
            let value = {
                let rows = self.rows.lock().unwrap();
                let visible =
                    |id: &str| !self.hidden.load(Ordering::Acquire) || !id.starts_with("900719");
                let current = selected
                    .filter(|id| visible(id))
                    .and_then(|id| rows.get(id))
                    .cloned();
                let comments: Vec<_> = rows
                    .iter()
                    .filter(|(id, _)| visible(id))
                    .take(payload["limit"].as_u64().unwrap() as usize)
                    .map(|(_, v)| v.clone())
                    .collect();
                json!({"code":200,"comments":comments,"currentComment":current,"hotComments":[],"topComments":[],"total":rows.len(),"more":false})
            };
            if self.hold_read.swap(false, Ordering::AcqRel) {
                self.started.notify_one();
                self.release.notified().await;
            }
            Ok(value)
        })
    }
}
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub store: Arc<StateStore>,
    pub api: Arc<Server>,
    pub session: Arc<AccountSession>,
    pub service: Arc<Comments>,
}
impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(Server {
            calls: Mutex::default(),
            rows: Mutex::new(BTreeMap::from([
                ("101".into(), item("101", "1", "Mine")),
                ("102".into(), item("102", "2", "Other")),
            ])),
            mode: Mutex::new(Mode::Ok),
            hidden: AtomicBool::new(false),
            hold_read: AtomicBool::new(false),
            started: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            serial: AtomicU64::new(12345),
        });
        let session = Arc::new(AccountSession::new(api.clone(), Some(store.clone())));
        session
            .adopt(
                api.clone(),
                json!({"userId":"1"}),
                &session.token().unwrap(),
            )
            .unwrap();
        let service = Arc::new(
            Comments::new(session.clone())
                .unwrap()
                .with_settle_delay(Duration::ZERO),
        );
        Self {
            root,
            store,
            api,
            session,
            service,
        }
    }
    pub fn writes(&self) -> usize {
        self.api
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| {
                matches!(
                    p.as_str(),
                    "/api/resource/comments/add"
                        | "/api/v1/resource/comments/reply"
                        | "/api/resource/comments/delete"
                )
            })
            .count()
    }
    pub async fn prepare(&self, command: CommentWriteCommand) -> Result<CommentWrite> {
        self.service
            .prepare_write_for(&self.session.token()?, &resource(), command)
            .await
    }
    pub fn trigger(&self, sql: &str) {
        rusqlite::Connection::open(self.root.path().join("core.sqlite3"))
            .unwrap()
            .execute_batch(sql)
            .unwrap();
    }
}
pub fn resource() -> CommentResource {
    CommentResource {
        kind: ResourceKind::Track,
        id: "123".into(),
        owner: None,
    }
}
pub fn publish() -> CommentWriteCommand {
    CommentWriteCommand::Publish {
        content: " 原文 🎵\n[微笑] ".into(),
        reply_to: None,
    }
}
