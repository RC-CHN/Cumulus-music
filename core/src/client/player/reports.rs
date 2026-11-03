use super::*;
use crate::lyrics::{
    LyricTarget,
    reports::{CorrectionType, Issue, Phase, Record, Reports, Target},
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricReportView {
    pub epoch: String,
    pub id: String,
    pub revision: String,
    pub title: String,
    pub options: Vec<CorrectionType>,
    pub selected: Option<String>,
    pub content: String,
    pub phase: Phase,
    pub issue: Option<Issue>,
}
fn project(epoch: &str, record: Record) -> LyricReportView {
    LyricReportView {
        epoch: epoch.into(),
        id: record.id,
        revision: record.revision,
        title: record.target.title,
        options: record.options,
        selected: record.selected,
        content: record.content,
        phase: record.phase,
        issue: record.issue,
    }
}
impl MusicClient {
    fn reports(&self, epoch: &str) -> Result<Reports> {
        Reports::new(self.session.clone(), self.player_token(epoch)?)
    }
    pub async fn lyric_report_open(&self, epoch: &str, play_id: &str) -> Result<LyricReportView> {
        let super::lyrics::LyricContext {
            token,
            service,
            target,
            ..
        } = self.lyric_context(epoch, play_id).await?;
        self.session.with_token(&token, |_| Ok(()))?;
        let state = self.player.controller().await?.snapshot();
        let selected = state.selected.as_ref().ok_or(Error::StaleResponse)?;
        if !selected.resource["voice_id"].is_null()
            || !selected.resource["song"]["voice_id"].is_null()
        {
            return Err(Error::InvalidInput("podcasts do not accept lyric reports"));
        }
        let song_id = match target {
            LyricTarget::Song(id) | LyricTarget::Cloud(id) => id,
            LyricTarget::Local(_) => {
                let id = selected.resource["local_id"]
                    .as_str()
                    .ok_or(Error::StaleResponse)?
                    .to_owned();
                let context = self.local.context().await?;
                let session = self.session.clone();
                let expected = token.clone();
                tokio::task::spawn_blocking(move || {
                    context
                        .library
                        .online_mappings(&session, &expected, std::slice::from_ref(&id))?
                        .remove(&id)
                        .map(|v| v.song_id)
                        .ok_or(Error::InvalidInput(
                            "local lyric report requires an online match",
                        ))
                        .and_then(|id| {
                            session.check_generation(&expected)?;
                            Ok(id)
                        })
                })
                .await
                .map_err(|_| Error::State("lyric report mapping worker stopped"))??
            }
        };
        // Reuses M370's parsed selection. In particular a visible YRC layer
        // does not substitute yrc.version for the selected lrc.version.
        self.player_lyrics(epoch, play_id).await?;
        let parsed = service.snapshot()?.lyric;
        let current = self.player.controller().await?.snapshot();
        if current.account != token || current.play_id.as_deref() != Some(play_id) {
            return Err(Error::StaleResponse);
        }
        let title = track(&selected.resource)?.name;
        let report = self.reports(epoch)?.open(Target {
            song_id,
            title,
            lyric_version: parsed.current_used_lyric_version,
        })?;
        Ok(project(epoch, report))
    }
    pub fn lyric_report_read(&self, epoch: &str, id: &str) -> Result<LyricReportView> {
        Ok(project(epoch, self.reports(epoch)?.read(id)?))
    }
    pub async fn lyric_report_types(
        &self,
        epoch: &str,
        id: &str,
        revision: &str,
    ) -> Result<LyricReportView> {
        Ok(project(
            epoch,
            self.reports(epoch)?.types(id, revision).await?,
        ))
    }
    pub async fn lyric_report_submit(
        &self,
        epoch: &str,
        id: &str,
        revision: &str,
        key: &str,
        content: &str,
    ) -> Result<LyricReportView> {
        Ok(project(
            epoch,
            self.reports(epoch)?
                .submit(id, revision, key, content)
                .await?,
        ))
    }
    pub fn lyric_report_abandon(
        &self,
        epoch: &str,
        id: &str,
        revision: &str,
    ) -> Result<LyricReportView> {
        Ok(project(epoch, self.reports(epoch)?.abandon(id, revision)?))
    }
}
