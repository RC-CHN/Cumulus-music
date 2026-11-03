//! Fixed official App handoff; neither a login ticket nor an account mutation.
use super::{MusicClient, dto::AccountData};
use crate::{Error, Result};
use qrcodegen::{QrCode, QrCodeEcc, QrSegment, Version};
use serde::Serialize;

const APP_SETTINGS: &str = "orpheus://settings/bindaccount";

#[derive(Serialize)]
pub struct AccountBindingQrView {
    pub modules: Vec<Vec<bool>>,
}

impl MusicClient {
    pub fn account_binding_qr(&self, epoch: &str) -> Result<AccountData<AccountBindingQrView>> {
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |_| {
            // M3153:186–190 supplies the fixed URI; M3047:23–28 uses ECC L.
            let qr = QrCode::encode_segments_advanced(
                &QrSegment::make_segments(APP_SETTINGS),
                QrCodeEcc::Low,
                Version::MIN,
                Version::MAX,
                None,
                false,
            )
            .map_err(|_| Error::Protocol("account binding QR encoding failed"))?;
            let modules = (0..qr.size())
                .map(|y| (0..qr.size()).map(|x| qr.get_module(x, y)).collect())
                .collect();
            Ok(AccountData {
                epoch: epoch.into(),
                value: AccountBindingQrView { modules },
            })
        })
    }
}
