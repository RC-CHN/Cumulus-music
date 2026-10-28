use super::{MusicClient, catalog::cards, dto::*, mapping};
use crate::{
    Error, Result,
    account::collections::{Collections, SavedCollectionPage},
    protocol::api::identifier,
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionView {
    pub query: CollectionQuery,
    pub fetched_at: u64,
    pub cached: bool,
    pub total: Option<u64>,
    pub next: Option<CollectionQuery>,
    pub page: CollectionPageView,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CollectionPageView {
    Albums { albums: Vec<AlbumCard> },
    Follows { entries: Vec<FollowCard> },
}
#[derive(Serialize)]
pub struct FollowCard {
    pub artist: Option<ArtistCard>,
    pub user: Option<FollowUser>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowUser {
    pub id: String,
    pub name: String,
    pub signature: String,
    pub avatar_url: Option<String>,
    pub unavailable: bool,
}
fn view(saved: SavedCollectionPage, cached: bool) -> Result<CollectionView> {
    let records = &saved.data.records;
    let page = match saved.query {
        CollectionQuery::SavedAlbums { .. } | CollectionQuery::PurchasedAlbums => {
            CollectionPageView::Albums {
                albums: records.iter().map(cards::album).collect::<Result<_>>()?,
            }
        }
        CollectionQuery::FollowedArtists { .. } => CollectionPageView::Follows {
            entries: records
                .iter()
                .map(|row| {
                    let user = &row["userProfile"];
                    Ok(FollowCard {
                        artist: row["artistInfo"]
                            .is_object()
                            .then(|| cards::artist(&row["artistInfo"]))
                            .transpose()?,
                        user: if user.is_object() {
                            Some(FollowUser {
                                id: identifier(&user["userId"])?,
                                name: mapping::string(&user["nickname"]),
                                signature: mapping::string(&user["signature"]),
                                avatar_url: mapping::artwork(&user["avatarUrl"]),
                                unavailable: user["accountStatus"]
                                    .as_i64()
                                    .is_some_and(|n| [30, 40].contains(&n)),
                            })
                        } else {
                            None
                        },
                    })
                })
                .collect::<Result<_>>()?,
        },
    };
    Ok(CollectionView {
        query: saved.query,
        fetched_at: saved.fetched_at,
        cached,
        total: saved.data.total,
        next: saved.data.next,
        page,
    })
}
impl MusicClient {
    pub async fn collection_page(
        &self,
        epoch: &str,
        query: CollectionQuery,
        refresh: bool,
    ) -> Result<AccountData<Option<CollectionView>>> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let service = Collections::new(self.session.clone())?;
        let saved = if refresh {
            Some(service.page_for(&token, &query).await?)
        } else {
            service.cached_for(&token, &query)?
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: saved.map(|saved| view(saved, !refresh)).transpose()?,
            })
        })
    }
}
