use super::*;
use crate::{
    catalog::CatalogApi,
    listen_together::{TogetherState, WriteIntent},
    protocol::api::identifier,
};
use std::collections::BTreeMap;
pub(super) async fn project(
    client: &MusicClient,
    token: &SessionToken,
    epoch: &str,
    state: TogetherState,
    refresh: bool,
) -> Result<AccountData<TogetherView>> {
    let ids = state
        .snapshot
        .playlist
        .as_ref()
        .map(|p| p.display.clone())
        .unwrap_or_default();
    if refresh && !ids.is_empty() {
        let songs = client
            .session
            .run_for(token, |api, _| async {
                CatalogApi::new(api)
                    .songs(&ids.iter().map(|id| json!(id)).collect::<Vec<_>>(), 500)
                    .await
            })
            .await?;
        let cards = songs
            .songs
            .into_iter()
            .flatten()
            .map(|s| Ok((identifier(&s["id"])?, s)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        client.session.with_token(token, |uid| {
            client.registry.store().put(
                uid,
                "listen-together-cards-v1",
                "current",
                &serde_json::to_value(cards)?,
            )
        })?;
    }
    let cached: BTreeMap<String, serde_json::Value> = client.session.with_token(token, |uid| {
        Ok(client
            .registry
            .store()
            .get(uid, "listen-together-cards-v1", "current")?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default())
    })?;
    let tracks = ids
        .iter()
        .map(|id| {
            super::super::mapping::track(
                cached
                    .get(id)
                    .unwrap_or(&json!({"id":id,"name":"歌曲信息待刷新"})),
            )
        })
        .collect::<Result<_>>()?;
    let pending = TogetherService::new(client.session.clone())?
        .operations_for(token)?
        .into_iter()
        .filter(|op| op.phase.pending())
        .map(|op| TogetherOperationView {
            id: op.id,
            action: match op.intent {
                WriteIntent::Create { .. } => "create",
                WriteIntent::Accept { .. } => "accept",
                WriteIntent::End { .. } => "end",
                WriteIntent::Queue { .. } => "queue",
                WriteIntent::Play { .. } => "play",
            },
            phase: op.phase,
            acknowledged: op.acknowledged,
        })
        .collect();
    let attached =
        client.together_attached(token, state.room.as_ref().map(|r| r.room_id.as_str()))?;
    let owner = token.uid.as_deref().ok_or(Error::State("no account"))?;
    let native_target_id = if attached {
        let controller = client.player.controller().await?;
        let queue = controller.queue(token).await?;
        selection::shared_selection(&client.session, token, client.registry.store(), &queue)?
            .current
    } else {
        None
    };
    let command = state.snapshot.play_command.as_ref();
    let value = TogetherView {
        room: state
            .room
            .as_ref()
            .map(|r| {
                Ok::<_, Error>(TogetherRoomView {
                    id: r.room_id.clone(),
                    role: r.role_for(owner)?,
                    members: r
                        .members
                        .iter()
                        .map(|m| TogetherMemberView {
                            nickname: m.nickname.clone(),
                            avatar_url: super::super::mapping::artwork(&json!(m.avatar_url)),
                            is_self: m.user_id == owner,
                        })
                        .collect(),
                })
            })
            .transpose()?,
        tracks,
        mode: state
            .snapshot
            .playlist
            .as_ref()
            .map(|p| p.mode)
            .unwrap_or_default(),
        target_id: command.map(|c| c.target_song_id.clone()),
        progress_ms: command.map(|c| c.progress),
        playing: command.is_some_and(|c| c.play_status == crate::listen_together::PlayState::Play),
        attached,
        realtime_ready: false,
        connection: client.together_connection(token)?,
        pending,
        sync_error: client
            .together
            .error
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .clone()
            .or_else(|| {
                state
                    .shared
                    .as_ref()
                    .filter(|v| v.phase != crate::listen_together::IncomingPhase::Applied)
                    .map(|_| "共享队列或共同播放权限尚未同步完成，请重新同步".into())
            })
            .or_else(|| {
                state
                    .incoming
                    .as_ref()
                    .filter(|v| {
                        matches!(
                            v.phase,
                            crate::listen_together::IncomingPhase::Pending
                                | crate::listen_together::IncomingPhase::Failed
                        )
                    })
                    .map(|_| "远端播放指令尚未完成，请重新同步以核对并恢复".into())
            }),
        checked_at_ms: state.checked_at_ms,
        local_recovery: client.together_binding(token)?.is_some_and(|binding| {
            state
                .room
                .as_ref()
                .is_none_or(|r| r.room_id != binding.room_id)
        }),
        ending: state.end_notice.is_some(),
        lifecycle_notice: client.session.with_token(token, |uid| {
            Ok(client
                .registry
                .store()
                .get(uid, lifecycle::NOTICE, "current")?
                .and_then(|value| value.as_str().map(str::to_owned)))
        })?,
        shared_phase: state.shared.as_ref().map(|v| v.phase),
        native_target_id,
        common_access: state
            .shared
            .as_ref()
            .map(|v| {
                v.privileges
                    .iter()
                    .map(|p| TogetherAccessView {
                        track_id: p.track_id.clone(),
                        can_play: p.can_play(),
                        reason: p.reason.clone(),
                        preview: p.preview,
                        limited_member: p.limited_member(owner),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    };
    client.session.with_token(token, |_| Ok(()))?;
    Ok(AccountData {
        epoch: epoch.into(),
        value,
    })
}
