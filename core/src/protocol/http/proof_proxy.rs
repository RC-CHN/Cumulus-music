//! Explicit native transport for the optional isolated desktop research probe.
use crate::{Error, Result};
use std::net::{Ipv4Addr, SocketAddr};

pub(crate) fn use_broker(
    builder: reqwest::ClientBuilder,
    broker: SocketAddr,
) -> Result<reqwest::ClientBuilder> {
    if broker.ip() != Ipv4Addr::LOCALHOST || broker.port() == 0 {
        return Err(Error::InvalidInput("proof broker must be IPv4 loopback"));
    }
    let proxy = reqwest::Proxy::https(format!("http://{broker}"))
        .map_err(|_| Error::InvalidInput("invalid proof HTTPS broker"))?;
    Ok(builder.no_proxy().proxy(proxy))
}
