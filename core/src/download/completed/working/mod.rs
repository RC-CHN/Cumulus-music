mod commit;
mod prepare;
use super::*;
use crate::{
    download::{
        directory::paths::mapped,
        file_identity::{Identity, identity},
        offline::OfflineRecord,
        service::{DownloadDestination, DownloadStatus, files as task_files},
        transfer::TransferJob,
    },
    library::{identity::fingerprint, relocate_download},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs as stdfs,
    path::{Path, PathBuf},
};

const RAW: &str = "media_transfers_v1";
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Working {
    after: DownloadTask,
    raw_before: Option<TransferJob>,
    raw_after: Option<TransferJob>,
    new_work_identity: Identity,
    pub old_work_retained: bool,
}
pub(in crate::download) struct Input {
    pub role: String,
    pub source: PathBuf,
    pub target: PathBuf,
}
impl CompletedDownloads {
    /// Read-only inspection, shared by batch preflight and the durable child.
    pub(in crate::download) fn working_inputs(
        &self,
        task: &DownloadTask,
        source: &Path,
        target: &Path,
    ) -> Result<Vec<Input>> {
        task_files::work(task)?;
        let mut owned = BTreeMap::new();
        for artifact in [&task.prepared, &task.cover, &task.lyric]
            .into_iter()
            .flatten()
        {
            if artifact.path.parent() != Some(task.work.as_path()) {
                return Err(Error::State("artifact escapes work directory"));
            }
            owned.insert(
                artifact.path.clone(),
                (artifact.identity.clone(), artifact.integrity.clone()),
            );
        }
        if let Some(raw) = self.raw(&task.id)? {
            if raw.id != task.id
                || raw.target != task.work.join("raw")
                || raw.part != task.work.join(format!(".cumulus-{}.part", task.id))
                || raw.parent_token != task.work_identity
            {
                return Err(Error::State("raw transfer escapes its task"));
            }
            owned.insert(raw.target, (raw.part_token.clone(), None));
            owned.insert(raw.part, (raw.part_token, None));
        }
        let mut paths: Vec<PathBuf> = stdfs::read_dir(&task.work)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<_>>()?;
        paths.sort();
        let mut out = Vec::new();
        for path in paths {
            let (expected, integrity) = owned
                .get(&path)
                .ok_or(Error::State("unknown file in download work directory"))?;
            let facts = fs::facts(&path)?;
            if facts.identity != *expected
                || integrity.as_ref().is_some_and(|v| *v != facts.integrity)
            {
                return Err(Error::State("working artifact changed"));
            }
            out.push(Input {
                role: "work".into(),
                target: mapped(&path, source, target)?,
                source: path,
            });
        }
        if let Some(path) = &task.target {
            for (role, path, artifact) in [
                ("audio", path.clone(), &task.prepared),
                ("lyric", path.with_extension("lrc"), &task.lyric),
            ] {
                match stdfs::symlink_metadata(&path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                    Ok(_) => (),
                }
                // Unmanaged .lrc files are not moved or removed.
                if role == "lyric" && artifact.is_none() {
                    continue;
                }
                if !artifact
                    .as_ref()
                    .is_some_and(|a| a.matches(&path).unwrap_or(false))
                {
                    if role == "lyric" {
                        continue;
                    }
                    return Err(Error::State(
                        "unfinished target is not its published artifact",
                    ));
                }
                let facts = fs::facts(&path)?;
                if artifact
                    .as_ref()
                    .and_then(|a| a.integrity.as_ref())
                    .is_some_and(|i| *i != facts.integrity)
                {
                    return Err(Error::State("published working artifact changed"));
                }
                out.push(Input {
                    role: role.into(),
                    target: mapped(&path, source, target)?,
                    source: path,
                });
            }
        }
        Ok(out)
    }
    fn raw(&self, id: &str) -> Result<Option<TransferJob>> {
        self.store
            .get(&self.owner, RAW, id)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }
}
