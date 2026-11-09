//! Prepare the ordinary song menu from the live native queue.
use super::{MusicClient, QueueCatalogMenu, origin, queue_collection, track};
use crate::{
    Error, Result,
    comments::{CommentApi, ResourceKind},
    download::service::DownloadService,
    playback::{QueueEntry, QueueSnapshot},
    storage::PlaybackDownloadKind,
};
use serde_json::Value;
use std::{path::Path, time::Duration};

fn eligible(resource: &Value) -> bool {
    queue_collection::catalog_row(resource)
}

fn entry<'a>(queue: &'a QueueSnapshot, entry_id: &str) -> Result<&'a QueueEntry> {
    queue
        .items
        .iter()
        .find(|item| {
            item.entry_id == entry_id && item.visible_in_queue() && eligible(&item.resource)
        })
        .ok_or(Error::InvalidInput(
            "queue menu requires a visible catalog song",
        ))
}

fn source_label(item: &QueueEntry) -> &str {
    let source = &item.source;
    match source["kind"].as_str() {
        Some("album" | "artist" | "playlist") => {
            origin::display_name(item).unwrap_or(match source["kind"].as_str() {
                Some("album") => "专辑",
                Some("artist") => "歌手",
                _ => "歌单",
            })
        }
        Some("songs") if source["scene"] == "search" => "搜索",
        Some("daily_recommendations") => "每日推荐",
        Some("style_recommendations") => "风格推荐",
        Some("queue_more_recommendation") => "更多推荐",
        Some("recall_prefill") => "新召推荐预填",
        _ => "",
    }
}

fn file_present(path: &Path) -> bool {
    path.is_absolute() && std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

impl MusicClient {
    pub async fn player_queue_catalog_menu(
        &self,
        epoch: &str,
        revision: &str,
        entry_id: &str,
    ) -> Result<QueueCatalogMenu> {
        let token = self.player_token(epoch)?;
        let revision_number = revision
            .parse::<u64>()
            .map_err(|_| Error::InvalidInput("invalid queue menu revision"))?;
        let controller = self.player.controller().await?;
        let (before, queue) = controller.queue_with_state(&token).await?;
        if before.queue_revision != revision_number {
            return Err(Error::QueueChanged);
        }
        if before.fm || self.queue_clear_context(&token)?.room_id.is_some() {
            return Err(Error::InvalidInput(
                "queue menu requires the ordinary queue",
            ));
        }
        let item = entry(&queue, entry_id)?;
        let song = track(&item.resource)?;
        let api = self.session.native_api(&token)?;
        let comment_count = tokio::time::timeout(
            Duration::from_secs(2),
            CommentApi::new(api.clone()).count(ResourceKind::Track, song.id.clone()),
        )
        .await
        .ok()
        .and_then(std::result::Result::ok);
        let (downloaded, download_task_id) = if token.uid.is_some() {
            let session = self.session.clone();
            let checked = token.clone();
            let id = song.id.clone();
            tokio::task::spawn_blocking(move || {
                let owner = session.with_token(&checked, |owner| Ok(owner.to_owned()))?;
                let downloaded = session
                    .persistent_store()?
                    .playback_download_rows(&owner, &id, PlaybackDownloadKind::Song)?
                    .into_iter()
                    .any(|row| {
                        row.status == "complete"
                            && !row.moving
                            && row.path.as_deref().is_some_and(file_present)
                    });
                let task_id = if downloaded {
                    DownloadService::published_catalog_song(&session, &checked, &id)?
                        .filter(|task| task.playback_target().is_ok_and(file_present))
                        .map(|task| task.id)
                } else {
                    None
                };
                Ok::<_, Error>((downloaded, task_id))
            })
            .await
            .map_err(|_| Error::State("queue menu download check stopped"))??
        } else {
            (false, None)
        };
        let (after, latest) = controller.queue_with_state(&token).await?;
        self.session.require_api(&token, &api)?;
        if after.queue_revision != revision_number
            || after.fm
            || self.queue_clear_context(&token)?.room_id.is_some()
            || entry(&latest, entry_id)? != item
        {
            return Err(Error::QueueChanged);
        }
        Ok(QueueCatalogMenu {
            epoch: epoch.into(),
            revision: revision.into(),
            entry_id: entry_id.into(),
            track: song,
            source_label: source_label(item).into(),
            origin: origin::queue(item),
            comment_count,
            downloaded,
            download_task_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn named_menu_source_uses_native_provenance_independently_of_the_song_and_route() {
        let mut entry = QueueEntry {
            entry_id: "entry".into(),
            resource: json!({"id":"1","song":{"id":"1","album":{"id":7,"name":"曲目的专辑"}}}),
            source: json!({"kind":"album","id":"8","name":"未证实的旧字段"}),
        };
        assert_eq!(source_label(&entry), "专辑");
        entry.source = origin::named(entry.source, Some("实际入队专辑"));
        assert_eq!(source_label(&entry), "实际入队专辑");
        assert_eq!(
            serde_json::to_value(origin::queue(&entry)).unwrap(),
            json!({"kind":"album","id":"8"})
        );
        entry.source = json!({"kind":"songs","scene":"search","source_name":"别的标签"});
        assert_eq!(source_label(&entry), "搜索");
        assert!(origin::queue(&entry).is_none());
    }
}
