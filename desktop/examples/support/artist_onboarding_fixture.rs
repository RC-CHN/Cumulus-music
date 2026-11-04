//! Artist onboarding over the actual QR EAPI fixture. No real credentials.
use cumulus_core::{Error, Result};
use serde_json::{Value, json};
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Recall,
    NewUncertain,
}
impl Mode {
    pub fn active() -> Option<Self> {
        match std::env::var("CUMULUS_ARTIST_ONBOARDING_PROOF").as_deref() {
            Ok("recall") => Some(Self::Recall),
            Ok("new-uncertain") => Some(Self::NewUncertain),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Recall => "recall",
            Self::NewUncertain => "new-uncertain",
        }
    }
    pub fn extend_identity(self, response: &mut Value) {
        let target = match self {
            Self::Recall => "RecallUserFavorModal",
            Self::NewUncertain => "NewUserFavorModal",
        };
        response["loginExtData"] = json!({"actionParam":{
            "targetUrl":format!("orpheus://appModal?comName={target}"),
        }});
    }
}

static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());
pub fn calls() -> Vec<String> {
    CALLS.lock().unwrap().clone()
}

pub fn response(mode: Mode, path: &str, data: &Value) -> Option<Result<Value>> {
    if !(path.contains("/favor/")
        && (path.starts_with("/api/new") || path.starts_with("/api/recall")))
    {
        return None;
    }
    let calls = {
        let mut rows = CALLS.lock().unwrap();
        rows.push(path.into());
        rows.clone()
    };
    let has = |suffix: &str| calls.iter().filter(|call| call.ends_with(suffix)).count();
    let artist_ids = || {
        data["artistIds"]
            .as_str()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .unwrap_or_else(|| data["artistIds"].clone())
    };
    let recall = mode == Mode::Recall;
    let rows = json!([{"artistId":701,"artistName":"合成艺人甲"},{"id":702,"name":"合成艺人乙"}]);
    Some(match path {
        "/api/recall/user/favor/rcmd-artists" if recall => Ok(
            json!({"code":200,"data":{"usedListenedArtists":rows,"recentHotArtists":[{"artistId":701,"artistName":"重复艺人"},{"id":704,"name":"合成艺人丙"}]}}),
        ),
        "/api/newuser/favor/rcmd-artists" if !recall => Ok(json!({"code":200,"data":rows})),
        "/api/recall/user/favor/similar-rcmd-artists"
        | "/api/newuser/favor/similar-rcmd-artists" => match data["artistId"].as_str() {
            Some("701") => {
                let rows = json!([{"artistId":701,"artistName":"重复艺人"},{"artistId":703,"artistName":"合成相似艺人"}]);
                Ok(
                    json!({"code":200,"data":if recall {json!({"similarRcmdArtists":rows})} else {rows}}),
                )
            }
            Some("703") => Ok(
                json!({"code":200,"data":if recall {json!({"similarRcmdArtists":[]})} else {json!([])}}),
            ),
            _ => Err(Error::InvalidInput("unexpected synthetic similar artist")),
        },
        "/api/newuser/favor/rcmd-artists/save"
            if artist_ids() == json!([701, 703]) && has("/save") == 1 =>
        {
            if recall {
                Ok(json!({"code":200}))
            } else {
                Err(Error::Transport(
                    "synthetic save acknowledgement lost".into(),
                ))
            }
        }
        "/api/recall/user/favor/artist/song"
            if recall && artist_ids() == json!([701, 703]) && has("/save") == 1 =>
        {
            if has("/artist/song") == 1 {
                Err(Error::Transport(
                    "synthetic recommendation read failed".into(),
                ))
            } else {
                Ok(json!({"code":200,"data":{"sortedSongIds":[2,1,2]}}))
            }
        }
        "/api/new/user/favor/artist/song"
            if !recall && artist_ids() == json!([701, 703]) && has("/save") == 1 =>
        {
            Ok(
                json!({"code":200,"data":[{"songData":{"id":1,"name":"合成新用户推荐","ar":[{"name":"合成艺人甲"}],"dt":12000},"privilege":{"id":1,"st":0}}]}),
            )
        }
        _ => Err(Error::InvalidInput(
            "unexpected synthetic artist preference route or argument",
        )),
    })
}
