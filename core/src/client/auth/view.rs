use super::super::dto::AccountData;
use super::*;
use crate::protocol::http::MusicHttpClient;
use serde_json::Value;

pub(super) fn short_text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|s| s.chars().count() <= 512 && !s.contains(['\0', '\r']))
        .map(str::to_owned)
}

impl MusicClient {
    pub(super) fn native_account_client(&self) -> Result<Option<Arc<MusicHttpClient>>> {
        let token = self.session.token()?;
        let api: Arc<dyn std::any::Any + Send + Sync> = self.session.native_api(&token)?;
        Ok(api.downcast::<MusicHttpClient>().ok())
    }
    pub fn saved_accounts(&self, epoch: &str) -> Result<AccountData<Vec<SavedAccountView>>> {
        let token = self.auth_token(epoch)?;
        let records = self.registry.saved()?;
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: records
                .into_iter()
                .map(|record| {
                    Ok(SavedAccountView {
                        removal_pending: self.registry.removal_pending(&record.session_id)?,
                        id: record.session_id,
                        nickname: record.nickname,
                        avatar_url: mapping::artwork(&serde_json::json!(record.avatar_url)),
                        current: token.uid.as_ref() == Some(&record.uid),
                        auto_login: record.auto_login,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        })
    }
    pub fn login_snapshot(&self, id: &str) -> Result<LoginView> {
        let slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
        let pending = slot
            .as_ref()
            .filter(|p| p.id == id)
            .ok_or(Error::StaleResponse)?;
        let PendingFlow::Credentials { mode, flow } = &pending.flow else {
            return Err(Error::StaleResponse);
        };
        let state = flow.snapshot()?;
        Ok(LoginView {
            operation_id: id.into(),
            mode: *mode,
            status: state.status,
            code: state.code,
            busy: state.busy || pending.captcha.active()?,
            captcha_available: state.captcha_available,
            retry_after_ms: state.retry_after_ms,
            interrupted: state.error_kind.is_some(),
            challenge: pending.view.clone(),
        })
    }
}
