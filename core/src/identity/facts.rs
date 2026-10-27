use super::derive_persistent_id;
use crate::{Error, Result};
use encoding_rs::GBK;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::LazyLock,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentitySource {
    pub platform: String,
    pub interface: String,
    pub disk_device: String,
    pub host_description_available: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentitySummary {
    pub platform: String,
    pub mac_available: bool,
    pub disk_available: bool,
    pub board_available: bool,
    pub host_description_available: bool,
}
/// Native-only hardware facts. Deliberately no Debug/Serialize implementation.
pub struct HardwareFacts {
    pub(super) parts: [String; 3],
    pub(super) hardware_bytes: Vec<u8>,
    pub(super) host_bytes: Vec<u8>,
    pub(super) source: IdentitySource,
}
impl HardwareFacts {
    /// Adapters supply normalized display components and actual narrow bytes.
    /// No conversion to UTF-8 or synthetic Windows values is performed here.
    pub fn from_encoded(
        parts: [String; 3],
        hardware_bytes: Vec<u8>,
        host_bytes: Vec<u8>,
        source: IdentitySource,
    ) -> Result<Self> {
        if parts.iter().all(String::is_empty) {
            return Err(Error::State("hardware identity unavailable"));
        }
        if parts
            .iter()
            .any(|v| v.chars().count() > 512 || v.contains(['\r', '\n', '\0']))
            || hardware_bytes.len() > 8192
            || host_bytes.len() > 8192
            || hardware_bytes.contains(&0)
            || host_bytes.contains(&0)
            || [&source.platform, &source.interface, &source.disk_device]
                .iter()
                .any(|v| v.len() > 512 || v.contains(['\r', '\n', '\0']))
        {
            return Err(Error::InvalidInput("invalid hardware facts"));
        }
        Ok(Self {
            parts,
            hardware_bytes,
            host_bytes,
            source,
        })
    }
    pub fn derived_persistent_id(&self) -> String {
        derive_persistent_id(&self.hardware_bytes, &self.host_bytes)
    }
    pub fn summary(&self) -> IdentitySummary {
        IdentitySummary {
            platform: self.source.platform.clone(),
            mac_available: !self.parts[0].is_empty(),
            disk_available: !self.parts[1].is_empty(),
            board_available: !self.parts[2].is_empty(),
            host_description_available: self.source.host_description_available,
        }
    }
}
fn whitespace(c: char) -> bool {
    c.is_whitespace() || ('\u{001c}'..='\u{001f}').contains(&c)
}
fn fact(path: &Path) -> String {
    let read = || -> Option<String> {
        let mut bytes = Vec::new();
        fs::File::open(path)
            .ok()?
            .take(2049)
            .read_to_end(&mut bytes)
            .ok()?;
        let text = String::from_utf8(bytes).ok()?;
        let text = text.trim_matches(whitespace);
        if text.chars().count() > 512
            || text.contains(['\r', '\n', '\0'])
            || [
                "unknown",
                "none",
                "not specified",
                "default string",
                "to be filled by o.e.m.",
            ]
            .contains(&text.to_lowercase().as_str())
        {
            return None;
        }
        Some(text.to_owned())
    };
    read().unwrap_or_default()
}
fn entries(root: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<_> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(|v| v.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    entries
}
fn gbk(text: &str) -> Result<Vec<u8>> {
    let (bytes, _, errors) = GBK.encode(text);
    if errors {
        return Err(Error::State(
            "hardware text cannot use recovered GBK encoding",
        ));
    }
    Ok(bytes.into_owned())
}

/// Archived Linux sysfs selection, not a claim to emulate Windows WMI/SCSI.
/// No privilege escalation, network, process execution or synthetic fallback.
pub fn collect_linux_identity(sysroot: &Path) -> Result<HardwareFacts> {
    static MAC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?:[0-9A-F]{2}:){5}[0-9A-F]{2}$").unwrap());
    static DISK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(?:nvme\d+n\d+|sd[a-z]+|vd[a-z]+)$").unwrap());
    let mut networks = Vec::new();
    for path in entries(&sysroot.join("class/net")) {
        let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        let mac = fact(&path.join("address")).to_ascii_uppercase();
        if !MAC.is_match(&mac) || mac == "FF:FF:FF:FF:FF:FF" || mac.starts_with("00:00:00:00:00:") {
            continue;
        }
        networks.push((
            (
                !path.join("device").exists(),
                fact(&path.join("operstate")) != "up",
                name.to_owned(),
            ),
            mac,
        ));
    }
    networks.sort();
    let (mac, interface) = networks
        .into_iter()
        .next()
        .map(|((_, _, name), mac)| (mac, name))
        .unwrap_or_default();
    let mut disk = String::new();
    let mut disk_device = String::new();
    for path in entries(&sysroot.join("block")) {
        let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        if !DISK.is_match(name) {
            continue;
        }
        let mut serial = fact(&path.join("device/serial"));
        if serial.is_empty() {
            serial = fact(&path.join("serial"));
        }
        if !serial.is_empty() {
            disk = serial.chars().filter(|c| !whitespace(*c)).collect();
            disk_device = name.into();
            break;
        }
    }
    let dmi = sysroot.join("class/dmi/id");
    let mut board = fact(&dmi.join("board_serial"));
    if board.is_empty() {
        board = fact(&dmi.join("product_serial"));
    }
    board = board.chars().filter(|c| !whitespace(*c)).collect();
    let vendor = fact(&dmi.join("sys_vendor"));
    let model = fact(&dmi.join("product_name"));
    let kind = if ["8", "9", "10", "14", "30", "31", "32"]
        .contains(&fact(&dmi.join("chassis_type")).as_str())
    {
        "笔记本电脑"
    } else {
        "台式电脑"
    };
    let host = if vendor.is_empty() && model.is_empty() {
        String::new()
    } else {
        format!("主机:{vendor} {model} {kind}")
    };
    let hardware_bytes = gbk(&format!("{mac}{disk}"))?;
    let host_bytes = gbk(&host)?;
    HardwareFacts::from_encoded(
        [mac, disk, board],
        hardware_bytes,
        host_bytes,
        IdentitySource {
            platform: "linux-sysfs-adapter".into(),
            interface,
            disk_device,
            host_description_available: !host.is_empty(),
        },
    )
}
