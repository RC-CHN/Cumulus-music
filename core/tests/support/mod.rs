#![allow(dead_code)]
pub mod download;
pub mod http;
pub mod transfer;
use cumulus_core::{
    Result,
    protocol::api::{Api, ApiFuture},
};
use serde_json::Value;
use std::{collections::VecDeque, sync::Mutex};
use tokio::sync::{Notify, oneshot};

pub enum Reply {
    Ready(Result<Value>),
    Pending(oneshot::Receiver<Result<Value>>),
}

impl From<Value> for Reply {
    fn from(value: Value) -> Self {
        Self::Ready(Ok(value))
    }
}

pub fn gate() -> (Reply, oneshot::Sender<Result<Value>>) {
    let (send, receive) = oneshot::channel();
    (Reply::Pending(receive), send)
}

#[derive(Clone, Debug)]
pub struct Call {
    pub path: String,
    pub payload: Value,
    pub cache: bool,
}

pub struct ScriptApi {
    replies: Mutex<VecDeque<Reply>>,
    calls: Mutex<Vec<Call>>,
    started: Notify,
}

impl ScriptApi {
    pub fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            calls: Mutex::new(vec![]),
            started: Notify::new(),
        }
    }
    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
    pub async fn wait_for_calls(&self, count: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while self.calls.lock().unwrap().len() < count {
                self.started.notified().await;
            }
        })
        .await
        .expect("API call did not start");
    }
}

impl Api for ScriptApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(Call {
                path: path.into(),
                payload,
                cache,
            });
            self.started.notify_one();
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected API request");
            match reply {
                Reply::Ready(value) => value,
                Reply::Pending(receiver) => receiver.await.expect("test gate closed"),
            }
        })
    }
}
