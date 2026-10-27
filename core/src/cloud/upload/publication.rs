use super::*;
use crate::protocol::api::identifier;
impl CloudUploads {
    pub(super) fn later(&self, seconds: &Value) -> Result<u64> {
        let delay = (seconds.as_f64().unwrap_or(0.0).clamp(0.5, 60.0) * 1000.0) as u64;
        self.clock
            .now_ms()?
            .checked_add(delay)
            .ok_or(Error::State("upload wait overflow"))
    }
    pub(super) async fn metadata(&self, task: &mut Task) -> Result<()> {
        self.verify_inputs(task).await?;
        let input = task.clone();
        self.begin(task, CloudPhase::MetadataPending)?;
        let result = self
            .call(|api| async move {
                let decision = input
                    .decision
                    .as_ref()
                    .ok_or(Error::State("missing cloud check decision"))?;
                let allocation = input
                    .allocation
                    .as_ref()
                    .map(transfer::AudioAllocation::parse)
                    .transpose()?;
                let attachments = input
                    .attachments
                    .iter()
                    .map(|a| {
                        if a.phase != AttachmentPhase::Uploaded {
                            return Err(Error::State("attachment was not acknowledged"));
                        }
                        let value = a
                            .allocation
                            .as_ref()
                            .ok_or(Error::State("missing uploaded attachment"))?;
                        Ok(super::super::UploadedAttachment {
                            kind: a.kind,
                            document_id: value["docId"].clone(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                api.metadata(super::super::AudioMetadata {
                    upload_ticket: &decision.ticket,
                    md5: &input.source.md5,
                    file_name: types::file_name(&input.source)?,
                    bitrate: input.info.bitrate.unwrap_or(0),
                    resource_id: allocation.as_ref().map(|a| a.resource_id()),
                    object: allocation.as_ref().map(|a| a.object()),
                    song: &input.info.title,
                    artist: input.info.artists.first().map_or("", String::as_str),
                    album: &input.info.album,
                    attachments: &attachments,
                })
                .await
            })
            .await;
        let response = self.rejected(task, CloudPhase::AttachmentsUploading, result)?;
        let top = response
            .get("songId")
            .filter(|v| !v.is_null())
            .map(identifier)
            .transpose()?;
        let nested = response["privateCloud"]
            .get("songId")
            .filter(|v| !v.is_null())
            .map(identifier)
            .transpose()?;
        if top.is_some() && nested.is_some() && top != nested {
            return Err(Error::Response("conflicting encoded cloud song IDs"));
        }
        let song_id = top
            .or(nested)
            .ok_or(Error::Response("metadata omitted encoded song ID"))?;
        let ready = response["exists"].as_bool() == Some(true)
            || response["privateCloud"]["simpleSong"]["mst"].as_i64() == Some(9);
        let next = if ready {
            0
        } else {
            self.later(&response["waitTime"])?
        };
        task.song_id = Some(song_id);
        task.metadata = Some(response);
        task.error = None;
        task.phase = if ready {
            CloudPhase::ReadyToPublish
        } else {
            CloudPhase::Encoding
        };
        task.next_check_ms = next;
        self.save(task)
    }
    pub(super) async fn encoding(&self, task: &mut Task) -> Result<bool> {
        if self.clock.now_ms()? < task.next_check_ms {
            return Ok(false);
        }
        let song_id = task
            .song_id
            .clone()
            .ok_or(Error::State("missing encoded song ID"))?;
        let id = song_id.clone();
        let response = self
            .call(|api| async move { api.status(&[json!(id)]).await })
            .await?;
        let status = response["statuses"]
            .get(&song_id)
            .filter(|v| v.is_object())
            .ok_or(Error::Response("missing encoding status for song"))?;
        match status["status"].as_i64() {
            Some(9) => {
                task.phase = CloudPhase::ReadyToPublish;
                task.next_check_ms = 0;
                task.error = None;
            }
            Some(0 | 1) => {
                task.next_check_ms = self.later(&status["waitTime"])?;
            }
            Some(code) => {
                task.phase = CloudPhase::Failed;
                task.retry_phase = None;
                task.error = Some(UploadIssue {
                    kind: "encoding_failed".into(),
                    code: Some(code),
                });
                task.next_check_ms = 0;
            }
            None => return Err(Error::Response("invalid encoding status")),
        }
        self.save(task)?;
        Ok(task.phase == CloudPhase::ReadyToPublish)
    }
    pub(super) async fn publish(&self, task: &mut Task) -> Result<()> {
        let song_id = task
            .song_id
            .clone()
            .ok_or(Error::State("missing cloud publication ID"))?;
        let id = song_id.clone();
        self.begin(task, CloudPhase::PublicationPending)?;
        let result = self
            .call(|api| async move { api.publish(&json!(id)).await })
            .await;
        let response = self.rejected(task, CloudPhase::ReadyToPublish, result)?;
        if identifier(&response["privateCloud"]["songId"])? != song_id {
            return Err(Error::Response("publication did not confirm this song"));
        }
        task.publication = Some(response);
        task.phase = CloudPhase::Published;
        task.next_check_ms = 0;
        task.error = None;
        self.save(task)
    }
    pub(super) async fn readback(&self, task: &mut Task) -> Result<()> {
        let song_id = task
            .song_id
            .as_ref()
            .ok_or(Error::State("missing cloud readback ID"))?;
        let items = self.call(|api| async move { api.list().await }).await?;
        let mut matches = items
            .into_iter()
            .filter(|item| identifier(&item["songId"]).ok().as_ref() == Some(song_id));
        let item = matches.next();
        if matches.next().is_some() {
            return Err(Error::Response("duplicate cloud readback identity"));
        }
        if let Some(item) = item {
            task.account_item = Some(item);
            task.phase = CloudPhase::Complete;
            task.next_check_ms = 0;
            task.error = None;
        } else {
            task.next_check_ms = self.later(&json!(1))?;
        }
        self.save(task)
    }
    /// Known publication identity only. Absence never authorizes replaying a
    /// possibly accepted publish; incomplete account lists remain errors.
    pub async fn reconcile_publication(&self, id: &str) -> Result<CloudView> {
        let _lock = self.lock(id)?;
        let mut task = self.read(id)?;
        if !matches!(
            task.phase,
            CloudPhase::PublicationPending | CloudPhase::Published
        ) {
            return Err(Error::State(
                "cloud task is not awaiting publication readback",
            ));
        }
        self.readback(&mut task).await?;
        self.view(&task)
    }
}
