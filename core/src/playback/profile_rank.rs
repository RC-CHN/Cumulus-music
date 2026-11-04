//! Native provenance for the profile card's virtual recent-week playlist.
use serde_json::{Value, json};

const VIRTUAL_ID: &str = "hostPlayRankPlaylist";

pub(crate) fn source(uid: &str, own: bool) -> Value {
    json!({
        "kind":"profile_rank_playlist","period":"week","scene":"unknown","href":"",
        "resourceType":"track","originalResourceType":VIRTUAL_ID,
        "computeSourceResourceType":VIRTUAL_ID,"pldScene":"playlist",
        "sourceData":{
            "id":VIRTUAL_ID, "userId":uid,
            "name":if own {"我的听歌排行"} else {"TA的听歌排行"},
            "toDetailUrlPath":format!("/userDetail/user/{uid}/recentWeekRecord")
        }
    })
}

pub(crate) fn uid(value: &Value) -> Option<&str> {
    if value["kind"] != "profile_rank_playlist"
        || value["period"] != "week"
        || value["originalResourceType"] != VIRTUAL_ID
        || value["sourceData"]["id"] != VIRTUAL_ID
    {
        return None;
    }
    let uid = value["sourceData"]["userId"].as_str()?;
    (!uid.is_empty()
        && uid.len() <= 64
        && uid.bytes().all(|byte| byte.is_ascii_digit())
        && uid.bytes().any(|byte| byte != b'0'))
    .then_some(uid)
}
