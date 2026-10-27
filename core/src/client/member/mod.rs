mod dto;
mod project;
use super::{MusicClient, dto::AccountData, mapping};
use crate::{
    Error, Result,
    account::SessionToken,
    membership::{MembershipManager, Quality, center::MemberCenter},
};
pub use dto::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn now() -> f64 {
    time::OffsetDateTime::now_utc().unix_timestamp_nanos() as f64 / 1e9
}
fn revision(page: &MemberPage) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(page)?)))
}
impl MusicClient {
    fn member_token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(token)
    }
    pub async fn member_rights(
        &self,
        epoch: &str,
        refresh: bool,
    ) -> Result<AccountData<MemberRightsView>> {
        let token = self.member_token(epoch)?;
        let manager = MembershipManager::new(self.session.clone())?;
        let snapshot = if refresh {
            manager.refresh_for(token.clone()).await?
        } else {
            manager.snapshot()?
        };
        let s = &snapshot.state;
        let entitlements = [
            (
                "vinyl",
                "黑胶VIP",
                s.is_vinyl_vip,
                s.vinyl_vip_expire_time,
                Some(s.vinyl_vip_level),
            ),
            (
                "plus",
                "黑胶SVIP",
                s.is_plus_vip,
                s.plus_vip_expire_time,
                Some(s.plus_vip_level),
            ),
            (
                "music_package",
                "音乐包",
                s.is_music_package,
                s.music_package_expire_time,
                None,
            ),
            (
                "album",
                "数字专辑会员",
                s.is_album_vip,
                s.album_vip_expire_time,
                None,
            ),
            (
                "voice_book",
                "有声书会员",
                s.is_voice_book_vip,
                s.voice_book_vip_expire_time,
                None,
            ),
        ]
        .into_iter()
        .map(
            |(kind, name, active, expires_at, level)| MemberEntitlement {
                kind,
                name,
                active,
                expires_at,
                level,
            },
        )
        .collect();
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: MemberRightsView {
                    server_now: snapshot.server_now,
                    refresh_after_ms: snapshot.refresh_after_ms,
                    entitlements,
                },
            })
        })
    }
    pub async fn member_page(
        &self,
        epoch: &str,
        query: MemberQuery,
        refresh: bool,
    ) -> Result<AccountData<MemberPageView>> {
        let token = self.member_token(epoch)?;
        let service = MemberCenter::new(self.session.clone())?;
        let (page, fetched_at, warning, page_revision) = match query {
            MemberQuery::Home => {
                let state = if refresh {
                    service
                        .refresh_checked_for(&token, |value| {
                            project::home(value)?;
                            Ok(())
                        })
                        .await?
                } else {
                    service.snapshot_for(&token)?
                };
                if state["status"] == "idle" {
                    return Err(Error::State("member page is not cached"));
                }
                let warning = if state["status"] == "error" {
                    Some("上次刷新未完成，当前显示缓存内容")
                } else {
                    None
                };
                let projection = project::home(&state)?;
                let revision = projection.revision()?;
                (
                    projection.page,
                    state["fetched"].as_f64().unwrap_or(0.0),
                    warning,
                    revision,
                )
            }
            MemberQuery::History { date } => {
                if refresh {
                    service
                        .history_checked_for(&token, None, |values| {
                            project::dates(values)?;
                            Ok(())
                        })
                        .await?;
                }
                let (values, dates_fetched) = service.history_cached_for(&token, None)?;
                let dates = project::dates(&values)?;
                let date = date.or_else(|| dates.first().cloned());
                let (tracks, fetched) = if let Some(date) = &date {
                    if !dates.contains(date) {
                        return Err(Error::InvalidInput("recommendation date was not offered"));
                    }
                    if refresh {
                        service
                            .history_checked_for(&token, Some(json!(date)), project::validate_songs)
                            .await?;
                    }
                    let (values, fetched) =
                        service.history_cached_for(&token, Some(json!(date)))?;
                    if values.len() > 5000 {
                        return Err(Error::Response("recommendation history too large"));
                    }
                    (
                        values
                            .iter()
                            .map(mapping::track)
                            .collect::<Result<Vec<_>>>()?,
                        fetched.min(dates_fetched),
                    )
                } else {
                    (Vec::new(), dates_fetched)
                };
                let page = MemberPage::History {
                    dates,
                    date,
                    tracks,
                };
                let revision = revision(&page)?;
                (page, fetched, None, revision)
            }
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: MemberPageView {
                    revision: page_revision,
                    cached: !refresh,
                    fresh: warning.is_none() && (0.0..300.0).contains(&(now() - fetched_at)),
                    fetched_at,
                    warning,
                    page,
                },
            })
        })
    }
    /// Resolve only a link from the displayed native snapshot. The host opens
    /// the public HTTPS URL; neither credentials nor a purchase are forwarded.
    pub fn member_external_url(&self, epoch: &str, expected: &str, key: &str) -> Result<String> {
        let token = self.member_token(epoch)?;
        let projection =
            project::home(&MemberCenter::new(self.session.clone())?.snapshot_for(&token)?)?;
        if projection.revision()? != expected {
            return Err(Error::QueueChanged);
        }
        self.session.with_token(&token, |_| {
            projection
                .links
                .get(key)
                .cloned()
                .ok_or(Error::InvalidInput("unknown member link"))
        })
    }
    pub(super) async fn member_song_selection(
        &self,
        epoch: &str,
        query: MemberQuery,
        expected: &str,
        section: Option<&str>,
        index: usize,
        quality: Quality,
    ) -> Result<(Vec<Value>, usize, Quality, Value)> {
        let view = self.member_page(epoch, query.clone(), false).await?.value;
        if view.revision != expected {
            return Err(Error::QueueChanged);
        }
        let (tracks, trial) = match view.page {
            MemberPage::Home { modules, .. } => {
                let module = modules
                    .into_iter()
                    .find(|m| Some(m.key.as_str()) == section)
                    .ok_or(Error::InvalidInput("unknown member section"))?;
                match module.content {
                    MemberContent::Tracks { tracks } => (tracks, 41),
                    _ => return Err(Error::InvalidInput("member section is not a song list")),
                }
            }
            MemberPage::History { tracks, .. } => (tracks, -1),
        };
        if tracks.is_empty() || index >= tracks.len() || tracks[index].missing {
            return Err(Error::InvalidInput("invalid member song selection"));
        }
        Ok((
            tracks.into_iter().map(|t| json!(t.id)).collect(),
            index,
            quality,
            json!({"kind":"member","query":query,"section":section,"revision":expected,"trial_mode":trial}),
        ))
    }
}
