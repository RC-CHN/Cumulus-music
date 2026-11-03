use super::MusicClient;
use crate::{
    Result,
    download::transfer::RunOptions,
    effects::{
        catalog::{CatalogueKind, EffectId},
        device_directory::EffectDeviceRow,
        directory::{EffectDirectoryDetail, EffectDirectoryRow},
        service::{EffectsLibrary, ParametricPreparation},
    },
};

impl MusicClient {
    /// Native-only preparation of one verified PEQ component. This does not
    /// register an effect, acquire a trial, or apply anything to playback.
    pub async fn prepare_native_effect_parametric(
        &self,
        epoch: &str,
        id: EffectId,
        options: RunOptions,
    ) -> Result<ParametricPreparation> {
        let token = self.player_token(epoch)?;
        EffectsLibrary::new(self.session.clone())?
            .prepare_parametric_for(&token, id, options)
            .await
    }

    pub async fn effect_catalogue(
        &self,
        epoch: &str,
        kind: CatalogueKind,
    ) -> Result<Vec<EffectDirectoryRow>> {
        let token = self.player_token(epoch)?;
        EffectsLibrary::new(self.session.clone())?
            .directory_for(&token, kind)
            .await
    }

    pub async fn effect_detail(
        &self,
        epoch: &str,
        id: EffectId,
    ) -> Result<Option<EffectDirectoryDetail>> {
        let token = self.player_token(epoch)?;
        EffectsLibrary::new(self.session.clone())?
            .directory_detail_for(&token, id)
            .await
    }
    pub async fn effect_device_brands(&self, epoch: &str) -> Result<Vec<EffectDeviceRow>> {
        let token = self.player_token(epoch)?;
        EffectsLibrary::new(self.session.clone())?
            .device_brands_for(&token)
            .await
    }

    pub async fn effect_device_models(
        &self,
        epoch: &str,
        brand: EffectId,
    ) -> Result<Vec<EffectDeviceRow>> {
        let token = self.player_token(epoch)?;
        EffectsLibrary::new(self.session.clone())?
            .device_models_for(&token, brand)
            .await
    }
}
