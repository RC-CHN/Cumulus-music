//! Ordinary episode quality selection (M/3085, M/71, M/754 and M/334).
use super::{Context, DownloadFailure, DownloadResult, Host, MusicClient};
use crate::client::{
    dto::{
        HistoryRowMenuTarget, HistorySelectionScope, PodcastLikedPageTarget,
        PodcastLikedProgramTarget, PodcastPurchasedTarget,
    },
    podcast::{
        liked_menu::{liked_playable_program, lock_liked_download_voice},
        liked_page::LikedPageAdmissionLease,
        purchased::{lock_purchased_download_voice, purchased_download_voice},
    },
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    download::service::{DownloadOptions, OutputContainer, ScopedTaskAdmission, ScopedTaskLease},
    membership::Quality,
    podcast::PodcastApi,
    protocol::api::{Api, identifier},
    storage::{OperationLock, StateTransaction},
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
    AlreadyAvailable,
}

#[derive(Serialize)]
pub struct VoiceDownloadPreparation {
    pub epoch: String,
    pub state: VoiceDownloadPreparationState,
    pub receipt: Option<String>,
    pub choices: Vec<VoiceDownloadChoice>,
    #[serde(rename = "admittedCount", skip_serializing_if = "Option::is_none")]
    pub admitted_count: Option<usize>,
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
    scope: Option<VoiceScope>,
    qualities: Vec<Quality>,
}

enum VoiceScope {
    Legacy(LegacyVoiceScope),
    LikedPage(PodcastLikedPageTarget),
    History(HistorySelectionScope),
    HistoryRow(HistoryRowMenuTarget),
}
impl VoiceScope {
    fn id(&self, client: &MusicClient, token: &SessionToken) -> Result<String> {
        match self {
            Self::Legacy(source) => source.id(&client.session, token),
            Self::LikedPage(target) => client.liked_page_download_id(token, target),
            Self::History(_) | Self::HistoryRow(_) => {
                Err(Error::InvalidInput("history requires scoped voice rows"))
            }
        }
    }
    fn history_revision(&self) -> Option<&str> {
        match self {
            Self::History(target) => Some(&target.revision),
            Self::HistoryRow(target) => Some(&target.revision),
            _ => None,
        }
    }
    fn check(&self, client: &MusicClient, token: &SessionToken) -> Result<()> {
        if let Some(revision) = self.history_revision() {
            client.verify_history_voice_download(token, revision)
        } else {
            self.id(client, token).map(|_| ())
        }
    }
}

enum LegacyVoiceScope {
    Purchased(PodcastPurchasedTarget),
    Liked(PodcastLikedProgramTarget),
}
impl LegacyVoiceScope {
    fn id(&self, session: &Arc<AccountSession>, token: &SessionToken) -> Result<String> {
        match self {
            Self::Purchased(source) => purchased_download_voice(session, token, source),
            Self::Liked(source) => Ok(liked_playable_program(session, token, source)?.id),
        }
    }
    fn lock(&self, session: &Arc<AccountSession>, token: &SessionToken) -> Result<OperationLock> {
        match self {
            Self::Purchased(source) => lock_purchased_download_voice(session, token, source),
            Self::Liked(source) => lock_liked_download_voice(session, token, source),
        }
    }
}

struct LikedPageAdmission<'a> {
    client: &'a MusicClient,
    token: &'a SessionToken,
    target: &'a PodcastLikedPageTarget,
}

impl ScopedTaskAdmission for LikedPageAdmission<'_> {
    fn acquire(&self, song_id: &str) -> Result<Box<dyn ScopedTaskLease + '_>> {
        Ok(Box::new(self.client.lock_liked_page_admission(
            self.token,
            self.target,
            song_id,
        )?))
    }
}

