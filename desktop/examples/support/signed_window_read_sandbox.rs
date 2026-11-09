//! Native admission for the host launcher's strictly isolated live mode.
use cumulus_core::{
    download::transfer::MediaUrl,
    protocol::http::{ApiOrigin, ClientProfile, OFFICIAL_WINDOWS_USER_AGENT},
};
use serde::Deserialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    net::SocketAddr,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

type Check<T> = Result<T, Box<dyn std::error::Error>>;
const PROXY: &str = "127.0.0.1:41389";
const WORK: &str = "/work";
const PROOF: &str = "/proof";
const PARAMS: &str = "/prepared/parameters.json";
const CREDENTIALS: &str = "/prepared/credentials/core.sqlite3";
const CONFIG: &str = "/prepared/config/core.sqlite3";
const MAX_DB: u64 = 64 * 1024 * 1024;
const MAX_PARAMETERS: u64 = 20 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Parameters {
    pub expected_uid: String,
    pub keyword: String,
    pub song_id: String,
    pub client_sign: String,
    pub os_version: String,
    pub media_host: String,
}

impl Parameters {
    pub fn profile(&self) -> ClientProfile {
        ClientProfile {
            user_agent: OFFICIAL_WINDOWS_USER_AGENT.into(),
            os_version: self.os_version.clone(),
            client_sign: Some(self.client_sign.clone()),
        }
    }

    fn validate(&self) -> Check<()> {
        self.profile().validate()?;
        if self.os_version.is_empty()
            || self.client_sign.is_empty()
            || !valid_media_host(&self.media_host)
        {
            return Err("invalid live proof parameters".into());
        }
        // Exact decimal UID/song, bounded query and signed scope are also
        // checked by Gate before it admits a request.
        Ok(())
    }
}

pub struct Prepared {
    pub params: Parameters,
    pub broker: SocketAddr,
    pub work: PathBuf,
    pub credential_dir: PathBuf,
    pub config_dir: PathBuf,
}

pub fn admit() -> Check<Prepared> {
    super::require_network_isolation()?;
    let v4 = fs::read_to_string("/proc/net/route")?;
    let v6 = match fs::read_to_string("/proc/net/ipv6_route") {
        Ok(routes) => routes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    if !loopback_routes(&v4, &v6) {
        return Err("direct network route in live proof".into());
    }
    let proxy = std::env::var("CUMULUS_SIGNED_WINDOW_PROXY")?;
    if proxy != PROXY
        || std::env::vars_os().any(|(key, _)| {
            let key = key.to_string_lossy();
            [
                "http_proxy",
                "https_proxy",
                "all_proxy",
                "no_proxy",
                "CUMULUS_PROOF_CAPTURE_ACK",
                "CUMULUS_PROOF_FRONTEND_DIST",
            ]
            .iter()
            .any(|blocked| key.eq_ignore_ascii_case(blocked))
        })
    {
        return Err("unexpected live proof environment".into());
    }
    for hidden in [
        "/home",
        "/sys",
        "/etc/resolv.conf",
        "/run/user",
        "/run/dbus",
    ] {
        match fs::symlink_metadata(hidden) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            _ => return Err("unsafe mount in live proof".into()),
        }
    }
    for (key, expected) in [
        ("HOME", "/profile"),
        ("XDG_RUNTIME_DIR", "/runtime"),
        ("XAUTHORITY", "/xauth"),
    ] {
        if std::env::var(key).as_deref() != Ok(expected) {
            return Err("private desktop environment is required".into());
        }
    }
    if !std::env::var("DISPLAY").is_ok_and(|display| display.starts_with(':'))
        || !std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .is_ok_and(|addr| addr.starts_with("unix:path=/"))
    {
        return Err("private local display and bus are required".into());
    }
    let work = Path::new(WORK);
    let proof = Path::new(PROOF);
    for path in [work, proof] {
        if !private_dir(path)? || fs::read_dir(path)?.next().is_some() {
            return Err("fresh private work and proof directories are required".into());
        }
    }
    let mounts = fs::read_to_string("/proc/self/mountinfo")?;
    for path in [PARAMS, CREDENTIALS, CONFIG] {
        if !readonly_mount(&mounts, Path::new(path)) {
            return Err("prepared input is not mounted read only".into());
        }
        for parent in Path::new(path)
            .ancestors()
            .skip(1)
            .take_while(|parent| *parent != Path::new("/"))
        {
            if !fs::symlink_metadata(parent)?.is_dir() {
                return Err("redirected prepared input directory".into());
            }
        }
    }
    let (parameters, size) = private_input(PARAMS, MAX_PARAMETERS)?;
    let mut bytes = Vec::with_capacity(size as usize);
    parameters
        .take(MAX_PARAMETERS + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != size {
        return Err("prepared parameters changed while reading".into());
    }
    let params: Parameters = serde_json::from_slice(&bytes)?;
    params.validate()?;
    let credential_dir = copy_database(CREDENTIALS, work.join("credentials"))?;
    let config_dir = copy_database(CONFIG, work.join("config"))?;
    Ok(Prepared {
        params,
        broker: proxy.parse()?,
        work: work.into(),
        credential_dir,
        config_dir,
    })
}

fn own_uid() -> u32 {
    // SAFETY: geteuid reads this process' identity and takes no pointers.
    unsafe { libc::geteuid() }
}

fn private_dir(path: &Path) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    Ok(metadata.is_dir() && metadata.uid() == own_uid() && metadata.mode() & 0o777 == 0o700)
}

