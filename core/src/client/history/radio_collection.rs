use super::{HistoryRadioCollection, radio_chapter};
use crate::{
    Error, Result,
    client::{
        MusicClient,
        dto::{AccountData, PodcastMutationView},
        podcast::view,
    },
    history::{HistoryKind, HistoryService},
    podcast::{PodcastAction, PodcastService, Radio},
};

pub(super) fn can_collect(radio: &Radio, id: &str, viewer: Option<&str>) -> bool {
    radio.id == id
        && radio.id.bytes().any(|digit| digit != b'0')
        && radio.search_creator_id().is_some_and(|id| {
            !id.is_empty() && viewer.is_none_or(|uid| id != uid.trim_start_matches('0'))
        })
}

impl MusicClient {
    pub async fn history_radio_collection(
        &self,
        epoch: &str,
        kind: HistoryKind,
        revision: &str,
        id: &str,
        action: HistoryRadioCollection,
    ) -> Result<AccountData<PodcastMutationView>> {
        if !radio_chapter::supported(kind) {
            return Err(Error::InvalidInput("not a radio history kind"));
        }
        let token = self.player_token(epoch)?;
        let viewer = self
            .session
            .with_token(&token, |viewer| Ok(viewer.to_owned()))?;
        let api = self.session.native_api(&token)?;
        let credentials = api.search_privilege_credential_revision()?;
        let history = HistoryService::new(self.session.clone())?;
        let current = || {
            self.session.require_api(&token, &api)?;
            if api.search_privilege_credential_revision()? != credentials {
                return Err(Error::CredentialChanged);
            }
            let snapshot = history.snapshot_for(&token, kind)?;
            if snapshot.revision != revision {
                return Err(Error::QueueChanged);
            }
            let row = snapshot
                .entries
                .iter()
                .find(|row| row.id == id)
                .ok_or(Error::QueueChanged)?;
            if !can_collect(&Radio::from_official(&row.resource)?, id, Some(&viewer)) {
                return Err(Error::InvalidInput("history radio cannot be collected"));
            }
            Ok(())
        };
        current()?;
        let service = PodcastService::new(self.session.clone())?;
        let operation = match action {
            HistoryRadioCollection::Toggle { expected } => {
                service
                    .change_scoped_subscription_for(&token, id, expected, &current)
                    .await?
            }
            HistoryRadioCollection::Reconcile => {
                service
                    .reconcile_for(&token, PodcastAction::Subscribe, id)
                    .await?
            }
        };
        current()?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: view::operation(operation),
        })
    }
}
