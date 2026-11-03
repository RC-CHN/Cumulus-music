//! Only the recovered PC brand/model labels. The native device descriptors
//! also contain artwork and material locators that never cross this boundary.
use super::{api::EffectsApi, catalog::EffectId, service::EffectsLibrary};
use crate::{Error, Result, account::SessionToken};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EffectDeviceRow {
    pub id: EffectId,
    pub name: String,
}

fn project(
    records: Vec<Value>,
    id_key: &str,
    name_key: &str,
    limit: usize,
) -> Result<Vec<EffectDeviceRow>> {
    if records.len() > limit {
        return Err(Error::Response("effect device directory is too large"));
    }
    let mut ids = HashSet::with_capacity(records.len());
    records
        .into_iter()
        .map(|record| {
            let id = EffectId::parse(&record[id_key])
                .map_err(|_| Error::Response("invalid effect device identity"))?;
            if !ids.insert(id.value()) {
                return Err(Error::Response("duplicate effect device identity"));
            }
            let name: String = record[name_key]
                .as_str()
                .ok_or(Error::Response("invalid effect device name"))?
                .trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(161)
                .collect();
            if name.is_empty() || name.chars().count() > 160 {
                return Err(Error::Response("invalid effect device name"));
            }
            Ok(EffectDeviceRow { id, name })
        })
        .collect()
}

impl EffectsLibrary {
    pub async fn device_brands_for(&self, token: &SessionToken) -> Result<Vec<EffectDeviceRow>> {
        let records = self
            .session
            .run_for(token, |api, _| async move {
                EffectsApi::new(api).device_brands().await
            })
            .await?;
        let rows = project(records, "deviceManagerId", "deviceName", 1024)?;
        self.session.with_token(token, |_| Ok(rows))
    }

    pub async fn device_models_for(
        &self,
        token: &SessionToken,
        brand: EffectId,
    ) -> Result<Vec<EffectDeviceRow>> {
        let records = self
            .session
            .run_for(token, |api, _| async move {
                EffectsApi::new(api).device_sounds(brand).await
            })
            .await?;
        let rows = project(records, "soundId", "deviceModel", 4096)?;
        self.session.with_token(token, |_| Ok(rows))
    }
}
