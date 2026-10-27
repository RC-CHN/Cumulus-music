use super::{
    control::Control,
    files::{self, ArtifactSlot},
    *,
};
use crate::{Error, Result, catalog::CatalogApi, media::integrity::inspect_file};
use serde_json::json;
use std::{
    fs,
    io::{Seek, Write},
};
impl DownloadService {
    pub(super) async fn lyrics(
        &self,
        mut task: DownloadTask,
        gate: &Control,
    ) -> Result<DownloadTask> {
        if !task.options.lyrics
            || matches!(
                task.lyrics_status,
                LyricsStatus::Complete
                    | LyricsStatus::Empty
                    | LyricsStatus::Conflict
                    | LyricsStatus::Missing
            )
        {
            return Ok(task);
        }
        let result = self.write_lyrics(&mut task, gate).await;
        if let Err(error) = result {
            if matches!(error, Error::StaleResponse | Error::Cancelled) {
                return Err(error);
            }
            task.lyrics_status = LyricsStatus::Error;
            task.lyrics_error = Some(error.to_string());
        }
        self.save_current(&gate.token, &task)?;
        Ok(task)
    }
    async fn write_lyrics(&self, task: &mut DownloadTask, gate: &Control) -> Result<()> {
        gate.require()?;
        files::work(task)?;
        let target = files::target(task)?.with_extension("lrc");
        if let Some(artifact) = &task.lyric
            && artifact.integrity.is_some()
        {
            artifact.verify(task)?;
        } else {
            // Preserve any pre-existing sidecar without downloading a replacement.
            if fs::symlink_metadata(&target).is_ok() {
                task.lyrics_status = LyricsStatus::Conflict;
                return Ok(());
            }
            let id = task.song_id.clone();
            let cloud = task.origin.is_cloud();
            let result = self
                .session
                .run_for(&gate.token, |api, uid| async move {
                    if cloud {
                        let raw = api
                            .call(
                                "/api/cloud/lyric/get",
                                json!({"songId":id,"userId":uid,"lv":-1,"kv":-1}),
                                false,
                                &[200],
                            )
                            .await?;
                        crate::lyrics::normalize_cloud_response(&raw)
                    } else {
                        CatalogApi::new(api).lyric(&json!(id)).await
                    }
                })
                .await?;
            gate.require()?;
            let text = match result.get("lrc").and_then(|v| v.get("lyric")) {
                None | Some(serde_json::Value::Null) => "",
                Some(v) => v.as_str().ok_or(Error::Response("invalid lyric text"))?,
            };
            if text.len() > 1024 * 1024 {
                return Err(Error::Response("lyric sidecar exceeds limit"));
            }
            if text.is_empty() {
                task.lyrics_status = LyricsStatus::Empty;
                return Ok(());
            }
            self.reserve_artifact(task, &gate.token, ArtifactSlot::Lyric)?;
            let mut artifact = task.lyric.take().expect("reserved");
            let mut f = artifact.open(task)?;
            f.set_len(0)?;
            f.rewind()?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            artifact.integrity = Some(inspect_file(&artifact.path)?);
            task.lyric = Some(artifact);
            task.lyrics_status = LyricsStatus::Prepared;
            self.save_current(&gate.token, task)?;
        }
        gate.require()?;
        self.session.with_token(&gate.token, |_| {
            gate.require_alive()?;
            files::target(task)?;
            let artifact = task.lyric.as_ref().expect("prepared lyric");
            if !artifact.matches(&target)? {
                if fs::symlink_metadata(&target).is_ok() {
                    task.lyrics_status = LyricsStatus::Conflict;
                    return Ok(());
                }
                fs::hard_link(&artifact.path, &target)?;
            }
            files::sync(target.parent().expect("target parent"))?;
            task.lyrics_status = LyricsStatus::Complete;
            task.lyrics_error = None;
            self.save(task)
        })
    }
}
