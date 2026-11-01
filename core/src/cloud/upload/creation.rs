//! Durable local creation receipts. Replaying a UI receipt never resumes a
//! paused/unknown/finished task or repeats its remote allocation/publication.
use super::manager::JOBS;
use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
const RECEIPTS: &str = "cloud_upload_creations_v1";
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Input {
    source: PathBuf,
    cover: Option<PathBuf>,
    lyric: Option<PathBuf>,
    cue: Option<PathBuf>,
}
impl CloudUploads {
    /// A caller keeps request_id for this unchanged selection. On a lost reply,
    /// an identical retry returns the original task without starting it again.
    /// The receipt and task are created in one owner-scoped transaction.
    pub async fn create_once(
        &self,
        request_id: &str,
        source: &Path,
        attachments: AttachmentInputs,
        allow_new: bool,
    ) -> Result<(CloudView, bool)> {
        types::valid_id(request_id)?;
        let input = Input {
            source: source.into(),
            cover: attachments.cover.clone(),
            lyric: attachments.lyric.clone(),
            cue: attachments.cue.clone(),
        };
        if std::iter::once(&input.source)
            .chain(
                [&input.cover, &input.lyric, &input.cue]
                    .into_iter()
                    .flatten(),
            )
            .any(|path| !path.is_absolute())
        {
            return Err(Error::InvalidInput("planned cloud paths must be absolute"));
        }
        let id = hex::encode(
            &Sha256::digest(format!("cumulus:cloud-upload-create:v1:{request_id}"))[..16],
        );
        if let Some(task) = self.creation_receipt(&id, &input)? {
            return Ok((task, false));
        }
        let _lock = self.lock(&id)?;
        if let Some(task) = self.creation_receipt(&id, &input)? {
            return Ok((task, false));
        }
        if !allow_new {
            return Err(Error::State("cloud capacity exhausted"));
        }
        let task = self.prepare_task(source, attachments, id.clone()).await?;
        self.session.with_token(&self.token, |_| {
            self.store.transaction(|tx| {
                if tx.get(&self.owner, JOBS, &id)?.is_some()
                    || tx.get(&self.owner, RECEIPTS, &id)?.is_some()
                {
                    return Err(Error::State("cloud creation receipt conflict"));
                }
                tx.put(&self.owner, JOBS, &id, &serde_json::to_value(&task)?)?;
                tx.put(&self.owner, RECEIPTS, &id, &serde_json::to_value(&input)?)
            })
        })?;
        Ok((self.view(&task)?, true))
    }
    fn creation_receipt(&self, id: &str, input: &Input) -> Result<Option<CloudView>> {
        let recorded = self.session.with_token(&self.token, |_| {
            self.store
                .get(&self.owner, RECEIPTS, id)?
                .map(serde_json::from_value::<Input>)
                .transpose()
                .map_err(Into::into)
        })?;
        match recorded {
            Some(recorded) if &recorded == input => self.get(id).map(Some),
            Some(_) => Err(Error::InvalidInput("cloud creation selection changed")),
            None => Ok(None),
        }
    }
}
