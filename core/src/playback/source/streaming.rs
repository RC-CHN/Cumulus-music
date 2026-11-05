use super::{ResolvedSource, SourceResolver};
use crate::{
    Result,
    account::SessionToken,
    playback::{
        engine::{Cancellation, MediaInput},
        stream_cache::{StreamingAudioReader, StreamingCacheKey},
    },
    protocol::api::{Api, identifier},
};
use serde_json::Value;
use std::sync::Arc;

impl SourceResolver {
    /// No URL-cache lookup: each possible byte cache hit first obtains an
    /// online response under exactly the originally verified native API.
    pub(crate) async fn fresh_streaming_source(
        &self,
        resource: &Value,
        token: &SessionToken,
    ) -> Result<(Arc<ResolvedSource>, Arc<dyn Api>)> {
        let api = self.session.native_api(token)?;
        self.session
            .with_verified_profile(token, &api, |_, _| Ok(()))?;
        let key = self.key(resource, token)?;
        let source = self.fetch_bound(resource, &key, Some(&api)).await?;
        self.session
            .with_verified_profile(token, &api, |_, _| Ok(()))?;
        Ok((Arc::new(source), api))
    }

    pub(crate) fn validate_streaming_source(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<()> {
        self.session
            .with_verified_profile(token, api, |_, _| Ok(()))
    }

    pub(crate) async fn prepare_streaming_reader(
        self: &Arc<Self>,
        revision: u64,
        source: &ResolvedSource,
        resource: &Value,
        token: SessionToken,
        api: Arc<dyn Api>,
        transport: Cancellation,
    ) -> Result<Option<StreamingAudioReader>> {
        let Some(cache) = self.streaming_cache.as_ref() else {
            return Ok(None);
        };
        let MediaInput::Url(url) = &source.input else {
            return Ok(None);
        };
        let id = identifier(&resource["id"])?;
        let Some(expiry) = source.reuse_until().filter(|expiry| *expiry > self.now()) else {
            return Ok(None);
        };
        let key = self
            .session
            .with_verified_profile(&token, &api, |owner, _| Ok(eligible(source, owner, &id)))?;
        let Some(key) = key else { return Ok(None) };
        let resolver = self.clone();
        let authority = Cancellation::new(move || {
            !transport.is_cancelled()
                && resolver.now() < expiry
                && resolver.session.require_api(&token, &api).is_ok()
        });
        StreamingAudioReader::prepare(
            cache.store.clone(),
            cache.backend.clone(),
            revision,
            key,
            url.clone(),
            authority,
        )
        .await
    }
}

fn eligible(source: &ResolvedSource, owner: &str, id: &str) -> Option<StreamingCacheKey> {
    let info = &source.info["resource_info"];
    let privilege = &info["freeTrialPrivilege"];
    if source.info["kind"] != "online"
        || source.info["trial"] != false
        || source.info["full_trial"] != false
        || identifier(&info["id"]).ok()?.as_str() != id
        || info["type"] != "mp3"
        || info["fee"] != 0
        || info["flag"] != 0
        || !info["freeTrialInfo"].is_null()
        || !(privilege.is_null()
            || (privilege["resConsumable"] == false
                && privilege["userConsumable"] == false
                && privilege["listenType"].is_null()))
    {
        return None;
    }
    StreamingCacheKey::mp3(
        owner,
        id,
        info["level"].as_str()?,
        info["size"].as_u64()?,
        info["md5"].as_str()?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::transfer::MediaUrl;
    use serde_json::json;

    #[test]
    fn streaming_cache_admits_only_the_exact_plain_nontrial_representation() {
        let source = ResolvedSource {
            input: MediaInput::Url(
                MediaUrl::parse("https://fixture.music.126.net/proof.mp3").unwrap(),
            ),
            info: json!({"kind":"online","trial":false,"full_trial":false,"resource_info":{
                "id":"42","type":"mp3","fee":0,"flag":0,"level":"standard","size":100_000,
                "md5":"d41d8cd98f00b204e9800998ecf8427e"
            }}),
            expires_at: None,
        };
        assert!(eligible(&source, "9", "42").is_some());
        assert!(eligible(&source, "9", "43").is_none());
        for (field, value) in [
            ("type", json!("aac")),
            ("fee", json!(1)),
            ("fee", Value::Null),
            ("flag", json!(-1)),
            ("level", json!("jymaster")),
            ("size", json!(0)),
            ("md5", json!("invalid")),
            ("freeTrialInfo", json!({"start":0,"end":30})),
            (
                "freeTrialPrivilege",
                json!({"resConsumable":true,"userConsumable":true}),
            ),
        ] {
            let mut candidate = source.clone();
            candidate.info["resource_info"][field] = value;
            assert!(
                eligible(&candidate, "9", "42").is_none(),
                "unexpected {field}"
            );
        }
    }
}
