//! The recovered PC v3 comprehensive search returns a single block preview.
mod hot;
use super::{ResultContext, SearchCategory, SearchChannel, SearchResultQuery, result_call};
use crate::{Error, Result, error::code_matches, protocol::api::Api};
pub(crate) use hot::HotZoneContext;
pub use hot::{SearchHotZoneSource, SearchHotZoneTarget};
use serde_json::{Map, Value, json};

pub(crate) const PATH: &str = "/api/search/pc/complex/page/v3";
const MAX_BLOCKS: usize = 50;
const MAX_RESOURCES: usize = 100;
const HOT: &str = "search_block_hot_result_hot_explain";
const HOT_CORE: &str = "search_block_hot_result_core_resources";
const BEST: &str = "search_block_best_match";
const CORRECTION: &str = "search_block_correction";

pub(crate) struct RawComprehensive {
    data: Value,
    partial: bool,
}
impl RawComprehensive {
    pub(crate) fn has_zone_candidate(&self) -> bool {
        self.data["blocks"].as_array().is_some_and(|blocks| {
            blocks
                .iter()
                .take(MAX_BLOCKS)
                .any(|block| block["blockCode"] == HOT && hot::candidate(block))
        })
    }
    pub(crate) fn project(self, zones: HotZoneContext<'_>) -> Result<Value> {
        normalize_mode(&self.data, self.partial, zones)
    }
}

#[cfg(test)]
async fn request(api: &dyn Api, q: &SearchResultQuery, zones: HotZoneContext<'_>) -> Result<Value> {
    fetch(api, q, None).await?.project(zones)
}

pub(crate) async fn fetch(
    api: &dyn Api,
    q: &SearchResultQuery,
    context: Option<ResultContext>,
) -> Result<RawComprehensive> {
    if q.category != SearchCategory::All || q.offset != 0 || !(1..=100).contains(&q.limit) {
        return Err(Error::InvalidInput("invalid comprehensive search page"));
    }
    let keyword = super::super::keyword(&q.keyword)?;
    let channel = context
        .as_ref()
        .and_then(ResultContext::channel)
        .unwrap_or_else(|| SearchChannel::wire(q.channel));
    let payload = json!({
        "keyword": keyword, "scene": "normal", "needCorrect": q.correct,
        "channel": channel, "bizQueryInfo": "",
    });
    match result_call(api, PATH, payload, context).await {
        Ok(mut raw) => Ok(RawComprehensive {
            data: raw["data"].take(),
            partial: false,
        }),
        Err(error) => match error.root() {
            Error::Service {
                endpoint,
                code,
                body,
            } if endpoint == PATH
                && code_matches(code, 599)
                && body["data"]["blocks"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty()) =>
            {
                Ok(RawComprehensive {
                    data: body["data"].clone(),
                    partial: true,
                })
            }
            // An empty 599 and a 405 carry a scoped user explanation. Leave
            // them as service errors for the caller's shared empty-state path.
            _ => Err(error),
        },
    }
}

type Fields = Map<String, Value>;

fn text(value: &Value, max: usize) -> Option<String> {
    let value = value.as_str()?.trim();
    let text = value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect::<String>();
    (!text.trim().is_empty()).then_some(text)
}

fn image(value: &Value) -> Option<Value> {
    let source = value.as_str().filter(|source| source.len() <= 2048)?;
    let mut url = url::Url::parse(source).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !url.host_str().is_some_and(|host| {
            host.ends_with(".music.126.net") || host.ends_with(".music.163.com")
        })
    {
        return None;
    }
    url.set_scheme("https").ok()?;
    url.set_query(Some("param=320y320"));
    url.set_fragment(None);
    Some(Value::String(url.into()))
}

