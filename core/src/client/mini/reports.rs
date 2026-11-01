use super::*;
impl MusicClient {
    pub async fn mini_lyric_report_open(
        &self,
        displayed: &MiniSnapshot,
    ) -> Result<super::super::dto::LyricReportView> {
        displayed.audio(self)?;
        if !displayed.view.can_lyric_report {
            return Err(Error::InvalidInput(
                "lyric report unavailable for this media",
            ));
        }
        let id = displayed
            .view
            .selection
            .audio_play_id
            .as_deref()
            .ok_or(Error::StaleResponse)?;
        let report = self.lyric_report_open(&displayed.view.epoch, id).await?;
        displayed.check(self)?;
        Ok(report)
    }
}
