//! Recovered PC search categories (M/15, M/2245 and page:searchResults).
use crate::{Error, Result, protocol::api::Api};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchCategory {
    All,
    Songs,
    Playlists,
    Artists,
    Voices,
    Podcasts,
    Lyrics,
    Albums,
    Mvs,
    Users,
}
impl SearchCategory {
    pub const ALL: [Self; 10] = [
        Self::All,
        Self::Songs,
        Self::Playlists,
        Self::Artists,
        Self::Voices,
        Self::Podcasts,
        Self::Lyrics,
        Self::Albums,
        Self::Mvs,
        Self::Users,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::All => "综合",
            Self::Songs => "单曲",
            Self::Playlists => "歌单",
            Self::Artists => "歌手",
            Self::Voices => "声音",
            Self::Podcasts => "播客",
            Self::Lyrics => "歌词",
            Self::Albums => "专辑",
            Self::Mvs => "MV",
            Self::Users => "用户",
        }
    }
    pub fn from_title(title: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.title() == title || (*kind == Self::Mvs && title == "视频"))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchResultQuery {
    pub keyword: String,
    pub category: SearchCategory,
    pub offset: usize,
    pub limit: usize,
    pub correct: bool,
}
pub const READ_PATHS: &[&str] = &[
    "/api/search/pc/result/tab",
    "/api/search/pc/complex/page",
    "/api/search/song/list/page",
    "/api/v1/search/playlist/get",
    "/api/v1/search/artist/get",
    "/api/v1/search/album/get",
    "/api/v1/search/user/get",
    "/api/search/voice/get",
    "/api/search/voicelist/get",
    "/api/search/resource/lyric",
    "/api/search/mlog/get",
];
pub async fn tabs(api: &dyn Api, keyword: &str) -> Result<Value> {
    api.call(
        READ_PATHS[0],
        json!({"keyword":super::keyword(keyword)?}),
        false,
        &[200],
    )
    .await
}
pub async fn request(api: &dyn Api, q: &SearchResultQuery) -> Result<Value> {
    let keyword = super::keyword(&q.keyword)?;
    if q.limit == 0
        || q.limit > 100
        || q.offset > 10000
        || (q.category == SearchCategory::All && q.offset != 0)
    {
        return Err(Error::InvalidInput("invalid search page"));
    }
    use SearchCategory::*;
    let legacy = matches!(q.category, Playlists | Artists | Albums | Users);
    let mut args = if legacy {
        json!({"s":keyword,"queryCorrect":q.correct})
    } else {
        json!({"keyword":keyword,"scene":"normal"})
    };
    args["limit"] = json!(q.limit);
    args["offset"] = json!(q.offset);
    let path = match q.category {
        All => {
            args = json!({"keyword":keyword,"scene":"normal","needCorrect":q.correct,"channel":"suggest","cursor":"{\"page\":0,\"ignoreBlocks\":[],\"traceId\":\"\"}","bizQueryInfo":""});
            READ_PATHS[1]
        }
        Songs => {
            args["needCorrect"] = json!(q.correct);
            args["channel"] = json!("suggest");
            READ_PATHS[2]
        }
        Playlists => READ_PATHS[3],
        Artists => READ_PATHS[4],
        Albums => READ_PATHS[5],
        Users => READ_PATHS[6],
        Voices => READ_PATHS[7],
        Podcasts => READ_PATHS[8],
        Lyrics => READ_PATHS[9],
        Mvs => {
            args["channel"] = json!("suggest");
            args["os"] = json!("pc");
            args["tag"] = json!("MV");
            READ_PATHS[10]
        }
    };
    api.call(path, args, false, &[200]).await
}
