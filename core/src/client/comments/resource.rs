//! Read-only metadata for the official standalone comment resource cards.
use super::{CommentResource, MusicClient};
use crate::{
    Error, Result,
    catalog::CatalogApi,
    client::{dto::*, mapping, podcast::view::episode},
    comments::ResourceKind,
    mv::{MvApi, MvDetail},
    podcast::{Episode, PodcastApi},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const CACHE: &str = "comment-resource-v1";

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommentResourceCard {
    Track {
        track: TrackView,
    },
    Voice {
        episode: EpisodeView,
        #[serde(rename = "creatorAccountId")]
        creator_account_id: Option<String>,
    },
    Mv {
        id: String,
        title: String,
        #[serde(rename = "coverUrl")]
        cover_url: Option<String>,
        artists: Vec<ArtistReference>,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentResourceView {
    pub resource: CommentResource,
    pub card: Option<CommentResourceCard>,
    pub cached: bool,
}
#[derive(Serialize, Deserialize)]
enum Payload {
    Track(Value),
    Voice(Box<Episode>),
    Mv(Box<MvDetail>),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    resource: CommentResource,
    payload: Payload,
}
impl Saved {
    fn project(self, resource: &CommentResource, cached: bool) -> Result<CommentResourceView> {
        if &self.resource != resource {
            return Err(Error::State("comment resource cache identity mismatch"));
        }
        let card = match (resource.kind, self.payload) {
            (ResourceKind::Track, Payload::Track(raw)) => {
                let track = mapping::track(&raw)?;
                if track.id != resource.id || track.voice_id.is_some() {
                    return Err(Error::Response("comment track identity mismatch"));
                }
                CommentResourceCard::Track { track }
            }
            (ResourceKind::Voice, Payload::Voice(value)) if value.id == resource.id => {
                // M/21 normalizes the selected dj/creator through its user model;
                // the route is a user account ID, never a podcast or artist ID.
                let creator = if value.raw["dj"].is_object() {
                    &value.raw["dj"]
                } else {
                    &value.raw["creator"]
                };
                CommentResourceCard::Voice {
                    creator_account_id: crate::protocol::api::identifier(&creator["userId"]).ok(),
                    episode: episode(&value, &[]),
                }
            }
            (ResourceKind::Mv, Payload::Mv(value)) if value.id == resource.id => {
                CommentResourceCard::Mv {
                    id: value.id,
                    title: value.title,
                    cover_url: mapping::artwork(&json!(value.cover_url)),
                    artists: value
                        .artists
                        .into_iter()
                        .map(|artist| ArtistReference {
                            id: artist
                                .id
                                .filter(|id| crate::protocol::api::identifier(&json!(id)).is_ok()),
                            name: artist.name,
                        })
                        .collect(),
                }
            }
            _ => {
                return Err(Error::Response(
                    "comment resource payload identity mismatch",
                ));
            }
        };
        Ok(CommentResourceView {
            resource: resource.clone(),
            card: Some(card),
            cached,
        })
    }
}
impl MusicClient {
    pub async fn comments_resource(
        &self,
        epoch: &str,
        resource: CommentResource,
        refresh: bool,
    ) -> Result<AccountData<CommentResourceView>> {
        let token = self.comment_token(epoch)?;
        let key = resource.thread()?;
        // M3120 only renders track/voice/MV. Its video implementation is explicitly
        // unimplemented; album/playlist comments continue to use their own context.
        let value =
            if !matches!(
                resource.kind,
                ResourceKind::Track | ResourceKind::Voice | ResourceKind::Mv
            ) {
                CommentResourceView {
                    resource,
                    card: None,
                    cached: false,
                }
            } else if !refresh {
                self.session.with_token(&token, |uid| {
                    let value = self
                        .session
                        .persistent_store()?
                        .get(uid, CACHE, &key)?
                        .ok_or(Error::State("comment resource is not cached"))?;
                    serde_json::from_value::<Saved>(value)?.project(&resource, true)
                })?
            } else {
                let payload =
                    self.session
                        .run_for(&token, |api, _| async {
                            let id = json!(resource.id);
                            match resource.kind {
                                ResourceKind::Track => {
                                    let mut batch = CatalogApi::new(api).songs(&[id], 1).await?;
                                    Ok(Payload::Track(
                                        batch.songs.pop().flatten().ok_or(Error::Response(
                                            "comment track is not readable",
                                        ))?,
                                    ))
                                }
                                ResourceKind::Voice => Ok(Payload::Voice(Box::new(
                                    PodcastApi::new(api).episode(&id).await?,
                                ))),
                                ResourceKind::Mv => {
                                    Ok(Payload::Mv(Box::new(MvApi::new(api).detail(&id).await?)))
                                }
                                _ => unreachable!("resource kind checked before network read"),
                            }
                        })
                        .await?;
                let saved = Saved {
                    resource: resource.clone(),
                    payload,
                };
                let encoded = serde_json::to_value(&saved)?;
                let projected = saved.project(&resource, false)?;
                self.session.with_token(&token, |uid| {
                    self.session
                        .persistent_store()?
                        .put(uid, CACHE, &key, &encoded)?;
                    Ok(projected)
                })?
            };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
