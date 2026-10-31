use super::*;
use crate::client::dto::QueueTrack;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiniQueuePage {
    pub epoch: String,
    pub current: Option<String>,
    pub tracks: Vec<QueueTrack>,
    pub offset: usize,
    pub total: usize,
    pub editable: bool,
}
impl MusicClient {
    /// Bounded public projection; the opaque target also fences queue revision.
    pub async fn mini_queue(
        &self,
        displayed: &MiniSnapshot,
        offset: usize,
        limit: usize,
    ) -> Result<MiniQueuePage> {
        if !(1..=100).contains(&limit) {
            return Err(Error::InvalidInput("mini queue page limit must be 1..100"));
        }
        let target = displayed.audio(self)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let (state, queue) = controller.queue_with_state(&displayed.token).await?;
        target.audio(&state)?;
        let page = MiniQueuePage {
            epoch: displayed.view.epoch.clone(),
            current: queue
                .current
                .as_ref()
                .filter(|id| {
                    queue
                        .items
                        .iter()
                        .any(|item| &item.entry_id == *id && item.visible_in_queue())
                })
                .cloned(),
            tracks: queue
                .items
                .iter()
                .filter(|item| item.visible_in_queue())
                .skip(offset)
                .take(limit)
                .map(|i| {
                    Ok(QueueTrack {
                        entry_id: i.entry_id.clone(),
                        track: super::super::player::track(&i.resource)?,
                    })
                })
                .collect::<Result<_>>()?,
            offset,
            total: state.visible_queue_len,
            editable: !state.fm,
        };
        displayed.check(self)?;
        Ok(page)
    }
}
