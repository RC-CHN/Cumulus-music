//! Standard vinyl encyclopedia metadata (M/182, M/109, M/15).
use super::{
    MusicClient,
    vinyl_similar::{seed, stale_credentials},
};
use crate::{
    Error, Result,
    protocol::{
        api::{VinylWikiRead, validate_response},
        wire::serialization::js_truthy,
    },
};
use serde::Serialize;
use serde_json::Value;

const MAX_ROWS: usize = 64;
const MAX_TERMS: usize = 64;
const MAX_TITLE: usize = 128;
const MAX_CONTENT: usize = 4096;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VinylWikiView {
    pub play_id: String,
    pub song_id: String,
    pub card: Option<VinylWikiCardView>,
}

#[derive(Serialize)]
pub struct VinylWikiCardView {
    pub metas: Vec<VinylWikiMetaView>,
}

#[derive(Serialize)]
pub struct VinylWikiMetaView {
    pub title: String,
    pub content: String,
}

fn bounded_text(value: &Value, max: usize) -> Result<&str> {
    let text = match value {
        Value::Null => "",
        Value::String(text) => text,
        _ => return Err(Error::Response("invalid vinyl wiki text")),
    };
    if text.len() > max
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(Error::Response("vinyl wiki text exceeds safe bounds"));
    }
    Ok(text)
}

fn projection(reply: Value) -> Result<Option<VinylWikiCardView>> {
    if reply["code"].as_f64() != Some(200.0) {
        // M/12 accepts a falsey envelope code and string "200", but M/109
        // emits no card unless the application code is numeric 200.
        if reply.is_object() && (!js_truthy(&reply["code"]) || reply["code"] == "200") {
            return Ok(None);
        }
        validate_response(VinylWikiRead::PATH, reply, &[200])?;
        return Ok(None);
    }
    let rows = &reply["data"]["wikiSubBlockBaseInfoVo"]["wikiSubElementVos"];
    if rows.is_null() {
        return Ok(None);
    }
    let rows = rows
        .as_array()
        .filter(|rows| rows.len() <= MAX_ROWS)
        .ok_or(Error::Response("invalid vinyl wiki metadata rows"))?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut metas = Vec::new();
    for row in rows {
        let title = bounded_text(&row["title"], MAX_TITLE)?;
        if title.is_empty() || (!js_truthy(&row["content"]) && !js_truthy(&row["wikiSubMetaVos"])) {
            continue;
        }
        let content = if row["pattern"].as_f64() == Some(1.0) {
            let terms = row["wikiSubMetaVos"]
                .as_array()
                .filter(|terms| terms.len() <= MAX_TERMS)
                .ok_or(Error::Response("invalid vinyl wiki metadata terms"))?;
            let mut text = String::new();
            for (index, term) in terms.iter().enumerate() {
                if index > 0 {
                    text.push('、');
                }
                text.push_str(bounded_text(&term["text"], MAX_CONTENT)?);
                if text.len() > MAX_CONTENT {
                    return Err(Error::Response("vinyl wiki content exceeds safe bounds"));
                }
            }
            text
        } else {
            bounded_text(&row["content"], MAX_CONTENT)?.to_owned()
        };
        metas.push(VinylWikiMetaView {
            title: title.into(),
            content,
        });
    }
    Ok(Some(VinylWikiCardView { metas }))
}

impl MusicClient {
    pub async fn player_vinyl_wiki(&self, epoch: &str, play_id: &str) -> Result<VinylWikiView> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let controller = self.player.controller.get().ok_or(Error::StaleResponse)?;
        let song_id = seed(&controller.snapshot(), &token, play_id)?;
        let read = VinylWikiRead::new(&song_id)?;
        let check_play = || {
            self.session
                .with_verified_profile(&token, &api, |_, _| Ok(()))?;
            if self.login_pending()? || seed(&controller.snapshot(), &token, play_id)? != song_id {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        check_play()?;
        // Check the signed native PC scope locally. The separate information
        // experiment affects only the mobile QR affordance, not the card read.
        let revision = api
            .verified_pc_experiment_revision()
            .map_err(stale_credentials)?
            .ok_or(Error::State(
                "vinyl wiki requires signed native PC credentials",
            ))?;
        let check_revision = || {
            if api
                .verified_pc_experiment_revision()
                .map_err(stale_credentials)?
                .as_deref()
                != Some(revision.as_str())
            {
                return Err(Error::StaleResponse);
            }
            check_play()
        };
        let reply = api.request_vinyl_wiki(read, &revision, &check_play).await;
        check_revision()?;
        let card = projection(reply.map_err(stale_credentials)?)?;
        check_revision()?;
        Ok(VinylWikiView {
            play_id: play_id.into(),
            song_id,
            card,
        })
    }
}
