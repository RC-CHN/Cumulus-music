use super::*;
use crate::{membership::Quality, protocol::api::identifier};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(super) fn resource(row: CloudTrackView, quality: Quality) -> Value {
    let track = row.track;
    json!({"id":track.id,"cloud_song_id":track.id,"level":quality.as_str(),"dt":track.duration_ms,
        "song":{"id":track.id,"name":track.name,"ar":track.artists.iter().map(|name|json!({"name":name})).collect::<Vec<_>>(),
        "al":{"name":track.album,"picUrl":track.cover_url},"dt":track.duration_ms,"fee":track.fee}})
}

impl MusicClient {
    pub(in crate::client) async fn cloud_selected_resources(
        &self,
        epoch: &str,
        ids: Vec<String>,
        revision: &str,
        quality: Quality,
    ) -> Result<Vec<Value>> {
        if ids.is_empty() || ids.len() > 10_000 {
            return Err(Error::InvalidInput(
                "invalid cloud batch playback selection",
            ));
        }
        let selected = ids
            .iter()
            .map(|id| identifier(&json!(id)))
            .collect::<Result<BTreeSet<_>>>()?;
        if selected.len() != ids.len() {
            return Err(Error::InvalidInput("duplicate cloud batch selection"));
        }
        let context = self.cloud.inner.context(epoch).await?;
        let snapshot = context.library.refresh().await?;
        if snapshot.revision != revision {
            return Err(Error::QueueChanged);
        }
        tokio::task::spawn_blocking(move || {
            context.guard()?;
            // Official UploadedTrackBatchOperation selects from the complete
            // cloudOnlineList in its original order, independent of page filters.
            let entries = view::catalog(
                Some(&snapshot),
                &CloudQuery {
                    tab: CloudTab::Uploaded,
                    keyword: String::new(),
                    sort: CloudSort::Natural,
                    descending: false,
                    offset: 0,
                    limit: 200,
                },
            )?;
            let resources = entries
                .into_iter()
                .filter(|v| selected.contains(&v.track.id))
                .map(|v| resource(v, quality))
                .collect::<Vec<_>>();
            if resources.len() != selected.len() {
                return Err(Error::QueueChanged);
            }
            context.guard()?;
            Ok(resources)
        })
        .await
        .map_err(|_| Error::State("cloud batch playback worker stopped"))?
    }
}
