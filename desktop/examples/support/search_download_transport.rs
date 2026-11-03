//! Private, one-receipt transport for the opt-in standalone download proof.
use super::allowed_audio;
use cumulus_core::{
    Error, Result,
    download::transfer::{MediaBackend, MediaBody, MediaReply, MediaRequest, ReqwestMediaBackend},
    media::tags::official_requires_encryption,
    protocol::api::{ApiFuture, identifier},
};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Mutex, time::Duration};

pub const PATH: &str = "/api/song/enhance/download/url/v1";
pub const URL: &str = "https://m1.music.126.net/cumulus-download-proof-only.flac";
const MAX_SIZE: u64 = 32 * 1024 * 1024;
pub const MD5: &str = "6b1c7d8698e1e941ad6f2046faa0b611";

#[derive(Default)]
struct State {
    song: Option<String>,
    owner: Option<String>,
    free: bool,
    receipt: Option<String>,
    reads: u8,
    offline: bool,
    blocked: u8,
}
pub struct Gate {
    state: Mutex<State>,
    transport: Option<ReqwestMediaBackend>,
}
impl Gate {
    pub fn new(live: bool) -> Result<Self> {
        Ok(Self {
            state: Mutex::default(),
            transport: live
                .then(|| ReqwestMediaBackend::new(Duration::from_secs(20)))
                .transpose()?,
        })
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| Error::LockPoisoned)
    }
    pub fn choose(&self, song: &str) -> Result<()> {
        let mut state = self.lock()?;
        if state.song.is_some() {
            return Err(Error::State("proof selection already set"));
        }
        state.song = Some(song.to_owned());
        Ok(())
    }
    pub fn identity(&self, response: &Value) -> Result<()> {
        self.lock()?.owner = Some(identifier(&response["profile"]["userId"])?);
        Ok(())
    }
    pub fn owner(&self) -> Result<String> {
        self.lock()?
            .owner
            .clone()
            .ok_or(Error::State("missing proof identity"))
    }
    pub fn check_api(&self) -> Result<()> {
        let mut state = self.lock()?;
        if state.offline {
            state.blocked = state.blocked.saturating_add(1);
            return Err(Error::Transport("proof API is disconnected".into()));
        }
        Ok(())
    }
    pub fn detail(&self, payload: &Value) -> Result<()> {
        let rows: Vec<Value> = serde_json::from_str(
            payload["c"]
                .as_str()
                .ok_or(Error::Response("proof detail lacks IDs"))?,
        )?;
        let state = self.lock()?;
        if rows.len() != 1 || state.song.as_deref() != Some(&identifier(&rows[0]["id"])?) {
            return Err(Error::InvalidInput("proof detail outside selection"));
        }
        Ok(())
    }
    pub fn record_detail(&self, response: &Value) -> Result<()> {
        let mut state = self.lock()?;
        let Some(rows) = response["songs"].as_array().filter(|rows| rows.len() == 1) else {
            return Err(Error::Response("missing selected proof song"));
        };
        state.free = state.song.as_deref() == Some(&identifier(&rows[0]["id"])?)
            && rows[0]["fee"] == 0
            && response["privileges"].as_array().is_some_and(|rows| {
                rows.len() == 1
                    && rows[0]["fee"] == 0
                    && rows[0]["st"] == 0
                    && rows[0]["dl"].as_u64().is_some_and(|rate| rate > 0)
                    && identifier(&rows[0]["id"])
                        .is_ok_and(|id| Some(id.as_str()) == state.song.as_deref())
            });
        Ok(())
    }
    pub fn allow_download(&self, payload: &Value) -> Result<()> {
        let state = self.lock()?;
        if !state.free
            || state.song.as_deref() != Some(&identifier(&payload["id"])?)
            || payload["level"] != "standard"
            || payload["immerseType"] != "c51"
        {
            return Err(Error::State(
                "proof requires an explicitly free standard download",
            ));
        }
        Ok(())
    }
    pub fn receipt(&self, response: &Value) -> Result<()> {
        let mut state = self.lock()?;
        let row = &response["data"];
        let url = row["url"]
            .as_str()
            .filter(|url| allowed_audio(url))
            .ok_or(Error::Response("proof download CDN unavailable"))?;
        if state.song.as_deref() != Some(&identifier(&row["id"])?)
            || state.receipt.is_some()
            || row["code"] != 200
            || row["level"] != "standard"
            || !row["freeTrialInfo"].is_null()
            || !matches!(row["type"].as_str(), Some("flac" | "mp3" | "m4a" | "wav"))
            || !row["size"]
                .as_u64()
                .is_some_and(|size| (1..=MAX_SIZE).contains(&size))
            || !row["md5"]
                .as_str()
                .is_some_and(|hash| hash.len() == 32 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
            || official_requires_encryption(word(&row["flag"])?, word(&row["payed"])?)
        {
            return Err(Error::Response(
                "proof download receipt is outside plain media limits",
            ));
        }
        state.receipt = Some(url.to_owned());
        Ok(())
    }
    pub fn cutoff(&self) -> std::result::Result<(), &'static str> {
        let mut state = self.lock().map_err(|_| "download_gate")?;
        if state.reads != 1 || state.offline {
            return Err("download_requires_one_media_read");
        }
        state.offline = true;
        Ok(())
    }
    pub fn facts(&self) -> Result<(u8, bool, u8)> {
        let state = self.lock()?;
        Ok((state.reads, state.offline, state.blocked))
    }
}
fn word(value: &Value) -> Result<u32> {
    if value.is_null() {
        return Ok(0);
    }
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(Error::Response("invalid proof resource flag"))
}

