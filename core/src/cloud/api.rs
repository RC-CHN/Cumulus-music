use crate::{
    Error, Result,
    protocol::{
        api::{Api, identifier, packed_ids},
        pagination,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone)]
pub struct CloudApi {
    pub(super) api: Arc<dyn Api>,
}
impl CloudApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }

    pub async fn list(&self) -> Result<Vec<Value>> {
        pagination::all(self.api.as_ref(), "/api/v1/cloud/get", json!({}), "data").await
    }

    pub async fn check(
        &self,
        md5: &str,
        length: u64,
        file_name: &str,
        check_mute: bool,
    ) -> Result<Value> {
        validate_digest(md5)?;
        validate_file_name(file_name)?;
        if length == 0 {
            return Err(Error::InvalidInput("empty audio file"));
        }
        // Native FileMd5Check RVA 0x833c0, URI supplied by M/78.
        let response = self.api.call("/api/cloud/upload/check",json!({"md5":md5.to_ascii_lowercase(),"length":length,"fileName":file_name,"checkMute":check_mute}),false,&[200]).await?;
        if !response["needUpload"].is_boolean() || !nonempty_ticket(&response["songId"]) {
            return Err(Error::Response("missing cloud upload decision"));
        }
        Ok(response)
    }

    pub async fn delete(&self, song_ids: &[Value]) -> Result<Value> {
        self.api
            .call(
                "/api/cloud/del",
                json!({"songIds":packed_ids(song_ids)?}),
                false,
                &[200],
            )
            .await
    }

    pub async fn publish(&self, song_id: &Value) -> Result<Value> {
        // M/3127 addToOnlineCloudList accepts 201 as well as 200.
        self.api
            .call(
                "/api/cloud/pub/v2",
                json!({"songid":identifier(song_id)?}),
                false,
                &[200, 201],
            )
            .await
    }

    pub async fn status(&self, song_ids: &[Value]) -> Result<Value> {
        self.api
            .call(
                "/api/v1/cloud/music/status",
                json!({"songIds":packed_ids(song_ids)?}),
                false,
                &[200],
            )
            .await
    }

    pub async fn download(&self, song_id: &Value) -> Result<Value> {
        // This misspelling is the official M/334 endpoint.
        self.api
            .call(
                "/api/cloud/dowonload",
                json!({"songId":identifier(song_id)?}),
                false,
                &[200],
            )
            .await
    }
}

pub(super) fn validate_digest(md5: &str) -> Result<()> {
    if md5.len() != 32 || !md5.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::InvalidInput("invalid audio MD5"));
    }
    Ok(())
}

pub(super) fn validate_file_name(file_name: &str) -> Result<()> {
    if file_name.is_empty() || file_name.contains(['/', '\\', '\r', '\n', '\0']) {
        return Err(Error::InvalidInput("expected a local file name"));
    }
    Ok(())
}

fn nonempty_ticket(value: &Value) -> bool {
    value.as_str().is_some_and(|s| !s.is_empty()) || value.as_u64().is_some_and(|n| n > 0)
}
