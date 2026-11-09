//! Catalogue actions on downloaded tracks. The local file ID is a different namespace.
use super::{Context, DownloadShareAccess, MusicClient};
use crate::{
    Error, Result,
    catalog::CatalogApi,
    client::dto::AccountData,
    download::service::{DownloadOrigin, DownloadStatus, DownloadTask},
    protocol::api::identifier,
};
use serde_json::json;
use std::time::Duration;

pub(super) fn catalogue_track_id(task: &DownloadTask) -> Option<String> {
    if task.status != DownloadStatus::Complete {
        return None;
    }
    match &task.origin {
        DownloadOrigin::Song => identifier(&json!(task.song_id)).ok(),
        DownloadOrigin::Cloud { .. } => {
            // CloudApi records this identity separately from the account's
            // original file ID and omits it for a private songType=1 row.
            let metadata = task.song.as_ref()?;
            let public = identifier(&metadata["_cloud_public_id"]).ok()?;
            let catalogue = &metadata["_cloud_catalog_id"];
            if !catalogue.is_null() && identifier(catalogue).ok().as_ref() != Some(&public) {
                return None;
            }
            Some(public)
        }
        DownloadOrigin::Voice { .. } | DownloadOrigin::Mv { .. } => None,
    }
}

pub(super) fn episode_comment_id(task: &DownloadTask) -> Option<String> {
    if task.status != DownloadStatus::Complete {
        return None;
    }
    let DownloadOrigin::Voice { voice_id } = &task.origin else {
        return None;
    };
    identifier(&json!(voice_id)).ok()
}

pub(super) fn link_track_id(task: &DownloadTask) -> Option<String> {
    // M556 invokes M35.j on the resource with its origin metadata: a negative
    // copyright status hides both actions. Paid status alone doesn't hide them.
    let removed = task.song.as_ref().is_some_and(|song| {
        let privilege = &song["privilege"];
        privilege["st"]
            .as_i64()
            .or_else(|| privilege["status"].as_i64())
            .is_some_and(|value| value < 0)
    });
    if removed {
        return None;
    }
    catalogue_track_id(task)
}

fn share_target(context: &Context, task_id: &str) -> Result<(String, bool)> {
    let task = context.service.get_for(&context.token, task_id)?;
    let id = link_track_id(&task).ok_or(Error::InvalidInput(
        "download has no completed public song sharing target",
    ))?;
    // Official M7 fee bit bb. No purchase is performed by the sharing action.
    let album_required = task
        .song
        .as_ref()
        .and_then(|song| song["privilege"]["fee"].as_u64())
        .is_some_and(|fee| fee & 4 != 0);
    Ok((id, album_required))
}

impl MusicClient {
    pub async fn download_song_share_access(
        &self,
        epoch: &str,
        task_id: &str,
    ) -> Result<AccountData<DownloadShareAccess>> {
        let context = self.downloads.inner.context(epoch)?;
        let (id, album_required) = share_target(&context, task_id)?;
        let request_id = id.clone();
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            context
                .session
                .run_for(&context.token, |api, _| async move {
                    CatalogApi::new(api).privileges(&[request_id], None).await
                }),
        )
        .await;
        if share_target(&context, task_id)? != (id.clone(), album_required) {
            return Err(Error::StaleResponse);
        }
        context.guard()?;
        let rows = response.map_err(|_| Error::Response("song sharing check timed out"))??;
        let [row] = rows.as_slice() else {
            return Err(Error::Response(
                "song sharing privilege is missing or ambiguous",
            ));
        };
        if identifier(&row["id"])? != id {
            return Err(Error::Response(
                "song sharing privilege belongs to another track",
            ));
        }
        // M21 maps sp to sharePriv. M3085 treats absent sp in a present
        // privilege as no share rights, not a missing privilege response.
        let shares = match row.get("sp") {
            None | Some(serde_json::Value::Null) => 0,
            Some(value) => value
                .as_i64()
                .ok_or(Error::Response("invalid song sharing privilege"))?,
        };
        let value = if shares > 0 {
            DownloadShareAccess::Allowed
        } else if album_required {
            DownloadShareAccess::AlbumRequired
        } else {
            DownloadShareAccess::Unavailable
        };
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