fn fields(input: &Value, scalar: &[&str], images: &[&str]) -> Fields {
    let mut out = Fields::new();
    for &key in scalar {
        let value = &input[key];
        let value = match value {
            Value::Bool(_) | Value::Number(_) => Some(value.clone()),
            Value::String(s)
                if matches!(
                    key,
                    "id" | "albumId"
                        | "userId"
                        | "resourceId"
                        | "voiceId"
                        | "productId"
                        | "voiceListId"
                        | "mainTrackId"
                        | "trackId"
                        | "pic_str"
                ) =>
            {
                (s.len() <= 256).then(|| value.clone())
            }
            Value::String(_) => text(value, 512).map(Value::String),
            _ => None,
        };
        if let Some(value) = value {
            out.insert(key.into(), value);
        }
    }
    for &key in images {
        if let Some(value) = image(&input[key]) {
            out.insert(key.into(), value);
        }
    }
    out
}

fn nested(out: &mut Fields, input: &Value, key: &str, map: impl Fn(&Value) -> Fields) {
    if input[key].is_object() {
        out.insert(key.into(), Value::Object(map(&input[key])));
    }
}

fn list(out: &mut Fields, input: &Value, key: &str, map: impl Fn(&Value) -> Option<Value>) {
    if let Some(rows) = input[key].as_array() {
        out.insert(
            key.into(),
            Value::Array(rows.iter().take(32).filter_map(map).collect()),
        );
    }
}

fn strings(out: &mut Fields, input: &Value, keys: &[&str]) {
    for &key in keys {
        list(out, input, key, |v| text(v, 128).map(Value::String));
    }
}

fn person(v: &Value) -> Fields {
    fields(v, &["id", "name"], &[])
}

fn creator(v: &Value) -> Fields {
    fields(v, &["userId", "nickname"], &["avatarUrl"])
}

fn credits(out: &mut Fields, input: &Value, key: &str) {
    list(out, input, key, |v| {
        v.is_object().then(|| Value::Object(person(v)))
    });
}

fn song_tag(value: &Value) -> Fields {
    let mut tag = Fields::new();
    if let Some(reasons) = value.get("algClickableTags") {
        tag.insert("algClickableTags".into(), super::reason::whitelist(reasons));
    }
    if let Some(alias) = value.get("songAlias") {
        tag.insert(
            "songAlias".into(),
            alias
                .as_str()
                .filter(|text| text.len() <= 1024)
                .map_or(Value::Null, |text| json!(text)),
        );
    }
    if value.get("lyrics").is_some() {
        // Preserve a supplied invalid override without retaining arbitrary fields.
        tag.insert("lyrics".into(), Value::Null);
    }
    if let (Some(txt), Some(range)) = (
        value["lyrics"]["txt"].as_str(),
        value["lyrics"]["range"].as_array(),
    ) && txt.len() <= 1024
        && range.len() <= 16
    {
        // Keep original text positions intact; client projection validates
        // UTF-16 ranges before this information crosses the desktop boundary.
        let range = range
            .iter()
            .map(|row| json!({"first": row["first"].as_u64(), "second": row["second"].as_u64()}))
            .collect::<Vec<_>>();
        tag.insert("lyrics".into(), json!({"txt":txt,"range":range}));
    }
    tag
}

