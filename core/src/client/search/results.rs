//! Whitelisted presentation models for recovered PC result pages.
use crate::{
    Error, Result,
    catalog::search::results as api,
    client::{MusicClient, catalog::cards, dto::*, mapping, podcast::view},
    podcast::{Episode, Radio},
    protocol::api::identifier,
};
pub use api::{SearchCategory, SearchResultQuery};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchItem {
    Song {
        track: TrackView,
        excerpt: Vec<String>,
    },
    Playlist {
        playlist: PlaylistView,
    },
    Artist {
        artist: ArtistCard,
    },
    Album {
        album: AlbumCard,
    },
    Voice {
        voice: EpisodeView,
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
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultView {
    pub query: SearchResultQuery,
    pub sections: Vec<SearchSection>,
    pub total: Option<u64>,
    pub more: bool,
    pub next_offset: usize,
    pub correction: Option<String>,
    pub unavailable_count: usize,
    pub notice: Option<String>,
}
impl MusicClient {
    pub async fn search_results(
        &self,
        epoch: &str,
        query: SearchResultQuery,
    ) -> Result<AccountData<SearchResultView>> {
        let token = self.player_token(epoch)?;
        let value = self
            .session
            .run_for(&token, |api, uid| async move {
                let raw = api::request(api.as_ref(), &query).await?;
                project(raw, query, &uid)
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
fn clean(s: &str) -> String {
    s.replace("<em>", "")
        .replace("</em>", "")
        .replace("<b>", "")
        .replace("</b>", "")
}
fn item(kind: &str, v: &Value, uid: &str, excerpt: &Value) -> Result<Option<SearchItem>> {
    Ok(Some(match kind {
        "song" => SearchItem::Song {
            track: mapping::track(v)?,
            excerpt: if let Some(s) = excerpt.as_str() {
                s.lines().take(5).map(clean).collect()
            } else {
                excerpt
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .take(5)
                    .map(clean)
                    .collect()
            },
        },
        "playlist" => SearchItem::Playlist {
            playlist: mapping::playlist(v, uid)?,
        },
        "artist" => SearchItem::Artist {
            artist: cards::artist(v)?,
        },
        "album" => SearchItem::Album {
            album: cards::album(v)?,
        },
        "voice" => SearchItem::Voice {
            voice: view::episode(&Episode::from_official(v)?, &[]),
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
        .find_map(|key| {
            v[key]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
        })
}
fn project(raw: Value, query: SearchResultQuery, uid: &str) -> Result<SearchResultView> {
    use SearchCategory::*;
    let mut result = SearchResultView {
        query: query.clone(),
        sections: vec![],
        total: None,
        more: false,
        next_offset: query.offset,
        correction: None,
        unavailable_count: 0,
        notice: None,
    };
    if query.category == All {
        for (index, block) in array(&raw["data"], "blocks")?.iter().take(50).enumerate() {
            let code = block["blockCode"]
                .as_str()
                .ok_or(Error::Response("missing search block code"))?;
            let resources = array(block, "resources")?;
            if code == "search_block_correction" {
                result.correction = resources.iter().find_map(|v| correction(&v["baseInfo"]));
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
                )? {
                    Some(item) => section.items.push(item),
                    None => result.unavailable_count += 1,
                }
            }
            if !section.items.is_empty() {
                result.sections.push(section);
            }
        }
        // The recovered comprehensive page is a block preview. Category links load the full lists.
        if raw["data"]["hasMore"] == true {
            result.notice = Some("综合结果为分类预览，可通过各区块右侧入口查看完整分类。".into());
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
    result.correction = correction(&container["queryRewrite"])
        .or_else(|| correction(&container["searchQcReminder"]));
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
            &resource["matchedLyric"]
        } else {
            &Value::Null
        };
        if let Some(item) = item(kind, base, uid, lyric)? {
            section.items.push(item);
        }
    }
    result.sections.push(section);
    Ok(result)
}
