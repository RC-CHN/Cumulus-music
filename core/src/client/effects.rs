use super::MusicClient;
use crate::{
    Result,
    download::transfer::RunOptions,
    effects::{
        authorization::{BoundParametric, EffectAuthorizations},
        catalog::{CatalogueKind, EffectId},
        device_directory::EffectDeviceRow,
        directory::{EffectDirectoryDetail, EffectDirectoryRow},
        service::{EffectsLibrary, ParametricPreparation, PreparedParametric},
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

    /// Read and bind an already acknowledged standalone PEQ registration.
    /// This never starts registration or a trial.
    pub async fn bind_native_effect_parametric(
        &self,
        epoch: &str,
        prepared: Box<PreparedParametric>,
    ) -> Result<BoundParametric> {
        let token = self.player_token(epoch)?;
        let library = EffectsLibrary::new(self.session.clone())?;
        EffectAuthorizations::new(self.session.clone())?
            .bind_parametric_for(&library, &token, prepared)
            .await
    }

    /// Native host only: a sealed already-registered binding, supervised by
    /// the playback actor. No remote write or refresh is started here.
    pub async fn apply_native_bound_parametric(
        &self,
        epoch: &str,
        binding: BoundParametric,
    ) -> Result<crate::playback::controller::ParametricApplication> {
        let token = self.player_token(epoch)?;
        let result = self
            .player
            .controller()
            .await?
            .apply_bound_parametric(&token, binding)
            .await?;
        self.session.check_generation(&token)?;
        Ok(result)
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