fn song(v: &Value) -> Fields {
    let mut out = fields(
        v,
        &[
            "id",
            "name",
            "dt",
            "duration",
            "fee",
            "mv",
            "mvid",
            "st",
            "status",
            "mark",
            "originCoverType",
        ],
        &[],
    );
    nested(&mut out, v, "newSongExt", song_tag);
    nested(&mut out, v, "extInfo", song_tag);
    if let Some(privilege) = v.get("privilege") {
        let projection = privilege.as_object().map(|_| {
            let mut fields = fields(privilege, &["st"], &[]);
            for key in ["flag", "maxbr", "fee"] {
                let value = &privilege[key];
                if value.is_number() || value.as_str().is_some_and(|raw| raw.len() <= 64) {
                    fields.insert(key.into(), value.clone());
                }
            }
            fields.insert(
                "toast".into(),
                json!(crate::protocol::wire::serialization::js_truthy(
                    &privilege["toast"]
                )),
            );
            if let Some(trial) = privilege["freeTrialPrivilege"].as_object() {
                use crate::protocol::wire::serialization::js_truthy;
                let mut bounded = json!({
                    "userConsumable":js_truthy(&privilege["freeTrialPrivilege"]["userConsumable"]),
                    "resConsumable":js_truthy(&privilege["freeTrialPrivilege"]["resConsumable"]),
                });
                if let Some(kind) = trial.get("freeLimitTagType").filter(|kind| {
                    kind.is_null()
                        || matches!(
                            kind.as_str(),
                            Some("VIP_AND_FREE_TAG" | "FREE_TAG" | "NO_TAG")
                        )
                }) {
                    bounded["freeLimitTagType"] = kind.clone();
                }
                fields.insert("freeTrialPrivilege".into(), bounded);
            }
            fields
        });
        out.insert(
            "privilege".into(),
            projection.map_or(Value::Null, Value::Object),
        );
    }
    if let Some(alternative) = v.get("noCopyrightRcmd") {
        let projection = alternative.as_object().map(|_| {
            let mut row = fields(alternative, &["type"], &[]);
            nested(&mut row, alternative, "mvInfo", |mv| {
                fields(mv, &["id"], &["pic16v9Url"])
            });
            nested(&mut row, alternative, "expInfo", |experiment| {
                fields(experiment, &["mvPlay"], &[])
            });
            row
        });
        out.insert(
            "noCopyrightRcmd".into(),
            projection.map_or(Value::Null, Value::Object),
        );
    }
    for key in ["ar", "artists"] {
        credits(&mut out, v, key);
    }
    for key in ["al", "album"] {
        nested(&mut out, v, key, |album| {
            fields(album, &["id", "albumId", "name", "mark"], &["picUrl"])
        });
    }
    out
}

fn playlist(v: &Value) -> Fields {
    let mut out = fields(
        v,
        &[
            "id",
            "userId",
            "name",
            "title",
            "description",
            "recommendText",
            "privacy",
            "specialType",
            "updateTime",
            "createTime",
            "trackCount",
            "subscribedCount",
            "playCount",
        ],
        &["coverImgUrl", "picUrl", "coverUrl"],
    );
    nested(&mut out, v, "creator", creator);
    strings(&mut out, v, &["tags"]);
    if let Some(ids) = v["trackIds"].as_array() {
        // Only distinguish absent, empty, and nonempty; no IDs from this
        // preview are needed or carried forward.
        out.insert(
            "trackIds".into(),
            Value::Array(if ids.is_empty() {
                vec![]
            } else {
                vec![Value::Null]
            }),
        );
    }
    out
}

#[cfg(test)]
#[test]
fn playlist_preview_preserves_only_track_presence() {
    for (raw, empty) in [
        (json!({"trackCount":0}), true),
        (json!({}), false),
        (json!({"trackCount":0,"trackIds":[{"id":"PRIVATE"}]}), false),
        (json!({"trackIds":[]}), true),
    ] {
        let projection = Value::Object(playlist(&raw));
        assert_eq!(super::playlist_known_empty(&projection), empty);
        assert!(!projection.to_string().contains("PRIVATE"));
    }
}

fn artist(v: &Value) -> Fields {
    let mut out = fields(
        v,
        &[
            "id",
            "name",
            "alias",
            "trans",
            "transName",
            "briefDesc",
            "img1v1",
            "musicSize",
            "albumSize",
            "mvSize",
        ],
        &["picUrl", "img1v1Url"],
    );
    strings(&mut out, v, &["alias"]);
    out
}

fn album(v: &Value) -> Fields {
    let mut out = fields(
        v,
        &[
            "id",
            "albumId",
            "name",
            "albumName",
            "description",
            "artistName",
            "trackCount",
            "size",
            "transName",
            "isSub",
            "publishTime",
            "mark",
        ],
        &["cover", "picUrl"],
    );
    nested(&mut out, v, "artist", person);
    credits(&mut out, v, "artists");
    strings(&mut out, v, &["alia", "ala", "alias", "tns", "transNames"]);
    out
}

