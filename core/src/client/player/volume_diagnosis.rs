//! M/83 volume-popover close and M/1384 diagnosis frequency control.
use crate::{Error, Result, account::SessionToken, client::MusicClient, storage::StateStore};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    future::Future,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CONFIG: &str = "preload#desktopVolumeDignoseConfig";
const OWNER: &str = "@device";
const KIND: &str = "volume-diagnosis-v1";
const KEY: &str = "last-dismissed";
const DAY_MS: f64 = 86_400_000.0;
const RECEIPT_LIFETIME: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeDiagnosisDismissal {
    AutoClose,
    ManualClose,
    JumpSettings,
}

#[derive(Clone, Copy)]
struct Policy {
    cooldown_ms: f64,
    remember_timeout: bool,
}

impl Policy {
    fn read(data: Option<&Value>) -> Self {
        let config = data
            .and_then(|data| data.get(CONFIG))
            .unwrap_or(&Value::Null);
        let multiplier = match config.get("unit").and_then(Value::as_str) {
            Some("hour") => 3_600_000.0,
            Some("minute") => 60_000.0,
            None | Some("day") => DAY_MS,
            Some(_) => {
                return Self {
                    cooldown_ms: 7.0 * DAY_MS,
                    remember_timeout: config["autoCloseFrequencyControl"].as_bool() == Some(true),
                };
            }
        };
        let duration = config["frequencyTime"]
            .as_f64()
            .filter(|value| *value >= 0.0)
            .map(|value| value * multiplier)
            .filter(|value| value.is_finite());
        Self {
            cooldown_ms: duration.unwrap_or(7.0 * multiplier),
            remember_timeout: config["autoCloseFrequencyControl"].as_bool() == Some(true),
        }
    }

    fn allows(self, last: Option<u64>, now: u64) -> bool {
        last.filter(|last| *last > 0)
            .is_none_or(|last| now >= last && (now - last) as f64 > self.cooldown_ms)
    }

    fn remember(self, action: VolumeDiagnosisDismissal) -> bool {
        !matches!(action, VolumeDiagnosisDismissal::AutoClose) || self.remember_timeout
    }
}

struct Offer {
    id: String,
    owner: SessionToken,
    issued: Instant,
}

#[derive(Default)]
pub(super) struct Host(Mutex<Option<Offer>>);

impl Host {
    fn offer(
        &self,
        store: &StateStore,
        owner: &SessionToken,
        policy: Policy,
        now: u64,
    ) -> Result<Option<String>> {
        let mut offer = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if offer
            .as_ref()
            .is_some_and(|offer| offer.owner == *owner && offer.issued.elapsed() < RECEIPT_LIFETIME)
            || !policy.allows(
                store
                    .get(OWNER, KIND, KEY)?
                    .and_then(|value| value.as_u64()),
                now,
            )
        {
            return Ok(None);
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        *offer = Some(Offer {
            id: id.clone(),
            owner: owner.clone(),
            issued: Instant::now(),
        });
        Ok(Some(id))
    }

    fn dismiss(
        &self,
        store: &StateStore,
        owner: &SessionToken,
        id: &str,
        action: VolumeDiagnosisDismissal,
        policy: Policy,
        now: u64,
    ) -> Result<()> {
        let mut offer = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if !offer.as_ref().is_some_and(|offer| {
            offer.id == id && offer.owner == *owner && offer.issued.elapsed() < RECEIPT_LIFETIME
        }) {
            return Err(Error::StaleResponse);
        }
        *offer = None;
        if policy.remember(action) {
            store.transaction(|tx| {
                let last = tx.get(OWNER, KIND, KEY)?.and_then(|value| value.as_u64());
                if last.is_none_or(|last| last <= now) {
                    tx.put(OWNER, KIND, KEY, &json!(now))?;
                }
                Ok(())
            })?;
        }
        Ok(())
    }
}

fn eligible(before: f64, after: f64) -> bool {
    (0.0..=100.0).contains(&before) && (80.0..=100.0).contains(&after) && after > before
}

fn low_master(value: f32) -> bool {
    value > 0.0 && f64::from(value) < 0.3
}

fn wall_ms() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| value.as_millis().try_into().ok())
        .ok_or(Error::State(
            "system clock is outside the volume diagnosis range",
        ))
}