impl ScopedTaskLease for LikedPageAdmissionLease<'_> {
    fn verify(&self, tx: &StateTransaction<'_>, owner: &str) -> Result<()> {
        self.verify_generation(tx, owner)
    }
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
        self.prepare_voice_quality(epoch, voices, None).await
    }

    pub async fn download_purchased_voice_prepare(
        &self,
        epoch: &str,
        target: PodcastPurchasedTarget,
    ) -> Result<VoiceDownloadPreparation> {
        self.prepare_voice_quality(
            epoch,
            Vec::new(),
            Some(VoiceScope::Legacy(LegacyVoiceScope::Purchased(target))),
        )
        .await
    }

    pub async fn download_liked_voice_prepare(
        &self,
        epoch: &str,
        target: PodcastLikedProgramTarget,
    ) -> Result<VoiceDownloadPreparation> {
        self.prepare_voice_quality(
            epoch,
            Vec::new(),
            Some(VoiceScope::Legacy(LegacyVoiceScope::Liked(target))),
        )
        .await
    }

    pub async fn download_liked_page_voice_prepare(
        &self,
        epoch: &str,
        target: PodcastLikedPageTarget,
    ) -> Result<VoiceDownloadPreparation> {
        self.prepare_voice_quality(epoch, Vec::new(), Some(VoiceScope::LikedPage(target)))
            .await
    }

    pub async fn download_history_voice_prepare(
        &self,
        epoch: &str,
        target: HistorySelectionScope,
    ) -> Result<VoiceDownloadPreparation> {
        self.prepare_voice_quality(epoch, Vec::new(), Some(VoiceScope::History(target)))
            .await
    }

    pub async fn download_history_row_voice_prepare(
        &self,
        epoch: &str,
        target: HistoryRowMenuTarget,
    ) -> Result<VoiceDownloadPreparation> {
        self.prepare_voice_quality(epoch, Vec::new(), Some(VoiceScope::HistoryRow(target)))
            .await
    }

    async fn prepare_voice_quality(
        &self,
        epoch: &str,
        voices: Vec<String>,
        scope: Option<VoiceScope>,
    ) -> Result<VoiceDownloadPreparation> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        let history_rows = match &scope {
            Some(VoiceScope::History(target)) => {
                Some(self.history_voice_download_rows(&token, target, MAX_SELECTION)?)
            }
            Some(VoiceScope::HistoryRow(target)) => {
                Some(self.history_row_voice_download_rows(&token, target, MAX_SELECTION)?)
            }
            _ => None,
        };
        let voices = match &scope {
            Some(VoiceScope::History(_) | VoiceScope::HistoryRow(_)) => Vec::new(),
            Some(source) => vec![source.id(self, &token)?],
            None => voice_ids(voices)?,
        };
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
        let voices = if let Some(rows) = history_rows {
            let voices = self.filter_history_voice_downloads(epoch, rows).await?;
            self.session.require_api(&token, &api)?;
            check_attempt(host, attempt)?;
            scope.as_ref().expect("history scope").check(self, &token)?;
            voices
        } else {
            voices
        };
        let mut presentation = VoiceDownloadPreparation {
            epoch: epoch.into(),
            state: VoiceDownloadPreparationState::Unavailable,
            receipt: None,
            choices: Vec::new(),
            admitted_count: scope
                .as_ref()
                .and_then(VoiceScope::history_revision)
                .map(|_| voices.len()),
        };
        if let [id] = voices.as_slice() {
            let expected = &token;
            let source = scope.as_ref();
            let result = tokio::time::timeout(
                READ_TIMEOUT,
                self.session.run_for(expected, |api, _| async move {
                    let voice = json!(id);
                    let rights = PodcastApi::new(api.clone())
                        .download_permission(std::slice::from_ref(&voice))
                        .await?;
                    self.session.require_api(expected, &api)?;
                    check_attempt(host, attempt)?;
                    if let Some(source) = source {
                        source.check(self, expected)?;
                    }
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
        } else if voices.is_empty() {
            presentation.state = VoiceDownloadPreparationState::AlreadyAvailable;
        } else {
            presentation.choices = batch_choices();
        }

        let mut state = host.voice_quality.lock().map_err(|_| Error::LockPoisoned)?;
        self.session.require_api(&token, &api)?;
        if state.attempt != attempt {
            return Err(Error::StaleResponse);
        }
        if let Some(source) = &scope {
            source.check(self, &token)?;
        }
        if !presentation.choices.is_empty() {
            let receipt = uuid::Uuid::new_v4().simple().to_string();
            state.plan = Some(Plan {
                token,
                api,
                receipt: receipt.clone(),
                expires: Instant::now() + CHOICE_LIFETIME,
                voices,
                scope,
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

    pub(super) async fn confirm_voice_download(
        &self,
        context: Context,
        confirmation: VoiceDownloadConfirmation,
    ) -> Result<DownloadResult> {
        let host = &self.downloads.inner;
        let plan = {
            let mut state = host.voice_quality.lock().map_err(|_| Error::LockPoisoned)?;
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
            if let Some(source) = &plan.scope {
                source.check(self, &plan.token)?;
            }
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
        let mut voices = plan.voices.into_iter();
        while let Some(id) = voices.next() {
            context.session.require_api(&plan.token, &plan.api)?;
            let history = plan.scope.as_ref().and_then(VoiceScope::history_revision);
            if let Some(revision) = history
                && let Err(err) = self.verify_history_voice_download(&plan.token, revision)
            {
                if !matches!(err, Error::QueueChanged) || result.accepted.is_empty() {
                    return Err(err);
                }
                retired_history_voices(&mut result, id, voices);
                break;
            }
            let voice = json!(id);
            let task = match &plan.scope {
                Some(VoiceScope::LikedPage(target)) => {
                    let source = LikedPageAdmission {
                        client: self,
                        token: &plan.token,
                        target,
                    };
                    context
                        .service
                        .create_visit_scoped_voice_default(&voice, options.clone(), &source)
                        .await
                }
                Some(VoiceScope::Legacy(source)) => {
                    context
                        .service
                        .create_scoped_voice_default(&voice, options.clone(), || {
                            source.lock(&context.session, &plan.token)
                        })
                        .await
                }
                Some(VoiceScope::History(_) | VoiceScope::HistoryRow(_)) => {
                    let source = super::history_voice::HistoryVoiceAdmission {
                        revision: history.expect("checked history scope"),
                        voice: &id,
                    };
                    context
                        .service
                        .create_visit_scoped_voice_default(&voice, options.clone(), &source)
                        .await
                }
                None => {
                    context
                        .service
                        .create_voice_default(&voice, options.clone())
                        .await
                }
            };
            match task {
                Ok(task) => result.accepted.push(task.id),
                Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                Err(Error::QueueChanged) if history.is_some() => {
                    let revision = history.expect("checked history scope");
                    match self.verify_history_voice_download(&plan.token, revision) {
                        Err(Error::QueueChanged) if !result.accepted.is_empty() => {
                            retired_history_voices(&mut result, id, voices);
                            break;
                        }
                        Err(error) => return Err(error),
                        Ok(()) => result.failed.push(DownloadFailure {
                            id,
                            message: "该节目已不可用，未创建下载",
                        }),
                    }
                }
                Err(Error::QueueChanged) if plan.scope.is_some() => {
                    return Err(Error::QueueChanged);
                }
                Err(_) => result.failed.push(DownloadFailure {
                    id,
                    message: "节目下载未能创建，请检查权限、格式选项和下载目录",
                }),
            }
            host.kick(&context)?;
        }
        context.guard()?;
        Ok(result)
    }
}

fn retired_history_voices(
    result: &mut DownloadResult,
    current: String,
    remaining: impl Iterator<Item = String>,
) {
    result.failed.extend(
        std::iter::once(current)
            .chain(remaining)
            .map(|id| DownloadFailure {
                id,
                message: "播放记录已更新，未创建该节目下载",
            }),
    );
}
