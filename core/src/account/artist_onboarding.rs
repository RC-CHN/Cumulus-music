//! Exact typed PC login extension. It is never interpreted as a general URL.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtistFavorScene {
    NewUser,
    RecallUser,
}

#[derive(Clone, Copy)]
pub(crate) struct ArtistLogin {
    pub scene: Option<ArtistFavorScene>,
}

impl ArtistLogin {
    pub(crate) fn from_reply(value: &Value) -> Self {
        let scene = match value["loginExtData"]["actionParam"]["targetUrl"].as_str() {
            Some("orpheus://appModal?comName=NewUserFavorModal") => Some(ArtistFavorScene::NewUser),
            Some("orpheus://appModal?comName=RecallUserFavorModal") => {
                Some(ArtistFavorScene::RecallUser)
            }
            _ => None,
        };
        Self { scene }
    }
}

pub(crate) async fn qr_identity(
    api: &dyn crate::protocol::api::Api,
) -> crate::Result<(Value, ArtistLogin)> {
    const PATH: &str = "/api/w/nuser/account/get";
    let reply = api.call(PATH, serde_json::json!({}), false, &[200]).await?;
    let profile = &reply["profile"];
    if !profile.is_object() || crate::protocol::api::identifier(&profile["userId"]).is_err() {
        return Err(crate::Error::service(PATH, serde_json::json!(301), reply));
    }
    Ok((profile.clone(), ArtistLogin::from_reply(&reply)))
}
