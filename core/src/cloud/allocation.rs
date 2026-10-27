use super::{
    CloudApi,
    api::{validate_digest, validate_file_name},
};
use crate::{Error, Result, protocol::api::identifier};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Cover,
    Lyric,
    Cue,
}

impl CloudApi {
    pub async fn allocate(&self, file_name: &str, md5: &str) -> Result<Value> {
        validate_file_name(file_name)?;
        validate_digest(md5)?;
        // Native U:0x246210, constants initialized at 0x3540/0x3590.
        let response = self.api.call("/api/whale/token/alloc",json!({"filename":file_name,"type":"audio","bucket":"jd-musicrep-privatecloud-audio-public","bizKey":"9af6516d","md5":md5.to_ascii_lowercase(),"channel":3}),false,&[200]).await?;
        let mut allocation = response
            .get("data")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or(Error::Response("missing upload allocation"))?;
        if let Some(key) = allocation
            .get("key")
            .filter(|value| value.as_str().is_some_and(|s| !s.is_empty()))
            .cloned()
        {
            if allocation
                .get("objectKey")
                .is_some_and(|v| !v.is_null() && v != "" && v != &key)
            {
                return Err(Error::Response("conflicting upload object keys"));
            }
            allocation["objectKey"] = key;
        }
        Ok(allocation)
    }

    pub async fn refresh_nos_token(&self, bucket: &str, object_key: &str) -> Result<String> {
        if bucket.is_empty()
            || !bucket
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            return Err(Error::InvalidInput("invalid NOS bucket"));
        }
        if object_key.is_empty()
            || object_key.chars().count() > 4096
            || object_key.contains(['\r', '\n', '\0'])
        {
            return Err(Error::InvalidInput("invalid NOS object key"));
        }
        // U:0x2407a0 and U:0x23f970 renew the SAME object's scope.
        let response = self
            .api
            .call(
                "/api/nos/token/refresh",
                json!({"object_key":object_key,"bucket":bucket}),
                false,
                &[200],
            )
            .await?;
        let token = response["result"]["token"]
            .as_str()
            .filter(|s| {
                !s.is_empty() && s.chars().count() <= 16384 && !s.contains(['\r', '\n', '\0'])
            })
            .ok_or(Error::Response("missing or invalid refreshed NOS token"))?;
        Ok(token.into())
    }

    pub async fn allocate_attachment(
        &self,
        file_name: &str,
        kind: AttachmentKind,
    ) -> Result<Value> {
        self.allocate_image_or_text(file_name, kind, None).await
    }

    /// Official M2167 explicitly selects IMAGE/yyimgs for playlist artwork.
    pub async fn allocate_playlist_cover(&self, file_name: &str) -> Result<Value> {
        self.allocate_image_or_text(file_name, AttachmentKind::Cover, Some("yyimgs"))
            .await
    }

    async fn allocate_image_or_text(
        &self,
        file_name: &str,
        kind: AttachmentKind,
        bucket: Option<&str>,
    ) -> Result<Value> {
        validate_file_name(file_name)?;
        if file_name.chars().count() > 255 {
            return Err(Error::InvalidInput(
                "attachment name exceeds 255 characters",
            ));
        }
        let ext = file_name
            .rsplit_once('.')
            .filter(|(name, _)| !name.is_empty())
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default();
        let valid = match kind {
            AttachmentKind::Cover => ["jpg", "jpeg", "png"].contains(&ext.as_str()),
            AttachmentKind::Lyric => ext == "lrc",
            AttachmentKind::Cue => ext == "cue",
        };
        if !valid {
            return Err(Error::InvalidInput("unsupported cloud attachment format"));
        }
        // U:0x23fdb0 includes no bucket when the caller did not supply one.
        let mut payload = json!({"filename":file_name,"ext":ext,"type":if kind==AttachmentKind::Cover {"image"} else {"other"}});
        if let Some(bucket) = bucket {
            payload["bucket"] = bucket.into();
        }
        let response = self
            .api
            .call("/api/nos/token/alloc", payload, false, &[200])
            .await?;
        let mut allocation = response
            .get("data")
            .filter(|v| v.is_object() && !v.as_object().unwrap().is_empty())
            .or_else(|| response.get("result"))
            .filter(|v| v.is_object())
            .cloned()
            .ok_or(Error::Response("missing attachment allocation"))?;
        for key in ["bucket", "objectKey", "token", "docId"] {
            let text = match &allocation[key] {
                Value::String(s) => s.clone(),
                Value::Number(n) if n.is_i64() || n.is_u64() => n.to_string(),
                _ => return Err(Error::Response("invalid attachment allocation field")),
            };
            if text.is_empty() || text.chars().count() > 16384 || text.contains(['\r', '\n', '\0'])
            {
                return Err(Error::Response("invalid attachment allocation field"));
            }
        }
        let document = identifier(&allocation["docId"]).map_err(|_| {
            Error::Response("attachment allocation did not return a usable document ID")
        })?;
        if bucket.is_some_and(|expected| allocation["bucket"].as_str() != Some(expected)) {
            return Err(Error::Response("cover allocation returned another bucket"));
        }
        allocation["channel"] = json!(1);
        allocation["resourceId"] = document.into();
        Ok(allocation)
    }
}
