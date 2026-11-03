//! Official PC device removal, independent of candidate login verification.
use super::{MusicClient, dto::AccountData, login_devices::DeviceList};
use crate::{
    Error, Result,
    account::SessionToken,
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const KICKOFF: &str = "/api/middle/user/security/device/kickoff";
const SMS: &str = "/api/sms/captcha/safe/sent";
const JOURNAL: &str = "login-device-removal-v1";
const ACTIVE: &str = "active";
const RESEND_SECONDS: u64 = 60;

fn preflight_error(error: Error) -> Error {
    if error.is_service_code(301)
        || matches!(
            error.root(),
            Error::StaleResponse | Error::InvalidInput(_) | Error::RequestNotSent(_)
        )
    {
        return error;
    }
    Error::RequestNotSent(Box::new(error))
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LoginDeviceRemovalCommand {
    Begin { device_id: String },
    ResolveBegin { device_id: String },
    SendSms { operation_id: String },
    SubmitSms { operation_id: String, code: String },
    Reconcile { operation_id: String },
    Cancel { operation_id: String },
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginDeviceRemovalPhase {
    NeedsBinding,
    NeedsSms,
    SmsReady,
    SmsSendUncertain,
    RecoveryRequired,
    ObservedAbsent,
    NotSent,
    Cancelled,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginDeviceRemovalView {
    pub id: String,
    pub phase: LoginDeviceRemovalPhase,
    pub phone_tail: Option<String>,
    pub resend_after_seconds: u64,
    pub sms_not_accepted: bool,
    /// Only a complete, fresh list read sets this field.
    pub observed_present: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct Phone {
    tail: String,
    country: String,
    fingerprint: String,
}

#[derive(Serialize, Deserialize)]
struct Intent {
    id: String,
    // A private, per-intent digest allows read-only recovery without storing the
    // raw server device key. Neither this digest nor the binding proof crosses IPC.
    target: String,
    phase: LoginDeviceRemovalPhase,
    phone: Option<Phone>,
    resend_not_before: u64,
    sms_not_accepted: bool,
    observed_present: Option<bool>,
}

fn now() -> u64 {
    time::OffsetDateTime::now_utc().unix_timestamp().max(0) as u64
}

fn digest(id: &str, domain: &str, value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(domain.as_bytes());
    hash.update(id.as_bytes());
    hash.update(value.as_bytes());
    hex::encode(hash.finalize())
}

fn hex_id(s: &str, length: usize) -> bool {
    s.len() == length
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn digits(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

impl Intent {
    fn view(&self) -> LoginDeviceRemovalView {
        LoginDeviceRemovalView {
            id: self.id.clone(),
            phase: self.phase,
            phone_tail: self.phone.as_ref().map(|phone| phone.tail.clone()),
            resend_after_seconds: self
                .resend_not_before
                .saturating_sub(now())
                .min(RESEND_SECONDS),
            sms_not_accepted: self.sms_not_accepted,
            observed_present: self.observed_present,
        }
    }

    fn can_sms(&self) -> bool {
        matches!(
            self.phase,
            LoginDeviceRemovalPhase::NeedsSms
                | LoginDeviceRemovalPhase::SmsReady
                | LoginDeviceRemovalPhase::SmsSendUncertain
        )
    }
}

struct Removal<'a> {
    client: &'a MusicClient,
    token: SessionToken,
    store: Arc<StateStore>,
    owner: String,
}

impl<'a> Removal<'a> {
    fn new(client: &'a MusicClient, epoch: &str) -> Result<Self> {
        let token = client.player_token(epoch)?;
        let owner = client
            .session
            .with_token(&token, |uid| Ok(uid.to_owned()))?;
        Ok(Self {
            client,
            token,
            store: client.session.persistent_store()?,
            owner,
        })
    }

    fn load(&self) -> Result<Option<Intent>> {
        let Some(value) = self.store.get(&self.owner, JOURNAL, ACTIVE)? else {
            return Ok(None);
        };
        let intent: Intent = serde_json::from_value(value)?;
        if !hex_id(&intent.id, 32)
            || !hex_id(&intent.target, 64)
            || intent.phone.as_ref().is_some_and(|phone| {
                !digits(&phone.tail, 4, 4)
                    || !digits(&phone.country, 1, 4)
                    || !hex_id(&phone.fingerprint, 64)
            })
        {
            return Err(Error::State("invalid saved login device removal"));
        }
        Ok(Some(intent))
    }

    fn save(&self, intent: &Intent) -> Result<()> {
        // Settle the original owner's intent even if another account was chosen
        // while awaiting the transport. Return views only through the epoch fence.
        self.store
            .put(&self.owner, JOURNAL, ACTIVE, &serde_json::to_value(intent)?)
    }

    async fn phone(&self, id: &str) -> Result<Option<Phone>> {
        self.client
            .session
            .run_for(&self.token, |api, uid| async move {
                let reply = api
                    .call(
                        &format!("/api/w/v1/user/bindings/{uid}"),
                        json!({"userId":uid}),
                        false,
                        &[200],
                    )
                    .await?;
                let rows = reply["bindings"]
                    .as_array()
                    .filter(|rows| rows.len() <= 32)
                    .ok_or(Error::Response("missing or oversized account bindings"))?;
                if rows.iter().any(|row| !row["type"].is_u64()) {
                    return Err(Error::Response("invalid account binding type"));
                }
                let mut phones = rows.iter().filter(|row| row["type"] == 1);
                let Some(row) = phones.next() else {
                    return Ok(None);
                };
                if phones.next().is_some() {
                    return Err(Error::Response("ambiguous account phone binding"));
                }
                let token = row["tokenJsonStr"]
                    .as_str()
                    .filter(|s| s.len() <= 16_384)
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                    .ok_or(Error::Response("invalid account phone binding"))?;
                let phone = token["cellphone"]
                    .as_str()
                    .filter(|s| digits(s, 4, 32))
                    .ok_or(Error::Response("missing account phone binding number"))?;
                let country = token["countrycode"]
                    .as_str()
                    .filter(|s| digits(s, 1, 4))
                    .ok_or(Error::Response("missing account phone country code"))?;
                Ok(Some(Phone {
                    tail: phone[phone.len() - 4..].into(),
                    country: country.into(),
                    fingerprint: digest(
                        id,
                        "login-device-removal-phone",
                        &format!("{country}:{phone}"),
                    ),
                }))
            })
            .await
    }

    async fn target_key(&self, intent: &Intent) -> Result<String> {
        let rows = self.client.read_login_devices(&self.token).await?.rows;
        if !rows.iter().any(|row| row.current) {
            return Err(Error::State("cannot identify the current login device"));
        }
        let target = rows
            .into_iter()
            .find(|row| digest(&intent.id, "login-device-removal", &row.key) == intent.target)
            .ok_or(Error::StaleResponse)?;
        if target.current {
            return Err(Error::State("cannot remove the current login device"));
        }
        Ok(target.key)
    }

    async fn verify_phone(&self, intent: &Intent) -> Result<()> {
        let phone = self.phone(&intent.id).await?;
        if phone
            .as_ref()
            .zip(intent.phone.as_ref())
            .is_none_or(|(fresh, original)| fresh.fingerprint != original.fingerprint)
        {
            return Err(Error::State("account phone binding changed"));
        }
        Ok(())
    }

    fn observe(&self, intent: &mut Intent, list: DeviceList) -> Result<()> {
        if !list.rows.iter().any(|row| row.current) {
            return Err(Error::Response(
                "missing current login device in removal readback",
            ));
        }
        let present = list
            .rows
            .iter()
            .any(|row| digest(&intent.id, "login-device-removal", &row.key) == intent.target);
        intent.observed_present = Some(present);
        if !present {
            intent.phase = LoginDeviceRemovalPhase::ObservedAbsent;
        }
        self.save(intent)
    }

    async fn kickoff(&self, intent: &mut Intent, key: String, code: Option<String>) -> Result<()> {
        let previous = intent.phase;
        let submitted_sms = code.is_some();
        intent.phase = LoginDeviceRemovalPhase::RecoveryRequired;
        intent.observed_present = None;
        intent.sms_not_accepted = false;
        self.client
            .session
            .with_token(&self.token, |_| self.save(intent))?;
        let result = self
            .client
            .session
            .run_for(&self.token, |api, _| async move {
                let mut payload = json!({"deviceKey":key});
                if let Some(code) = code {
                    payload["captcha"] = code.into();
                }
                match api.call(KICKOFF, payload, false, &[200]).await {
                    Ok(_) => Ok(true),
                    Err(error) if error.is_service_code(401) => Ok(false),
                    Err(error) => Err(error),
                }
            })
            .await;
        match result {
            Ok(true) => {
                // Acknowledgement is not evidence of absence. Failure or crash
                // before observation retains the already-saved unknown outcome.
                if let Ok(list) = self.client.read_login_devices(&self.token).await
                    && list.rows.iter().any(|row| row.current)
                {
                    self.observe(intent, list)?;
                }
            }
            Ok(false) => {
                intent.phase = if submitted_sms {
                    previous
                } else {
                    LoginDeviceRemovalPhase::NeedsSms
                };
                intent.sms_not_accepted = submitted_sms;
                self.save(intent)?;
            }
            Err(error) if matches!(error.root(), Error::RequestNotSent(_)) => {
                intent.phase = previous;
                self.save(intent)?;
            }
            Err(_) => {}
        }
        Ok(())
    }

    async fn send_sms(&self, intent: &mut Intent) -> Result<()> {
        if !intent.can_sms() || intent.resend_not_before > now() {
            return Err(Error::State("login device SMS is not ready to send"));
        }
        self.target_key(intent).await?;
        self.verify_phone(intent).await?;
        let country = intent
            .phone
            .as_ref()
            .ok_or(Error::State("missing account phone binding"))?
            .country
            .clone();
        let previous = intent.phase;
        intent.phase = LoginDeviceRemovalPhase::SmsSendUncertain;
        intent.sms_not_accepted = false;
        intent.resend_not_before = now().saturating_add(RESEND_SECONDS);
        self.client
            .session
            .with_token(&self.token, |_| self.save(intent))?;
        let result = self
            .client
            .session
            .run_for(&self.token, |api, _| async move {
                api.call(SMS, json!({"ctCode":country}), false, &[200])
                    .await
            })
            .await;
        match result {
            Ok(_) => intent.phase = LoginDeviceRemovalPhase::SmsReady,
            Err(error) if matches!(error.root(), Error::RequestNotSent(_)) => {
                intent.phase = previous
            }
            Err(_) => return Ok(()),
        }
        self.save(intent)
    }

    async fn begin(&self, id: String, existing: Option<Intent>) -> Result<Intent> {
        if !hex_id(&id, 32) {
            return Err(Error::InvalidInput("invalid login device identifier"));
        }
        let resend_not_before = existing
            .as_ref()
            .map_or(0, |intent| intent.resend_not_before);
        if let Some(existing) = existing {
            if existing.id == id {
                return Ok(existing);
            }
            if existing.can_sms() || existing.phase == LoginDeviceRemovalPhase::RecoveryRequired {
                return Err(Error::MutationUncertain);
            }
        }
        let key = self.client.login_device_rows.key_for(&self.token, &id)?;
        let target = digest(&id, "login-device-removal", &key);
        let mut intent = Intent {
            id,
            target,
            phase: LoginDeviceRemovalPhase::NotSent,
            phone: None,
            resend_not_before,
            sms_not_accepted: false,
            observed_present: None,
        };
        intent.phone = self.phone(&intent.id).await.map_err(preflight_error)?;
        if intent.phone.is_none() {
            intent.phase = LoginDeviceRemovalPhase::NeedsBinding;
            self.client
                .session
                .with_token(&self.token, |_| self.save(&intent))?;
        } else {
            let key = self.target_key(&intent).await.map_err(preflight_error)?;
            self.kickoff(&mut intent, key, None).await?;
        }
        Ok(intent)
    }

    async fn execute(&self, command: LoginDeviceRemovalCommand) -> Result<LoginDeviceRemovalView> {
        let _lock = OperationLock::acquire(self.store.directory(), JOURNAL, &self.owner)?;
        let current = self.load()?;
        let (operation_id, command) = match command {
            LoginDeviceRemovalCommand::Begin { device_id } => {
                return self
                    .begin(device_id, current)
                    .await
                    .map(|intent| intent.view());
            }
            LoginDeviceRemovalCommand::ResolveBegin { device_id } => {
                if !hex_id(&device_id, 32) {
                    return Err(Error::InvalidInput("invalid login device identifier"));
                }
                if let Some(intent) = current.filter(|intent| intent.id == device_id) {
                    return Ok(intent.view());
                }
                // Held under the same lock as Begin. A later Begin for this
                // handle cannot resolve it, and a new list mints new handles.
                self.client.session.with_token(&self.token, |_| {
                    self.client
                        .login_device_rows
                        .invalidate(&self.token, &device_id)
                })?;
                return Ok(LoginDeviceRemovalView {
                    id: device_id,
                    phase: LoginDeviceRemovalPhase::NotSent,
                    phone_tail: None,
                    resend_after_seconds: 0,
                    sms_not_accepted: false,
                    observed_present: None,
                });
            }
            LoginDeviceRemovalCommand::SendSms { ref operation_id }
            | LoginDeviceRemovalCommand::SubmitSms {
                ref operation_id, ..
            }
            | LoginDeviceRemovalCommand::Reconcile { ref operation_id }
            | LoginDeviceRemovalCommand::Cancel { ref operation_id } => {
                (operation_id.clone(), command)
            }
        };
        let mut intent = current
            .filter(|intent| intent.id == operation_id)
            .ok_or(Error::StaleResponse)?;
        match command {
            LoginDeviceRemovalCommand::SendSms { .. } => self.send_sms(&mut intent).await?,
            LoginDeviceRemovalCommand::SubmitSms { code, .. } => {
                if !matches!(
                    intent.phase,
                    LoginDeviceRemovalPhase::SmsReady | LoginDeviceRemovalPhase::SmsSendUncertain
                ) {
                    return Err(Error::State("login device SMS confirmation is not pending"));
                }
                // Local bounded input policy; the recovered UI only requires nonempty.
                if !digits(&code, 4, 8) {
                    return Err(Error::InvalidInput("invalid login device SMS code"));
                }
                self.verify_phone(&intent).await?;
                let key = self.target_key(&intent).await?;
                self.kickoff(&mut intent, key, Some(code)).await?;
            }
            LoginDeviceRemovalCommand::Reconcile { .. } => {
                if intent.phase == LoginDeviceRemovalPhase::RecoveryRequired {
                    let list = self.client.read_login_devices(&self.token).await?;
                    self.observe(&mut intent, list)?;
                }
            }
            LoginDeviceRemovalCommand::Cancel { .. } => {
                if !intent.can_sms() {
                    return Err(Error::State(
                        "only a pending SMS challenge can be cancelled",
                    ));
                }
                intent.phase = LoginDeviceRemovalPhase::Cancelled;
                self.client
                    .session
                    .with_token(&self.token, |_| self.save(&intent))?;
            }
            LoginDeviceRemovalCommand::Begin { .. }
            | LoginDeviceRemovalCommand::ResolveBegin { .. } => unreachable!(),
        }
        Ok(intent.view())
    }
}

impl MusicClient {
    pub fn login_device_removal(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Option<LoginDeviceRemovalView>>> {
        let operation = Removal::new(self, epoch)?;
        let value = operation.load()?.map(|intent| intent.view());
        self.session.with_token(&operation.token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn login_device_removal_command(
        &self,
        epoch: &str,
        command: LoginDeviceRemovalCommand,
    ) -> Result<AccountData<LoginDeviceRemovalView>> {
        let operation = Removal::new(self, epoch)?;
        let value = operation.execute(command).await?;
        self.session.with_token(&operation.token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
