#![allow(dead_code)]
use cumulus_core::{
    Error, Result,
    cloud::nos::{NosBackend, NosMethod, NosNode, NosRequest},
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{Notify, Semaphore};
pub struct RenewApi {
    pub calls: AtomicUsize,
    pub replies: Mutex<VecDeque<Result<String>>>,
}
impl Default for RenewApi {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            replies: Mutex::new(VecDeque::new()),
        }
    }
}
impl Api for RenewApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/nos/token/refresh");
            assert_eq!(
                payload,
                json!({"bucket":"bucket-test","object_key":"a/中文+#%.flac"})
            );
            self.calls.fetch_add(1, Ordering::SeqCst);
            let token = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok("fresh".into()))?;
            Ok(json!({"code":200,"result":{"token":token}}))
        })
    }
}
#[derive(Clone)]
pub struct Call {
    pub query: bool,
    pub offset: u64,
    pub complete: bool,
    pub token: String,
    pub url: String,
    pub md5: String,
    pub content_type: String,
    pub body: Vec<u8>,
}
pub enum Fault {
    Http(u16),
    LostReply,
    MissingContext,
    Offset(u64),
    ContextLost,
}
pub struct Gate {
    pub entered: Notify,
    pub release: Semaphore,
}
impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Notify::new(),
            release: Semaphore::new(0),
        })
    }
}
#[derive(Default)]
pub struct State {
    pub data: Vec<u8>,
    pub calls: Vec<Call>,
    pub faults: VecDeque<Option<Fault>>,
    pub required_token: Option<String>,
}
#[derive(Default)]
pub struct Backend {
    pub state: Mutex<State>,
    pub gate: Mutex<Option<Arc<Gate>>>,
}
impl Backend {
    pub fn calls(&self) -> Vec<Call> {
        self.state.lock().unwrap().calls.clone()
    }
    pub fn fault(&self, f: Fault) {
        self.state.lock().unwrap().faults.push_back(Some(f));
    }
    fn handle(&self, request: NosRequest) -> Result<Value> {
        let url = url::Url::parse(request.url()).unwrap();
        let params = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
        let query = request.method() == NosMethod::Query;
        let offset = params
            .get("offset")
            .map(|s| s.parse().unwrap())
            .unwrap_or(0);
        let complete = params.get("complete").is_some_and(|s| s == "true");
        let mut state = self.state.lock().unwrap();
        state.calls.push(Call {
            query,
            offset,
            complete,
            token: request.scoped_token().into(),
            url: request.url().into(),
            md5: request.md5().into(),
            content_type: request.content_type().into(),
            body: request.body().into(),
        });
        if state
            .required_token
            .as_deref()
            .is_some_and(|s| s != request.scoped_token())
        {
            return Err(Error::HttpStatus {
                endpoint: None,
                status: 403,
                body: None,
            });
        }
        let fault = state.faults.pop_front().flatten();
        if let Some(Fault::Http(status)) = fault {
            return Err(Error::HttpStatus {
                endpoint: None,
                status,
                body: None,
            });
        }
        if let Some(Fault::ContextLost) = fault {
            assert!(query);
            state.data.clear();
            return Err(Error::HttpStatus {
                endpoint: None,
                status: 404,
                body: None,
            });
        }
        if !query {
            assert_eq!(offset as usize, state.data.len());
            state.data.extend_from_slice(request.body());
        }
        if matches!(fault, Some(Fault::LostReply)) {
            return Err(Error::Transport("model lost reply".into()));
        }
        let mut result = json!({"offset":state.data.len(),"context":"context-model"});
        if let Some(Fault::Offset(n)) = fault {
            result["offset"] = json!(n);
        }
        if matches!(fault, Some(Fault::MissingContext)) {
            result.as_object_mut().unwrap().remove("context");
        }
        Ok(result)
    }
}
impl NosBackend for Backend {
    fn discover<'a>(&'a self, bucket: &'a str) -> ApiFuture<'a, NosNode> {
        Box::pin(async move {
            assert_eq!(bucket, "bucket-test");
            NosNode::parse("http://nosup-test.127.net")
        })
    }
    fn request(&self, request: NosRequest) -> ApiFuture<'_, Value> {
        Box::pin(async move {
            let is_chunk = request.method() == NosMethod::Chunk;
            let result = self.handle(request);
            let gate = if is_chunk {
                self.gate.lock().unwrap().take()
            } else {
                None
            };
            if let Some(gate) = gate {
                gate.entered.notify_one();
                gate.release.acquire().await.unwrap().forget();
            }
            result
        })
    }
}