impl MediaBackend for Gate {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            {
                let mut state = self.lock()?;
                if state.offline {
                    state.blocked = state.blocked.saturating_add(1);
                    return Err(Error::Transport("proof media is disconnected".into()));
                }
                if state.reads != 0
                    || state.receipt.as_deref() != Some(request.url.as_str())
                    || request.offset != 0
                    || request.if_range.is_some()
                {
                    return Err(Error::InvalidInput("outside the single download receipt"));
                }
                state.reads = 1;
            }
            if let Some(transport) = &self.transport {
                return transport.get(request).await;
            }
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([("content-length".into(), super::SEED.len().to_string())]),
                body: Box::new(Synthetic(super::SEED)),
            })
        })
    }
}
struct Synthetic(&'static [u8]);
impl MediaBody for Synthetic {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move {
            let n = self.0.len().min(buffer.len());
            buffer[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cumulus_core::download::transfer::MediaUrl;
    use serde_json::json;

    fn receipt() -> Value {
        json!({"data":{"id":1,"url":URL,"code":200,"type":"flac",
            "level":"standard","size":super::super::SEED.len(),"md5":MD5}})
    }

    #[tokio::test]
    async fn paid_or_unverified_media_never_reaches_the_download_transport() {
        let gate = Gate::new(false).unwrap();
        gate.choose("1").unwrap();
        let request = json!({"id":"1","level":"standard","immerseType":"c51"});
        let mut detail = json!({"songs":[{"id":1,"fee":0}],
            "privileges":[{"id":1,"fee":1,"st":0,"dl":128000}]});
        gate.record_detail(&detail).unwrap();
        assert!(gate.allow_download(&request).is_err());
        detail["privileges"][0]["fee"] = json!(0);
        gate.record_detail(&detail).unwrap();
        gate.allow_download(&request).unwrap();
        let mut encrypted = receipt();
        encrypted["data"]["flag"] = json!(32);
        assert!(gate.receipt(&encrypted).is_err());
        let mut wrong_host = receipt();
        wrong_host["data"]["url"] = json!("https://example.invalid/audio");
        assert!(gate.receipt(&wrong_host).is_err());
        let media = MediaRequest {
            url: MediaUrl::parse(URL).unwrap(),
            offset: 0,
            if_range: None,
        };
        assert!(gate.get(media).await.is_err());
        assert_eq!(gate.facts().unwrap(), (0, false, 0));
    }

    #[tokio::test]
    async fn a_valid_receipt_is_single_use_and_cutoff_rejects_both_transports() {
        let gate = Gate::new(false).unwrap();
        gate.choose("1").unwrap();
        let wrong = MediaRequest {
            url: MediaUrl::parse(URL).unwrap(),
            offset: 4,
            if_range: None,
        };
        gate.receipt(&receipt()).unwrap();
        assert!(gate.get(wrong).await.is_err());
        let valid = MediaRequest {
            url: MediaUrl::parse(URL).unwrap(),
            offset: 0,
            if_range: None,
        };
        assert_eq!(gate.get(valid.clone()).await.unwrap().status, 200);
        assert!(gate.get(valid.clone()).await.is_err());
        gate.cutoff().unwrap();
        assert!(gate.get(valid).await.is_err());
        assert!(gate.check_api().is_err());
        assert_eq!(gate.facts().unwrap(), (1, true, 2));
    }
}
