use crate::{Error, Result, protocol::api::identifier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MvArtist {
    pub id: Option<String>,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MvBitrate {
    pub br: u32,
    pub size: Option<u64>,
    pub point: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MvPrivilege {
    pub fee: Option<u64>,
    pub mv_fee: Option<u64>,
    pub paid: Option<u64>,
    pub max_play_br: Option<u32>,
    pub max_download_br: Option<u32>,
    pub comment_permission: Option<u64>,
    pub unauthorized: bool,
}
/// Native metadata. Project a dedicated DTO before crossing an IPC boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MvDetail {
    pub id: String,
    pub title: String,
    pub description: String,
    pub brief_description: String,
    pub cover_url: String,
    pub thread_id: String,
    pub duration_ms: u64,
    pub published_at_ms: Option<u64>,
    pub published_date: String,
    pub artists: Vec<MvArtist>,
    pub artist_name: String,
    pub bitrates: Vec<MvBitrate>,
    pub privilege: Option<MvPrivilege>,
    pub subscribed: bool,
    pub liked: bool,
    /// Missing legacy metadata is not evidence of an unliked resource.
    #[serde(default)]
    pub liked_known: bool,
    pub play_count: u64,
    pub like_count: u64,
    pub comment_count: u64,
    pub share_count: u64,
    pub sub_count: u64,
    pub related_songs: Vec<Value>,
    #[serde(default)]
    pub related_privilege_failed: bool,
}
pub(super) fn number(v: &Value) -> Result<Option<u64>> {
    if v.is_null() {
        return Ok(None);
    }
    let n = v
        .as_u64()
        .or_else(|| {
            v.as_f64()
                .filter(|n| {
                    n.is_finite() && *n >= 0.0 && *n <= 9_007_199_254_740_991.0 && n.fract() == 0.0
                })
                .map(|n| n as u64)
        })
        .or_else(|| {
            v.as_str()
                .filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
                .and_then(|s| s.parse().ok())
        });
    n.map(Some)
        .ok_or(Error::Response("invalid MV numeric field"))
}
fn text<'a>(a: &'a Value, b: &'a Value) -> &'a str {
    a.as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| b.as_str())
        .unwrap_or("")
}
fn count(a: &Value, b: &Value) -> Result<u64> {
    Ok(number(a)?.filter(|v| *v > 0).or(number(b)?).unwrap_or(0))
}
fn resolution(v: &Value) -> Result<Option<u32>> {
    number(v)?
        .map(|n| u32::try_from(n).map_err(|_| Error::Response("MV resolution exceeds bounds")))
        .transpose()
}
impl MvDetail {
    pub fn from_official(value: &Value) -> Result<Self> {
        let id = identifier(&value["id"])?;
        let resource = value
            .get("resource")
            .filter(|v| v.is_object())
            .ok_or(Error::Response("MV resource missing"))?;
        let base = &resource["mlogBaseData"];
        if !base.is_null() && !base.is_object() {
            return Err(Error::Response("invalid MV base data"));
        }
        if number(&base["type"])?.is_some_and(|n| n != 3) {
            return Err(Error::Response("resource is not an MV"));
        }
        let extra = &resource["mlogExtVO"];
        let data = &resource["data"];
        if !data.is_null() && !data.is_object() || !extra.is_null() && !extra.is_object() {
            return Err(Error::Response("invalid MV metadata"));
        }
        if let Some(other) = data.get("id").filter(|v| !v.is_null())
            && identifier(other)? != id
        {
            return Err(Error::Response("MV metadata identity mismatch"));
        }
        let rows = extra
            .get("artists")
            .filter(|v| !v.is_null())
            .or_else(|| data.get("artists").filter(|v| !v.is_null()));
        let mut artists = vec![];
        if let Some(rows) = rows {
            for artist in rows
                .as_array()
                .ok_or(Error::Response("invalid MV artists"))?
            {
                let key = &artist["id"];
                let id = if key.is_null() || key == 0 || key == "0" || key == "" {
                    None
                } else {
                    Some(identifier(key)?)
                };
                artists.push(MvArtist {
                    id,
                    name: text(&artist["name"], &Value::Null).into(),
                });
            }
        }
        let mut bitrates = vec![];
        if let Some(rows) = data.get("brs").filter(|v| !v.is_null()) {
            let mut seen = std::collections::BTreeSet::new();
            for item in rows
                .as_array()
                .ok_or(Error::Response("invalid MV bitrate list"))?
            {
                let br = resolution(&item["br"])?
                    .filter(|v| *v > 0 && *v <= 8640)
                    .ok_or(Error::Response("invalid MV resolution"))?;
                if !seen.insert(br) {
                    return Err(Error::Response("duplicate MV resolution"));
                }
                bitrates.push(MvBitrate {
                    br,
                    size: number(&item["size"])?,
                    point: number(&item["point"])?,
                });
            }
        }
        let privilege = match &resource["mp"] {
            Value::Null => None,
            v if v.is_object() => Some(MvPrivilege {
                fee: number(&v["fee"])?,
                mv_fee: number(&v["mvFee"])?,
                paid: number(&v["payed"])?,
                max_play_br: resolution(&v["pl"])?,
                max_download_br: resolution(&v["dl"])?,
                comment_permission: number(&v["cp"])?,
                unauthorized: v["unauthorized"] == true,
            }),
            _ => return Err(Error::Response("invalid MV privilege")),
        };
        let related = extra
            .get("relateSong")
            .filter(|v| !v.is_null())
            .or_else(|| data.get("relateSong").filter(|v| !v.is_null()));
        let related_songs = related
            .map(|v| {
                v.as_array()
                    .cloned()
                    .ok_or(Error::Response("invalid MV related songs"))
            })
            .transpose()?
            .unwrap_or_default();
        let liked = extra
            .get("liked")
            .filter(|v| !v.is_null())
            .unwrap_or(&resource["liked"]);
        if !liked.is_null() && !liked.is_boolean() {
            return Err(Error::Response("invalid MV liked flag"));
        }
        Ok(Self {
            id,
            title: text(&base["text"], &data["name"]).into(),
            description: text(&base["desc"], &data["desc"]).into(),
            brief_description: text(&data["briefDesc"], &Value::Null).into(),
            cover_url: text(&base["coverUrl"], &data["cover"]).into(),
            thread_id: text(&base["threadId"], &resource["threadId"]).into(),
            duration_ms: count(&base["duration"], &data["duration"])?,
            published_at_ms: number(&base["pubTime"])?.filter(|v| *v > 0),
            published_date: text(&data["publishTime"], &Value::Null).into(),
            artists,
            artist_name: text(&extra["artistName"], &data["artistName"]).into(),
            bitrates,
            privilege,
            subscribed: extra["subed"] == true || resource["subed"] == true,
            liked: liked == true,
            liked_known: liked.is_boolean(),
            play_count: count(&extra["playCount"], &data["playCount"])?,
            like_count: count(&extra["likedCount"], &resource["likedCount"])?,
            comment_count: count(&extra["commentCount"], &resource["commentCount"])?,
            share_count: count(&extra["shareCount"], &resource["shareCount"])?,
            sub_count: count(&extra["subCount"], &data["subCount"])?,
            related_songs,
            related_privilege_failed: false,
        })
    }
    /// M/334.b filters the ordered download choices against maxDownBr.
    pub fn download_options(&self) -> Vec<MvBitrate> {
        let Some(max) = self.privilege.as_ref().and_then(|p| p.max_download_br) else {
            return vec![];
        };
        let mut rows = self
            .bitrates
            .iter()
            .filter(|b| b.br <= max)
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|b| b.br);
        rows
    }
    pub fn naming_metadata(&self) -> Value {
        json!({"id":self.id,"name":self.title,"title":self.title,"ar":self.artists.iter().map(|a|json!({"id":a.id,"name":a.name})).collect::<Vec<_>>(),"artists":self.artists,"al":{"name":"","picUrl":self.cover_url},"dt":self.duration_ms,"privilege":{"fee":self.privilege.as_ref().and_then(|p|p.fee),"payed":self.privilege.as_ref().and_then(|p|p.paid)}})
    }
}
