use super::*;
use sha2::Digest;
use std::io::Read;
pub(super) fn inspect(inputs: AttachmentInputs) -> Result<Vec<Attachment>> {
    let mut output = Vec::new();
    for (kind, path) in [
        (AttachmentKind::Cover, inputs.cover),
        (AttachmentKind::Lyric, inputs.lyric),
        (AttachmentKind::Cue, inputs.cue),
    ] {
        let Some(path) = path else { continue };
        let source = Source::inspect(&path)?;
        let name = types::file_name(&source)?;
        let ext = source
            .path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let valid = match kind {
            AttachmentKind::Cover => ["jpg", "jpeg", "png"].contains(&ext.as_str()),
            AttachmentKind::Lyric => ext == "lrc",
            AttachmentKind::Cue => ext == "cue",
        };
        if !valid || name.chars().count() > 255 || source.bytes > 4 * 1024 * 1024 {
            return Err(Error::InvalidInput("unsupported cloud attachment file"));
        }
        let mut file = crate::media::open_regular(&source.path)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 != source.bytes
            || hex::encode(sha2::Sha256::digest(&bytes)) != source.sha256
        {
            return Err(Error::Media("cloud attachment changed while reading"));
        }
        source.verify(&mut file)?;
        let mime = if kind == AttachmentKind::Cover {
            crate::media::validate_cover(&bytes)?.mime
        } else {
            if bytes.starts_with(b"krc1")
                || (bytes.contains(&0)
                    && !bytes.starts_with(&[0xff, 0xfe])
                    && !bytes.starts_with(&[0xfe, 0xff]))
            {
                return Err(Error::Media("unsupported cloud text attachment"));
            }
            "text/plain"
        };
        output.push(Attachment {
            kind,
            source,
            mime: mime.into(),
            phase: AttachmentPhase::Queued,
            allocation: None,
            error: None,
        });
    }
    Ok(output)
}
impl CloudUploads {
    pub(super) async fn attachments(
        &self,
        task: &mut Task,
        options: &CloudRunOptions,
    ) -> Result<bool> {
        for index in 0..task.attachments.len() {
            if self.halt(task).await? {
                return Ok(false);
            }
            if task.attachments[index].phase == AttachmentPhase::Uploaded {
                continue;
            }
            if task.attachments[index].phase == AttachmentPhase::AllocationPending {
                return Err(Error::MutationUncertain);
            }
            if task.attachments[index].phase == AttachmentPhase::Rejected {
                if !options.retry_rejected {
                    return Err(Error::State(
                        "attachment allocation requires explicit retry",
                    ));
                }
                task.attachments[index].phase = AttachmentPhase::Queued;
                task.attachments[index].error = None;
                self.save(task)?;
            }
            task.attachments[index].source.open_async().await?;
            self.guard()?;
            if task.attachments[index].phase == AttachmentPhase::Queued {
                let name = types::file_name(&task.attachments[index].source)?.to_owned();
                let kind = task.attachments[index].kind;
                task.attachments[index].phase = AttachmentPhase::AllocationPending;
                self.begin(task, CloudPhase::AttachmentsUploading)?;
                let result = self
                    .call(|api| async move { api.allocate_attachment(&name, kind).await })
                    .await;
                if let Err(Error::Service { code, .. }) = &result {
                    task.attachments[index].phase = AttachmentPhase::Rejected;
                    task.attachments[index].error = Some(UploadIssue {
                        kind: "service_rejected".into(),
                        code: code.as_i64(),
                    });
                    self.save(task)?;
                }
                let allocation = result?;
                // Invalid successful allocations remain Pending: allocating
                // again could create another object whose identity was lost.
                NosAllocation::parse(&allocation)?;
                task.attachments[index].allocation = Some(allocation);
                task.attachments[index].phase = AttachmentPhase::Allocated;
                self.save(task)?;
            }
            if self.halt(task).await? {
                return Ok(false);
            }
            let a = task.attachments[index].clone();
            let allocation = NosAllocation::parse(
                a.allocation
                    .as_ref()
                    .ok_or(Error::State("missing attachment allocation"))?,
            )?;
            if a.kind != AttachmentKind::Cover && allocation.bucket() == "yyimgs" {
                task.attachments[index].error =
                    Some(UploadIssue::new("image_only_bucket_for_text"));
                self.save(task)?;
                return Err(Error::State(
                    "service did not allocate text attachment storage",
                ));
            }
            let child_id = task.child_id(Some(a.kind));
            if a.phase == AttachmentPhase::Allocated {
                if self.nos.find(&child_id)?.is_none() {
                    let node = self.nos.discover(allocation.bucket()).await?;
                    if self.halt(task).await? {
                        return Ok(false);
                    }
                    self.nos
                        .create(
                            Some(&child_id),
                            &a.source.path,
                            allocation,
                            node,
                            &a.source.md5,
                            &a.mime,
                        )
                        .await?;
                }
                task.attachments[index].phase = AttachmentPhase::Uploading;
                self.save(task)?;
            }
            if self.halt(task).await? {
                return Ok(false);
            }
            let result = self
                .nos
                .run(
                    &child_id,
                    NosRunOptions {
                        chunk_budget: options.chunk_budget,
                        ..Default::default()
                    },
                )
                .await?;
            if self.halt(task).await? {
                return Ok(false);
            }
            if result.status != NosStatus::Uploaded {
                return Ok(false);
            }
            task.attachments[index].phase = AttachmentPhase::Uploaded;
            task.attachments[index].error = None;
            self.save(task)?;
        }
        Ok(true)
    }
}
