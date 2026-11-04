//! Membership card delivery slot. Keep offer context and redirects in native code.
use super::super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    membership::portal::{MEMBER_PORTAL, MemberPortalQr, member_portal_qr},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    App,
    Qr,
    Browser,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MemberCardOffer {
    mode: Mode,
    label: Option<String>,
    revision: String,
}

struct Observed {
    epoch: String,
    at: Instant,
    offer: MemberCardOffer,
    external: Option<&'static str>,
}

#[derive(Default)]
pub(in crate::client) struct MemberOfferHost {
    serial: tokio::sync::Mutex<()>,
    observed: std::sync::Mutex<Option<Observed>>,
}

fn public_portal(link: Option<&str>) -> bool {
    // The only destination verified to be public in the recovered card source.
    // Exact comparison also rejects added account, SSO, order and redirect fields.
    link == Some(MEMBER_PORTAL)
}

fn project(data: &Value) -> (Mode, Option<String>, Option<&'static str>) {
    let offer = &data["resourceData"];
    let label = offer["text"]
        .as_str()
        .map(str::trim)
        .filter(|value| {
            !value.is_empty()
                && value.chars().count() <= 40
                && !value.chars().any(char::is_control)
                && !value.contains("://")
        })
        .map(str::to_owned);
    if label.is_none() || !public_portal(offer["jumpUrl"].as_str()) {
        return (Mode::App, None, None);
    }
    match offer["pcSideDisplay"].as_str() {
        Some("2") => (Mode::Browser, label, Some(MEMBER_PORTAL)),
        Some("3") => (Mode::Qr, label, None),
        Some("4") => (
            Mode::Browser,
            label,
            Some(concat!(
                "https://music.163.com/g/vip-portal?nm_style=sbt&scene=pc_viptab_scan",
                "&source=pc"
            )),
        ),
        _ => (Mode::App, None, None),
    }
}

impl MusicClient {
    pub async fn member_card_offer(
        &self,
        epoch: &str,
        refresh: bool,
    ) -> Result<AccountData<MemberCardOffer>> {
        let token = self.member_token(epoch)?;
        let _serial = self.member_offer.serial.lock().await;
        let observed = || {
            self.member_offer
                .observed
                .lock()
                .map_err(|_| Error::State("member offer cache unavailable"))
        };
        let wrap = |value| AccountData {
            epoch: epoch.into(),
            value,
        };
        if refresh {
            self.session.with_token(&token, |_| {
                *observed()? = None;
                Ok(())
            })?;
        } else {
            let cached = self.session.with_token(&token, |_| {
                Ok(observed()?
                    .as_ref()
                    .filter(|entry| entry.epoch == epoch && entry.at.elapsed() < TTL)
                    .map(|entry| entry.offer.clone()))
            })?;
            if let Some(offer) = cached {
                return Ok(wrap(offer));
            }
        }
        let reply = self
            .session
            .run_for(&token, |api, _| async move {
                api.call(
                    "/api/delivery/deliver",
                    json!({"positionId":65}),
                    false,
                    &[200],
                )
                .await
            })
            .await?;
        self.session.with_token(&token, |_| {
            let (mode, label, external) = project(&reply["data"]);
            let offer = MemberCardOffer {
                mode,
                label,
                revision: uuid::Uuid::new_v4().simple().to_string(),
            };
            *observed()? = Some(Observed {
                epoch: epoch.into(),
                at: Instant::now(),
                offer: offer.clone(),
                external,
            });
            Ok(wrap(offer))
        })
    }

    fn with_current_member_offer<T>(
        &self,
        epoch: &str,
        revision: &str,
        use_offer: impl FnOnce(&Observed) -> Result<T>,
    ) -> Result<T> {
        let token = self.member_token(epoch)?;
        self.session.with_token(&token, |_| {
            let observed = self
                .member_offer
                .observed
                .lock()
                .map_err(|_| Error::State("member offer cache unavailable"))?;
            let entry = observed
                .as_ref()
                .filter(|entry| {
                    entry.epoch == epoch
                        && entry.at.elapsed() < TTL
                        && entry.offer.revision == revision
                })
                .ok_or(Error::StaleResponse)?;
            use_offer(entry)
        })
    }

    pub fn member_card_offer_qr(
        &self,
        epoch: &str,
        revision: &str,
    ) -> Result<AccountData<MemberPortalQr>> {
        self.with_current_member_offer(epoch, revision, |entry| {
            if entry.offer.mode != Mode::Qr {
                return Err(Error::StaleResponse);
            }
            Ok(AccountData {
                epoch: epoch.into(),
                // Projection accepts only the exact, public portal address.
                value: member_portal_qr()?,
            })
        })
    }

    pub fn member_card_external_url(&self, epoch: &str, revision: &str) -> Result<&'static str> {
        self.with_current_member_offer(epoch, revision, |entry| {
            entry.external.ok_or(Error::StaleResponse)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_confirmed_public_membership_destinations_can_be_activated() {
        let offer = |mode: Value, link: &str| {
            project(&json!({
                "resourceData": {"text":"查看优惠","gotoCashier":true,"pcSideDisplay":mode,"jumpUrl":link},
                "logContext":"not exposed"
            }))
        };
        assert_eq!(offer(json!("2"), MEMBER_PORTAL).2, Some(MEMBER_PORTAL));
        assert_eq!(offer(json!("3"), MEMBER_PORTAL).0, Mode::Qr);
        assert!(
            offer(json!("4"), MEMBER_PORTAL)
                .2
                .unwrap()
                .ends_with("&source=pc")
        );
        for mode in [json!("1"), json!(2), json!("unknown")] {
            assert_eq!(offer(mode, MEMBER_PORTAL), (Mode::App, None, None));
        }
        for link in [
            "https://music.163.com/g/vip-portal?nm_style=sbt&scene=pc_viptab_scan&token=private",
            "https://music.163.com/st/vipcashier-v4",
            "https://outside.invalid/",
        ] {
            assert_eq!(offer(json!("4"), link), (Mode::App, None, None));
        }
    }
}
