use super::*;
use crate::listen_together::{Room, TogetherApi};
use sha2::{Digest, Sha256};
use std::time::Duration;

/// Only an observed two-member room has an unambiguous relation. The
/// renderer receives a comparison key, never the two IDs used on the wire.
pub(super) fn relation(room: &Room, owner: &str) -> Option<(String, String)> {
    if room.members.len() != 2 || !room.members.iter().any(|member| member.user_id == owner) {
        return None;
    }
    let peer = &room
        .members
        .iter()
        .find(|member| member.user_id != owner)?
        .user_id;
    let mut digest = Sha256::new();
    for field in [&room.room_id, owner, peer] {
        digest.update((field.len() as u64).to_le_bytes());
        digest.update(field);
    }
    Some((hex::encode(digest.finalize()), peer.clone()))
}

impl MusicClient {
    pub async fn together_statistics(
        &self,
        epoch: &str,
        relation_key: &str,
    ) -> Result<AccountData<TogetherStatisticsView>> {
        let token = self.together_token(epoch)?;
        let owner = token.uid.as_deref().ok_or(Error::State("no account"))?;
        let service = TogetherService::new(self.session.clone())?;
        let checked = || {
            let state = service.state_for(&token)?;
            let room = state
                .room
                .filter(|_| state.end_notice.is_none())
                .ok_or(Error::State("together room changed; refresh first"))?;
            let (_, peer) = relation(&room, owner)
                .filter(|(key, _)| key == relation_key)
                .ok_or(Error::State("together room members changed; refresh first"))?;
            if service.operations_for(&token)?.iter().any(|operation| {
                operation.phase.pending()
                    && matches!(&operation.intent,
                        crate::listen_together::WriteIntent::End { room_id, .. }
                        if room_id == &room.room_id)
            }) {
                return Err(Error::MutationUncertain);
            }
            Ok::<_, Error>((room.room_id, peer))
        };
        let (room_id, peer) = checked()?;
        let users = [owner.to_owned(), peer.clone()];
        let response = tokio::time::timeout(
            Duration::from_secs(8),
            self.session.run_for(&token, |api, _| async {
                TogetherApi::new(api).statistics(&room_id, &users).await
            }),
        )
        .await
        .map_err(|_| Error::Transport("together statistics timed out".into()))??;
        if checked()? != (room_id, peer) {
            return Err(Error::State("together room members changed; refresh first"));
        }
        let count = |key: &str| match &response[key] {
            serde_json::Value::Null => Ok(0),
            value => value
                .as_u64()
                .filter(|number| *number <= 9_007_199_254_740_991)
                .ok_or(Error::Response("invalid together statistic")),
        };
        Ok(AccountData {
            epoch: epoch.into(),
            value: TogetherStatisticsView {
                listen_count: count("listenCount")?,
                total_seconds: count("totalConnectionTime")?,
            },
        })
    }
}