fn radio(v: &Value) -> Fields {
    let mut out = fields(
        v,
        &[
            "id",
            "resourceId",
            "voiceListId",
            "productId",
            "name",
            "voiceListName",
            "description",
            "desc",
            "voiceCount",
            "programCount",
            "subed",
            "subscribed",
            "creatorName",
            "radioFeeType",
            "categoryId",
            "grayDisplay",
            "underShelf",
        ],
        &["coverImgUrl", "picUrl", "coverUrl"],
    );
    for key in ["dj", "creator"] {
        nested(&mut out, v, key, creator);
    }
    out
}

fn voice(v: &Value) -> Fields {
    let mut out = fields(
        v,
        &[
            "id",
            "resourceId",
            "voiceId",
            "productId",
            "name",
            "voiceName",
            "mainTrackId",
            "trackId",
            "duration",
            "creatorName",
            "belongPodcastName",
            "listenerCount",
            "playCount",
            "adjustedPlayCount",
            "programFeeType",
            "grayDisplay",
        ],
        &["coverUrl"],
    );
    nested(&mut out, v, "radio", radio);
    nested(&mut out, v, "mainSong", |song| {
        fields(song, &["id", "duration", "dt"], &[])
    });
    for key in ["dj", "creator"] {
        nested(&mut out, v, key, creator);
    }
    out
}

fn video(v: &Value) -> Fields {
    let mut out = fields(v, &["id"], &[]);
    nested(&mut out, v, "resource", |resource| {
        let mut out = Fields::new();
        nested(&mut out, resource, "mlogBaseData", |base| {
            fields(base, &["type", "text", "desc", "duration"], &["coverUrl"])
        });
        nested(&mut out, resource, "mlogExtVO", |extra| {
            let mut out = fields(extra, &["artistName", "playCount"], &[]);
            credits(&mut out, extra, "artists");
            out
        });
        nested(&mut out, resource, "data", |data| {
            let mut out = fields(
                data,
                &["name", "desc", "duration", "artistName", "playCount"],
                &["cover"],
            );
            credits(&mut out, data, "artists");
            out
        });
        out
    });
    out
}

fn user(v: &Value) -> Fields {
    fields(
        v,
        &["userId", "nickname", "signature", "accountStatus"],
        &["avatarUrl"],
    )
}

#[derive(Clone, Copy)]
enum Kind {
    Song,
    Playlist,
    Artist,
    Album,
    Voice,
    Radio,
    User,
    Mv,
}

impl Kind {
    fn block(code: &str) -> Option<Self> {
        Some(match code {
            "search_block_song" => Self::Song,
            "search_block_playlist" => Self::Playlist,
            "search_block_artist" => Self::Artist,
            "search_block_album" => Self::Album,
            "search_block_voice" => Self::Voice,
            "search_block_voicelist" => Self::Radio,
            "search_block_user" => Self::User,
            "search_block_video" => Self::Mv,
            _ => return None,
        })
    }

    fn mixed(v: &Value) -> Option<Self> {
        Some(match v["type"].as_str()? {
            "playlist" => Self::Playlist,
            "artist" => Self::Artist,
            "album" => Self::Album,
            "voice" => Self::Voice,
            "voicelist" => Self::Radio,
            "user" => Self::User,
            "mv" if v["subType"] == "mv" => Self::Mv,
            _ => return None,
        })
    }

