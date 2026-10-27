use super::PlaylistApi;
use crate::{
    Result,
    protocol::api::{identifier, packed_ids},
};
use serde_json::{Value, json};

impl PlaylistApi {
    pub async fn catalogue(&self) -> Result<Value> {
        self.api
            .call("/api/playlist/catalogue", json!({}), false, &[200])
            .await
    }
    /// M/444 b. The object must already have been uploaded and confirmed.
    pub async fn cover(&self, id: &Value, document: &Value) -> Result<Value> {
        self.api
            .call(
                "/api/playlist/cover/update",
                json!({"id":identifier(id)?,"coverImgId":identifier(document)?}),
                false,
                &[200],
            )
            .await
    }
    /// M/232 L; the full selected create/favorite group, never a partial page.
    pub async fn order(&self, ids: &[Value]) -> Result<Value> {
        self.api
            .call(
                "/api/playlist/order/update",
                json!({"ids":packed_ids(ids)?}),
                false,
                &[200],
            )
            .await
    }
    pub async fn subscribe(&self, id: &Value, subscribe: bool) -> Result<Value> {
        self.api
            .call(
                if subscribe {
                    "/api/playlist/subscribe"
                } else {
                    "/api/playlist/unsubscribe"
                },
                json!({"id":identifier(id)?}),
                false,
                &[200],
            )
            .await
    }
    /// The recovered action only makes an existing private playlist public.
    pub async fn publish(&self, id: &Value) -> Result<Value> {
        self.api
            .call(
                "/api/playlist/update/privacy",
                json!({"id":identifier(id)?,"privacy":0}),
                false,
                &[200],
            )
            .await
    }
}
