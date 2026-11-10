//! Ordinary episode quality selection (M/3085, M/71, M/754 and M/334).
use super::{Context, DownloadFailure, DownloadResult, Host, MusicClient};
use crate::{
    Error, Result,
    account::SessionToken,
    download::service::{DownloadOptions, OutputContainer},
    membership::Quality,
    podcast::PodcastApi,
    protocol::api::{Api, identifier},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio::time::Instant;

const MUSICS: &str = "/api/dj/program/song/musics";
const MAX_SELECTION: usize = 5000;
const MAX_MUSICS: usize = 128;
const READ_TIMEOUT: Duration = Duration::from_secs(20);
const CHOICE_LIFETIME: Duration = Duration::from_secs(600);
const QUALITIES: [(Quality, &str); 3] = [
    (Quality::Standard, "普通"),
    (Quality::Higher, "较高"),
    (Quality::ExHigh, "极高"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceDownloadConfirmation {
    receipt: String,
    quality: Quality,
    tags: bool,
    cover: bool,
}

#[derive(Serialize)]
pub struct VoiceDownloadChoice {
    quality: Quality,
    label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceDownloadPreparationState {
    Ready,
    Restricted,
    Unavailable,
}

#[derive(Serialize)]
pub struct VoiceDownloadPreparation {
    pub epoch: String,
    pub state: VoiceDownloadPreparationState,
    pub receipt: Option<String>,
    pub choices: Vec<VoiceDownloadChoice>,
}

#[derive(Default)]
pub(super) struct Preparation {
    attempt: u64,
    plan: Option<Plan>,
}

struct Plan {
    token: SessionToken,
    api: Arc<dyn Api>,
    receipt: String,
    expires: Instant,
    voices: Vec<String>,
    qualities: Vec<Quality>,
}

fn voice_ids(values: Vec<String>) -> Result<Vec<String>> {
    if values.is_empty() || values.len() > MAX_SELECTION {
        return Err(Error::InvalidInput(
            "invalid program download selection size",
        ));
    }
    let mut unique = BTreeSet::new();
    let mut ids = Vec::new();
    for value in values {
        if value.len() > 32 {
            return Err(Error::InvalidInput("invalid program download identifier"));
        }
        let id = identifier(&json!(value))?;
        if unique.insert(id.clone()) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn batch_choices() -> Vec<VoiceDownloadChoice> {
    QUALITIES
        .into_iter()
        .map(|(quality, name)| VoiceDownloadChoice {
            quality,
            label: format!("{name}（{}kbit/s）", quality.bitrate() / 1000),
        })
        .collect()
}

fn size_label(bytes: u64) -> String {
    // M/32.d: KiB rounded to an integer, the other supported units to one decimal.
    if bytes == 0 {
        return "0b".into();
    }
    let decimal = |unit: u64, label| {
        let tenths = (bytes * 10 + unit / 2) / unit;
        format!("{}.{}{label}", tenths / 10, tenths % 10)
    };
    match bytes {
        ..=1023 => format!("{bytes}.0B"),
        1024..=1_048_575 => format!("{}KB", (bytes + 512) / 1024),
        1_048_576..=1_073_741_823 => decimal(1 << 20, "M"),
        _ => decimal(1 << 30, "G"),
    }
}

fn single_choices(value: &Value) -> Vec<VoiceDownloadChoice> {
    // M/334.g reads the first program of the single-seed response. No music
    // id in the response establishes the episode's main-track identity.
    let Some(rows) = value["programs"][0]["musics"]
        .as_array()
        .filter(|rows| rows.len() <= MAX_MUSICS)
    else {
        return Vec::new();
    };
    let mut music: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            let bitrate = row["bitrate"].as_u64().filter(|n| *n > 0)?;
            // M/32.d presents a missing/null size as "0b". This does not
            // turn a missing or nonpositive bitrate into an offered choice.
            let bytes = if row["size"].is_null() {
                0
            } else {
                row["size"].as_u64().filter(|n| *n < 1 << 40)?
            };
            Some((bitrate, bytes))
        })
        .collect();
    music.sort_by_key(|&(bitrate, _)| bitrate);
    let mut choices: Vec<VoiceDownloadChoice> = Vec::new();
    for (bitrate, size) in music {
        let (quality, name) = QUALITIES[match bitrate {
            ..=128_000 => 0,
            128_001..=192_000 => 1,
            _ => 2,
        }];
        if choices.last().is_none_or(|old| old.quality != quality) {
            choices.push(VoiceDownloadChoice {
                quality,
                label: format!("{name}（{}）", size_label(size)),
            });
        }
    }
    choices
}

fn check_attempt(host: &Host, attempt: u64) -> Result<()> {
    let state = host.voice_quality.lock().map_err(|_| Error::LockPoisoned)?;
    if state.attempt != attempt {
        return Err(Error::StaleResponse);
    }
    Ok(())
}

impl MusicClient {
    pub async fn download_voice_prepare(
        &self,
        epoch: &str,
        voices: Vec<String>,
    ) -> Result<VoiceDownloadPreparation> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        let voices = voice_ids(voices)?;
        let api = self.session.native_api(&token)?;
        let host = &self.downloads.inner;
        let attempt = {
            let mut state = host.voice_quality.lock().map_err(|_| Error::LockPoisoned)?;
            self.session.require_api(&token, &api)?;
            state.attempt = state
                .attempt
                .checked_add(1)
                .ok_or(Error::State("program download preparation exhausted"))?;
            state.plan = None;
            state.attempt
        };
        let mut presentation = VoiceDownloadPreparation {
            epoch: epoch.into(),
            state: VoiceDownloadPreparationState::Unavailable,
            receipt: None,
            choices: Vec::new(),
        };
        if let [id] = voices.as_slice() {
            let expected = &token;
            let result = tokio::time::timeout(
                READ_TIMEOUT,
                self.session.run_for(expected, |api, _| async move {
                    let voice = json!(id);
                    let rights = PodcastApi::new(api.clone())
                        .download_permission(std::slice::from_ref(&voice))
                        .await?;
                    self.session.require_api(expected, &api)?;
                    check_attempt(host, attempt)?;
                    if rights.allowed.as_slice() != [id.as_str()] {
                        return Ok(None);
                    }
                    let response = api
                        .call(MUSICS, json!({"ids":[voice]}), false, &[200])
                        .await?;
                    Ok(Some(single_choices(&response)))
                }),
            )
            .await
            .map_err(|_| Error::Transport("program download quality read timed out".into()))??;
            match result {
                Some(choices) => presentation.choices = choices,
                None => presentation.state = VoiceDownloadPreparationState::Restricted,
            }
        } else {
            presentation.choices = batch_choices();
        }

        let mut state = host.voice_quality.lock().map_err(|_| Error::LockPoisoned)?;
        self.session.require_api(&token, &api)?;
        if state.attempt != attempt {
            return Err(Error::StaleResponse);
        }
        if !presentation.choices.is_empty() {
            let receipt = uuid::Uuid::new_v4().simple().to_string();
            state.plan = Some(Plan {
                token,
                api,
                receipt: receipt.clone(),
                expires: Instant::now() + CHOICE_LIFETIME,
                voices,
                qualities: presentation.choices.iter().map(|row| row.quality).collect(),
            });
            presentation.receipt = Some(receipt);
            presentation.state = VoiceDownloadPreparationState::Ready;
        }
        Ok(presentation)
    }

    pub fn download_voice_discard(&self, epoch: &str, receipt: &str) -> Result<()> {
        let host = &self.downloads.inner;
        let mut state = host.voice_quality.lock().map_err(|_| Error::LockPoisoned)?;
        if state.plan.as_ref().is_some_and(|plan| {
            plan.receipt == receipt && plan.token.generation.to_string() == epoch
        }) {
            state.plan = None;
        }
        Ok(())
    }
}

impl Host {
    pub(super) async fn confirm_voice_download(
        &self,
        context: Context,
        confirmation: VoiceDownloadConfirmation,
    ) -> Result<DownloadResult> {
        let plan = {
            let mut state = self.voice_quality.lock().map_err(|_| Error::LockPoisoned)?;
            let plan = state
                .plan
                .as_ref()
                .filter(|plan| {
                    plan.token == context.token
                        && plan.receipt == confirmation.receipt
                        && Instant::now() < plan.expires
                })
                .ok_or(Error::StaleResponse)?;
            context.session.require_api(&plan.token, &plan.api)?;
            if !plan.qualities.contains(&confirmation.quality) {
                return Err(Error::InvalidInput(
                    "program download quality was not offered",
                ));
            }
            state.plan.take().expect("verified preparation")
        };
        let options = DownloadOptions {
            quality: confirmation.quality,
            tags: confirmation.tags,
            cover: confirmation.cover,
            lyrics: false,
            container: OutputContainer::Audio,
        };
        let mut result = DownloadResult {
            epoch: plan.token.generation.to_string(),
            ..Default::default()
        };
        for id in plan.voices {
            context.session.require_api(&plan.token, &plan.api)?;
            match context
                .service
                .create_voice_default(&json!(id), options.clone())
                .await
            {
                Ok(task) => result.accepted.push(task.id),
                Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                Err(_) => result.failed.push(DownloadFailure {
                    id,
                    message: "节目下载未能创建，请检查权限、格式选项和下载目录",
                }),
            }
            self.kick(&context)?;
        }
        context.guard()?;
        Ok(result)
    }
}
