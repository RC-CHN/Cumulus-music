use super::MusicClient;
use crate::{
    Result,
    effects::{
        catalog::{CatalogueKind, EffectId},
        directory::{EffectDirectoryDetail, EffectDirectoryRow},
        service::EffectsLibrary,
    },
};

impl MusicClient {
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
}
