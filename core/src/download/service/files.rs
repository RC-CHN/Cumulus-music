use super::{DownloadService, DownloadTask};
use crate::{
    Error, Result,
    media::integrity::{FileIntegrity, inspect_file},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

pub(super) use crate::download::file_identity::{Identity, identity};
pub(super) fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(_) => Err(Error::State("download destination already exists")),
    }
}
pub(in crate::download) fn directory(path: &Path, expected: &Identity) -> Result<()> {
    if !fs::symlink_metadata(path)?.is_dir() || identity(path)? != *expected {
        return Err(Error::State("download directory identity changed"));
    }
    Ok(())
}
pub(in crate::download) fn work(task: &DownloadTask) -> Result<()> {
    directory(&task.root, &task.root_identity)?;
    if task.work != task.root.join(format!(".cumulus-{}", task.id)) {
        return Err(Error::State("invalid task working path"));
    }
    directory(&task.work, &task.work_identity)
}
pub(super) fn sync(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
pub(in crate::download) fn target(task: &DownloadTask) -> Result<&Path> {
    directory(&task.root, &task.root_identity)?;
    let target = task
        .target
        .as_deref()
        .ok_or(Error::State("download target not resolved"))?;
    let parent = target
        .parent()
        .ok_or(Error::State("download target has no parent"))?;
    directory(
        parent,
        task.target_parent
            .as_ref()
            .ok_or(Error::State("missing target directory identity"))?,
    )?;
    if !parent.starts_with(&task.root) {
        return Err(Error::State("download target escapes root"));
    }
    Ok(target)
}
pub(in crate::download) fn published(task: &DownloadTask) -> Result<bool> {
    let Some(artifact) = &task.prepared else {
        return Ok(false);
    };
    let target = target(task)?;
    artifact.matches(target)
}

#[derive(Clone, Serialize, Deserialize)]
pub(in crate::download) struct Artifact {
    pub path: PathBuf,
    pub identity: Identity,
    pub integrity: Option<FileIntegrity>,
}
pub(super) enum ArtifactSlot {
    Prepared,
    Cover,
    Lyric,
}
impl DownloadService {
    pub(super) fn reserve_artifact(
        &self,
        task: &mut DownloadTask,
        token: &crate::account::SessionToken,
        slot: ArtifactSlot,
    ) -> Result<()> {
        let (existing, name) = match slot {
            ArtifactSlot::Prepared => (&task.prepared, "prepared"),
            ArtifactSlot::Cover => (&task.cover, "cover"),
            ArtifactSlot::Lyric => (&task.lyric, "lyric"),
        };
        if existing.is_some() {
            return Ok(());
        }
        let artifact = Artifact::reserve(task, name)?;
        match slot {
            ArtifactSlot::Prepared => task.prepared = Some(artifact.clone()),
            ArtifactSlot::Cover => task.cover = Some(artifact.clone()),
            ArtifactSlot::Lyric => task.lyric = Some(artifact.clone()),
        }
        if let Err(error) = self.save_current(token, task) {
            let _ = artifact.remove(task);
            match slot {
                ArtifactSlot::Prepared => task.prepared = None,
                ArtifactSlot::Cover => task.cover = None,
                ArtifactSlot::Lyric => task.lyric = None,
            }
            return Err(error);
        }
        Ok(())
    }
}
impl Artifact {
    pub fn reserve(task: &DownloadTask, name: &str) -> Result<Self> {
        work(task)?;
        let path = task
            .work
            .join(format!("{name}-{}", uuid::Uuid::new_v4().simple()));
        let mut opts = OpenOptions::new();
        opts.create_new(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(&path)?;
        file.sync_all()?;
        let result = Self {
            identity: identity(&path)?,
            path,
            integrity: None,
        };
        result.open(task)?;
        sync(&task.work)?;
        Ok(result)
    }
    pub fn matches(&self, path: &Path) -> Result<bool> {
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
            Ok(m) => Ok(m.is_file() && identity(path)? == self.identity),
        }
    }
    pub fn open(&self, task: &DownloadTask) -> Result<File> {
        work(task)?;
        if self.path.parent() != Some(task.work.as_path()) || !self.matches(&self.path)? {
            return Err(Error::State("task artifact changed identity"));
        }
        let f = OpenOptions::new().read(true).write(true).open(&self.path)?;
        if same_file::Handle::from_file(f.try_clone()?)?
            != same_file::Handle::from_path(&self.path)?
        {
            return Err(Error::State("task artifact changed while opening"));
        }
        Ok(f)
    }
    pub fn verify(&self, task: &DownloadTask) -> Result<()> {
        self.open(task)?;
        if self.integrity.as_ref() != Some(&inspect_file(&self.path)?)
            || !self.matches(&self.path)?
        {
            return Err(Error::Media("task artifact integrity changed"));
        }
        Ok(())
    }
    pub fn remove(&self, task: &DownloadTask) -> Result<()> {
        work(task)?;
        match fs::symlink_metadata(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
            Ok(_) => (),
        }
        self.open(task)?;
        fs::remove_file(&self.path)?;
        sync(&task.work)
    }
}