    fn normal(self, v: &Value, mixed: bool, partial: bool) -> Option<Value> {
        let (kind, base_key, whitelist): (&str, &str, fn(&Value) -> Fields) = match self {
            Self::Song => ("song", "simpleSongData", song),
            Self::Playlist => ("playlist", "pubPlaylistData", playlist),
            Self::Artist => ("artist", "artistDTO", artist),
            Self::Album => ("album", "albumData", album),
            Self::Voice => ("voice", "pubDJProgramData", voice),
            Self::Radio => ("voicelist", "pubDJRadioData", radio),
            Self::User => ("user", "pubUserProfileData", user),
            Self::Mv if v["type"] == "mv" => ("mv", "squareFeedDto", video),
            Self::Mv => return None,
        };
        let base = &v["baseInfo"][base_key];
        if !base.is_object() {
            return Some(json!({"type":kind,"baseInfo":null}));
        }
        // M/2245 merges display fields in order: resource, nested object,
        // then extInfo for songs/playlists and for mixed best/hot cards.
        let mut fields = whitelist(v);
        fields.extend(whitelist(base));
        if mixed || matches!(self, Self::Song | Self::Playlist) {
            fields.extend(whitelist(&v["extInfo"]));
            if matches!(self, Self::Song) {
                nested(&mut fields, v, "extInfo", song_tag);
            }
        }
        if matches!(self, Self::Playlist) {
            for (target, alternatives) in [
                ("name", &["title"][..]),
                ("coverImgUrl", &["picUrl", "coverUrl"][..]),
            ] {
                for &alternate in alternatives {
                    if let Some(value) = fields.remove(alternate) {
                        fields.entry(target).or_insert(value);
                    }
                }
            }
        }
        if !mixed && matches!(self, Self::Playlist) {
            if partial {
                fields.remove("playCount");
            }
            if let Some(count) = v["baseInfo"]["playCount"]
                .as_u64()
                .filter(|&n| partial || n > 0)
                .or_else(|| (!partial).then(|| base["playCount"].as_u64()).flatten())
            {
                fields.insert("playCount".into(), json!(count));
            }
        }
        Some(json!({"type":kind,"baseInfo":fields}))
    }
}

fn rewrite(resource: &Value) -> Option<Value> {
    let data = &resource["baseInfo"]["queryRewriteDTO"];
    let word = ["rewriteQuery", "correctQuery", "query"]
        .into_iter()
        .filter_map(|key| data[key].as_str())
        .find_map(|word| super::super::keyword(word).ok())?;
    let mut base = json!({"rewriteQuery":word});
    if let Some(parts) = super::correction_parts(data) {
        base["searchQcReminder"] = json!({"qcReminders":parts.into_iter().map(|part|
            json!({"qcReminderPart":part.text,"highLight":part.highlight})
        ).collect::<Vec<_>>()});
        if let Some(channel) = super::correction_channel(data) {
            base["searchQcReminder"]["qcReminderChannel"] = json!(channel);
        }
    }
    Some(json!({"baseInfo":base}))
}

#[cfg(test)]
fn normalize(data: &Value, zones: HotZoneContext<'_>) -> Result<Value> {
    normalize_mode(data, false, zones)
}

