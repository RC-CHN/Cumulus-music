use cumulus_core::{
    Result,
    protocol::{
        api::ApiFuture,
        http::{BodyStatus, HttpBackend, HttpReply, RequestPlan},
        wire::eapi,
    },
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
};
use tokio::sync::{Notify, oneshot};

pub enum HttpResponse {
    Ready(Result<HttpReply>),
    Pending(oneshot::Receiver<Result<HttpReply>>),
}
impl From<Value> for HttpResponse {
    fn from(value: Value) -> Self {
        Self::Ready(Ok(reply(value)))
    }
}
pub fn reply(value: Value) -> HttpReply {
    HttpReply {
        status: 200,
        body: serde_json::to_vec(&value).unwrap(),
        body_status: BodyStatus::Complete,
        set_cookies: vec![],
        refresh_token: None,
        metadata: BTreeMap::new(),
    }
}
pub fn gate() -> (HttpResponse, oneshot::Sender<Result<HttpReply>>) {
    let (send, receive) = oneshot::channel();
    (HttpResponse::Pending(receive), send)
}
#[derive(Clone)]
pub struct WireCall {
    pub path: String,
    pub payload: Value,
    pub cookie: Option<String>,
    pub login_chain: Option<String>,
}
pub struct ScriptHttp {
    replies: Mutex<VecDeque<HttpResponse>>,
    calls: Mutex<Vec<WireCall>>,
    started: Notify,
}
impl ScriptHttp {
    pub fn new(replies: impl IntoIterator<Item = HttpResponse>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            calls: Mutex::new(vec![]),
            started: Notify::new(),
        }
    }
    pub fn calls(&self) -> Vec<WireCall> {
        self.calls.lock().unwrap().clone()
    }
    pub async fn wait(&self, count: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while self.calls.lock().unwrap().len() < count {
                self.started.notified().await;
            }
        })
        .await
        .expect("HTTP call did not start");
    }
}
impl HttpBackend for ScriptHttp {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let fields: BTreeMap<_, _> = url::form_urlencoded::parse(&plan.body)
                .into_owned()
                .collect();
            let (path, payload) = eapi::unwrap(&fields["params"]).unwrap();
            self.calls.lock().unwrap().push(WireCall {
                path,
                payload,
                cookie: plan.headers.get("Cookie").cloned(),
                login_chain: plan.headers.get("x-login-chain-id").cloned(),
            });
            self.started.notify_one();
            let response = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected HTTP request/retry");
            match response {
                HttpResponse::Ready(reply) => reply,
                HttpResponse::Pending(receiver) => {
                    receiver.await.expect("test response gate closed")
                }
            }
        })
    }
}
