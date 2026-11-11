//! Naming and organization are independent local settings, even before the first root.
use super::*;

pub(super) const LAYOUT_SEED: &str = "layout_seed";

impl DownloadDirectory {
    pub fn layout(&self) -> Result<DownloadLayoutSettings> {
        self.files.guarded(&self.files.token()?, || {
            self.read_layout(self.read_settings()?.as_ref())
        })
    }

    pub fn change_layout(&self, change: DownloadLayoutChange) -> Result<DownloadLayoutSettings> {
        if change.value > 2 {
            return Err(Error::InvalidInput("download layout must be 0..2"));
        }
        let token = self.files.token()?;
        let _lock = gate::lock(&self.files.store, &self.files.owner)?;
        self.files.guarded(&token, || {
            gate::require(&self.files.store, &self.files.owner, None)?;
            let settings = self.read_settings()?;
            let mut layout = self.read_layout(settings.as_ref())?;
            if layout.revision != change.expected_revision {
                return Err(Error::StaleResponse);
            }
            let selected = match change.field {
                DownloadLayoutField::PathType => &mut layout.path_type,
                DownloadLayoutField::FileType => &mut layout.file_type,
            };
            if *selected == change.value {
                return Ok(layout);
            }
            *selected = change.value;
            let revision = new_id();
            layout.revision = Some(revision.clone());
            let (key, value) = if let Some(mut settings) = settings {
                settings.path_type = layout.path_type;
                settings.file_type = layout.file_type;
                settings.revision = revision;
                ("settings", serde_json::to_value(settings)?)
            } else {
                (LAYOUT_SEED, serde_json::to_value(&layout)?)
            };
            self.files
                .store
                .put(&self.files.owner, SETTINGS, key, &value)?;
            Ok(layout)
        })
    }

    /// Root setup from a download prompt inherits the current naming settings.
    pub fn configure_for_download(
        &self,
        root: &Path,
        expected_revision: Option<&str>,
    ) -> Result<DirectorySettings> {
        let token = self.files.token()?;
        let _lock = gate::lock(&self.files.store, &self.files.owner)?;
        let (before, layout) = self.choice_settings(&token)?;
        if layout.revision.as_deref() != expected_revision {
            return Err(Error::StaleResponse);
        }
        if let Some(settings) = before.filter(|settings| settings.root == root) {
            return Ok(settings);
        }
        self.configure_locked(root, layout.path_type, layout.file_type, None, &token)
    }

    pub(super) fn read_layout(
        &self,
        settings: Option<&DirectorySettings>,
    ) -> Result<DownloadLayoutSettings> {
        if let Some(settings) = settings {
            return Ok(DownloadLayoutSettings {
                path_type: settings.path_type,
                file_type: settings.file_type,
                revision: Some(settings.revision.clone()),
            });
        }
        decode_seed(
            self.files
                .store
                .get(&self.files.owner, SETTINGS, LAYOUT_SEED)?,
        )
    }
}

pub(super) fn decode_seed(value: Option<serde_json::Value>) -> Result<DownloadLayoutSettings> {
    Ok(value
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default())
}
