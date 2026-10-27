use super::{ResolvedSource, SourceResolver, cache::Key};
use crate::{
    Error, Result,
    catalog::{CatalogApi, PlayOptions},
    download::transfer::MediaUrl,
    playback::engine::MediaInput,
};
use serde_json::{Value, json};

impl SourceResolver {
    pub(super) async fn fetch(&self, resource: &Value, key: &Key) -> Result<ResolvedSource> {
        let info = self
            .session
            .run_for(&key.token, |api, _| async move {
                if let Some(voice_id) = &key.voice_id {
                    let api = crate::podcast::PodcastApi::new(api);
                    let episode = api.episode(&json!(voice_id)).await?;
                    if episode.media_id()? != key.id {
                        return Err(Error::Response("queued program main song changed"));
                    }
                    return api
                        .play_url(
                            &episode,
                            PlayOptions {
                                level: &key.level,
                                encode_type: &key.encode,
                                trial_mode: key.trial,
                            },
                        )
                        .await
                        .map(Some);
                }
                CatalogApi::new(api)
                    .play_url(
                        &json!(key.id),
                        PlayOptions {
                            level: &key.level,
                            encode_type: &key.encode,
                            trial_mode: key.trial,
                        },
                    )
                    .await
            })
            .await?
            .ok_or(Error::Response("no playback resource for requested track"))?;
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
        Ok(source)
    }
}
