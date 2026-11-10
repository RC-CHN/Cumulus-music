use super::MusicClient;
use crate::{
    Error, Result,
    download::transfer::RunOptions,
    effects::{
        authorization::{EffectAuthorizations, NativeModernLimiterBinding},
        catalog::EffectId,
        service::ModernLimiterPreflight,
    },
};

impl MusicClient {
    /// Explicit native host action for a fresh acknowledged registration.
    /// It returns only a pre-handoff binding: no audio is switched, no trial
    /// is started and there is no desktop command for this operation.
    pub async fn register_native_owned_modern_limiter(
        &self,
        epoch: &str,
        id: EffectId,
        options: RunOptions,
    ) -> Result<NativeModernLimiterBinding> {
        let token = self.player_token(epoch)?;
        let source = self
            .player
            .controller()
            .await?
            .capture_modern_effect_source(&token)
            .await?;
        source.check_for(&self.session, &token)?;
        let library = self.effect_material_library()?;
        let material = match library
            .preflight_modern_limiter_for(&token, id, options)
            .await?
        {
            ModernLimiterPreflight::Ready(material) => material,
            ModernLimiterPreflight::Pending { .. } => {
                return Err(Error::State("modern effect material is incomplete"));
            }
            ModernLimiterPreflight::Unsupported => {
                return Err(Error::State("unsupported modern effect material"));
            }
        };
        EffectAuthorizations::new(self.session.clone())?
            .register_owned_modern_limiter_for(&library, &token, material, source)
            .await
    }

    pub async fn validate_native_owned_modern_limiter(
        &self,
        epoch: &str,
        binding: &NativeModernLimiterBinding,
    ) -> Result<()> {
        let token = self.player_token(epoch)?;
        EffectAuthorizations::new(self.session.clone())?
            .validate_owned_modern_limiter_for(&token, binding)
            .await
    }
}
