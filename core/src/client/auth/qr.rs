use super::super::dto::{AccountProfileShortcut, AccountView, QrView};
use super::{LoginChallengeView, MusicClient, PendingFlow, PendingLogin, mapping};
use crate::{
    Error, Result,
    account::{
        QrStatus, SessionToken,
        login::{LoginProtocol, QrAuthentication},
    },
};
use qrcodegen::{QrCode, QrCodeEcc, QrSegment, Version};
use std::sync::Arc;

/// The renderer cannot cancel a QR attempt until it has received its ID.
struct UnpublishedQr<'a> {
    client: &'a MusicClient,
    id: &'a str,
    published: bool,
}
impl Drop for UnpublishedQr<'_> {
    fn drop(&mut self) {
        if !self.published {
            let _ = self.client.cancel_qr(self.id);
        }
    }
}

struct ClassicGuideAdmission<'a> {
    base: &'a SessionToken,
    ticket: &'a str,
    window: &'a (dyn Fn() -> Result<()> + Sync),
}

impl ClassicGuideAdmission<'_> {
    fn require(&self, client: &MusicClient) -> Result<()> {
        (self.window)()?;
        client
            .session
            .require_classic_remove_guide(self.base, self.ticket)
    }
}

impl MusicClient {
    pub fn account(&self) -> Result<AccountView> {
        let snapshot = self.session.snapshot()?;
        let can_sign_out = self
            .native_account_client()?
            .map(|client| {
                let stored = client.credentials();
                let credentials = stored.login_credentials()?;
                Ok::<_, Error>(
                    credentials.token.is_some()
                        || credentials.refresh_token.is_some()
                        || stored.pending_queue_reset()?.is_some(),
                )
            })
            .transpose()?
            .unwrap_or(false);
        self.session.check_generation(&snapshot.token)?;
        let profile = snapshot.profile.unwrap_or_default();
        let count = |field: &str| {
            profile[field]
                .as_u64()
                .filter(|value| *value <= 9_007_199_254_740_991)
        };
        let profile_shortcut = if snapshot.status == crate::account::SessionStatus::Authenticated {
            snapshot
                .token
                .uid
                .as_ref()
                .map(|id| AccountProfileShortcut {
                    id: id.clone(),
                    notes: count("eventCount"),
                    following: count("follows"),
                    followers: count("followeds"),
                })
        } else {
            None
        };
        Ok(AccountView {
            epoch: snapshot.token.generation.to_string(),
            status: snapshot.status,
            nickname: profile["nickname"].as_str().map(str::to_owned),
            avatar_url: mapping::artwork(&profile["avatarUrl"]),
            can_sign_out,
            profile_shortcut,
            login_notice: snapshot.login_notice,
            classic_remove_guide: snapshot.classic_remove_guide,
            live_jsk_expiry: snapshot.live_jsk_expiry,
        })
    }

    pub async fn restore_account(&self) -> Result<AccountView> {
        match self.session.refresh().await {
            Ok(_) => self.cold_offline_close().await?,
            Err(error) if error.is_service_code(301) => self.cold_offline_close().await?,
            Err(Error::LoginDeferred) => (),
            Err(error) => return Err(error),
        }
        if self.session.token()?.uid.is_some()
            && let Some(client) = self.native_account_client()?
        {
            self.registry.record_current(&self.session, &client)?;
        }
        self.account()
    }

    pub async fn begin_qr(&self) -> Result<QrView> {
        let _gate = self.together.gate.lock().await;
        self.check_account_transition()?;
        let protocol = self
            .session
            .new_login_protocol(&self.session.token()?)
            .await?;
        let flow = Arc::new(QrAuthentication::new_with_protocol(
            self.session.clone(),
            self.qr_login_candidate()?,
            protocol,
        )?);
        self.present_qr(flow, None).await
    }

    pub async fn begin_classic_remove_guide_qr(
        &self,
        epoch: &str,
        ticket: &str,
        retry: bool,
        window: impl Fn() -> Result<()> + Send + Sync,
    ) -> Result<QrView> {
        let _gate = self.together.gate.lock().await;
        window()?;
        self.check_account_transition()?;
        (self.auth.pending_guard())()?;
        let base = self
            .session
            .begin_classic_remove_guide(epoch, ticket, retry)?;
        let flow = Arc::new(QrAuthentication::for_token(
            self.session.clone(),
            self.qr_login_candidate()?,
            LoginProtocol::Legacy,
            base.clone(),
            Some(ticket.to_owned()),
        )?);
        self.present_qr(
            flow,
            Some(ClassicGuideAdmission {
                base: &base,
                ticket,
                window: &window,
            }),
        )
        .await
    }

