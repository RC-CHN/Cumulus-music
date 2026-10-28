//! Artist subpages from official M/15 Sr/Tr/Pr/Mr/Ir/Ar/Nr and M/2999.
//! API responses stay inside core; desktop projection is a separate boundary.
use crate::{
    Error, Result,
    protocol::api::{Api, identifier},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) fn video_cursor(cursor: &str) -> Result<&str> {
    if cursor.is_empty() || cursor.len() > 128 || !cursor.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::InvalidInput("invalid artist video cursor"));
    }
    let cursor = cursor.trim_start_matches('0');
    Ok(if cursor.is_empty() { "0" } else { cursor })
}

#[derive(Clone, Copy, Debug)]
pub struct ArtistPageRequest {
    pub offset: usize,
    pub limit: usize,
}
impl Default for ArtistPageRequest {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 20,
        }
    }
}
impl ArtistPageRequest {
    pub(super) fn validate(self) -> Result<()> {
        if !(1..=100).contains(&self.limit) || self.offset > 1_000_000 {
            return Err(Error::InvalidInput("invalid artist page bounds"));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ArtistApi {
    api: Arc<dyn Api>,
}
impl ArtistApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }

    pub async fn detail(&self, id: &Value) -> Result<Value> {
        super::CatalogApi::new(self.api.clone()).artist(id).await
    }

    pub async fn dynamic(&self, id: &Value) -> Result<Value> {
        self.api
            .call(
                "/api/artist/detail/dynamic",
                json!({"id":identifier(id)?}),
                false,
                &[200],
            )
            .await
    }

    pub async fn albums(&self, id: &Value, page: ArtistPageRequest) -> Result<Value> {
        page.validate()?;
        self.api
            .call(
                &format!("/api/artist/albums/{}", identifier(id)?),
                json!({"limit":page.limit,"offset":page.offset}),
                false,
                &[200],
            )
            .await
    }

    pub async fn songs(&self, id: &Value, page: ArtistPageRequest) -> Result<Value> {
        page.validate()?;
        // M/15 Nr's uid is a local normalization option, not an HTTP field.
        self.api
            .call(
                "/api/v2/artist/songs",
                json!({"id":identifier(id)?,"limit":page.limit,"offset":page.offset}),
                false,
                &[200],
            )
            .await
    }

    pub async fn introduction(&self, id: &Value) -> Result<Value> {
        self.api
            .call(
                "/api/artist/introduction",
                json!({"id":identifier(id)?}),
                false,
                &[200],
            )
            .await
    }

    pub async fn similar(&self, id: &Value, page: ArtistPageRequest) -> Result<Value> {
        page.validate()?;
        // Official spelling is lower-case artistid, unlike the videos API.
        self.api
            .call(
                "/api/discovery/simiArtist",
                json!({"artistid":identifier(id)?,"limit":page.limit,"offset":page.offset,"total":true}),
                false,
                &[200],
            )
            .await
    }

    pub async fn videos(&self, id: &Value, cursor: &str, size: usize) -> Result<Value> {
        ArtistPageRequest {
            offset: 0,
            limit: size,
        }
        .validate()?;
        let cursor = video_cursor(cursor)?;
        // M/15 Pr embeds a decimal cursor in a JSON *string*. Do not convert
        // it through f64/u64 or send a nested object instead of that string.
        let page = format!(r#"{{"size":{size},"cursor":{cursor}}}"#);
        self.api
            .call(
                "/api/mlog/artist/video",
                json!({"artistId":identifier(id)?,"tab":1,"order":0,"page":page}),
                false,
                &[200],
            )
            .await
    }

    pub async fn concerts(&self, id: &Value, page: ArtistPageRequest) -> Result<Value> {
        page.validate()?;
        self.api
            .call(
                "/api/concert/list/byartist",
                json!({"artistId":identifier(id)?,"limit":page.limit,"offset":page.offset}),
                false,
                &[200],
            )
            .await
    }
}
