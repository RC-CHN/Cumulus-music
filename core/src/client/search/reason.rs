pub use crate::catalog::search::results::reason::ReasonTarget;
use crate::{
    Error, Result,
    catalog::search::results::reason::{atom, text},
    client::{MusicClient, dto::*, mapping},
    protocol::api::identifier,
};
use serde::Serialize;
use serde_json::json;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchReasonPage {
    title: String,
    play_count: Option<u64>,
    tracks: Vec<TrackView>,
}

impl MusicClient {
    pub async fn search_reason_page(
        &self,
        epoch: &str,
        target: ReasonTarget,
    ) -> Result<AccountData<SearchReasonPage>> {
        let token = self.player_token(epoch)?;
        let request_token = token.clone();
        let value = self.session.run_for(&token, |api, _| async move {
            let (summary, result, list, title, city) = match target {
                ReasonTarget::Recommendation { reason_id, song_id, name } => {
                    let song_id = identifier(&json!(song_id))?;
                    if song_id.len() > 32 || !atom(&reason_id) || !text(&name) {
                        return Err(Error::InvalidInput("invalid search recommendation tag"));
                    }
                    let reply = api.call("/api/homepage/rcmd/tag/songs", json!({
                        "reasonId":reason_id, "songId":song_id, "reasonName":name, "reasonType":1
                    }), false, &[200]).await?;
                    (reply["data"].clone(), reply, "reasonRcmdSongCommonVOS", "title", false)
                }
                ReasonTarget::City { chart_code, target_id, target_type } => {
                    if ![&chart_code,&target_id,&target_type].into_iter().all(|s| atom(s)) {
                        return Err(Error::InvalidInput("invalid search chart tag"));
                    }
                    let args = json!({"chartCode":chart_code,"targetId":target_id,"targetType":target_type});
                    let info = api.call("/api/chart/detail",args.clone(),false,&[200]).await?;
                    self.session.check_generation(&request_token)?;
                    let tracks = api.call("/api/chart/song/detail",args,false,&[200]).await?;
                    (info["data"].clone(),tracks,"charts","name",true)
                }
            };
            let title = summary[title].as_str().filter(|s| text(s))
                .ok_or(Error::Response("missing search tag title"))?;
            let rows = result["data"][list].as_array().filter(|rows| rows.len() <= 1000)
                .ok_or(Error::Response("invalid search tag song list"))?;
            let mut tracks = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for row in rows {
                let mut song = row["songData"].clone();
                if !song.is_object() { continue; }
                if city && let Some(picture) = song.pointer("/al/extProperties/picUrl").cloned() {
                    song["al"]["picUrl"] = picture;
                }
                let Ok(track) = mapping::track(&song) else { continue };
                if seen.insert(track.id.clone()) { tracks.push(track); }
            }
            let play_count = summary["playCount"].as_u64()
                .or_else(|| summary["playCount"].as_str()?.parse().ok())
                .filter(|count| *count < (1 << 53));
            Ok(SearchReasonPage { title: title.into(), play_count, tracks })
        }).await?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
