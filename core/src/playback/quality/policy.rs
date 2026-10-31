use super::{QualityRight, QualityUseRight, TrialPhase};
use crate::{
    Error, Result,
    membership::{MembershipState, Quality},
    protocol::api::identifier,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityAvailability {
    Available,
    Unavailable,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityAccess {
    Allowed,
    TrialEligible,
    TrialActive,
    RequiresVip,
    RequiresSvip,
    RequiresPurchase,
    Unavailable,
    TrialUnsupported,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityPrivilege {
    pub id: String,
    pub flag: Option<u32>,
    pub max_song_br: Option<u64>,
    pub max_free_br: Option<u64>,
    pub payed: Option<u32>,
    pub fee: Option<u32>,
}
impl QualityPrivilege {
    pub fn from_official(value: &Value) -> Result<Self> {
        let number = |key: &str| -> Result<Option<u64>> {
            match value.get(key).filter(|v| !v.is_null()) {
                None => Ok(None),
                Some(v) => v
                    .as_u64()
                    .map(Some)
                    .ok_or(Error::Response("invalid quality privilege number")),
            }
        };
        let flags = |key: &str| -> Result<Option<u32>> {
            number(key)?
                .map(|v| {
                    u32::try_from(v).map_err(|_| Error::Response("invalid quality privilege flags"))
                })
                .transpose()
        };
        Ok(Self {
            id: identifier(&value["id"])?,
            flag: flags("flag")?,
            max_song_br: number("maxbr")?.map(|v| v.div_ceil(1000)),
            max_free_br: number("fl")?.map(|v| v.div_ceil(1000)),
            payed: flags("payed")?,
            fee: flags("fee")?,
        })
    }
    fn purchased(&self) -> Option<bool> {
        self.payed.map(|v| v & 6 != 0)
    }
    pub fn availability(&self, quality: Quality) -> QualityAvailability {
        let available = if quality.bitrate() < Quality::HiRes.bitrate() {
            self.max_song_br
                .map(|br| br >= u64::from(quality.bitrate().div_ceil(1000)))
        } else {
            let bit = match quality {
                Quality::HiRes => 12,
                Quality::Dolby => 14,
                Quality::JyMaster => 16,
                Quality::JyEffect => 17,
                Quality::Sky => 18,
                Quality::Vivid => 21,
                _ => unreachable!(),
            };
            self.flag.map(|flag| flag & (1 << bit) != 0)
        };
        match available {
            Some(true) => QualityAvailability::Available,
            Some(false) => QualityAvailability::Unavailable,
            None => QualityAvailability::Unknown,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualitySubscriber {
    pub vip: bool,
    pub svip: bool,
    pub music_package: bool,
}
impl From<&MembershipState> for QualitySubscriber {
    fn from(value: &MembershipState) -> Self {
        Self {
            vip: value.is_vinyl_vip,
            svip: value.is_plus_vip,
            music_package: value.is_music_package,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QualityExperiments {
    /// preload#musicPackageCanUseHiresAndLoseless. None is not false.
    pub music_package_hires: Option<bool>,
    /// StandardAudio_VIPLogo_PCV2=t1, applicable to current trial tracks.
    pub standard_trial_requires_purchase: Option<bool>,
}
pub struct QualityPolicy<'a> {
    pub privilege: Option<&'a QualityPrivilege>,
    pub subscriber: Option<QualitySubscriber>,
    pub use_rights: &'a BTreeMap<QualityRight, Option<QualityUseRight>>,
    pub experiments: QualityExperiments,
    /// The currently authorized partial/full-trial timeline, not an IPC claim.
    pub trial: bool,
}
impl QualityPolicy<'_> {
    pub fn availability(&self, quality: Quality) -> QualityAvailability {
        self.privilege
            .map_or(QualityAvailability::Unknown, |v| v.availability(quality))
    }
    pub fn access(&self, quality: Quality) -> QualityAccess {
        if self.availability(quality) == QualityAvailability::Unavailable {
            return QualityAccess::Unavailable;
        }
        if quality == Quality::Vivid && self.trial {
            return QualityAccess::TrialUnsupported;
        }
        let base = self.base(quality);
        if base == QualityAccess::Allowed {
            return base;
        }
        if let Some(right) = QualityRight::for_quality(quality) {
            let phase = self
                .use_rights
                .get(&right)
                .and_then(Option::as_ref)
                .map(QualityUseRight::phase);
            match phase {
                Some(TrialPhase::Eligible) => return QualityAccess::TrialEligible,
                Some(TrialPhase::Active) => return QualityAccess::TrialActive,
                Some(TrialPhase::Granted) => return QualityAccess::Allowed,
                Some(TrialPhase::Unavailable) => (),
                _ => return QualityAccess::Unknown,
            }
        }
        base
    }
    fn base(&self, quality: Quality) -> QualityAccess {
        let purchased = self.privilege.and_then(QualityPrivilege::purchased);
        if purchased == Some(true) {
            return QualityAccess::Allowed;
        }
        if quality == Quality::Standard && self.trial {
            let ts = self.privilege.and_then(|p| p.fee).map(|fee| fee & 1 != 0);
            if ts != Some(false) {
                match self.experiments.standard_trial_requires_purchase {
                    Some(true) if purchased == Some(false) => {
                        return QualityAccess::RequiresPurchase;
                    }
                    Some(false) => (),
                    _ => return QualityAccess::Unknown,
                }
            }
        }
        // M/40 intrinsically free levels. A free quality does not grant full-song
        // copyright; the source resolver still enforces its authorized timeline.
        if matches!(
            quality,
            Quality::Standard | Quality::Higher | Quality::Aac64
        ) {
            return QualityAccess::Allowed;
        }
        let free = self
            .privilege
            .and_then(|p| p.max_free_br)
            .map(|br| if br == 0 { 192 } else { br });
        if free.is_some_and(|br| br >= u64::from(quality.bitrate().div_ceil(1000))) {
            return QualityAccess::Allowed;
        }
        let svip = matches!(
            quality,
            Quality::Vivid | Quality::Sky | Quality::JyMaster | Quality::Dolby
        );
        if let Some(subscriber) = self.subscriber {
            if subscriber.svip || (subscriber.vip && !svip) {
                return QualityAccess::Allowed;
            }
            if subscriber.music_package
                && !subscriber.vip
                && !subscriber.svip
                && matches!(
                    quality,
                    Quality::ExHigh | Quality::Lossless | Quality::HiRes
                )
            {
                match self.experiments.music_package_hires {
                    Some(true) => return QualityAccess::Allowed,
                    Some(false) => (),
                    None => return QualityAccess::Unknown,
                }
            }
        } else {
            return QualityAccess::Unknown;
        }
        if free.is_none() || purchased.is_none() {
            return QualityAccess::Unknown;
        }
        if svip {
            QualityAccess::RequiresSvip
        } else {
            QualityAccess::RequiresVip
        }
    }
}
