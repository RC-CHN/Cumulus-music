//! Versioned region names from the recovered PC host. Raw service data stays native.
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken, profile::edit::ProfileRegion},
    storage::{OperationLock, StateStore},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock},
    time::Duration,
};

const NAMESPACE: &str = "account-profile-regions-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionTable {
    pub version: String,
    pub provinces: Vec<Province>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Province {
    pub id: u32,
    pub name: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<u8>,
    pub cities: BTreeMap<u32, String>,
}

impl RegionTable {
    fn validate(self) -> Result<Self> {
        fn label(text: &str) -> bool {
            !text.trim().is_empty()
                && text.chars().count() <= 80
                && !text.chars().any(char::is_control)
        }
        let mut seen = BTreeSet::new();
        if !label(&self.version)
            || !(1..=96).contains(&self.provinces.len())
            || self.provinces.iter().any(|region| {
                region.id == 0
                    || !seen.insert(region.id)
                    || !label(&region.name)
                    || !(1..=256).contains(&region.cities.len())
                    || region
                        .cities
                        .iter()
                        .any(|(id, name)| *id == 0 || !label(name))
            })
        {
            return Err(Error::Response("invalid account region table"));
        }
        Ok(self)
    }

    fn contains(&self, region: ProfileRegion) -> bool {
        self.provinces
            .iter()
            .any(|p| p.id == region.province && p.cities.contains_key(&region.city))
    }

    fn bundled() -> Result<&'static Self> {
        static TABLE: OnceLock<Option<RegionTable>> = OnceLock::new();
        TABLE
            .get_or_init(|| {
                let provinces =
                    serde_json::from_str(include_str!("../../../assets/account-regions.json"))
                        .ok()?;
                Self {
                    version: "bundled".into(),
                    provinces,
                }
                .validate()
                .ok()
            })
            .as_ref()
            .ok_or(Error::State("bundled profile regions are unavailable"))
    }

    fn cached(store: &StateStore, uid: &str) -> Result<Option<Self>> {
        Ok(store
            .get(uid, NAMESPACE, "current")?
            .and_then(|value| serde_json::from_value::<Self>(value).ok())
            .and_then(|table| table.validate().ok()))
    }

    pub(super) fn cached_label(
        store: &StateStore,
        uid: &str,
        province: u32,
        city: u32,
    ) -> Result<Option<String>> {
        Ok(Self::cached(store, uid)?.and_then(|table| {
            let region = table
                .provinces
                .iter()
                .find(|region| region.id == province)?;
            let city = region.cities.get(&city)?;
            if matches!(region.kind, Some(1 | 2)) {
                Some(city.clone())
            } else {
                Some(format!("{} · {city}", region.name))
            }
        }))
    }

    pub(super) fn allows(store: &StateStore, uid: &str, region: ProfileRegion) -> Result<bool> {
        Ok(match Self::cached(store, uid)? {
            Some(table) => table.contains(region),
            None => Self::bundled()?.contains(region),
        })
    }

    /// PC host M/198: preserve last validated table when the read fails.
    pub(crate) async fn refresh_for(
        session: Arc<AccountSession>,
        token: &SessionToken,
    ) -> Result<Option<Self>> {
        let store = session.persistent_store()?;
        let uid = session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let _lock = match OperationLock::acquire(store.directory(), NAMESPACE, &uid) {
            Ok(lock) => lock,
            Err(Error::MutationBusy) => {
                return session.with_token(token, |uid| Self::cached(&store, uid));
            }
            Err(error) => return Err(error),
        };
        let request = session.run_for(token, |api, _| async move {
            let reply = api
                .call("/api/lbs/region/code", json!({}), false, &[200])
                .await?;
            serde_json::from_value::<Self>(reply["data"].clone())?.validate()
        });
        match tokio::time::timeout(Duration::from_secs(5), request).await {
            Ok(Ok(table)) => session.with_token(token, |uid| {
                if Self::cached(&store, uid)?.as_ref() != Some(&table) {
                    store.put(uid, NAMESPACE, "current", &serde_json::to_value(&table)?)?;
                }
                Ok(Some(table))
            }),
            Ok(Err(error))
                if error.is_service_code(301) || matches!(error.root(), Error::StaleResponse) =>
            {
                Err(error)
            }
            _ => session.with_token(token, |uid| Self::cached(&store, uid)),
        }
    }
}
