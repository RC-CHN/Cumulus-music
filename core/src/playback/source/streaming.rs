use super::{ResolvedSource, SourceResolver, cache::Provenance};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::{
        engine::{Cancellation, MediaInput},
        stream_cache::{StreamingAudioReader, StreamingCacheKey},
    },
    protocol::api::{Api, identifier},
};
use serde_json::Value;
use std::sync::Arc;

pub(crate) struct StreamingSelection {
    pub source: Arc<ResolvedSource>,
    pub fresh_api: Option<Arc<dyn Api>>,
    pub stock_fallback: Option<Cancellation>,
}

impl StreamingSelection {
    pub(crate) fn stock(source: Arc<ResolvedSource>) -> Self {
        Self {
            source,
            fresh_api: None,
            stock_fallback: None,
        }
    }
}

impl SourceResolver {
    /// Select the stock source first. Only a fresh request owned by this
    /// selection, or a separately reauthorized exact warm candidate, may
    /// replace the stock URL with our optional byte reader.
    pub(crate) async fn resolve_streaming_source(
        &self,
        resource: &Value,
        token: &SessionToken,
        revision: u64,
    ) -> Result<StreamingSelection> {
        let (stock, provenance) = self
            .resolve_online_observed(resource, token, false, None)
            .await?;
        let Some(owner) = token.uid.as_deref() else {
            return Ok(StreamingSelection::stock(stock));
        };
        let id = identifier(&resource["id"])?;
        let Some(key) = eligible(&stock, owner, &id) else {
            return Ok(StreamingSelection::stock(stock));
        };
        let stock_authorization = match provenance {
            Provenance::Fresh(api) => {
                let mut selected = StreamingSelection::stock(stock);
                selected.fresh_api = self
                    .validate_streaming_source(token, &api)
                    .is_ok()
                    .then_some(api);
                return Ok(selected);
            }
            Provenance::Memory(scope) => scope,
            Provenance::Shared => None,
        };
        if !self
            .streaming_cache
            .as_ref()
            .is_some_and(|cache| cache.store.has_hit(revision, &key).is_ok_and(|hit| hit))
        {
            return Ok(StreamingSelection::stock(stock));
        }
        let api = self.session.native_api(token)?;
        self.validate_streaming_source(token, &api)?;
        // This exact prior byte hit is the only reason to discard the stock
        // memory URL. Fresh invalidates its cache entry even on failure.
        let renewed = self
            .resolve_online_observed(resource, token, true, Some(&api))
            .await;
        let (current, provenance) = match renewed {
            Ok(current) => current,
            Err(error) if matches!(error.root(), Error::Transport(_)) => {
                // Only ambiguous transport failure can use a still-bound
                // original URL for this one load. It never returns warm bytes
                // or republishes the old authorization into the memory cache.
                let Some(scope) = stock_authorization.filter(|scope| {
                    scope
                        .api
                        .upgrade()
                        .is_some_and(|origin| Arc::ptr_eq(&api, &origin))
                }) else {
                    return Err(error);
                };
                let Some(deadline) = stock.reuse_until() else {
                    return Err(error);
                };
                let (clock, session, token) =
                    (self.clock.clone(), self.session.clone(), token.clone());
                let guard = Cancellation::new(move || {
                    clock.now() < deadline
                        && session
                            .with_verified_profile(&token, &api, |_, _| Ok(()))
                            .is_ok()
                        && api
                            .playback_source_credential_revision()
                            .ok()
                            .flatten()
                            .as_deref()
                            == Some(scope.revision.as_str())
                });
                if guard.is_cancelled() {
                    return Err(error);
                }
                return Ok(StreamingSelection {
                    source: stock,
                    fresh_api: None,
                    stock_fallback: Some(guard),
                });
            }
            Err(error) => return Err(error),
        };
        self.validate_streaming_source(token, &api)?;
        let mut selected = StreamingSelection::stock(current);
        if let Provenance::Fresh(verified) = provenance
            && Arc::ptr_eq(&verified, &api)
            && eligible(&selected.source, owner, &id).is_some_and(|current| current == key)
        {
            selected.fresh_api = Some(verified);
        }
        Ok(selected)
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

    #[tokio::test]
    async fn unsupported_and_memory_only_misses_keep_the_stock_authorization_requests() {
        use crate::{
            Error, account::AccountSession, playback::stream_cache::StreamingCache,
            protocol::api::ApiFuture, storage::StateStore,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Default)]
        struct Authorization(AtomicUsize);
        impl Api for Authorization {
            fn request<'a>(
                &'a self,
                path: &'a str,
                args: Value,
                cache: bool,
            ) -> ApiFuture<'a, Value> {
                Box::pin(async move {
                    if path != "/api/song/enhance/player/url/v1" || cache {
                        return Err(Error::Response("closed stock selection proof"));
                    }
                    let ids: Vec<String> = serde_json::from_str(args["ids"].as_str().unwrap())?;
                    let id = &ids[0];
                    self.0.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({"code":200,"data":[{"id":id,"code":200,
                        "url":"https://fixture.music.126.net/proof.mp3",
                        "type":"mp3","fee":u8::from(id == "42"),"flag":0,
                        "level":"standard","size":100_000,"expi":120,
                        "md5":"d41d8cd98f00b204e9800998ecf8427e"
                    }]}))
                })
            }
        }
        let root = tempfile::tempdir().unwrap();
        let state = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(Authorization::default());
        let session = Arc::new(AccountSession::new(api.clone(), Some(state.clone())));
        session
            .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
            .unwrap();
        let token = session.token().unwrap();
        let mut resolver = SourceResolver::new(session, None);
        resolver.streaming_cache = Some(StreamingCache::open(&state, 512 * 1024).unwrap());
        let unsupported = json!({"id":"42"});
        let first = resolver
            .resolve_streaming_source(&unsupported, &token, 0)
            .await
            .unwrap();
        assert!(first.fresh_api.is_none());
        let memory = resolver
            .resolve_streaming_source(&unsupported, &token, 0)
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&first.source, &memory.source) && memory.fresh_api.is_none());
        assert_eq!(api.0.load(Ordering::SeqCst), 1);

        let ordinary = json!({"id":"43"});
        let first = resolver
            .resolve_streaming_source(&ordinary, &token, 0)
            .await
            .unwrap();
        assert!(
            first.fresh_api.is_some(),
            "fresh stock request can qualify a first fill"
        );
        let memory = resolver
            .resolve_streaming_source(&ordinary, &token, 0)
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&first.source, &memory.source) && memory.fresh_api.is_none());
        assert_eq!(
            api.0.load(Ordering::SeqCst),
            2,
            "a memory URL without an exact byte hit never forces new authorization"
        );
    }
}
