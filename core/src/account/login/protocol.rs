//! PC M/17 delegates to JSK only under M/34's two experiment assignments.
use crate::protocol::{
    api::{Api, ApiFuture},
    http::{JskLoginContext, MusicHttpClient},
};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LoginProtocol {
    Legacy,
    Jsk,
}

impl LoginProtocol {
    pub(crate) fn api(self, client: Arc<MusicHttpClient>) -> Arc<dyn Api> {
        match self {
            Self::Legacy => client,
            Self::Jsk => Arc::new(JskLoginApi {
                client,
                context: Default::default(),
            }),
        }
    }
    pub(crate) fn qr_type(self) -> u32 {
        match self {
            Self::Legacy => 3,
            Self::Jsk => 5,
        }
    }
}

struct JskLoginApi {
    client: Arc<MusicHttpClient>,
    context: JskLoginContext,
}
impl Api for JskLoginApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.client
                .request_jsk_login(path, payload, &self.context)
                .await
        })
    }
    fn account_reply(&self) -> ApiFuture<'_, Value> {
        // Community identity verification is separate from the SDK fetch path.
        // Challenge headers must not escape into general account requests.
        self.client.account_reply()
    }
}
