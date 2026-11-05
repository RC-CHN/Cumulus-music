//! Whitelisted presentation models for recovered PC result pages.
use super::track_hint::{self, TrackHint};
use crate::{
    Error, Result,
    catalog::search::results::{self as api, reason},
    client::{MusicClient, catalog::cards, dto::*, mapping, podcast::view},
    podcast::{Episode, Radio},
    protocol::{api::identifier, mconfig::PcConfig, wire::serialization::js_truthy},
};
pub use api::{SearchCategory, SearchCorrectionPart, SearchResultQuery};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchItem {
    Song {
        track: TrackView,
        #[serde(rename = "queueDropReceipt", skip_serializing_if = "Option::is_none")]
        queue_drop_receipt: Option<String>,
        #[serde(rename = "mvId", skip_serializing_if = "Option::is_none")]
        mv_id: Option<String>,
        #[serde(rename = "replacementMv", skip_serializing_if = "std::ops::Not::not")]
        replacement_mv: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        hint: Option<TrackHint>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        explicit: bool,
        #[serde(
            rename = "originalRecording",
            skip_serializing_if = "std::ops::Not::not"
        )]
        original_recording: bool,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        reasons: Vec<reason::ReasonTag>,
        excerpt: Vec<String>,
        #[serde(rename = "excerptTruncated")]
        excerpt_truncated: bool,
    },
    Playlist {
        playlist: PlaylistView,
        #[serde(rename = "knownEmpty", skip_serializing_if = "std::ops::Not::not")]
        known_empty: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        recommendation: Option<String>,
    },
    Artist {
        artist: ArtistCard,
    },
    Album {
        album: AlbumCard,
    },
    Voice {
        voice: EpisodeView,
        #[serde(rename = "listenerCount", skip_serializing_if = "Option::is_none")]
        listener_count: Option<u64>,
    },
    Podcast {
        podcast: PodcastCard,
        #[serde(skip_serializing_if = "Option::is_none")]
        table: Option<PodcastSearchTable>,
    },
    Mv {
        mv: CatalogVideoCard,
    },
    User {
        user: FollowUser,
    },
    Related {
        keyword: String,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastSearchTable {
    #[serde(skip_serializing_if = "Option::is_none")]
    creator_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    program_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    play_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    score: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recommendation: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    subscription_eligible: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSection {
    pub id: String,
    pub title: String,
    pub category: Option<SearchCategory>,
    pub items: Vec<SearchItem>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHotExplanation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_corner_marker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hot_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<SearchHotAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub play_target: Option<SearchHotPlaybackTarget>,
}
#[derive(Serialize, Deserialize)]
pub struct SearchHotAction {
    pub label: String,
    pub target: SearchHotTarget,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchHotTarget {
    Song {
        id: String,
    },
    Voice {
        id: String,
    },
    Album {
        id: String,
    },
    Artist {
        id: String,
    },
    Playlist {
        id: String,
    },
    Podcast {
        id: String,
    },
    Mv {
        id: String,
    },
    VerticalZone {
        #[serde(flatten)]
        zone: api::SearchHotZoneTarget,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchHotPlaybackTarget {
    Song { id: String },
    Voice { id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchPreviewLayout {
    Carousel,
    CompactCarousel,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultView {
    pub query: SearchResultQuery,
    pub sections: Vec<SearchSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privilege_receipt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_receipt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_revision: Option<String>,
    pub total: Option<u64>,
    pub more: bool,
    pub next_offset: usize,
    pub correction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correction_parts: Option<Vec<SearchCorrectionPart>>,
    pub unavailable_count: usize,
    pub notice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub empty_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tabs: Option<Vec<SearchCategory>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hot_explanation: Option<SearchHotExplanation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_layout: Option<SearchPreviewLayout>,
}
impl MusicClient {
    /// Enabled by the production desktop host. Test/research hosts opt in
    /// explicitly; callers cannot choose a server experiment through IPC.
    pub fn with_official_search_experiment(mut self) -> Self {
        self.search.enabled = true;
        self
    }

    /// Trusted native embedding for hosts that already supply an AccountSession.
    /// A fixed RequestContext, an IPC flag and search response config cannot enable this policy.
    pub fn with_native_search_preview_config(mut self, config: Arc<PcConfig>) -> Self {
        self.search.preview_config = Some(config);
        self
    }

    fn search_replacement_mv_enabled(&self) -> bool {
        let snapshot = match self.auth.context.as_ref() {
            Some(context) => context.pc_snapshot().ok().flatten(),
            None => self
                .search
                .preview_config
                .as_ref()
                .and_then(|config| config.snapshot().ok()),
        };
        snapshot
            .and_then(|snapshot| snapshot.data)
            .is_some_and(|data| data["preload#isOpenNoCopyrightMV"].as_bool() == Some(true))
    }

    pub async fn search_results(
        &self,
        epoch: &str,
        query: SearchResultQuery,
    ) -> Result<AccountData<SearchResultView>> {
        self.search_results_with_default(epoch, query, None).await
    }

    pub async fn search_results_with_default(
        &self,
        epoch: &str,
        query: SearchResultQuery,
        default_receipt: Option<&str>,
    ) -> Result<AccountData<SearchResultView>> {
        let token = self.player_token(epoch)?;
        let token = &token;
        let value = self
            .session
            .run_for(token, |api, uid| async move {
                let credentials = if query.category == SearchCategory::All {
                    api.search_privilege_credential_revision().ok()
                } else {
                    None
                };
                let mut footer_revision = None;
                let mut presentation_credentials = None;
                let (endpoint, tabs, preview_layout, raw) = if query.category == SearchCategory::All
                {
                    // Validate before any optional experiment or tab request.
                    let keyword = api::validate_query(&query)?;
                    let default_context = || {
                        if query.channel != Some(api::SearchChannel::Defaultquery) {
                            return Ok(None);
                        }
                        self.search.default_contexts.resolve(
                            &self.session,
                            token,
                            &api,
                            &keyword,
                            default_receipt,
                        )
                    };
                    let assignment = self.search_assignment_for(token, &api).await?;
                    let mut flags = assignment.flags;
                    // M147 restores one global web assignment at bootstrap.
                    // Its display snapshot also selects V1/V3 after a failed
                    // refresh; the separate fresh cache remains authorization-only.
                    if let Ok(snapshot) = self.pc_presentation_snapshot(token, &api) {
                        flags.v3 = snapshot.search_v3;
                        flags.expanded = snapshot.view.search_expanded;
                        flags.dense = snapshot.view.search_dense;
                    }
                    footer_revision = assignment.footer_revision;
                    let layout = flags.expanded.then_some(if flags.dense {
                        SearchPreviewLayout::CompactCarousel
                    } else {
                        SearchPreviewLayout::Carousel
                    });
                    if flags.v3 {
                        let tabs = self
                            .search
                            .tabs_for(&self.session, token, api.as_ref(), &query.keyword)
                            .await?;
                        let context = default_context()?;
                        presentation_credentials =
                            api.pc_presentation_credential_revision().ok().flatten();
                        let raw = api::v3::fetch(api.as_ref(), &query, context).await;
                        let projected = match raw {
                            Ok(raw)
                                if raw.has_zone_candidate() && !assignment.optional_read_failed =>
                            {
                                let context = match self.vertical_zone_search_registry(epoch).await
                                {
                                    Ok(context) => context,
                                    Err(error)
                                        if error.is_service_code(301)
                                            || matches!(
                                                error,
                                                Error::StaleResponse | Error::CredentialChanged
                                            ) =>
                                    {
                                        return Err(error);
                                    }
                                    Err(_) => None,
                                };
                                self.session.require_api(token, &api)?;
                                if let Some((groups, menu)) = &context {
                                    let contains = |page: &str| menu.contains(page);
                                    raw.project(api::v3::HotZoneContext {
                                        groups: Some(groups),
                                        ready_pages: Some(&contains),
                                    })
                                } else {
                                    raw.project(api::v3::HotZoneContext::default())
                                }
                            }
                            Ok(raw) => raw.project(api::v3::HotZoneContext::default()),
                            Err(error) => Err(error),
                        };
                        (api::v3::PATH, Some(tabs), layout, projected)
                    } else {
                        let context = default_context()?;
                        presentation_credentials =
                            api.pc_presentation_credential_revision().ok().flatten();
                        (
                            api::COMPREHENSIVE_PATH,
                            None,
                            layout,
                            api::request_with_default(api.as_ref(), &query, context).await,
                        )
                    }
                } else {
                    ("", None, None, api::request(api.as_ref(), &query).await)
                };
                let mut view = match raw {
                    Ok(raw) => {
                        let explanation = if tabs.is_some() {
                            raw["data"]
                                .get("hotExplanation")
                                .and_then(|value| serde_json::from_value(value.clone()).ok())
                        } else {
                            None
                        };
                        let replacement_mv =
                            endpoint == api::v3::PATH && self.search_replacement_mv_enabled();
                        let (mut view, preview) = project(raw, query, &uid, replacement_mv)?;
                        if let (Some(preview), Some(credentials)) = (preview, credentials) {
                            self.session.require_api(token, &api)?;
                            if api.search_privilege_credential_revision().ok()
                                == Some(credentials.clone())
                            {
                                view.privilege_receipt = self.search.privilege_pages.capture(
                                    token,
                                    &api,
                                    credentials,
                                    preview,
                                )?;
                            }
                        }
                        view.hot_explanation = explanation;
                        view
                    }
                    Err(error) => match error.root() {
                        Error::Service {
                            endpoint: source,
                            body,
                            ..
                        } if query.category == SearchCategory::All
                            && source == endpoint
                            && (error.is_service_code(405)
                                || (endpoint == api::v3::PATH && error.is_service_code(599))) =>
                        {
                            let mut result = SearchResultView::empty(query);
                            result.empty_title = Some(empty_title(body));
                            result
                        }
                        _ => return Err(error),
                    },
                };
                view.tabs = tabs;
                view.preview_layout = preview_layout;
                // TrackCard M2071:255/480 gates the complete footer separately
                // from searchHot, including both hints and clickable reasons.
                // A credential change during search cannot publish an older
                // assignment; the ordinary search result itself remains usable.
                let live_presentation = self.capture_search_presentation(
                    token,
                    &api,
                    presentation_credentials.as_deref(),
                    &mut view,
                )?;
                if !live_presentation
                    && view.query.category == SearchCategory::All
                    && (presentation_credentials.is_some()
                        || !footer_revision.as_deref().is_some_and(|revision| {
                            !self.login_pending().unwrap_or(true)
                                && api
                                    .verified_pc_experiment_revision()
                                    .ok()
                                    .flatten()
                                    .as_deref()
                                    == Some(revision)
                        }))
                {
                    if presentation_credentials.is_some() {
                        view.preview_layout = None;
                    }
                    for section in &mut view.sections {
                        for item in &mut section.items {
                            if let SearchItem::Song { hint, reasons, .. } = item {
                                *hint = None;
                                reasons.clear();
                            }
                        }
                    }
                }
                if view.query.category == SearchCategory::Songs {
                    self.session.with_token(token, |_| {
                        view.publish_drop_receipts(&self.search.queue_drops, token)
                    })?;
                }
                Ok(view)
            })
            .await?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value]> {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or(Error::Response("missing search result list"))
}
pub(super) fn clean(s: &str) -> String {
    s.replace("<em>", "")
        .replace("</em>", "")
        .replace("<b>", "")
        .replace("</b>", "")
}
fn voice_listener_count(value: &Value) -> Option<u64> {
    let mut valid = ["listenerCount", "playCount", "adjustedPlayCount"]
        .into_iter()
        .filter_map(|key| value[key].as_u64())
        .filter(|count| *count < (1 << 53));
    let first = valid.next()?;
    Some(if first == 0 {
        valid.find(|count| *count > 0).unwrap_or(0)
    } else {
        first
    })
}
fn podcast_count(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|count| *count < (1 << 53))
}
fn podcast_table_item(resource: &Value, uid: &str) -> Result<Option<SearchItem>> {
    let base = &resource["baseInfo"];
    let radio = Radio::from_official(base)?;
    let podcast = view::card(&radio);
    // M/15 lifts the score and recommendation from the search envelope.
    // M/21 normalizes only the nested dj/creator as the account resource.
    let creator = if base["dj"].is_object() {
        &base["dj"]
    } else {
        &base["creator"]
    };
    let verified_creator = radio.search_creator_id();
    let subscription_eligible = verified_creator
        .as_deref()
        .is_some_and(|id| id != uid.trim_start_matches('0'));
    let creator_id = creator["nickname"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .and(verified_creator);
    let count = podcast_count(&base["voiceCount"]);
    let program_count = count
        .filter(|count| *count > 0)
        .or_else(|| podcast_count(&base["programCount"]))
        .or(count);
    let raw_score = &resource["extInfo"]["scoreDto"]["score"];
    let score = raw_score
        .as_str()
        .map(str::trim)
        .map(str::to_owned)
        .or_else(|| raw_score.as_number().map(ToString::to_string))
        .filter(|text| {
            text.len() <= 12
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
                && text
                    .parse::<f64>()
                    .is_ok_and(|value| value.is_finite() && value > 0.0)
        });
    let recommendation = resource["uiElement"]["desc"]["title"]
        .as_str()
        .filter(|value| !value.contains("://"))
        .map(|value| {
            clean(value)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .filter(|ch| !ch.is_control())
                .take(160)
                .collect::<String>()
        })
        .filter(|value| !value.is_empty());
    Ok(Some(SearchItem::Podcast {
        podcast,
        table: Some(PodcastSearchTable {
            creator_id,
            program_count,
            play_count: podcast_count(&base["playCount"]),
            score,
            recommendation,
            subscription_eligible,
        }),
    }))
}
fn song_mv(value: &Value) -> Option<String> {
    if [&value["st"], &value["status"], &value["privilege"]["st"]]
        .into_iter()
        .any(|status| {
            status
                .as_i64()
                .or_else(|| status.as_str().and_then(|text| text.parse().ok()))
                .is_some_and(|status| status < 0)
        })
    {
        return None;
    }
    original_mv(value)
}
fn original_mv(value: &Value) -> Option<String> {
    ["mv", "mvid"]
        .into_iter()
        .filter_map(|key| identifier(&value[key]).ok())
        .find(|id| id.len() <= 32)
}

/// M/21.A normalizes privilege.st to status; M/21.S checks a truthy
/// toast before the special area restriction, and T requires exact booleans.
fn replacement_mv(value: &Value) -> Option<(String, Option<String>)> {
    let privilege = &value["privilege"];
    let status = privilege["st"]
        .as_f64()
        .or_else(|| privilege["st"].as_str()?.trim().parse().ok())?;
    if !status.is_finite() || status >= 0.0 || (status == -100.0 && !js_truthy(&privilege["toast"]))
    {
        return None;
    }
    alternative_mv(value)
}
fn alternative_mv(value: &Value) -> Option<(String, Option<String>)> {
    let candidate = &value["noCopyrightRcmd"];
    if candidate["type"].as_f64() != Some(3.0)
        || candidate["expInfo"]["mvPlay"].as_bool() != Some(true)
    {
        return None;
    }
    let id = identifier(&candidate["mvInfo"]["id"])
        .ok()
        .filter(|id| id.len() <= 32)?;
    Some((id, mapping::artwork(&candidate["mvInfo"]["pic16v9Url"])))
}

/// M/21 considers a nonzero song mark authoritative; otherwise only the
/// full album (not the compact al object) provides the explicit marker.
fn explicit_song(value: &Value) -> bool {
    const EXPLICIT: u64 = 1 << 20;
    let integer_mark = |mark: &Value| {
        mark.as_u64()
            .or_else(|| mark.as_i64().map(|mark| mark as u64))
    };
    match &value["mark"] {
        mark if mark.as_f64().is_some_and(|value| value != 0.0) => {
            integer_mark(mark).is_some_and(|mark| mark & EXPLICIT != 0)
        }
        _ => integer_mark(&value["album"]["mark"]).is_some_and(|mark| mark & EXPLICIT != 0),
    }
}

fn item(
    kind: &str,
    v: &Value,
    uid: &str,
    excerpt: &Value,
    comprehensive: Option<&Value>,
    allow_replacement_mv: bool,
) -> Result<Option<SearchItem>> {
    Ok(Some(match kind {
        "song" => {
            let (excerpt, excerpt_truncated) = super::lyrics::project(excerpt);
            let hint = comprehensive.and_then(|resource| track_hint::project(v, resource));
            let reasons = if comprehensive.is_some() && hint.is_none() {
                reason::tags(
                    v["extInfo"]
                        .get("algClickableTags")
                        .unwrap_or(&v["newSongExt"]["algClickableTags"]),
                )
            } else {
                Vec::new()
            };
            let mut track = mapping::track(v)?;
            let alternative = allow_replacement_mv.then(|| replacement_mv(v)).flatten();
            let is_replacement = alternative.is_some();
            let mv_id = if let Some((id, cover)) = alternative {
                if cover.is_some() {
                    track.cover_url = cover;
                }
                Some(id)
            } else {
                comprehensive.and_then(|_| song_mv(v))
            };
            SearchItem::Song {
                track,
                queue_drop_receipt: None,
                mv_id,
                replacement_mv: is_replacement,
                hint,
                reasons,
                explicit: comprehensive.is_some() && explicit_song(v),
                original_recording: v["originCoverType"].as_f64() == Some(1.0),
                excerpt,
                excerpt_truncated,
            }
        }
        "playlist" => SearchItem::Playlist {
            playlist: mapping::playlist(v, uid)?,
            known_empty: api::playlist_known_empty(v),
            recommendation: v["recommendText"]
                .as_str()
                .map(|value| {
                    clean(value)
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(160)
                        .collect::<String>()
                })
                .filter(|value| !value.is_empty()),
        },
        "artist" => SearchItem::Artist {
            artist: cards::artist(v)?,
        },
        "album" => SearchItem::Album {
            album: cards::album(v)?,
        },
        "voice" => SearchItem::Voice {
            voice: view::episode(&Episode::from_official(v)?, &[]),
            listener_count: voice_listener_count(v),
        },
        "voicelist" => SearchItem::Podcast {
            podcast: view::card(&Radio::from_official(v)?),
            table: None,
        },
        "mv" => {
            let mut v = v.clone();
            v["isMv"] = Value::Bool(true);
            SearchItem::Mv {
                mv: cards::video(&v)?,
            }
        }
        "user" => SearchItem::User {
            user: FollowUser {
                id: identifier(&v["userId"])?,
                name: mapping::string(&v["nickname"]),
                signature: mapping::string(&v["signature"]),
                avatar_url: mapping::artwork(&v["avatarUrl"]),
                unavailable: v["accountStatus"]
                    .as_i64()
                    .is_some_and(|n| [30, 40].contains(&n)),
            },
        },
        "related_search" => SearchItem::Related {
            keyword: crate::catalog::search::keyword(
                v["keyword"]
                    .as_str()
                    .ok_or(Error::Response("missing related keyword"))?,
            )?,
        },
        _ => return Ok(None),
    }))
}
fn correction(v: &Value) -> Option<String> {
    ["rewriteQuery", "correctQuery", "query"]
        .into_iter()
        .find_map(|key| crate::catalog::search::keyword(v[key].as_str()?).ok())
}
impl SearchResultView {
    fn publish_drop_receipts(
        &mut self,
        pages: &super::queue_drop::SearchDropPages,
        token: &crate::account::SessionToken,
    ) -> Result<()> {
        let candidates = self
            .sections
            .iter()
            .flat_map(|section| &section.items)
            .filter_map(|item| match item {
                SearchItem::Song {
                    track,
                    queue_drop_receipt: Some(_),
                    ..
                } => Some(track.clone()),
                _ => None,
            })
            .collect();
        let receipt = pages.capture(token, candidates)?;
        for item in self
            .sections
            .iter_mut()
            .flat_map(|section| &mut section.items)
        {
            if let SearchItem::Song {
                queue_drop_receipt: ticket @ Some(_),
                ..
            } = item
            {
                *ticket = receipt.clone();
            }
        }
        Ok(())
    }

    fn empty(query: SearchResultQuery) -> Self {
        Self {
            next_offset: query.offset,
            query,
            sections: vec![],
            privilege_receipt: None,
            presentation_receipt: None,
            presentation_revision: None,
            total: None,
            more: false,
            correction: None,
            correction_parts: None,
            unavailable_count: 0,
            notice: None,
            empty_title: None,
            tabs: None,
            hot_explanation: None,
            preview_layout: None,
        }
    }
}

fn empty_title(body: &Value) -> String {
    ["msg", "message"]
        .into_iter()
        .filter_map(|key| body[key].as_str())
        .map(str::trim)
        .find(|text| !text.is_empty() && !text.contains("://"))
        .map(|text| {
            text.chars()
                .filter(|c| !c.is_control())
                .take(160)
                .collect::<String>()
        })
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| "暂无可展示的综合搜索结果".into())
}

fn project(
    raw: Value,
    query: SearchResultQuery,
    uid: &str,
    allow_replacement_mv: bool,
) -> Result<(SearchResultView, Option<super::privilege::Preview>)> {
    use SearchCategory::*;
    let mut privilege_preview = None;
    let mut result = SearchResultView::empty(query.clone());
    if query.category == All {
        for (index, block) in array(&raw["data"], "blocks")?.iter().take(50).enumerate() {
            let code = block["blockCode"]
                .as_str()
                .ok_or(Error::Response("missing search block code"))?;
            let resources = array(block, "resources")?;
            if code == "search_block_correction" {
                if query.correct && result.correction.is_none() {
                    let candidate = resources.iter().take(100).find_map(|value| {
                        let base = &value["baseInfo"];
                        let dto = base.get("queryRewriteDTO").unwrap_or(base);
                        let word = correction(dto).filter(|word| word != query.keyword.trim())?;
                        Some((word, dto))
                    });
                    if let Some((word, dto)) = candidate {
                        result.correction = Some(word);
                        result.correction_parts = api::correction_parts(dto).filter(|parts| {
                            parts.iter().any(|part| {
                                part.highlight && part.text.trim() == query.keyword.trim()
                            })
                        });
                    }
                }
                continue;
            }
            let category = match code {
                "search_block_song" | "search_block_song_rec" => Some(Songs),
                "search_block_playlist" => Some(Playlists),
                "search_block_artist" => Some(Artists),
                "search_block_album" => Some(Albums),
                "search_block_voice" => Some(Voices),
                "search_block_voicelist" => Some(Podcasts),
                "search_block_video" => Some(Mvs),
                "search_block_user" => Some(Users),
                _ => None,
            };
            let mut section = SearchSection {
                id: format!("{code}:{index}"),
                title: block["uiElement"]["mainTitle"]["title"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(if code == "search_block_best_match" {
                        "最佳匹配"
                    } else {
                        "相关结果"
                    })
                    .into(),
                category,
                items: vec![],
            };
            let mut preview =
                (code == "search_block_song" && privilege_preview.is_none()).then(|| {
                    super::privilege::Preview {
                        section: section.id.clone(),
                        rows: Vec::new(),
                    }
                });
            for resource in resources.iter().take(100) {
                let base = &resource["baseInfo"];
                match item(
                    resource["type"].as_str().unwrap_or(""),
                    &resource["baseInfo"],
                    uid,
                    &Value::Null,
                    Some(resource),
                    allow_replacement_mv,
                ) {
                    Ok(Some(item)) => {
                        if let (Some(preview), SearchItem::Song { track, .. }) =
                            (&mut preview, &item)
                            && preview.rows.len() < super::privilege::ROW_LIMIT
                            && let Ok(original) = mapping::track(base)
                        {
                            let blocked =
                                [&base["st"], &base["status"]].into_iter().any(|status| {
                                    super::privilege::status(status)
                                        .is_some_and(|status| status < 0.0)
                                });
                            preview.rows.push(super::privilege::PreviewRow {
                                id: track.id.clone(),
                                original_cover: original.cover_url,
                                original_mv: (!blocked).then(|| original_mv(base)).flatten(),
                                alternative: allow_replacement_mv
                                    .then(|| alternative_mv(base))
                                    .flatten(),
                            });
                        }
                        section.items.push(item);
                    }
                    // A card can be unsupported or malformed independently of
                    // the response envelope. Keep its valid siblings visible.
                    Ok(None) | Err(_) => result.unavailable_count += 1,
                }
            }
            if let Some(preview) = preview {
                privilege_preview = Some(preview);
            }
            if !section.items.is_empty() {
                result.sections.push(section);
            }
        }
        // The recovered comprehensive page is a block preview. Category links load the full lists.
        if raw["data"]["hasMore"] == true {
            result.notice = Some("综合结果为分类预览，可通过区块标题入口查看完整分类。".into());
        }
        return Ok((result, privilege_preview));
    }
    let (container, list, total, kind) = match query.category {
        Playlists => (&raw["result"], "playlists", "playlistCount", "playlist"),
        Artists => (&raw["result"], "artists", "artistCount", "artist"),
        Albums => (&raw["result"], "albums", "albumCount", "album"),
        Users => (&raw["result"], "userprofiles", "userprofileCount", "user"),
        Songs => (&raw["data"], "resources", "totalCount", "song"),
        Lyrics => (&raw["data"], "resources", "totalCount", "song"),
        Voices => (&raw["data"], "resources", "totalCount", "voice"),
        Podcasts => (&raw["data"], "resources", "totalCount", "voicelist"),
        Mvs => (&raw["data"], "resources", "totalCount", "mv"),
        All => unreachable!(),
    };
    let rows = array(container, list)?;
    if rows.len() > 100 {
        return Err(Error::Response("oversized search result page"));
    }
    result.total = container[total].as_u64();
    result.next_offset = query.offset + rows.len();
    let remote_more = container["hasMore"].as_bool().unwrap_or_else(|| {
        result.total.map_or(rows.len() == query.limit, |n| {
            (result.next_offset as u64) < n
        })
    });
    result.more = remote_more && !rows.is_empty() && result.next_offset <= 10000;
    if remote_more && rows.is_empty() {
        result.notice =
            Some("服务返回了结果数量，但本页没有可展示的内容。可重试或更换关键词。".into());
    }
    if query.correct {
        result.correction = [
            &container["queryRewriteInfo"],
            &raw["queryRewriteInfo"],
            &container["queryRewrite"],
            &container["searchQcReminder"],
        ]
        .into_iter()
        .filter_map(correction)
        .find(|word| word != query.keyword.trim());
    }
    let mut section = SearchSection {
        id: query.category.title().into(),
        title: query.category.title().into(),
        category: Some(query.category),
        items: vec![],
    };
    for resource in rows {
        if query.category == Mvs && resource["type"] != "mv" {
            result.unavailable_count += 1;
            continue;
        }
        let base = match query.category {
            Songs => &resource["baseInfo"]["simpleSongData"],
            Lyrics => &resource["song"],
            Voices | Podcasts | Mvs => &resource["baseInfo"],
            _ => resource,
        };
        let lyric = if query.category == Lyrics {
            // Official 15.Wn merges the resource with its song; 21.x keeps lyrics || lyric.
            ["lyrics", "lyric"]
                .iter()
                .find_map(|key| {
                    resource["song"]
                        .get(key)
                        .or_else(|| resource.get(key))
                        .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
                })
                .unwrap_or(&resource["matchedLyric"])
        } else {
            &Value::Null
        };
        let projected = if query.category == Podcasts {
            podcast_table_item(resource, uid)
        } else {
            item(kind, base, uid, lyric, None, false)
        };
        match projected {
            Ok(Some(mut item)) => {
                if query.category == Songs
                    && let SearchItem::Song {
                        track,
                        queue_drop_receipt,
                        ..
                    } = &mut item
                    && !track.missing
                    && !track.local
                    && track.voice_id.is_none()
                    && !js_truthy(&base["privilege"]["cs"])
                    && super::queue_drop::bounded_track(track)
                {
                    // Only the song-tab projector may mark a row for issuance.
                    *queue_drop_receipt = Some(String::new());
                }
                section.items.push(item);
            }
            Ok(None) | Err(_) => result.unavailable_count += 1,
        }
    }
    result.sections.push(section);
    Ok((result, None))
}
