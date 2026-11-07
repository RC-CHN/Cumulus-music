use super::{
    ResolvedSource, SourceResolver,
    cache::{Key, StockAuthorization},
};
use crate::{
    Error, Result,
    catalog::{CatalogApi, PlayOptions},
    download::transfer::MediaUrl,
    playback::engine::{Cancellation, MediaInput},
    protocol::api::Api,
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct Fetched {
    pub source: ResolvedSource,
    pub api: Arc<dyn Api>,
    pub authorization: Option<StockAuthorization>,
}

impl SourceResolver {
    pub(super) async fn fetch(
        &self,
        resource: &Value,
        key: &Key,
        expected: Option<&Arc<dyn Api>>,
        guard: Option<&Cancellation>,
    ) -> Result<Fetched> {
        let (info, authorization_api, authorization) = self
            .session
            .run_for(&key.token, |api, _| async move {
                if expected.is_some_and(|expected| !Arc::ptr_eq(expected, &api)) {
                    return Err(Error::StaleResponse);
                }
                let check = || guard.map_or(Ok(()), Cancellation::check);
                check()?;
                let stock_scope = (self.streaming_cache.is_some()
                    && key.voice_id.is_none()
                    && key.token.uid.is_some())
                .then(|| api.playback_source_credential_revision().ok().flatten())
                .flatten();
                let options = PlayOptions {
                    level: &key.level,
                    encode_type: &key.encode,
                    trial_mode: key.trial,
                };
                let info = if let Some(voice_id) = &key.voice_id {
                    let podcast = crate::podcast::PodcastApi::new(api.clone());
                    let episode = podcast.episode(&json!(voice_id)).await?;
                    if episode.media_id()? != key.id {
                        return Err(Error::Response("queued program main song changed"));
                    }
                    Some(podcast.play_url_guarded(&episode, options, check).await?)
                } else {
                    CatalogApi::new(api.clone())
                        .play_url(&json!(key.id), options)
                        .await?
                };
                let info =
                    info.ok_or(Error::Response("no playback resource for requested track"))?;
                let authorization = stock_scope
                    .filter(|revision| {
                        api.playback_source_credential_revision()
                            .ok()
                            .flatten()
                            .as_ref()
                            == Some(revision)
                    })
                    .map(|revision| StockAuthorization {
                        api: Arc::downgrade(&api),
                        revision,
                    });
                Ok((info, api, authorization))
            })
            .await?;
        // M/163 requires all three facts, independently of fee/trial flags.
        if info["code"].as_i64() != Some(200)
            || info["url"].as_str().is_none_or(|s| s.is_empty())
            || info["md5"].as_str().is_none_or(|s| s.is_empty())
        {
            return Err(Error::service(
                "/api/song/enhance/player/url/v1",
                info["code"].clone(),
                info,
            ));
        }
        let input = MediaInput::Url(MediaUrl::parse(
            info["url"].as_str().expect("checked above"),
        )?);
        let trial = !info["freeTrialInfo"].is_null();
        let full_trial = crate::protocol::wire::serialization::js_truthy(
            &info["freeTrialPrivilege"]["resConsumable"],
        ) && crate::protocol::wire::serialization::js_truthy(
            &info["freeTrialPrivilege"]["userConsumable"],
        );
        let mut rights = json!({});
        for field in [
            "fee",
            "flag",
            "freeTrialInfo",
            "freeTrialPrivilege",
            "rightSource",
            "level",
            "type",
            "br",
        ] {
            rights[field] = info[field].clone();
        }
        let mut source = ResolvedSource {
            input,
            expires_at: self.authorization_expiry(&info["expi"]),
            info: json!({"kind":"online","resource_info":info,"trial":trial,"rights":rights,"full_trial":full_trial}),
        };
        if trial {
            source.timeline(None)?;
            let mut duration = resource["metadata"]["duration"]
                .as_f64()
                .or_else(|| resource["dt"].as_f64().map(|v| v / 1000.0));
            if duration.is_none() {
                let songs = self
                    .session
                    .run_for(&key.token, |api, _| async move {
                        CatalogApi::new(api).songs(&[json!(key.id)], 100).await
                    })
                    .await?;
                duration = songs
                    .songs
                    .first()
                    .and_then(Option::as_ref)
                    .and_then(|song| song["dt"].as_f64())
                    .map(|v| v / 1000.0);
            }
            source.info["track_duration"] = json!(duration);
            source.timeline(None)?;
        }
        Ok(Fetched {
            source,
            api: authorization_api,
            authorization,
        })
    }
}