fn normalize_mode(data: &Value, partial: bool, zones: HotZoneContext<'_>) -> Result<Value> {
    let rows = if data.is_null() {
        &[][..]
    } else {
        data["blocks"]
            .as_array()
            .map(Vec::as_slice)
            .ok_or(Error::Response("missing v3 search blocks"))?
    };
    let mut blocks = Vec::new();
    let mut explanation = None;
    for block in rows.iter().take(MAX_BLOCKS) {
        let Some(code) = block["blockCode"].as_str() else {
            continue;
        };
        if code == HOT {
            if explanation.is_none() {
                explanation = hot::explanation(block, zones);
            }
            continue;
        }
        let Some(resources) = block["resources"].as_array() else {
            continue;
        };
        let resources = resources.iter().take(MAX_RESOURCES);
        let resources: Vec<_> = match code {
            CORRECTION => resources.filter_map(rewrite).collect(),
            BEST | HOT_CORE => resources
                .filter_map(|v| Kind::mixed(v)?.normal(v, true, partial))
                .collect(),
            _ => {
                let Some(kind) = Kind::block(code) else {
                    continue;
                };
                resources
                    .filter_map(|v| kind.normal(v, false, partial))
                    .collect()
            }
        };
        let mut block_out = json!({"blockCode":code,"resources":resources});
        if let Some(title) = text(&block["uiElement"]["mainTitle"]["title"], 120) {
            block_out["uiElement"] = json!({"mainTitle":{"title":title}});
        } else if code == HOT_CORE {
            block_out["uiElement"] = json!({"mainTitle":{"title":"热点相关"}});
        }
        blocks.push(block_out);
    }
    let mut normalized = json!({"code":200,"data":{"blocks":blocks}});
    if let Some(explanation) = explanation {
        normalized["data"]["hotExplanation"] = explanation;
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::api::ApiFuture;

    async fn request(api: &dyn Api, q: &SearchResultQuery) -> Result<Value> {
        super::request(api, q, HotZoneContext::default()).await
    }

    fn normalize(data: &Value) -> Result<Value> {
        super::normalize(data, HotZoneContext::default())
    }

    fn query() -> SearchResultQuery {
        SearchResultQuery {
            keyword: " 合成 ".into(),
            category: SearchCategory::All,
            offset: 0,
            limit: 30,
            correct: false,
            channel: Some(SearchChannel::Hotquery),
        }
    }

    struct Wire(u16, &'static str);
    impl Api for Wire {
        fn request<'a>(
            &'a self,
            path: &'a str,
            payload: Value,
            cache: bool,
        ) -> ApiFuture<'a, Value> {
            Box::pin(async move {
                assert_eq!(path, PATH);
                assert!(!cache);
                assert_eq!(
                    payload,
                    json!({"keyword":"合成","scene":"normal","needCorrect":false,"channel":self.1,"bizQueryInfo":""})
                );
                let data = json!({"blocks":[{"blockCode":"search_block_song","resources":[
                    {"baseInfo":{"simpleSongData":{"id":71,"name":"歌","al":{"id":2,"picUrl":"https://p1.music.126.net/x?secret=omitted"}}},
                     "extInfo":{"dt":60000,"url":"https://private.invalid/a"}}
                ]}]});
                match self.0 {
                    200 | 599 => Ok(json!({"code":self.0,"data":data})),
                    405 => Ok(json!({"code":405,"msg":"说明"})),
                    598 => Ok(json!({"code":599,"msg":"暂不可展示","data":{"blocks":[]}})),
                    _ => Err(Error::HttpStatus {
                        status: 599,
                        endpoint: Some(PATH.into()),
                        body: Some(Box::new(json!({"code":599,"data":data}))),
                    }),
                }
            })
        }
    }

    #[tokio::test]
    async fn v3_forwards_search_input_origins_without_tracking_data() {
        for (channel, wire) in [
            (SearchChannel::Typing, "typing"),
            (SearchChannel::Defaultquery, "defaultquery"),
            (SearchChannel::History, "history"),
            (SearchChannel::RecQuery, "rec_query"),
        ] {
            let mut query = query();
            query.channel = Some(channel);
            request(&Wire(200, wire), &query).await.unwrap();
        }
    }

    #[test]
    fn v3_preserves_mark_and_only_bounded_display_fields_in_replaced_song_privilege() {
        let raw = json!({"blocks":[{"blockCode":"search_block_song","resources":[
            {"mark":3,"baseInfo":{"simpleSongData":{"id":71,"mark":1048576,"privilege":{
                "st":0,"flag":4096,"maxbr":"320001","fee":0,"pl":999000,"cookie":"PRIVATE",
                "freeTrialPrivilege":{"userConsumable":1,"resConsumable":true,
                    "freeLimitTagType":null,"url":"https://private.invalid/"}
            }}}},
            {"baseInfo":{"simpleSongData":{"id":72,"mark":1048576,"privilege":{"flag":65536}}},
                "extInfo":{"mark":0,"privilege":{"maxbr":1998001,"url":"https://private.invalid/"}}},
            {"baseInfo":{"simpleSongData":{"id":73,"privilege":{"flag":"655\u{0}36","maxbr":[999000]}}}}
        ]}]});
        let projected = normalize(&raw).unwrap();
        let rows = &projected["data"]["blocks"][0]["resources"];
        assert_eq!(rows[0]["baseInfo"]["mark"], 1048576);
        assert_eq!(
            rows[0]["baseInfo"]["privilege"],
            json!({
                "st":0,"flag":4096,"maxbr":"320001","fee":0,"toast":false,
                "freeTrialPrivilege":{"userConsumable":true,"resConsumable":true,"freeLimitTagType":null}
            })
        );
        assert_eq!(rows[1]["baseInfo"]["mark"], 0);
        assert_eq!(
            rows[1]["baseInfo"]["privilege"],
            json!({"maxbr":1998001,"toast":false})
        );
        assert_eq!(
            rows[2]["baseInfo"]["privilege"],
            json!({"flag":"655\u{0}36","toast":false})
        );
        assert!(!projected.to_string().contains("private.invalid"));
    }

    #[tokio::test]
    async fn exact_request_and_scoped_partial_business_result() {
        for code in [200, 599] {
            let page = request(&Wire(code, "hotquery"), &query()).await.unwrap();
            let song = &page["data"]["blocks"][0]["resources"][0]["baseInfo"];
            assert_eq!(song["id"], 71);
            assert_eq!(song["dt"], 60000);
            assert_eq!(
                song["al"]["picUrl"],
                "https://p1.music.126.net/x?param=320y320"
            );
            assert!(song.get("url").is_none());
            assert!(page["data"].get("cursor").is_none());
        }
        for (status, expected) in [(405, 405), (598, 599)] {
            assert!(matches!(
                request(&Wire(status, "hotquery"), &query()).await.unwrap_err(),
                Error::Service { endpoint, code, .. } if endpoint == PATH && code == expected
            ));
        }
        assert!(matches!(
            request(&Wire(500, "hotquery"), &query()).await.unwrap_err(),
            Error::HttpStatus { status: 599, .. }
        ));
        let mut next = query();
        next.offset = 1;
        assert!(matches!(
            request(&Wire(200, "hotquery"), &next).await,
            Err(Error::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn v3_zone_gate_applies_to_complete_and_partial_search_responses() {
        struct Wire(u16);
        impl Api for Wire {
            fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
                Box::pin(async move {
                    assert_eq!(path, PATH);
                    Ok(json!({"code":self.0,"data":{"blocks":[
                        {"blockCode":HOT,"resources":[{"baseInfo":{"hotExplainModuleData":{
                            "title":"专区热点","text":"热点说明","canClick":true,
                            "textAction":"orpheus://rnpage?component=cube-renderer-rn&page=ROCK&tabKey=%E6%91%87%E6%BB%9A"
                        }}}]},
                        {"blockCode":"search_block_song","resources":[{"baseInfo":{"simpleSongData":{"id":77,"name":"独立歌曲"}}}]}
                    ]}}))
                })
            }
        }

        let groups =
            crate::protocol::pc_experiments::PcExperimentGroups::refreshed(&json!({"data":[
                {"expName":"PH-PC-zhuanqu","expGroupName":"t1"}
            ]}))
            .unwrap();
        let enabled = HotZoneContext {
            groups: Some(&groups),
            ready_pages: Some(&|page| page == "ROCK"),
        };
        for code in [200, 599] {
            let approved = super::request(&Wire(code), &query(), enabled)
                .await
                .unwrap();
            let blocked = request(&Wire(code), &query()).await.unwrap();
            assert_eq!(approved["data"]["blocks"], blocked["data"]["blocks"]);
            assert_eq!(
                approved["data"]["hotExplanation"]["action"],
                json!({
                    "label":"查看详情","target":{"kind":"vertical_zone","pageCode":"ROCK","tabKeyName":"摇滚","from":"searchHotExplain"}
                })
            );
            assert!(blocked["data"].get("hotExplanation").is_none());
        }
    }

    #[test]
    fn nested_resources_keep_safe_cards_and_exclude_untyped_actions() {
        let page = normalize(&json!({"blocks":[
            {"blockCode":CORRECTION,"resources":[{"baseInfo":{"queryRewriteDTO":{"rewriteQuery":"原词","searchQcReminder":{
                "qcReminderChannel":"not public","qcReminders":[
                    {"qcReminderPart":"仍查 "},{"qcReminderPart":"旧词","highLight":true,"action":"not public"}
                ]
            }}}}]},
            {"blockCode":HOT,"resources":[{"baseInfo":{"hotExplainModuleData":{
                "title":"热点","text":"有趣","textAction":"orpheus://route/elsewhere",
                "pictureUrl":"https://untrusted.invalid/image"}}}]},
            {"blockCode":HOT_CORE,"resources":[
                {"type":"artist","baseInfo":{"artistDTO":{"id":12,"name":"歌手"}}},
                {"type":"orpheus","baseInfo":{"activityDTO":{"id":"private"}}},
                {"type":"mv","subType":"video","baseInfo":{"squareFeedDto":{"id":17}}}
            ]},
            {"blockCode":"search_block_playlist","resources":[{"baseInfo":{
                "playCount":88,"pubPlaylistData":{"id":34,"creator":{"userId":12},"playCount":13}
            },"extInfo":{"recommendText":"合成推荐理由"}}]},
            {"blockCode":"search_block_video","resources":[{"type":"mv","baseInfo":{"squareFeedDto":{
                "id":17,"resource":{"mlogBaseData":{"text":"影片","duration":90},"data":{"url":"secret"}}
            }}}]}
        ]})).unwrap();
        let data = &page["data"];
        let rows = data["blocks"].as_array().unwrap();
        assert_eq!(rows[0]["resources"][0]["baseInfo"]["rewriteQuery"], "原词");
        assert_eq!(
            rows[0]["resources"][0]["baseInfo"]["searchQcReminder"],
            json!({"qcReminders":[
                {"qcReminderPart":"仍查 ","highLight":false},{"qcReminderPart":"旧词","highLight":true}
            ]})
        );
        assert_eq!(
            data["hotExplanation"],
            json!({"title":"热点","text":"有趣"})
        );
        assert_eq!(rows[1]["blockCode"], HOT_CORE);
        assert_eq!(rows[1]["resources"].as_array().unwrap().len(), 1);
        assert_eq!(rows[1]["resources"][0]["baseInfo"]["id"], 12);
        assert_eq!(rows[2]["resources"][0]["baseInfo"]["playCount"], 88);
        assert_eq!(
            rows[2]["resources"][0]["baseInfo"]["recommendText"],
            "合成推荐理由"
        );
        assert_eq!(
            rows[3]["resources"][0]["baseInfo"]["resource"]["mlogBaseData"]["text"],
            "影片"
        );
        assert!(!page.to_string().contains("secret"));
    }

    #[test]
    fn preview_bounds_and_malformed_resource_do_not_remove_its_valid_siblings() {
        let mut songs = vec![json!({"baseInfo":{}})];
        songs.extend((0..120).map(|n| json!({"baseInfo":{"simpleSongData":{"id":n + 1}}})));
        let block = json!({"blockCode":"search_block_song","resources":songs});
        let data = json!({"blocks":vec![block; 56]});
        let page = normalize(&data).unwrap();
        let rows = page["data"]["blocks"].as_array().unwrap();
        assert_eq!(rows.len(), MAX_BLOCKS);
        let resources = rows[0]["resources"].as_array().unwrap();
        assert_eq!(resources.len(), MAX_RESOURCES);
        assert!(resources[0]["baseInfo"].is_null());
        assert_eq!(resources[1]["baseInfo"]["id"], 1);
    }
}
