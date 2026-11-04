//! Fixed public member-center handoff. It is not an account or payment ticket.
use crate::{Error, Result};
use qrcodegen::{QrCode, QrCodeEcc, QrSegment, Version};
use serde::Serialize;

// Recovered PC TrackSignInCard/Modal.tsx, QRCodeCloundShell.
pub(crate) const MEMBER_PORTAL: &str =
    "https://music.163.com/g/vip-portal?nm_style=sbt&scene=pc_viptab_scan";
const SIGN_PORTAL: &str = "https://music.163.com/g/vip-portal?nm_style=sbt&scene=pc_vip_sign";

#[derive(Serialize)]
pub struct MemberPortalQr {
    pub modules: Vec<Vec<bool>>,
}

pub fn member_portal_qr() -> Result<MemberPortalQr> {
    encode(MEMBER_PORTAL)
}

pub(crate) fn member_sign_portal_qr() -> Result<MemberPortalQr> {
    encode(SIGN_PORTAL)
}

fn encode(destination: &str) -> Result<MemberPortalQr> {
    let code = QrCode::encode_segments_advanced(
        &QrSegment::make_segments(destination),
        QrCodeEcc::Low,
        Version::MIN,
        Version::MAX,
        None,
        false,
    )
    .map_err(|_| Error::Protocol("member portal QR encoding failed"))?;
    let modules = (0..code.size())
        .map(|y| (0..code.size()).map(|x| code.get_module(x, y)).collect())
        .collect();
    Ok(MemberPortalQr { modules })
}
