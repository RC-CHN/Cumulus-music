use super::*;
use crate::{
    client::{dto::AccountData, mapping},
    mv::MvService,
};
use serde_json::json;
impl MusicClient {
    pub async fn mv_like(
        &self,
        epoch: &str,
        id: &str,
        desired: bool,
    ) -> Result<AccountData<MvLikeView>> {
        let token = self.mv.token(epoch)?;
        let value = MvService::new(self.session.clone())?
            .like_for(&token, id, desired)
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: view::like(value),
            })
        })
    }
    pub async fn mv_like_reconcile(
        &self,
        epoch: &str,
        id: &str,
    ) -> Result<AccountData<MvLikeView>> {
        let token = self.mv.token(epoch)?;
        let value = MvService::new(self.session.clone())?
            .reconcile_like_for(&token, id)
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: view::like(value),
            })
        })
    }
    pub async fn mv_recommendations(
        &self,
        epoch: &str,
        id: &str,
        refresh: bool,
    ) -> Result<AccountData<MvRecommendationsView>> {
        let token = self.mv.token(epoch)?;
        let service = MvService::new(self.session.clone())?;
        let rows = if refresh {
            service.recommendations_for(&token, id).await?
        } else {
            service
                .cached_recommendations_for(&token, id)?
                .ok_or(Error::State("MV recommendations are not cached"))?
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: MvRecommendationsView {
                    id: id.into(),
                    cached: !refresh,
                    items: rows
                        .into_iter()
                        .map(|r| MvRecommendationView {
                            id: r.id,
                            title: r.title,
                            artists: r.artists,
                            cover_url: r
                                .cover
                                .as_ref()
                                .and_then(|url| mapping::artwork(&json!(url))),
                            duration_ms: r.duration_ms,
                            play_count: r.play_count,
                        })
                        .collect(),
                },
            })
        })
    }
}
