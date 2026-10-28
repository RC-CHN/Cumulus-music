use super::super::MusicClient;
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::details::{CatalogDetails, CatalogQuery},
    protocol::api::identifier,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSelectionPage {
    pub query: CatalogQuery,
    pub revision: String,
}

impl MusicClient {
    /// Artist listData follows the displayed page order (official AllSongsList).
    /// Album resources are re-read on selection (official M/404 album resolver).
    pub(crate) async fn catalog_song_selection(
        &self,
        token: &SessionToken,
        pages: &[CatalogSelectionPage],
        index: usize,
    ) -> Result<(Vec<Value>, usize, Value)> {
        if pages.is_empty() || pages.len() > 500 {
            return Err(Error::InvalidInput("invalid catalog selection pages"));
        }
        let first = &pages[0].query;
        let id = first.id();
        let service = CatalogDetails::new(self.session.clone())?;
        let mut ids = Vec::new();
        let mut next_offset = 0;
        for page in pages {
            page.query.validate()?;
            if page.query.id() != id {
                return Err(Error::InvalidInput("mixed catalog selection resources"));
            }
            match (&first, &page.query) {
                (CatalogQuery::Artist { .. }, CatalogQuery::Artist { .. })
                | (CatalogQuery::Album { .. }, CatalogQuery::Album { .. })
                    if pages.len() == 1 => {}
                (
                    CatalogQuery::ArtistSongs { .. },
                    CatalogQuery::ArtistSongs { offset, limit, .. },
                ) if *offset == next_offset => {
                    next_offset = offset + limit;
                }
                _ => return Err(Error::InvalidInput("noncontiguous catalog selection")),
            }
            let saved = service
                .cached_for(token, &page.query)?
                .ok_or(Error::QueueChanged)?;
            if saved.revision != page.revision {
                return Err(Error::QueueChanged);
            }
            let refreshed;
            let songs = if matches!(first, CatalogQuery::Album { .. }) {
                refreshed = self
                    .session
                    .run_for(token, |api, _| async move {
                        crate::catalog::CatalogApi::new(api)
                            .album_playback_tracks(&json!(id))
                            .await
                    })
                    .await?;
                saved.verify_song_order(&refreshed)?;
                refreshed.as_slice()
            } else {
                saved
                    .songs()
                    .ok_or(Error::InvalidInput("catalog page has no songs"))?
            };
            ids.extend(
                songs
                    .iter()
                    .map(|song| identifier(&song["id"]).map(Value::from))
                    .collect::<Result<Vec<_>>>()?,
            );
            if ids.len() > 50000 {
                return Err(Error::InvalidInput("catalog selection too large"));
            }
        }
        if index >= ids.len() {
            return Err(Error::InvalidInput("invalid catalog selection index"));
        }
        self.session.with_token(token, |_| Ok(()))?;
        let kind = if matches!(first, CatalogQuery::Album { .. }) {
            "album"
        } else {
            "artist"
        };
        Ok((
            ids,
            index,
            json!({"kind":kind,"id":id,"scene":kind,"trial_mode":-1}),
        ))
    }
}
