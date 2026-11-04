//! Native headphone material preflight from the official model directory.
use super::{EffectsLibrary, records::MaterialHandle};
use crate::{
    Error, Result,
    account::SessionToken,
    download::transfer::RunOptions,
    effects::{
        api::EffectsApi,
        catalog::{EffectId, MaterialDescriptor},
        material::{Component, peq_calls, route_sections},
        parametric::ParametricPlan,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceMaterialCheck {
    NoMaterial,
    Pending,
    StandalonePeq,
    UnsupportedGraph,
}

impl EffectsLibrary {
    /// Verify one selected model and cache its exact device material. The
    /// directory supplies the descriptor; no URL or digest comes from IPC.
    pub async fn check_device_material_for(
        &self,
        token: &SessionToken,
        brand: EffectId,
        model: EffectId,
        options: RunOptions,
    ) -> Result<DeviceMaterialCheck> {
        if options.byte_budget == Some(0) {
            return Err(Error::InvalidInput("material byte budget must be positive"));
        }
        let records = self
            .session
            .run_for(token, |api, _| async move {
                EffectsApi::new(api).device_sounds(brand).await
            })
            .await?;
        if records.len() > 4096 {
            return Err(Error::Response("effect device directory is too large"));
        }
        let mut matching = records
            .iter()
            .filter(|row| EffectId::parse(&row["soundId"]).is_ok_and(|id| id == model));
        let row = matching
            .next()
            .ok_or(Error::Response("selected model not in this device brand"))?;
        if matching.next().is_some() {
            return Err(Error::Response("duplicate effect device identity"));
        }
        let sound_type = 2;
        if row["soundType"].as_u64() != Some(u64::from(sound_type)) {
            return Err(Error::Response("unsupported device material type"));
        }
        if row["soundMd5"].is_null() || row["soundMd5"].as_str() == Some("") {
            self.session.check_generation(token)?;
            return Ok(DeviceMaterialCheck::NoMaterial);
        }
        let descriptor = MaterialDescriptor::from_fields(
            &row["deviceSoundUrl"],
            &row["soundMd5"],
            &row["size"],
        )?;
        let owner = self.owner(token)?;
        let handle = MaterialHandle::new(model, sound_type, &descriptor);
        let _lock = self.lock(&owner, &handle.lock_key())?;
        let (_, _, material) = self
            .acquire_descriptor_locked(token, &owner, &handle, descriptor, options)
            .await?;
        let Some(material) = material else {
            return Ok(DeviceMaterialCheck::Pending);
        };
        let routing = route_sections(&material.parameters)?;
        let check = match routing.sections.as_slice() {
            [entry]
                if entry.component == Component::Peq
                    && ParametricPlan::from_calls(&peq_calls(entry.section)?)?.enabled() =>
            {
                DeviceMaterialCheck::StandalonePeq
            }
            _ => DeviceMaterialCheck::UnsupportedGraph,
        };
        self.session.with_token(token, |_| Ok(check))
    }
}
