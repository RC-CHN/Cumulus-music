//! M/15/28/85/168 effect requests. Registration/release are explicit; reads
//! never start a trial, register an interest or initiate payment.
use super::{
    catalog::{CatalogueKind, EffectId, Sound},
    rights::Authorization,
};
use crate::{
    Error, Result,
    protocol::api::{
        Api, EffectCataloguePermission, EffectCatalogueRead, NativeEffectRegistration,
        effect_authorization_payload, effect_detail_payload, identifier,
    },
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone)]
pub struct EffectsApi {
    api: Arc<dyn Api>,
}
impl EffectsApi {
    pub fn new(api: Arc<dyn Api>) -> Self {
        Self { api }
    }
    async fn data(&self, path: &str, params: Value) -> Result<Value> {
        effect_data(self.api.call(path, params, false, &[200]).await?)
    }
    pub async fn catalogue(&self, kind: CatalogueKind) -> Result<Vec<Sound>> {
        sounds(
            self.data(
                "/api/usertool/sound/queryAllSound",
                json!({"exposeType":kind.expose_type()}),
            )
            .await?,
        )
    }
    pub async fn legacy_catalogue(&self, recommended: bool) -> Result<Vec<Sound>> {
        let read = if recommended {
            EffectCatalogueRead::LegacyRecommended
        } else {
            EffectCatalogueRead::LegacyAll
        };
        let (path, args) = read.into_request();
        legacy_sounds(self.data(path, args).await?)
    }

