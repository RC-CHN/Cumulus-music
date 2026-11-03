use super::{MusicClient, PendingFlow, PendingLogin};
use crate::{
    Error, Result,
    account::profiles::{QuickLoginTransport, ServerQuickLoginPhase, ServerQuickLoginView},
    client::dto::AccountData,
    protocol::http::ReqwestBackend,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerQuickLoginCommand {
    Execute,
    Reconcile,
    Cancel,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerQuickLoginOperationView {
    pub operation_id: String,
    #[serde(flatten)]
    pub state: ServerQuickLoginView,
}

fn allow_quick_replacement(previous: Option<&PendingLogin>) -> Result<()> {
    if let Some(PendingLogin {
        flow: PendingFlow::ServerQuick(flow),
        ..
    }) = previous
        && !matches!(
            flow.snapshot()?.phase,
            ServerQuickLoginPhase::Ready | ServerQuickLoginPhase::Cancelled
        )
    {
        return Err(Error::State("cancel the previous server quick login first"));
    }
    Ok(())
}

fn cancelled_view() -> ServerQuickLoginView {
    ServerQuickLoginView {
        phase: ServerQuickLoginPhase::Cancelled,
        can_reconcile: false,
        status: None,
    }
}

impl MusicClient {
    pub async fn begin_server_quick_login(
        &self,
        epoch: &str,
        saved_id: &str,
        consent: bool,
    ) -> Result<AccountData<ServerQuickLoginOperationView>> {
        if !consent {
            return Err(Error::InvalidInput("server quick login requires consent"));
        }
        let base = self.auth_token(epoch)?;
        if base.uid.is_some() {
            return Err(Error::State(
                "server quick login is only available when signed out",
            ));
        }
        let previous_id = {
            let pending = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
            allow_quick_replacement(pending.as_ref())?;
            pending.as_ref().map(|attempt| attempt.id.clone())
        };
        let backend = match &self.auth.backend {
            Some(backend) => backend.clone(),
            None => Arc::new(ReqwestBackend::new(Duration::from_secs(20))?),
        };
        // Don't hold the account transition gate during a read-only preflight.
        // A regular login may supersede this before the native attempt exists.
        let flow = Arc::new(
            self.registry
                .begin_guest_server_quick_for(
                    self.session.clone(),
                    &base,
                    saved_id,
                    QuickLoginTransport {
                        profile: self.auth.profile.clone(),
                        options: self.auth.options,
                        context: self.auth.context.clone(),
                        backend,
                    },
                )
                .await?,
        );
        let _transition = self.together.gate.lock().await;
        self.session.check_generation(&base)?;
        let mut pending = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
        if pending.as_ref().map(|attempt| &attempt.id) != previous_id.as_ref() {
            return Err(Error::StaleResponse);
        }
        allow_quick_replacement(pending.as_ref())?;
        if let Some(previous) = pending.as_ref() {
            previous.cancel()?;
        }
        let operation_id = uuid::Uuid::new_v4().simple().to_string();
        let state = flow.snapshot()?;
        *pending = Some(PendingLogin {
            id: operation_id.clone(),
            flow: PendingFlow::ServerQuick(flow),
            view: Default::default(),
            captcha: Default::default(),
        });
        Ok(AccountData {
            epoch: epoch.into(),
            value: ServerQuickLoginOperationView {
                operation_id,
                state,
            },
        })
    }

    pub async fn server_quick_login_command(
        &self,
        epoch: &str,
        id: &str,
        command: ServerQuickLoginCommand,
    ) -> Result<AccountData<ServerQuickLoginOperationView>> {
        let base = self.auth_token(epoch)?;
        if base.uid.is_some() {
            return Err(Error::StaleResponse);
        }
        if id.len() != 32
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::InvalidInput("invalid server quick login operation"));
        }
        let _transition = if matches!(command, ServerQuickLoginCommand::Cancel) {
            None
        } else {
            Some(self.together.gate.lock().await)
        };
        self.session.check_generation(&base)?;
        let flow = {
            let pending = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
            match pending.as_ref().filter(|attempt| attempt.id == id) {
                Some(PendingLogin {
                    flow: PendingFlow::ServerQuick(flow),
                    ..
                }) => Some(flow.clone()),
                Some(_) => return Err(Error::StaleResponse),
                None => None,
            }
        };
        let state = match (command, flow) {
            (ServerQuickLoginCommand::Execute, Some(flow)) => flow.execute().await?,
            (ServerQuickLoginCommand::Execute, None) => return Err(Error::StaleResponse),
            (ServerQuickLoginCommand::Reconcile, Some(flow)) => flow.reconcile().await?,
            (ServerQuickLoginCommand::Cancel, Some(flow)) => match self.cancel_login(id) {
                Ok(()) => flow.snapshot()?,
                Err(Error::StaleResponse) => cancelled_view(),
                Err(error) => return Err(error),
            },
            // The operation already ended or was replaced. A lost cancel
            // response is recoverable without touching the replacement.
            (ServerQuickLoginCommand::Reconcile | ServerQuickLoginCommand::Cancel, None) => {
                cancelled_view()
            }
        };
        if state.phase != ServerQuickLoginPhase::Authenticated {
            self.session.check_generation(&base)?;
        }
        // The response belongs to the old guest UI. The desktop separately
        // publishes the actual new authenticated account and epoch.
        Ok(AccountData {
            epoch: epoch.into(),
            value: ServerQuickLoginOperationView {
                operation_id: id.into(),
                state,
            },
        })
    }
}
