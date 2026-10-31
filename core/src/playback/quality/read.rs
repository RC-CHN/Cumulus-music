use super::{
    QualityApi, QualityFormat, QualityPrivilege, QualityRight, QualitySizes, QualitySubscriber,
    QualityUseRight,
};
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    catalog::CatalogApi,
    membership::MembershipManager,
};
use serde::Serialize;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityReadState {
    Ready,
    Missing,
    Failed,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityReadStatus {
    pub privilege: QualityReadState,
    pub subscriber: QualityReadState,
    pub sizes: QualityReadState,
    pub use_rights: BTreeMap<QualityRight, QualityReadState>,
}
pub struct QualityInformation {
    pub privilege: Option<QualityPrivilege>,
    pub subscriber: Option<QualitySubscriber>,
    pub sizes: Option<QualitySizes>,
    pub use_rights: BTreeMap<QualityRight, Option<QualityUseRight>>,
    pub status: QualityReadStatus,
    pub membership_refresh_after_ms: Option<f64>,
}
#[derive(Clone)]
pub struct QualityReader {
    session: Arc<AccountSession>,
    timeout: Duration,
}
impl QualityReader {
    pub fn new(session: Arc<AccountSession>) -> Self {
        Self {
            session,
            timeout: Duration::from_secs(8),
        }
    }
    async fn bounded<T>(&self, task: impl std::future::Future<Output = Result<T>>) -> Result<T> {
        tokio::time::timeout(self.timeout, task)
            .await
            .map_err(|_| Error::State("quality metadata request timed out"))?
    }
    async fn right(
        &self,
        token: &SessionToken,
        right: QualityRight,
    ) -> Result<Option<QualityUseRight>> {
        self.bounded(self.session.run_for(token, |api, _| async move {
            QualityApi::new(api).use_right(right).await
        }))
        .await
    }
    /// All operations are reads. Missing/error rows remain unknown independently;
    /// login/account cancellation remains a fatal error for the whole snapshot.
    pub async fn read_for(
        &self,
        token: &SessionToken,
        song: &str,
        trial_mode: Option<i32>,
        format: QualityFormat,
    ) -> Result<QualityInformation> {
        let song = crate::protocol::api::identifier(&json!(song))?;
        self.session.with_token(token, |_| Ok(()))?;
        let membership = async {
            let result = MembershipManager::new(self.session.clone())?
                .refresh_for(token.clone())
                .await?;
            Ok((
                QualitySubscriber::from(&result.state),
                result.refresh_after_ms,
            ))
        };
        let (privileges, sizes, subscriber, vivid, master, sky, effect) = tokio::join!(
            self.bounded(self.session.run_for(token, |api, _| async {
                CatalogApi::new(api)
                    .privileges(std::slice::from_ref(&song), trial_mode)
                    .await
            })),
            self.bounded(self.session.run_for(token, |api, _| async {
                QualityApi::new(api).sizes(&json!(song), format).await
            })),
            self.bounded(membership),
            self.right(token, QualityRight::Vivid),
            self.right(token, QualityRight::JyMaster),
            self.right(token, QualityRight::Sky),
            self.right(token, QualityRight::JyEffect),
        );
        self.session.check_generation(token)?;
        let privileges = privileges.and_then(|rows| {
            rows.into_iter()
                .rev()
                .find(|r| crate::protocol::api::identifier(&r["id"]).is_ok_and(|id| id == song))
                .map(|row| QualityPrivilege::from_official(&row))
                .transpose()
        });
        let (privilege, privilege_status) = part(privileges)?;
        let (sizes, sizes_status) = part(sizes.map(Some))?;
        let (subscriber, subscriber_status) = part(subscriber.map(Some))?;
        let (subscriber, refresh) = subscriber
            .map(|(s, r)| (Some(s), r))
            .unwrap_or((None, None));
        let mut use_rights = BTreeMap::new();
        let mut rights_status = BTreeMap::new();
        for (right, value) in [
            (QualityRight::Vivid, vivid),
            (QualityRight::JyMaster, master),
            (QualityRight::Sky, sky),
            (QualityRight::JyEffect, effect),
        ] {
            let (value, status) = part(value)?;
            use_rights.insert(right, value);
            rights_status.insert(right, status);
        }
        Ok(QualityInformation {
            privilege,
            subscriber,
            sizes,
            use_rights,
            membership_refresh_after_ms: refresh,
            status: QualityReadStatus {
                privilege: privilege_status,
                subscriber: subscriber_status,
                sizes: sizes_status,
                use_rights: rights_status,
            },
        })
    }
}
fn part<T>(result: Result<Option<T>>) -> Result<(Option<T>, QualityReadState)> {
    match result {
        Ok(Some(value)) => Ok((Some(value), QualityReadState::Ready)),
        Ok(None) => Ok((None, QualityReadState::Missing)),
        Err(error)
            if error.is_service_code(301)
                || matches!(error.root(), Error::StaleResponse | Error::Cancelled) =>
        {
            Err(error)
        }
        Err(_) => Ok((None, QualityReadState::Failed)),
    }
}