fn private_input(path: &str, max: u64) -> Check<(File, u64)> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != own_uid()
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > max
    {
        return Err("invalid prepared file shape".into());
    }
    Ok((file, metadata.len()))
}

fn copy_database(source: &str, directory: PathBuf) -> Check<PathBuf> {
    let (source, size) = private_input(source, MAX_DB)?;
    fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join("core.sqlite3"))?;
    let count = io::copy(&mut source.take(MAX_DB + 1), &mut target)?;
    if count != size {
        return Err("prepared database changed while copying".into());
    }
    target.sync_all()?;
    Ok(directory)
}

fn readonly_mount(mounts: &str, path: &Path) -> bool {
    mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_ascii_whitespace().skip(4);
            let mount = Path::new(fields.next()?);
            let options = fields.next()?;
            path.starts_with(mount).then(|| {
                (
                    mount.components().count(),
                    options.split(',').any(|item| item == "ro"),
                )
            })
        })
        .max_by_key(|entry| entry.0)
        .is_some_and(|(_, readonly)| readonly)
}

fn loopback_routes(ipv4: &str, ipv6: &str) -> bool {
    let mut v4 = ipv4.lines();
    if !v4.next().is_some_and(|header| header.starts_with("Iface")) {
        return false;
    }
    v4.filter(|line| !line.trim().is_empty()).all(|line| {
        let columns: Vec<_> = line.split_ascii_whitespace().collect();
        columns.len() == 11
            && columns[0] == "lo"
            && columns[2] == "00000000"
            && u32::from_str_radix(columns[1], 16).is_ok_and(|dest| dest & 255 == 127)
    }) && ipv6
        .lines()
        .filter(|line| !line.trim().is_empty())
        .all(|line| {
            let columns: Vec<_> = line.split_ascii_whitespace().collect();
            columns.len() == 10
                && columns[9] == "lo"
                && columns[4] == "00000000000000000000000000000000"
                && ((columns[0] == "00000000000000000000000000000001" && columns[1] == "80")
                // Linux can include an unreachable IPv6 default on loopback.
                || (columns[0] == "00000000000000000000000000000000" && columns[1] == "00"
                    && u32::from_str_radix(columns[8], 16).is_ok_and(|flags| flags & 0x200 != 0)))
        })
}