impl MusicClient {
    fn volume_diagnosis_policy(&self) -> Policy {
        let snapshot = self
            .auth
            .context
            .as_ref()
            .and_then(|context| context.pc_snapshot().ok().flatten());
        Policy::read(
            snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.data.as_ref()),
        )
    }

    /// Trusted platform callback only. The renderer receives an optional
    /// one-use prompt receipt, never the native scalar. No playback command,
    /// token fetch, or configuration refresh is performed here.
    pub async fn check_volume_diagnosis(
        &self,
        epoch: &str,
        before: f64,
        after: f64,
        read_effective_master: impl Future<Output = Option<f32>>,
    ) -> Result<Option<String>> {
        if !eligible(before, after) {
            return Ok(None);
        }
        let owner = self.player_token(epoch)?;
        let Some(controller) = self.player.controller.get() else {
            return Ok(None);
        };
        let matches_player = || {
            let current = controller.snapshot();
            current.account == owner && current.volume == after
        };
        if !matches_player() || !read_effective_master.await.is_some_and(low_master) {
            return Ok(None);
        }
        let policy = self.volume_diagnosis_policy();
        self.session.with_local_owner(&owner, |_| {
            if !matches_player() {
                return Ok(None);
            }
            self.player
                .volume_diagnosis
                .offer(&self.registry.store(), &owner, policy, wall_ms()?)
        })
    }

    pub fn dismiss_volume_diagnosis(
        &self,
        epoch: &str,
        receipt: &str,
        action: VolumeDiagnosisDismissal,
    ) -> Result<()> {
        let owner = self.player_token(epoch)?;
        let policy = self.volume_diagnosis_policy();
        self.session.with_local_owner(&owner, |_| {
            self.player.volume_diagnosis.dismiss(
                &self.registry.store(),
                &owner,
                receipt,
                action,
                policy,
                wall_ms()?,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_increased_player_volume_and_unrounded_nonzero_system_scalar() {
        assert!(eligible(79.0, 80.0));
        for (before, after) in [
            (79.0, 79.999),
            (80.0, 80.0),
            (90.0, 85.0),
            (0.0, 101.0),
            (f64::NAN, 80.0),
        ] {
            assert!(!eligible(before, after));
        }
        assert!(low_master(0.2999));
        for value in [0.0, 0.3, 1.0, f32::NAN, -0.01, f32::INFINITY] {
            assert!(!low_master(value));
        }
    }

    #[test]
    fn prompt_receipt_binds_owner_and_persists_only_the_configured_close_paths() {
        let dir = tempfile::tempdir().unwrap();
        let store = StateStore::open(dir.path()).unwrap();
        let owner = SessionToken {
            generation: 3,
            uid: Some("42".into()),
        };
        let foreign = SessionToken {
            generation: 4,
            ..owner.clone()
        };
        let host = Host::default();
        let default = Policy::read(None);
        let receipt = host.offer(&store, &owner, default, 100).unwrap().unwrap();
        assert!(host.offer(&store, &owner, default, 101).unwrap().is_none());
        assert!(matches!(
            host.dismiss(
                &store,
                &foreign,
                &receipt,
                VolumeDiagnosisDismissal::JumpSettings,
                default,
                102
            ),
            Err(Error::StaleResponse)
        ));
        host.dismiss(
            &store,
            &owner,
            &receipt,
            VolumeDiagnosisDismissal::AutoClose,
            default,
            103,
        )
        .unwrap();
        assert!(store.get(OWNER, KIND, KEY).unwrap().is_none());
        let receipt = host.offer(&store, &owner, default, 104).unwrap().unwrap();
        host.dismiss(
            &store,
            &owner,
            &receipt,
            VolumeDiagnosisDismissal::ManualClose,
            default,
            105,
        )
        .unwrap();
        assert!(matches!(
            host.dismiss(
                &store,
                &owner,
                &receipt,
                VolumeDiagnosisDismissal::JumpSettings,
                default,
                106
            ),
            Err(Error::StaleResponse)
        ));
        drop(store);
        let store = StateStore::open(dir.path()).unwrap();
        let host = Host::default();
        assert!(
            host.offer(&store, &foreign, default, 105 + 7 * 86_400_000)
                .unwrap()
                .is_none()
        );
        let configured = Policy::read(Some(&json!({(CONFIG):{
            "frequencyTime":2,"unit":"minute","autoCloseFrequencyControl":true
        }})));
        assert!(
            host.offer(&store, &foreign, configured, 105 + 120_000)
                .unwrap()
                .is_none()
        );
        let receipt = host
            .offer(&store, &foreign, configured, 105 + 120_001)
            .unwrap()
            .unwrap();
        host.dismiss(
            &store,
            &foreign,
            &receipt,
            VolumeDiagnosisDismissal::AutoClose,
            configured,
            105 + 125_001,
        )
        .unwrap();
        assert_eq!(store.get(OWNER, KIND, KEY).unwrap(), Some(json!(125_106)));
    }
}
