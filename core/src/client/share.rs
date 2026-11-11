//! Local public sharing artifacts. Source authorization stays in its owning feature.
use crate::{Error, Result};
use qrcodegen::{QrCode, QrCodeEcc, QrSegment, Version};

pub(super) fn qr_modules(url: &str) -> Result<Vec<Vec<bool>>> {
    // Recovered QrcodeModal/index.jsx:45–49 selects L. Disable the library's
    // automatic strength boost even if a stronger level fits this version.
    let qr = QrCode::encode_segments_advanced(
        &QrSegment::make_segments(url),
        QrCodeEcc::Low,
        Version::MIN,
        Version::MAX,
        None,
        false,
    )
    .map_err(|_| Error::Protocol("sharing QR encoding failed"))?;
    Ok((0..qr.size())
        .map(|y| (0..qr.size()).map(|x| qr.get_module(x, y)).collect())
        .collect())
}
