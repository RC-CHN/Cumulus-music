//! Official M/3095 + M/3001: refresh only the ordinary six-song preview.
use super::{AccountData, MusicClient};
use crate::{
    Error, Result,
    account::{SessionToken, session::MembershipObservation},
    catalog::CatalogApi,
    protocol::{
        api::{Api, identifier},
        wire::serialization::js_truthy,
    },
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

const PAGE_LIMIT: usize = 128;
const LIFETIME: Duration = Duration::from_secs(30 * 60);
pub(super) const ROW_LIMIT: usize = 6;

pub(super) struct PreviewRow {
    pub(super) id: String,
    pub(super) original_cover: Option<String>,
    pub(super) original_mv: Option<String>,
    pub(super) alternative: Option<(String, Option<String>)>,
}

pub(super) struct Preview {
    pub(super) section: String,
    pub(super) rows: Vec<PreviewRow>,
}

struct Page {
    receipt: String,
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: Option<String>,
    issued: Instant,
    preview: Preview,
}

#[derive(Default)]
pub(super) struct SearchPrivilegePages(Mutex<VecDeque<Arc<Page>>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPrivilegePatch {
    receipt: String,
    section: String,
    rows: Vec<PrivilegeRow>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PrivilegeRow {
    id: String,
    cover_url: Option<String>,
    mv_id: Option<String>,
    replacement_mv: bool,
    vip_badge: bool,
    limited_free: bool,
}

impl SearchPrivilegePages {
    pub(super) fn capture(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<String>,
        preview: Preview,
    ) -> Result<Option<String>> {
        if preview.rows.is_empty()
            || preview.rows.len() > ROW_LIMIT
            || preview.rows.iter().any(|row| row.id.len() > 32)
        {
            return Ok(None);
        }
        let mut pages = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        pages.retain(|page| page.token == *token && page.issued.elapsed() < LIFETIME);
        if pages.len() >= PAGE_LIMIT {
            pages.pop_front();
        }
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        pages.push_back(Arc::new(Page {
            receipt: receipt.clone(),
            token: token.clone(),
            api: Arc::downgrade(api),
            credentials,
            issued: Instant::now(),
            preview,
        }));
        Ok(Some(receipt))
    }

    fn page(&self, token: &SessionToken, receipt: &str) -> Result<Arc<Page>> {
        if receipt.len() != 32 || !receipt.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput(
                "invalid comprehensive privilege receipt",
            ));
        }
        self.0
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .iter()
            .find(|page| {
                page.token == *token && page.receipt == receipt && page.issued.elapsed() < LIFETIME
            })
            .cloned()
            .ok_or(Error::StaleResponse)
    }
}

impl MusicClient {
    /// Desktop/native observer only; the renderer cannot write or fabricate observations.
    pub fn subscribe_membership_observations(
        &self,
    ) -> Result<tokio::sync::watch::Receiver<Option<MembershipObservation>>> {
        self.session.subscribe_membership()
    }

    pub async fn search_refresh_privileges(
        &self,
        epoch: &str,
        receipt: &str,
        observation: Option<&str>,
    ) -> Result<AccountData<SearchPrivilegePatch>> {
        let token = self.player_token(epoch)?;
        let page = self.search.privilege_pages.page(&token, receipt)?;
        let api = page.api.upgrade().ok_or(Error::StaleResponse)?;
        let check = |expected: &Option<String>| -> Result<()> {
            self.session.require_api(&token, &api)?;
            if self.login_pending()?
                || page.issued.elapsed() >= LIFETIME
                || api.search_privilege_credential_revision()? != page.credentials
                || &self.session.membership_observation(&token)? != expected
            {
                return Err(Error::StaleResponse);
            }
            Ok(())
        };
        let expected = self.session.membership_observation(&token)?;
        if observation.is_some_and(|event| expected.as_deref() != Some(event)) {
            return Err(Error::StaleResponse);
        }
        check(&expected)?;
        let ids: Vec<_> = page.preview.rows.iter().map(|row| row.id.clone()).collect();
        let privileges = self
            .session
            .run_for(&token, |current, _| async {
                if !Arc::ptr_eq(&api, &current) {
                    return Err(Error::StaleResponse);
                }
                CatalogApi::new(current).privileges(&ids, None).await
            })
            .await?;
        check(&expected)?;

        // M/21 B keeps the last privilege for each ID. Do not let unsolicited
        // metadata insert rows into the observed search or treat a missing ID
        // as evidence of access; no change is projected for missing replies.
        let by_id: HashMap<_, _> = privileges
            .iter()
            .map(|privilege| Ok((identifier(&privilege["id"])?, privilege)))
            .collect::<Result<_>>()?;
        let rows = page
            .preview
            .rows
            .iter()
            .filter_map(|row| {
                let privilege = *by_id.get(&row.id)?;
                Some(row.project(privilege))
            })
            .collect();
        Ok(AccountData {
            epoch: epoch.into(),
            value: SearchPrivilegePatch {
                receipt: receipt.into(),
                section: page.preview.section.clone(),
                rows,
            },
        })
    }
}

pub(super) fn status(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|value| value.is_finite())
}

pub(super) fn badges(privilege: &Value) -> (bool, bool) {
    // M/35.m and .k: VIP and limited-free badges may coexist.
    let trial = &privilege["freeTrialPrivilege"];
    if js_truthy(&trial["userConsumable"]) && js_truthy(&trial["resConsumable"]) {
        return match trial.get("freeLimitTagType") {
            Some(Value::Null) => (true, false),
            Some(value) if value == "VIP_AND_FREE_TAG" => (true, true),
            Some(value) if value == "FREE_TAG" => (false, true),
            _ => (false, false),
        };
    }
    let fee = privilege["fee"]
        .as_u64()
        .or_else(|| privilege["fee"].as_str()?.parse().ok())
        .unwrap_or(0);
    (fee & (1 | 16 | 32) != 0, false)
}

impl PreviewRow {
    fn project(&self, privilege: &Value) -> PrivilegeRow {
        let status = status(&privilege["st"]);
        let restricted = status.is_some_and(|status| status < 0.0);
        let replace = restricted && (status != Some(-100.0) || js_truthy(&privilege["toast"]));
        let alternative = self.alternative.as_ref().filter(|_| replace);
        let (vip_badge, limited_free) = badges(privilege);
        PrivilegeRow {
            id: self.id.clone(),
            cover_url: alternative
                .and_then(|(_, cover)| cover.clone())
                .or_else(|| self.original_cover.clone()),
            mv_id: alternative.map(|(id, _)| id.clone()).or_else(|| {
                if restricted {
                    None
                } else {
                    self.original_mv.clone()
                }
            }),
            replacement_mv: alternative.is_some(),
            vip_badge,
            limited_free,
        }
    }
}
