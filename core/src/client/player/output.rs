use super::{MusicClient, PlayerHost};
use crate::{
    Error, Result,
    client::dto::AccountData,
    playback::{engine::OutputDevice, output::*},
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputEngineView {
    kind: OutputKind,
    revision: Option<String>,
    phase: OutputPhase,
    issue: Option<OutputIssue>,
    configured: Option<String>,
    driver: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputView {
    pub revision: Option<String>,
    pub selected: OutputSelection,
    pub damaged: bool,
    pub supported: Option<bool>,
    pub devices: Vec<OutputDevice>,
    pub engines: Vec<OutputEngineView>,
}
impl From<OutputSnapshot> for OutputView {
    fn from(value: OutputSnapshot) -> Self {
        Self {
            revision: value.preferences.as_ref().map(|p| p.revision.to_string()),
            selected: value.preferences.map(|p| p.selection).unwrap_or_default(),
            damaged: value.damaged,
            supported: value.supported,
            devices: value.devices,
            engines: value
                .engines
                .into_iter()
                .map(|e| OutputEngineView {
                    kind: e.kind,
                    revision: e.revision.map(|r| r.to_string()),
                    phase: e.phase,
                    issue: e.issue,
                    configured: e.configured,
                    driver: e.driver,
                })
                .collect(),
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OutputCommand {
    Select {
        expected_revision: String,
        id: String,
    },
    Retry {
        expected_revision: String,
    },
    Repair,
}
fn revision(value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .ok()
        .filter(|r| r.to_string() == value)
        .ok_or(Error::InvalidInput("invalid audio output revision"))
}
impl PlayerHost {
    async fn discover_output(&self) -> Result<()> {
        let inspector = self
            .inspector
            .get_or_try_init(|| async {
                let factory = self.factory.clone();
                tokio::task::spawn_blocking(move || factory().map(|e| Arc::new(Mutex::new(e))))
                    .await
                    .map_err(|_| Error::State("audio device discovery initialization failed"))?
            })
            .await?
            .clone();
        let devices = tokio::task::spawn_blocking(move || {
            inspector
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .output_devices()
        })
        .await
        .map_err(|_| Error::State("audio device discovery failed"))??;
        self.output.catalog(devices)
    }
}
impl MusicClient {
    pub async fn output_settings(
        &self,
        epoch: &str,
        refresh: bool,
    ) -> Result<AccountData<OutputView>> {
        let token = self.player_token(epoch)?;
        if refresh {
            self.player.discover_output().await?;
        }
        self.session.with_local_owner(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: self.output.snapshot()?.into(),
            })
        })
    }
    pub fn output_command(
        &self,
        epoch: &str,
        command: OutputCommand,
    ) -> Result<AccountData<OutputView>> {
        let token = self.player_token(epoch)?;
        self.session.with_local_owner(&token, |_| {
            match command {
                OutputCommand::Select {
                    expected_revision,
                    id,
                } => self.output.select(revision(&expected_revision)?, &id)?,
                OutputCommand::Retry { expected_revision } => {
                    self.output.retry(revision(&expected_revision)?)?
                }
                OutputCommand::Repair => self.output.repair()?,
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value: self.output.snapshot()?.into(),
            })
        })
    }
}
