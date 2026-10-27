use super::{
    control::Control,
    files::{self, ArtifactSlot},
    *,
};
use crate::{
    Error, Result,
    download::transfer::{MediaRequest, MediaUrl, TransferStatus},
    media::{
        integrity::inspect_file,
        ncm,
        tags::{self, TagPlan},
        validate_cover,
    },
};
use std::{
    fs::File,
    io::{Read, Seek, Write},
};

impl DownloadService {
    pub(super) async fn prepare(
        &self,
        mut task: DownloadTask,
        gate: Control,
    ) -> Result<DownloadTask> {
        gate.require()?;
        self.detail(&mut task, &gate).await?;
        let raw = self
            .transfer
            .run(
                &task.id,
                crate::download::transfer::RunOptions {
                    byte_budget: None,
                    control: std::sync::Arc::new(gate.clone()),
                },
            )
            .await?;
        if raw.status != TransferStatus::Complete {
            return Err(Error::State("raw media is not complete"));
        }
        if task.raw_integrity.as_ref() != raw.integrity.as_ref() {
            return Err(Error::Media("download changed before preparation"));
        }
        let plan = if task.options.tags || task.encrypted {
            let builder = if task.origin.is_voice() {
                tags::voice_plan
            } else {
                tags::track_plan
            };
            let song = task.song.as_ref().expect("detail");
            Some(builder(
                if task.origin.is_voice() {
                    &song["voice"]
                } else {
                    song
                },
                task.resource
                    .as_ref()
                    .ok_or(Error::State("missing download resource"))?,
                task.encrypted,
            )?)
        } else {
            None
        };
        let cover = if task.options.cover && plan.as_ref().is_some_and(|p| !p.cover_url.is_empty())
        {
            Some(
                self.cover(&mut task, plan.as_ref().expect("plan"), &gate)
                    .await?,
            )
        } else {
            None
        };
        gate.require()?;
        task.status = DownloadStatus::Processing;
        task.phase = DownloadPhase::Prepare;
        self.reserve_artifact(&mut task, &gate.token, ArtifactSlot::Prepared)?;
        self.save_current(&gate.token, &task)?;
        let service = self.clone();
        tokio::task::spawn_blocking(move || {
            gate.require()?;
            files::work(&task)?;
            if files::published(&task)? {
                return Err(Error::State("cannot rewrite a published artifact"));
            }
            let artifact = task.prepared.as_ref().expect("reserved");
            let mut output = artifact.open(&task)?;
            output.set_len(0)?;
            output.rewind()?;
            let raw_path = task.work.join("raw");
            let expected = task.raw_integrity.as_ref().expect("verified");
            if inspect_file(&raw_path)?.sha256 != expected.sha256 {
                return Err(Error::Media("raw file changed before processing"));
            }
            if task.encrypted {
                let mut input = File::open(&raw_path)?;
                let report = ncm::write_plan_stream(
                    &mut input,
                    &mut output,
                    plan.as_ref().expect("encrypted plan"),
                    cover.as_deref().unwrap_or_default(),
                )?;
                if report.audio_sha256 != expected.sha256 || report.audio_bytes != expected.bytes {
                    return Err(Error::Media("raw file changed during NCM preparation"));
                }
            } else if let Some(plan) = &plan {
                tags::writer::prepare_file(&raw_path, &mut output, plan, cover.as_deref())?;
            } else {
                std::io::copy(&mut File::open(&raw_path)?, &mut output)?;
            }
            output.sync_all()?;
            drop(output);
            if inspect_file(&raw_path)?.sha256 != expected.sha256 {
                return Err(Error::Media("raw file changed during preparation"));
            }
            gate.require()?;
            let mut artifact = task.prepared.take().expect("reserved");
            artifact.open(&task)?;
            artifact.integrity = Some(inspect_file(&artifact.path)?);
            if !task.encrypted
                && !task.options.tags
                && artifact
                    .integrity
                    .as_ref()
                    .is_none_or(|i| i.sha256 != expected.sha256 || i.bytes != expected.bytes)
            {
                return Err(Error::Media("raw output copy changed bytes"));
            }
            if task.encrypted {
                ncm::NcmReader::open(&artifact.path, ncm::HeaderPolicy::Strict)?;
            }
            task.prepared = Some(artifact);
            files::sync(&task.work)?;
            task.phase = DownloadPhase::Publish;
            service.save_current(&gate.token, &task)?;
            Ok(task)
        })
        .await
        .map_err(|_| Error::State("download preparation worker stopped"))?
    }
    async fn cover(
        &self,
        task: &mut DownloadTask,
        plan: &TagPlan,
        gate: &Control,
    ) -> Result<Vec<u8>> {
        if let Some(artifact) = &task.cover
            && artifact.integrity.is_some()
        {
            artifact.verify(task)?;
            let mut data = Vec::new();
            artifact
                .open(task)?
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut data)?;
            validate_cover(&data)?;
            return Ok(data);
        }
        gate.require()?;
        let checked = MediaUrl::parse(&plan.cover_url)?;
        let mut url =
            url::Url::parse(checked.as_str()).map_err(|_| Error::Response("invalid cover URL"))?;
        url.set_query(Some("imageView&enlarge=1&quality=100&thumbnail=640y640"));
        let mut reply = self
            .backend
            .get(MediaRequest {
                url: MediaUrl::parse(url.as_str())?,
                offset: 0,
                if_range: None,
            })
            .await?;
        if reply.status != 200 {
            return Err(Error::HttpStatus {
                endpoint: None,
                status: reply.status,
                body: None,
            });
        }
        if reply
            .headers
            .get("content-encoding")
            .is_some_and(|s| !s.eq_ignore_ascii_case("identity"))
        {
            return Err(Error::Response("encoded cover response"));
        }
        let expected = reply
            .headers
            .get("content-length")
            .map(|s| {
                s.parse::<usize>()
                    .map_err(|_| Error::Response("invalid cover size"))
            })
            .transpose()?;
        if expected.is_some_and(|n| n > 4 * 1024 * 1024) {
            return Err(Error::Response("cover exceeds limit"));
        }
        let mut data = Vec::new();
        let mut buffer = [0u8; 16384];
        loop {
            gate.require()?;
            let n = reply.body.read(&mut buffer).await?;
            gate.require()?;
            if n > buffer.len() {
                return Err(Error::Response("invalid cover read size"));
            }
            if n == 0 {
                break;
            }
            if data.len() + n > 4 * 1024 * 1024 {
                return Err(Error::Response("cover exceeds limit"));
            }
            data.extend_from_slice(&buffer[..n]);
        }
        if expected.is_some_and(|n| n != data.len()) {
            return Err(Error::Response("cover length mismatch"));
        }
        validate_cover(&data)?;
        self.reserve_artifact(task, &gate.token, ArtifactSlot::Cover)?;
        let mut artifact = task.cover.take().expect("reserved");
        let mut file = artifact.open(task)?;
        file.set_len(0)?;
        file.rewind()?;
        file.write_all(&data)?;
        file.sync_all()?;
        artifact.integrity = Some(inspect_file(&artifact.path)?);
        task.cover = Some(artifact);
        gate.require()?;
        self.save_current(&gate.token, task)?;
        Ok(data)
    }
}
