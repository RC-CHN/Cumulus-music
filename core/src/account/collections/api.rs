use super::{CollectionQuery, query};
use crate::{
    Error, Result,
    catalog::metadata::{AlbumMetadata, ArtistMetadata},
    protocol::api::{Api, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Serialize, Deserialize)]
pub struct CollectionData {
    /// Original native metadata; never sent directly to the UI.
    pub records: Vec<Value>,
    pub total: Option<u64>,
    pub next: Option<CollectionQuery>,
}
impl CollectionData {
    pub(super) fn validate(&self, query: &CollectionQuery) -> Result<()> {
        query.validate()?;
        if self.records.len() > 50_000 || self.next.is_some() && self.records.is_empty() {
            return Err(Error::Response(
                "collection page did not advance or exceeds limit",
            ));
        }
        if let Some(next) = &self.next {
            next.validate()?;
            match (query, next) {
                (
                    CollectionQuery::SavedAlbums { offset, limit },
                    CollectionQuery::SavedAlbums {
                        offset: n,
                        limit: l,
                    },
                ) if *n == offset + limit && l == limit => {}
                (
                    CollectionQuery::FollowedArtists { cursor, size },
                    CollectionQuery::FollowedArtists { cursor: n, size: s },
                ) if n != cursor && s == size => {}
                _ => return Err(Error::Response("invalid collection continuation")),
            }
        }
        for row in &self.records {
            if !row.is_object() {
                return Err(Error::Response("invalid collection record"));
            }
            match query {
                CollectionQuery::SavedAlbums { .. } | CollectionQuery::PurchasedAlbums => {
                    AlbumMetadata::from_official(row)?;
                }
                CollectionQuery::FollowedArtists { .. } => {
                    // Official en renders artist+user, user-only, artist-only or
                    // no card. Do not mistake a user ID for an artist ID.
                    if !row["artistInfo"].is_null() {
                        ArtistMetadata::from_official(&row["artistInfo"])?;
                    }
                    if !row["userProfile"].is_null() {
                        identifier(&row["userProfile"]["userId"])?;
                    }
                }
            }
        }
        Ok(())
    }
}
pub struct CollectionApi {
    api: Arc<dyn Api>,
}
impl CollectionApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    pub async fn page(&self, query: &CollectionQuery) -> Result<CollectionData> {
        query.validate()?;
        let (path, payload) = match query {
            CollectionQuery::SavedAlbums { offset, limit } => (
                "/api/album/sublist",
                json!({"offset":offset,"limit":limit,"total":true}),
            ),
            CollectionQuery::PurchasedAlbums => {
                ("/api/digitalAlbum/purchased", json!({"substatus":true}))
            }
            CollectionQuery::FollowedArtists { cursor, size } => (
                "/api/user/follow/users/mixed/get/v2",
                json!({"page":{"size":size,"cursor":cursor},"size":size,"scene":1,"sortType":0,"authority":false}),
            ),
        };
        // M/15 g has enable:false, including the purchase-list read.
        let response = self.api.call(path, payload, false, &[200]).await?;
        let (rows, total, next) = match query {
            CollectionQuery::SavedAlbums { offset, limit } => {
                let more = response["hasMore"]
                    .as_bool()
                    .ok_or(Error::Response("missing saved album pagination"))?;
                (
                    &response["data"],
                    response["count"].as_u64(),
                    more.then_some(CollectionQuery::SavedAlbums {
                        offset: offset + limit,
                        limit: *limit,
                    }),
                )
            }
            CollectionQuery::PurchasedAlbums => {
                (&response["paidAlbums"], response["total"].as_u64(), None)
            }
            CollectionQuery::FollowedArtists { size, .. } => {
                let data = &response["data"];
                let more = data["page"]["more"]
                    .as_bool()
                    .ok_or(Error::Response("missing followed artist pagination"))?;
                let next = if more {
                    let value = &data["page"]["cursor"];
                    let cursor = match value {
                        Value::String(s) => s.clone(),
                        Value::Number(n) if n.is_u64() => n.to_string(),
                        _ => return Err(Error::Response("missing followed artist cursor")),
                    };
                    query::cursor(&cursor)?;
                    Some(CollectionQuery::FollowedArtists {
                        cursor,
                        size: *size,
                    })
                } else {
                    None
                };
                (&data["records"], data["page"]["recordCount"].as_u64(), next)
            }
        };
        let result = CollectionData {
            records: rows
                .as_array()
                .ok_or(Error::Response("missing collection records"))?
                .clone(),
            total,
            next,
        };
        result.validate(query)?;
        Ok(result)
    }
}
