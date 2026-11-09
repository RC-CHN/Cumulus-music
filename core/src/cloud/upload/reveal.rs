//! Native-only selection of the verified original for one cloud upload task.
use super::{CloudUploads, Source};
use crate::{
    Error, Result,
    download::{
        completed::{fs as verified, verify_reveal_bytes},
        file_identity::{Identity, identity},
    },
    media::integrity::FileIntegrity,
};
use std::{fs, path::Path};

pub struct CloudUploadSourceReveal {
    uploads: CloudUploads,
    task_id: String,
    source: Source,
    file: verified::Facts,
    parent: Identity,
}

impl CloudUploadSourceReveal {
    pub fn path(&self) -> &Path {
        &self.source.path
    }

    /// The shell accepts a path; recheck the task and pinned identities just
    /// before handoff. A removed task or retired session cannot reveal a file.
    pub fn check_current(&self) -> Result<()> {
        if self.uploads.read(&self.task_id)?.source != self.source {
            return Err(Error::StaleResponse);
        }
        verified::parent(self.path(), &self.parent)?;
        verified::unchanged(self.path(), &self.file)?;
        self.uploads.guard()
    }
}

impl CloudUploads {
    pub(crate) fn task_source_reveal(&self, id: &str) -> Result<CloudUploadSourceReveal> {
        let source = self.read(id)?.source;
        let path = &source.path;
        if path.canonicalize()? != *path
            || !path
                .ancestors()
                .all(|p| fs::symlink_metadata(p).is_ok_and(|m| !m.file_type().is_symlink()))
        {
            return Err(Error::State("cloud upload source unavailable"));
        }
        let parent = identity(
            path.parent()
                .ok_or(Error::State("cloud upload directory missing"))?,
        )?;
        let integrity = FileIntegrity {
            bytes: source.bytes,
            sha256: source.sha256.clone(),
        };
        let file = verified::recorded_facts(path, integrity.clone())?;
        verify_reveal_bytes(path, &integrity, &file)
            .map_err(|_| Error::State("cloud upload source unavailable"))?;
        let reveal = CloudUploadSourceReveal {
            uploads: self.clone(),
            task_id: id.to_owned(),
            source,
            file,
            parent,
        };
        reveal.check_current()?;
        Ok(reveal)
    }
}
