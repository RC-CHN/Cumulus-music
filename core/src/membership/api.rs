use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone)]
pub struct MembershipApi {
    api: Arc<dyn Api>,
}
impl MembershipApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }

    pub async fn info(&self, uid: Option<&Value>) -> Result<Value> {
        let params = match uid {
            Some(uid) => json!({"userId":identifier(uid)?}),
            None => json!({}),
        };
        let response = self
            .api
            .call(
                "/api/music-vip-membership/client/vip/info",
                params,
                false,
                &[200],
            )
            .await?;
        response
            .get("data")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or(Error::Response("missing membership data"))
    }
}
