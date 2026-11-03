//! The recovered PC v3 comprehensive search returns a single block preview.
use super::{SearchCategory, SearchChannel, SearchResultQuery};
use crate::{Error, Result, error::code_matches, protocol::api::Api};
use serde_json::{Map, Value, json};

pub(crate) const PATH: &str = "/api/search/pc/complex/page/v3";
const MAX_BLOCKS: usize = 50;
const MAX_RESOURCES: usize = 100;
const HOT: &str = "search_block_hot_result_hot_explain";
const HOT_CORE: &str = "search_block_hot_result_core_resources";
const BEST: &str = "search_block_best_match";
const CORRECTION: &str = "search_block_correction";

pub(crate) async fn request(api: &dyn Api, q: &SearchResultQuery) -> Result<Value> {
    if q.category != SearchCategory::All || q.offset != 0 || !(1..=100).contains(&q.limit) {
        return Err(Error::InvalidInput("invalid comprehensive search page"));
    }
    let keyword = super::super::keyword(&q.keyword)?;
    let channel = SearchChannel::wire(q.channel);
    let payload = json!({
        "keyword": keyword, "scene": "normal", "needCorrect": q.correct,
        "channel": channel, "bizQueryInfo": "",
    });
    match api.call(PATH, payload, false, &[200]).await {
        Ok(raw) => normalize(&raw["data"]),
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
                normalize_mode(&body["data"], true)
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

fn song(v: &Value) -> Fields {
    let mut out = fields(v, &["id", "name", "dt", "duration", "fee"], &[]);
    for key in ["ar", "artists"] {
        credits(&mut out, v, key);
    }
    for key in ["al", "album"] {
        nested(&mut out, v, key, |album| {
            fields(album, &["id", "albumId", "name"], &["picUrl"])
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
    out
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
    }
    Some(json!({"baseInfo":base}))
}

fn hot_explanation(block: &Value) -> Option<Value> {
    let first = block["resources"].as_array()?.first()?;
    let data = &first["baseInfo"]["hotExplainModuleData"];
    let title = text(&data["title"], 160)?;
    let content = text(&data["text"], 600)?;
    let mut result = json!({"title":title,"text":content});
    if let Some(picture) = image(&data["pictureUrl"]) {
        result["pictureUrl"] = picture;
    }
    Some(result)
}

fn normalize(data: &Value) -> Result<Value> {
    normalize_mode(data, false)
}

fn normalize_mode(data: &Value, partial: bool) -> Result<Value> {
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
                explanation = hot_explanation(block);
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
