use super::{MusicClient, dto::AccountData};
use crate::{Error, Result, account::SessionToken, playlist::*};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEditor {
    pub id: String,
    pub revision: String,
    pub name: String,
    pub description: String,
    pub cover_url: Option<String>,
    pub tags: Vec<String>,
    pub owned: bool,
    pub favorite: bool,
    pub private: bool,
    pub track_ids: Vec<String>,
    pub has_local: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistOrderView {
    pub revision: String,
    pub created: Vec<String>,
    pub subscribed: Vec<String>,
}
#[derive(Serialize)]
pub struct PlaylistTag {
    pub name: String,
    pub category: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistMutationView {
    pub operation_id: String,
    pub kind: &'static str,
    pub playlist_id: Option<String>,
    pub phase: PlaylistPhase,
    pub label: String,
    pub service_codes: BTreeMap<String, i64>,
    pub message: &'static str,
    pub observed: Option<PlaylistEditor>,
}
pub(super) fn editor(p: PlaylistSnapshot, uid: &str) -> PlaylistEditor {
    PlaylistEditor {
        owned: p.summary.owner == uid,
        id: p.summary.id,
        revision: p.revision,
        name: p.summary.name,
        description: p.description,
        cover_url: super::mapping::artwork(&serde_json::json!(p.cover_url)),
        tags: p.tags,
        favorite: p.summary.favorite,
        private: p.summary.private,
        track_ids: p.tracks,
        has_local: p.has_local,
    }
}
fn view(r: PlaylistMutation, uid: &str) -> Result<PlaylistMutationView> {
    let observed = r
        .observed
        .as_ref()
        .filter(|v| v.get("playlists").is_none())
        .map(|v| serde_json::from_value::<PlaylistSnapshot>(v.clone()).map(|p| editor(p, uid)))
        .transpose()?;
    let label = match &r.command {
        PlaylistCommand::Create { name, .. } | PlaylistCommand::Metadata { name, .. } => {
            name.clone()
        }
        _ => observed
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_default(),
    };
    let message = match r.phase {
        PlaylistPhase::Confirmed => "已保存并确认",
        PlaylistPhase::Unchanged => "已是目标状态",
        PlaylistPhase::Partial => "部分字段保存失败，请按回读结果重新编辑",
        PlaylistPhase::Rejected if r.error_class.as_deref() == Some("not_sent") => {
            "请求未提交，可稍后重新操作"
        }
        PlaylistPhase::Rejected if r.codes.values().any(|c| *c == 405) => {
            "操作过于频繁，请稍后再试"
        }
        PlaylistPhase::Rejected => "此次操作未完成，已保留实际状态",
        _ => "操作结果尚未确认，请先核对，不要重复提交",
    };
    Ok(PlaylistMutationView {
        operation_id: r.operation_id,
        kind: r.command.kind(),
        playlist_id: r.playlist_id,
        phase: r.phase,
        label,
        service_codes: r.codes,
        message,
        observed,
    })
}
impl MusicClient {
    pub async fn playlist_tags(&self, epoch: &str) -> Result<AccountData<Vec<PlaylistTag>>> {
        let token = self.playlist_token(epoch)?;
        let result = self
            .session
            .run_for(&token, |api, _| async move {
                PlaylistApi::new(api).catalogue().await
            })
            .await?;
        let rows = result["sub"]
            .as_array()
            .ok_or(Error::Response("missing playlist tags"))?;
        let value = rows
            .iter()
            .map(|v| {
                let name = v["name"]
                    .as_str()
                    .ok_or(Error::Response("invalid playlist tag"))?
                    .to_owned();
                let key = v["category"]
                    .as_u64()
                    .map(|n| n.to_string())
                    .ok_or(Error::Response("invalid tag category"))?;
                let category = result["categories"][&key]
                    .as_str()
                    .ok_or(Error::Response("missing tag category"))?
                    .to_owned();
                Ok(PlaylistTag { name, category })
            })
            .collect::<Result<Vec<_>>>()?;
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub(super) fn playlist_token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(token)
    }
    pub async fn playlist_editor(
        &self,
        epoch: &str,
        id: &str,
    ) -> Result<AccountData<PlaylistEditor>> {
        let token = self.playlist_token(epoch)?;
        let p = PlaylistService::new(self.session.clone())?
            .detail(&token, id)
            .await?;
        let value = self.session.with_token(&token, |uid| Ok(editor(p, uid)))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub async fn playlist_order_view(&self, epoch: &str) -> Result<AccountData<PlaylistOrderView>> {
        let token = self.playlist_token(epoch)?;
        let list = PlaylistService::new(self.session.clone())?
            .list(&token)
            .await?;
        let value = self.session.with_token(&token, |uid| {
            Ok(PlaylistOrderView {
                created: list.group(uid, PlaylistGroup::Created),
                subscribed: list.group(uid, PlaylistGroup::Subscribed),
                revision: list.revision,
            })
        })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub async fn mutate_playlist(
        &self,
        epoch: &str,
        command: PlaylistCommand,
    ) -> Result<AccountData<PlaylistMutationView>> {
        let token = self.playlist_token(epoch)?;
        if matches!(command, PlaylistCommand::Cover { .. }) {
            return Err(Error::InvalidInput("use confirmed native cover upload"));
        }
        let r = PlaylistService::new(self.session.clone())?
            .execute(&token, command)
            .await?;
        let value = self.session.with_token(&token, |uid| view(r, uid))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub async fn reconcile_playlist(
        &self,
        epoch: &str,
        operation_id: &str,
    ) -> Result<AccountData<PlaylistMutationView>> {
        let token = self.playlist_token(epoch)?;
        let r = PlaylistService::new(self.session.clone())?
            .reconcile(&token, operation_id)
            .await?;
        let value = self.session.with_token(&token, |uid| view(r, uid))?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
    pub fn playlist_operations(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Vec<PlaylistMutationView>>> {
        let token = self.playlist_token(epoch)?;
        let mut records = PlaylistService::new(self.session.clone())?.operations(&token)?;
        records.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
        let value = self.session.with_token(&token, |uid| {
            records
                .into_iter()
                .filter(|r| {
                    r.phase.pending()
                        || matches!(r.phase, PlaylistPhase::Partial | PlaylistPhase::Rejected)
                })
                .map(|r| view(r, uid))
                .collect::<Result<Vec<_>>>()
        })?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
