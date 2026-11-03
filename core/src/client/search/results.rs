//! Whitelisted presentation models for recovered PC result pages.
use super::track_hint::{self, TrackHint};
use crate::{
    Error, Result,
    catalog::search::results::{self as api, reason},
    client::{MusicClient, catalog::cards, dto::*, mapping, podcast::view},
    podcast::{Episode, Radio},
    protocol::api::identifier,
};
pub use api::{SearchCategory, SearchCorrectionPart, SearchResultQuery};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchItem {
    Song {
        track: TrackView,
        #[serde(rename = "mvId", skip_serializing_if = "Option::is_none")]
        mv_id: Option<String>,
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
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultView {
    pub query: SearchResultQuery,
    pub sections: Vec<SearchSection>,
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
}
impl MusicClient {
    /// Enabled by the production desktop host. Test/research hosts opt in
    /// explicitly; callers cannot choose a server experiment through IPC.
    pub fn with_official_search_experiment(mut self) -> Self {
        self.search.enabled = true;
        self
    }

    pub async fn search_results(
        &self,
        epoch: &str,
        query: SearchResultQuery,
    ) -> Result<AccountData<SearchResultView>> {
        let token = self.player_token(epoch)?;
        let token = &token;
        let value = self
            .session
            .run_for(token, |api, uid| async move {
                let (endpoint, tabs, raw) = if query.category == SearchCategory::All {
                    // Validate before any optional experiment or tab request.
                    api::validate_query(&query)?;
                    if self
                        .search
                        .enabled_for(&self.session, token, api.as_ref())
                        .await?
                    {
                        let tabs = self
                            .search
                            .tabs_for(&self.session, token, api.as_ref(), &query.keyword)
                            .await?;
                        (
                            api::v3::PATH,
                            Some(tabs),
                            api::v3::request(api.as_ref(), &query).await,
                        )
                    } else {
                        (
                            api::COMPREHENSIVE_PATH,
                            None,
                            api::request(api.as_ref(), &query).await,
                        )
                    }
                } else {
                    ("", None, api::request(api.as_ref(), &query).await)
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
                        let mut view = project(raw, query, &uid)?;
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
    ["mv", "mvid"]
        .into_iter()
        .filter_map(|key| identifier(&value[key]).ok())
        .find(|id| id.len() <= 32)
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
            SearchItem::Song {
                track: mapping::track(v)?,
                mv_id: comprehensive.and_then(|_| song_mv(v)),
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
            recommendation: comprehensive
                .and_then(|_| v["recommendText"].as_str())
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
    fn empty(query: SearchResultQuery) -> Self {
        Self {
            next_offset: query.offset,
            query,
            sections: vec![],
            total: None,
            more: false,
            correction: None,
            correction_parts: None,
            unavailable_count: 0,
            notice: None,
            empty_title: None,
            tabs: None,
            hot_explanation: None,
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

fn project(raw: Value, query: SearchResultQuery, uid: &str) -> Result<SearchResultView> {
    use SearchCategory::*;
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
            for resource in resources.iter().take(100) {
                match item(
                    resource["type"].as_str().unwrap_or(""),
                    &resource["baseInfo"],
                    uid,
                    &Value::Null,
                    Some(resource),
                ) {
                    Ok(Some(item)) => section.items.push(item),
                    // A card can be unsupported or malformed independently of
                    // the response envelope. Keep its valid siblings visible.
                    Ok(None) | Err(_) => result.unavailable_count += 1,
                }
            }
            if !section.items.is_empty() {
                result.sections.push(section);
            }
        }
        // The recovered comprehensive page is a block preview. Category links load the full lists.
        if raw["data"]["hasMore"] == true {
            result.notice = Some("综合结果为分类预览，可通过区块标题入口查看完整分类。".into());
        }
        return Ok(result);
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
        match item(kind, base, uid, lyric, None) {
            Ok(Some(item)) => section.items.push(item),
            Ok(None) | Err(_) => result.unavailable_count += 1,
        }
    }
    result.sections.push(section);
    Ok(result)
}
