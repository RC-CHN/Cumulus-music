//! Fixed public member-center handoff. It is not an account or payment ticket.
use crate::{Error, Result};
use qrcodegen::{QrCode, QrCodeEcc, QrSegment, Version};
use serde::Serialize;

// Recovered PC TrackSignInCard/Modal.tsx, QRCodeCloundShell.
const MEMBER_PORTAL: &str = "https://music.163.com/g/vip-portal?nm_style=sbt&scene=pc_viptab_scan";

#[derive(Serialize)]
pub struct MemberPortalQr {
    pub modules: Vec<Vec<bool>>,
}

pub fn member_portal_qr() -> Result<MemberPortalQr> {
    let code = QrCode::encode_segments_advanced(
        &QrSegment::make_segments(MEMBER_PORTAL),
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