    pub(super) async fn directory_catalogue(
        &self,
        read: EffectCatalogueRead,
        revision: Option<&str>,
        current: &EffectCataloguePermission<'_>,
    ) -> Result<Vec<Sound>> {
        let data = self.guarded_data(read, revision, current).await?;
        if matches!(
            read,
            EffectCatalogueRead::LegacyRecommended | EffectCatalogueRead::LegacyAll
        ) {
            legacy_sounds(data)
        } else {
            sounds(data)
        }
    }
    async fn guarded_data(
        &self,
        read: EffectCatalogueRead,
        revision: Option<&str>,
        current: &EffectCataloguePermission<'_>,
    ) -> Result<Value> {
        effect_data(
            self.api
                .request_effect_catalogue(read, revision, current)
                .await?,
        )
    }
    pub async fn device_brands(&self) -> Result<Vec<Value>> {
        array(
            self.data("/api/usertool/home/soundeffect/devicebrand/list", json!({}))
                .await?,
        )
    }
    pub async fn device_sounds(&self, manager: EffectId) -> Result<Vec<Value>> {
        array(
            self.data(
                "/api/usertool/home/soundeffect/devicesound/list",
                json!({"deviceManagerId":manager.value()}),
            )
            .await?,
        )
    }
    pub async fn detail(&self, id: EffectId, sound_type: u32) -> Result<Option<Sound>> {
        let data = self
            .data(
                "/api/usertool/home/sound/info",
                effect_detail_payload(id, sound_type),
            )
            .await?;
        Self::detail_response(id, sound_type, data)
    }
    pub(super) async fn guarded_material_detail(
        &self,
        id: EffectId,
        revision: &str,
        current: &EffectCataloguePermission<'_>,
    ) -> Result<Option<Sound>> {
        let data = self
            .guarded_data(
                EffectCatalogueRead::PcMaterialDetail(id),
                Some(revision),
                current,
            )
            .await?;
        Self::detail_response(id, 10, data)
    }
    fn detail_response(id: EffectId, sound_type: u32, data: Value) -> Result<Option<Sound>> {
        let rows = data["soundList"]
            .as_array()
            .ok_or(Error::Response("missing sound detail list"))?;
        if rows.len() > 1 {
            return Err(Error::Response("ambiguous sound detail response"));
        }
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        if EffectId::parse(&row["soundId"])? != id
            || row["soundType"].as_u64() != Some(u64::from(sound_type))
        {
            return Err(Error::Response("sound detail identity mismatch"));
        }
        if row["pcSoundDetail"].is_null() {
            return Ok(None);
        }
        let sound = Sound::from_official(&row["pcSoundDetail"])?;
        if sound.id != id {
            return Err(Error::Response("PC sound identity mismatch"));
        }
        Ok(Some(sound))
    }
    /// M/342 uses 10 for PC detail even when catalogue soundType is 3 or 4.
    pub async fn pc_detail(&self, id: EffectId) -> Result<Option<Sound>> {
        self.detail(id, 10).await
    }
    pub async fn authorization(&self, id: EffectId) -> Result<Option<Authorization>> {
        self.authorization_inner(id, false).await
    }
    pub(super) async fn directory_authorization(
        &self,
        id: EffectId,
    ) -> Result<Option<Authorization>> {
        self.authorization_inner(id, true).await
    }
    async fn authorization_inner(
        &self,
        id: EffectId,
        require_observed_boolean: bool,
    ) -> Result<Option<Authorization>> {
        let data = self
            .data(
                "/api/vipauth/app/interests/auth/query",
                effect_authorization_payload(id),
            )
            .await?;
        Self::authorization_response(id, require_observed_boolean, data)
    }
    pub(super) async fn guarded_material_authorization(
        &self,
        id: EffectId,
        revision: &str,
        current: &EffectCataloguePermission<'_>,
    ) -> Result<Option<Authorization>> {
        let data = self
            .guarded_data(
                EffectCatalogueRead::MaterialAuthorization(id),
                Some(revision),
                current,
            )
            .await?;
        Self::authorization_response(id, true, data)
    }
    fn authorization_response(
        id: EffectId,
        require_observed_boolean: bool,
        data: Value,
    ) -> Result<Option<Authorization>> {
        let rows = data
            .as_array()
            .ok_or(Error::Response("invalid effect authorization list"))?;
        let mut found = None;
        for row in rows {
            if row["interestsId"].as_u64() == Some(id.value()) {
                if found.is_some() {
                    return Err(Error::Response("duplicate effect authorization"));
                }
                // M/85 matches interestsId; require a matching type when supplied.
                if row
                    .get("interestsType")
                    .is_some_and(|v| v.as_u64() != Some(u64::from(id.interests_type())))
                {
                    return Err(Error::Response("effect authorization type mismatch"));
                }
                let response = &row["authResp"];
                if require_observed_boolean && response["authResult"]["canUse"].as_bool().is_none()
                {
                    return Err(Error::Response("missing effect authorization decision"));
                }
                found = Some(Authorization::from_official(response));
            }
        }
        Ok(found.flatten())
    }
    pub async fn recycle_query(&self, id: EffectId) -> Result<Value> {
        // M/308 restoreAudioEffect -> M/28 Z: unlike auth/query, this is
        // the direct type-to-string-ID map, without an interestsMap wrapper.
        self.data(
            "/api/vipauth/app/interests/recyclequery",
            json!({"queryInfo":{id.interests_type().to_string():[id.value().to_string()]}}),
        )
        .await
    }
    /// A missing row is unknown, not evidence that registration was released.
    pub async fn recyclable(&self, id: EffectId) -> Result<Option<bool>> {
        Self::recycle_response(id, self.recycle_query(id).await?)
    }
    pub(super) async fn guarded_material_recyclable(
        &self,
        id: EffectId,
        revision: &str,
        current: &EffectCataloguePermission<'_>,
    ) -> Result<Option<bool>> {
        let value = self
            .guarded_data(
                EffectCatalogueRead::MaterialRecycle(id),
                Some(revision),
                current,
            )
            .await?;
        Self::recycle_response(id, value)
    }
    fn recycle_response(id: EffectId, value: Value) -> Result<Option<bool>> {
        if !value.is_object() {
            return Err(Error::Response("invalid effect recycle response"));
        }
        let Some(rows) = value.get(id.interests_type().to_string()) else {
            return Ok(None);
        };
        let rows = rows
            .as_array()
            .ok_or(Error::Response("invalid effect recycle list"))?;
        let mut found = None;
        for row in rows {
            if EffectId::parse(&row["interestsId"])? == id {
                if found.is_some() {
                    return Err(Error::Response("duplicate effect recycle result"));
                }
                found = Some(
                    row["canRecycle"]
                        .as_bool()
                        .ok_or(Error::Response("invalid effect recycle flag"))?,
                );
            }
        }
        Ok(found)
    }
    /// M/28 U, called with the string effect identity by M/342.
    pub async fn register_interest(&self, id: EffectId) -> Result<()> {
        let value = self.api.call("/api/vipauth/app/interests/auth/set", json!({"interestsType":id.interests_type(),"interestsId":id.value().to_string(),"type":0}), false, &[200]).await?;
        acknowledged(value)
    }
    pub(super) async fn guarded_register_interest(
        &self,
        id: EffectId,
        revision: &str,
        current: &EffectCataloguePermission<'_>,
    ) -> Result<()> {
        let write = NativeEffectRegistration::ordinary(id)?;
        acknowledged(
            self.api
                .request_native_effect_registration(write, revision, current)
                .await?,
        )
    }
    /// M/28 G. A successful acknowledgement does not imply auth/query.canUse
    /// becomes false: membership and occupancy are different server concepts.
    pub async fn release_interest(&self, id: EffectId) -> Result<()> {
        let value = self.api.call("/api/vipauth/app/interests/auth/stop", json!({"interestsType":id.interests_type(),"interestsId":id.value().to_string(),"type":1}), false, &[200]).await?;
        acknowledged(value)
    }
    pub async fn smart_key(&self, song: &Value) -> Result<String> {
        let value = self
            .data(
                "/api/usertool/smart/sound/get",
                json!({"songId":identifier(song)?}),
            )
            .await?;
        value["soundKey"]
            .as_str()
            .map(str::to_owned)
            .ok_or(Error::Response("missing smart sound key"))
    }
    pub async fn song_parameters(&self, song: &Value) -> Result<Option<Value>> {
        self.single_song("/api/vip-rights/svip/sound/effect/get", song)
            .await
    }
    pub async fn emotion_features(&self, song: &Value) -> Result<Option<Value>> {
        self.single_song("/api/vip-rights/emotion/audio/effect/get", song)
            .await
    }
    async fn single_song(&self, path: &str, song: &Value) -> Result<Option<Value>> {
        let id = identifier(song)?;
        let rows = array(self.data(path, json!({"songIds":[id]})).await?)?;
        if rows.len() > 1 {
            return Err(Error::Response("ambiguous single-song effect response"));
        }
        // The AI record's id is not assumed to be the requested song identity.
        // Official callers bind the returned record to their captured trackId.
        Ok(rows.into_iter().next())
    }
}
fn acknowledged(value: Value) -> Result<()> {
    // Current official service returns a boolean acknowledgement. A code 200
    // envelope with false/missing/malformed data cannot confirm a mutation.
    if value.get("data").and_then(Value::as_bool) != Some(true) {
        return Err(Error::Response(
            "effect mutation has no positive acknowledgement",
        ));
    }
    Ok(())
}
fn effect_data(value: Value) -> Result<Value> {
    value
        .get("data")
        .cloned()
        .ok_or(Error::Response("missing effect response data"))
}
fn array(value: Value) -> Result<Vec<Value>> {
    let rows = value
        .as_array()
        .filter(|v| v.len() <= 10000)
        .ok_or(Error::Response("invalid effect list"))?;
    if rows.iter().any(|v| !v.is_object()) {
        return Err(Error::Response("effect list item is not an object"));
    }
    Ok(rows.clone())
}
pub(super) fn sounds(value: Value) -> Result<Vec<Sound>> {
    let mut seen = BTreeSet::new();
    array(value)?
        .iter()
        .map(|value| {
            let item = Sound::from_official(value)?;
            if !seen.insert(item.id) {
                return Err(Error::Response("duplicate sound identity"));
            }
            Ok(item)
        })
        .collect()
}

fn legacy_sounds(value: Value) -> Result<Vec<Sound>> {
    // M/85 maps every row in order. M/3091 only removes recommendations
    // appearing in the first six, and does not deduplicate either response.
    array(value)?.iter().map(Sound::from_official).collect()
}
