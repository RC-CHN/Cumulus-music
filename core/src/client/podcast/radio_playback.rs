//! M404 shared 250-item playback list, independent from a detail page.
use crate::{Error, Result, playback::engine::Cancellation, podcast::Episode, protocol::api::Api};
use serde_json::{Value, json};
use std::collections::HashSet;

const PROGRAM_PAGE: usize = 250;

pub(in crate::client) async fn by_radio(
    api: &dyn Api,
    radio: &str,
    ascending: Option<bool>,
    source: &Cancellation,
) -> Result<Vec<Episode>> {
    let mut voices = Vec::new();
    let mut seen = HashSet::new();
    // M/404 uses 250 per page and leaves asc absent without a saved order.
    for offset in (0..50_000).step_by(PROGRAM_PAGE) {
        if source.is_cancelled() {
            return Err(Error::QueueChanged);
        }
        let mut args = json!({"radioId":radio,"offset":offset,"limit":PROGRAM_PAGE});
        if let Some(ascending) = ascending {
            args["asc"] = json!(ascending);
        }
        let response = api
            .call("/api/v6/dj/program/byradio", args, false, &[200])
            .await?;
        let data = &response["data"];
        let rows = data["programs"]
            .as_array()
            .filter(|rows| rows.len() <= PROGRAM_PAGE)
            .ok_or(Error::Response("invalid podcast playback program page"))?;
        for raw in rows {
            let voice = Episode::from_official(raw)?;
            if !seen.insert(voice.id.clone()) {
                return Err(Error::QueueChanged);
            }
            voices.push(voice);
        }
        match data["more"].as_bool() {
            Some(false) => break,
            None if data["more"].is_null() => break,
            Some(true) if !rows.is_empty() && offset + PROGRAM_PAGE < 50_000 => (),
            _ => {
                return Err(Error::Response(
                    "podcast playback program pages do not advance",
                ));
            }
        }
    }
    Ok(voices)
}

// M71.d / M15.Ke. M3085 distinguishes a resolved missing list from a
// thrown request. Keep transport and successful-body validation separate.
const AROUND_RADIUS: usize = 250;
pub(in crate::client) async fn around_request(api: &dyn Api, episode: &str) -> Result<Value> {
    api.call(
        "/api/dj/around/program/v1",
        json!({"programId":episode,"roundNum":AROUND_RADIUS}),
        false,
        &[200],
    )
    .await
}

pub(in crate::client) fn around_response(
    response: &Value,
    ascending: bool,
) -> Result<Option<Vec<Episode>>> {
    let mut voices = parse_around_programs(response)?;
    if let Some(voices) = voices.as_mut() {
        order_around_programs(voices, response, ascending);
    }
    Ok(voices)
}

pub(in crate::client) fn parse_around_programs(response: &Value) -> Result<Option<Vec<Episode>>> {
    let data = &response["data"];
    if data["list"].is_null() {
        return Ok(None);
    }
    let rows = data["list"]
        .as_array()
        .filter(|rows| rows.len() <= 2 * AROUND_RADIUS + 1)
        .ok_or(Error::Response("invalid around-program list"))?;
    let mut seen = HashSet::new();
    let mut voices = Vec::with_capacity(rows.len());
    for raw in rows {
        let voice = Episode::from_official(raw)?;
        if !seen.insert(voice.id.clone()) {
            return Err(Error::QueueChanged);
        }
        voices.push(voice);
    }
    Ok(Some(voices))
}

pub(in crate::client) fn order_around_programs(
    voices: &mut [Episode],
    response: &Value,
    ascending: bool,
) {
    // M71: the comparison is strict; a missing asc is not boolean false.
    if response["data"]["asc"] != ascending {
        voices.reverse();
    }
}