    pub fn dismiss_classic_remove_guide(&self, epoch: &str, ticket: &str) -> Result<()> {
        self.session.dismiss_classic_remove_guide(epoch, ticket)
    }

    async fn present_qr(
        &self,
        flow: Arc<QrAuthentication>,
        guide: Option<ClassicGuideAdmission<'_>>,
    ) -> Result<QrView> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let pending = PendingLogin {
            id: id.clone(),
            flow: PendingFlow::Qr(flow.clone()),
            view: LoginChallengeView::default(),
            captcha: Default::default(),
        };
        if guide.is_some() {
            self.auth.insert_if_idle(pending)?;
        } else {
            self.auth.replace(pending)?;
        }
        let mut unpublished = UnpublishedQr {
            client: self,
            id: &id,
            published: false,
        };
        if let Some(guide) = &guide {
            guide.require(self)?;
        }
        let key = flow.begin().await?;
        if let Some(guide) = &guide {
            guide.require(self)?;
        }
        let mut link = url::Url::parse("https://music.163.com/login")
            .map_err(|_| Error::Protocol("QR URL"))?;
        link.query_pairs_mut().append_pair("codekey", &key);
        let qr = if guide.is_some() {
            QrCode::encode_segments_advanced(
                &QrSegment::make_segments(link.as_str()),
                QrCodeEcc::Low,
                Version::MIN,
                Version::MAX,
                None,
                false,
            )
        } else {
            QrCode::encode_text(link.as_str(), QrCodeEcc::Medium)
        }
        .map_err(|_| Error::Response("QR payload exceeds limit"))?;
        let modules = (0..qr.size())
            .map(|y| (0..qr.size()).map(|x| qr.get_module(x, y)).collect())
            .collect();
        // A new start/cancel may have replaced this operation during the await.
        self.qr_flow(&id)?;
        unpublished.published = true;
        drop(unpublished);
        Ok(QrView {
            operation_id: id,
            status: QrStatus::WaitingScan,
            // Official ordinary PC Qrcode.jsx and M/1368 both use 1 s.
            poll_after_ms: 1_000,
            modules,
        })
    }

    pub(super) fn qr_flow(&self, id: &str) -> Result<Arc<QrAuthentication>> {
        self.auth
            .pending
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .as_ref()
            .filter(|p| p.id == id)
            .map(|p| {
                p.captcha.check_idle()?;
                match &p.flow {
                    PendingFlow::Qr(flow) => Ok(flow.clone()),
                    _ => Err(Error::StaleResponse),
                }
            })
            .unwrap_or(Err(Error::StaleResponse))
    }

    pub async fn poll_qr(&self, id: &str) -> Result<QrStatus> {
        let _gate = self.together.gate.lock().await;
        self.check_account_transition()?;
        let flow = self.qr_flow(id)?;
        let status = flow.poll(None).await?;
        self.qr_flow(id)?;
        if status == QrStatus::Authorized {
            self.suspend_account_media().await?;
        }
        Ok(status)
    }

    pub fn qr_identity_pending(&self, id: &str) -> Result<bool> {
        self.qr_flow(id)?.identity_pending()
    }

    pub async fn reconcile_qr(&self, id: &str) -> Result<()> {
        let _gate = self.together.gate.lock().await;
        self.check_account_transition()?;
        self.qr_flow(id)?.reconcile().await?;
        self.qr_flow(id)?;
        self.suspend_account_media().await
    }

    pub fn cancel_qr(&self, id: &str) -> Result<()> {
        {
            let slot = self.auth.pending.lock().map_err(|_| Error::LockPoisoned)?;
            if !slot
                .as_ref()
                .is_some_and(|p| p.id == id && matches!(p.flow, PendingFlow::Qr(_)))
            {
                return Err(Error::StaleResponse);
            }
        }
        self.cancel_login(id)
    }
}