fn valid_media_host(host: &str) -> bool {
    (1..=253).contains(&host.len())
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len()) && !label.starts_with('-') && !label.ends_with('-')
        })
        && host
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        && MediaUrl::parse(&format!("https://{host}/proof.m4a")).is_ok()
        && ApiOrigin::Pc.url().host_str() != Some(host)
}

pub fn write_report(proof: &serde_json::Value) -> Check<()> {
    write_report_at(Path::new(PROOF), proof)
}

fn write_report_at(directory: &Path, proof: &serde_json::Value) -> Check<()> {
    if !private_dir(directory)? || directory.join("report.json").exists() {
        return Err("refusing existing proof output".into());
    }
    let pending = directory.join("report.pending");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&pending)?;
    file.write_all(&serde_json::to_vec_pretty(proof)?)?;
    file.sync_all()?;
    let publish = fs::hard_link(&pending, directory.join("report.json"));
    fs::remove_file(pending)?;
    publish?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_routes_are_not_a_usable_network_fence() {
        let v4 =
            "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n";
        let rejected = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo\n";
        assert!(loopback_routes(v4, rejected));
        assert!(!loopback_routes(
            v4,
            &rejected.replace("00200200", "00000001")
        ));
        assert!(!loopback_routes(
            &format!("{v4}eth0 00000000 0100007f 0003 0 0 0 00000000 0 0 0\n"),
            rejected
        ));
        assert!(!loopback_routes(
            &format!("{v4}lo 00000000 00000000 0001 0 0 0 00000000 0 0 0\n"),
            rejected
        ));
    }

    #[test]
    fn readonly_inputs_and_one_canonical_cdn_host_are_required() {
        let mounts = "31 1 1:1 / / rw - tmpfs tmpfs rw\n32 31 1:2 /src /prepared ro - ext4 /dev/test rw\n33 32 1:3 /src /prepared/config rw - tmpfs tmpfs rw\n";
        assert!(readonly_mount(mounts, Path::new(CREDENTIALS)));
        assert!(!readonly_mount(mounts, Path::new(CONFIG)));
        assert!(valid_media_host("m1.music.126.net"));
        for invalid in [
            "M1.music.126.net",
            "m1.music.126.net:443",
            "interfacepc.music.163.com",
            "m1.music.126.net.evil",
            "-m1.music.126.net",
            "m1..music.126.net",
            "127.0.0.1",
        ] {
            assert!(!valid_media_host(invalid));
        }
    }

    #[test]
    fn prepared_configuration_is_strict_and_needs_no_transport() {
        let base = serde_json::json!({"expectedUid":"83","keyword":"合成检索","songId":"41",
            "clientSign":"synthetic-sign","osVersion":"synthetic-version","mediaHost":"m1.music.126.net"});
        let checked: Parameters = serde_json::from_value(base.clone()).unwrap();
        assert!(checked.validate().is_ok());
        let mut extra = base.clone();
        extra["proxy"] = "https://localhost:1".into();
        assert!(serde_json::from_value::<Parameters>(extra).is_err());
        let mut bad_sign = base;
        bad_sign["clientSign"] = "invalid\nheader".into();
        let invalid: Parameters = serde_json::from_value(bad_sign).unwrap();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn publishes_one_private_report_without_overwriting_it() {
        let root = tempfile::tempdir().unwrap();
        let proof = root.path().join("proof");
        fs::DirBuilder::new().mode(0o700).create(&proof).unwrap();
        let original = serde_json::json!({"ok": true});
        write_report_at(&proof, &original).unwrap();
        assert!(write_report_at(&proof, &serde_json::json!({"ok": false})).is_err());
        let published = proof.join("report.json");
        assert_eq!(fs::metadata(&published).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(published).unwrap()).unwrap(),
            original
        );
        assert_eq!(fs::read_dir(proof).unwrap().count(), 1);
    }
}
