//! Local network checks. No music client, DNS, external destination, account or logger.

use serde::Serialize;
use std::{
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultRoute {
    Present,
    Missing,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Loopback {
    Available,
    PermissionDenied,
    Unavailable,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalNetworkReport {
    pub ipv4_loopback: Loopback,
    pub ipv6_loopback: Loopback,
    pub default_route: DefaultRoute,
    pub proxy_environment_configured: bool,
}

const PROXY_VARIABLES: [&str; 6] = [
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
];

fn status(result: std::io::Result<()>) -> Loopback {
    match result {
        Ok(()) => Loopback::Available,
        Err(error) if error.kind() == ErrorKind::PermissionDenied => Loopback::PermissionDenied,
        Err(_) => Loopback::Unavailable,
    }
}

fn loopback(address: IpAddr) -> std::io::Result<()> {
    let server = TcpListener::bind(SocketAddr::new(address, 0))?;
    let _connection =
        TcpStream::connect_timeout(&server.local_addr()?, Duration::from_millis(250))?;
    Ok(())
}

/// Only loopback listeners are opened. The host provides the result of its
/// local routing-table read; no addresses, routes or proxy values are returned.
pub fn check_local_network(default_route: DefaultRoute) -> LocalNetworkReport {
    LocalNetworkReport {
        ipv4_loopback: status(loopback(Ipv4Addr::LOCALHOST.into())),
        ipv6_loopback: status(loopback(Ipv6Addr::LOCALHOST.into())),
        default_route,
        proxy_environment_configured: PROXY_VARIABLES
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_checks_never_need_external_sockets_and_classify_local_denial() {
        assert_eq!(
            status(loopback(Ipv4Addr::LOCALHOST.into())),
            Loopback::Available
        );
        assert_eq!(
            status(Err(ErrorKind::PermissionDenied.into())),
            Loopback::PermissionDenied
        );
        assert_eq!(
            status(Err(ErrorKind::AddrNotAvailable.into())),
            Loopback::Unavailable
        );
        let report = serde_json::to_value(check_local_network(DefaultRoute::Present)).unwrap();
        assert_eq!(report["ipv4Loopback"], "available");
        assert_eq!(report["defaultRoute"], "present");
        assert!(report["proxyEnvironmentConfigured"].is_boolean());
        assert_eq!(report.as_object().unwrap().len(), 4);
    }
}
