//! Read the signed-in account's device list, separate from login challenges.
use super::{MusicClient, dto::AccountData};
use crate::{Error, Result, account::SessionToken};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::Mutex,
    time::{Duration, Instant},
};

const PATH: &str = "/api/middle/user/device/list";
const MAX_DEVICES: usize = 128;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginDevicesView {
    pub devices: Vec<LoginDeviceView>,
    pub limits: Option<LoginDeviceLimitsView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginDeviceView {
    /// An opaque display key for this response, never the server deviceKey.
    pub id: String,
    pub name: Option<String>,
    pub location: Option<String>,
    pub login_type: Option<String>,
    /// Server time in milliseconds, absent when missing or malformed.
    pub last_active_time: Option<u64>,
    pub os: Option<String>,
    pub current: bool,
    /// M/3122 groups only status 1 as signed in.
    pub signed_in: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginDeviceLimitsView {
    pub current_num: Option<u32>,
    pub device_limit: Option<u32>,
}

fn text(value: &Value, limit: usize) -> Option<&str> {
    value
        .as_str()
        .filter(|s| s.len() <= limit && !s.trim().is_empty() && !s.chars().any(char::is_control))
}

fn unsigned(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        let text = value.as_str()?;
        if text.len() > 20 || !text.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        text.parse().ok()
    })
}

pub(super) struct DeviceRow {
    pub id: String,
    pub key: String,
    pub current: bool,
}

pub(super) struct DeviceList {
    pub rows: Vec<DeviceRow>,
    view: LoginDevicesView,
}

struct PublishedRows {
    owner: SessionToken,
    published: Instant,
    rows: Vec<DeviceRow>,
}

#[derive(Default)]
pub(super) struct DeviceRows(Mutex<Option<PublishedRows>>);

impl DeviceRows {
    pub fn key_for(&self, owner: &SessionToken, id: &str) -> Result<String> {
        let list = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let list = list
            .as_ref()
            .filter(|list| {
                &list.owner == owner && list.published.elapsed() < Duration::from_secs(300)
            })
            .ok_or(Error::StaleResponse)?;
        let row = list
            .rows
            .iter()
            .find(|row| row.id == id)
            .ok_or(Error::StaleResponse)?;
        if row.current {
            return Err(Error::State("cannot remove the current login device"));
        }
        Ok(row.key.clone())
    }
}

fn parse(response: &Value) -> Result<DeviceList> {
    let data = &response["data"];
    let rows = data["userDeviceList"]
        .as_array()
        .ok_or(Error::Response("missing login device list"))?;
    if rows.len() > MAX_DEVICES {
        return Err(Error::Response("login device list is too large"));
    }
    let mut keys = HashSet::with_capacity(rows.len());
    let mut seen_current = false;
    let mut devices = Vec::with_capacity(rows.len());
    let mut native_rows = Vec::with_capacity(rows.len());
    for row in rows {
        let key = text(&row["deviceKey"], 512)
            .ok_or(Error::Response("invalid login device identifier"))?;
        if !keys.insert(key) {
            return Err(Error::Response("duplicate login device identifier"));
        }
        let current = row["ifLoginDevice"] == true || unsigned(&row["ifLoginDevice"]) == Some(1);
        if current && seen_current {
            return Err(Error::Response("multiple current login devices"));
        }
        seen_current |= current;
        let id = uuid::Uuid::new_v4().simple().to_string();
        native_rows.push(DeviceRow {
            id: id.clone(),
            key: key.into(),
            current,
        });
        devices.push(LoginDeviceView {
            id,
            name: text(&row["deviceName"], 512).map(str::to_owned),
            location: text(&row["loginPlace"], 256)
                .or_else(|| text(&row["region"], 256))
                .map(str::to_owned),
            login_type: text(&row["loginType"], 128)
                .map(str::to_owned)
                .or_else(|| row["loginType"].as_u64().map(|n| n.to_string())),
            last_active_time: unsigned(&row["lastActiveTime"])
                .filter(|n| (1..=253_402_300_799_999).contains(n)),
            os: text(&row["os"], 128).map(str::to_owned),
            current,
            signed_in: row["status"] == 1 || row["status"] == "1",
        });
    }
    // Keep the service order within the same group; pin the current device.
    devices.sort_by_key(|device| (!device.current, !device.signed_in));
    let raw_limits = &data["userDeviceLimitInfoDTO"];
    let limits = raw_limits.is_object().then(|| LoginDeviceLimitsView {
        current_num: unsigned(&raw_limits["currentNum"]).and_then(|n| u32::try_from(n).ok()),
        device_limit: unsigned(&raw_limits["deviceLimit"]).and_then(|n| u32::try_from(n).ok()),
    });
    Ok(DeviceList {
        rows: native_rows,
        view: LoginDevicesView { devices, limits },
    })
}

impl MusicClient {
    pub(super) async fn read_login_devices(&self, token: &SessionToken) -> Result<DeviceList> {
        // M/15 defines the request; M/3122 fetchDeviceList passes excStatus=9.
        self.session
            .run_for(token, |api, _| async move {
                let reply = api
                    .call(PATH, json!({"excStatus":9}), false, &[200])
                    .await?;
                parse(&reply)
            })
            .await
    }

    pub async fn login_devices(&self, epoch: &str) -> Result<AccountData<LoginDevicesView>> {
        let token = self.player_token(epoch)?;
        let list = self.read_login_devices(&token).await?;
        self.session.with_token(&token, |_| {
            *self
                .login_device_rows
                .0
                .lock()
                .map_err(|_| Error::LockPoisoned)? = Some(PublishedRows {
                owner: token.clone(),
                published: Instant::now(),
                rows: list.rows,
            });
            Ok(AccountData {
                epoch: epoch.into(),
                value: list.view,
            })
        })
    }
}
