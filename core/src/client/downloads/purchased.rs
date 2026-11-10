//! Pure quality projection from native service purchase rights. The caller
//! still owns account, source and download authorization checks.
use crate::{membership::Quality, protocol::pc_experiments::PcExperimentGroups};
use std::cmp::Reverse;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::client) struct Offer {
    max_song_kbps: Option<u64>,
    flags: u32,
}

impl Offer {
    pub(in crate::client) fn from_privilege(p: &serde_json::Value) -> Option<Self> {
        // M/35.h is a purchase mask, not a truthiness test. Numeric wire
        // fields and unsigned bounds avoid admitting malformed service data.
        if p["payed"].as_u64().and_then(|n| u32::try_from(n).ok())? & 6 == 0
            || p["st"].as_i64()? < 0
            || p["dl"].as_u64()? == 0
            || crate::protocol::wire::serialization::js_truthy(&p["bd"])
        {
            return None;
        }
        Some(Self {
            // M/21.A keeps the old maxbr-derived maxSongBr even if
            // maxBrLevel is available. Missing maxbr only removes base rows.
            max_song_kbps: p["maxbr"].as_u64().map(|n| n.div_ceil(1000)),
            flags: u32::try_from(p["flag"].as_u64().unwrap_or(0)).ok()?,
        })
    }

    pub(in crate::client) fn qualities(self, vivid: bool) -> Vec<Quality> {
        use Quality::*;
        // M/40.c + M/23.v, then M/3051 filters by tagSelectionMeta and
        // sorts by bitrate descending. No VIP or trial adds a choice here.
        [
            (Vivid, vivid && self.flags & (1 << 21) != 0),
            (Sky, self.flags & (1 << 18) != 0),
            (JyMaster, self.flags & (1 << 16) != 0),
            (JyEffect, self.flags & (1 << 17) != 0),
            (HiRes, self.flags & (1 << 12) != 0),
            (Lossless, self.base(999)),
            (ExHigh, self.base(320)),
            (Standard, self.base(128)),
        ]
        .into_iter()
        .filter_map(|(quality, offered)| offered.then_some(quality))
        .collect()
    }

    fn base(self, kbps: u64) -> bool {
        self.max_song_kbps.is_some_and(|max| max >= kbps)
    }
}

pub(in crate::client) fn purchased_qualities(
    offers: impl IntoIterator<Item = Offer>,
    vivid: bool,
) -> Vec<Quality> {
    let mut union = offers
        .into_iter()
        .flat_map(|offer| offer.qualities(vivid))
        .collect::<Vec<_>>();
    union.sort_unstable_by_key(|quality| Reverse(quality.bitrate()));
    union.dedup();
    union
}

pub(in crate::client) fn vivid_available(groups: Option<&PcExperimentGroups>) -> bool {
    groups.is_some_and(|groups| {
        groups.group("PH-PC-hyyinzhi") == Some("t1")
            && groups.group("PH-PC-zhenyingn") == Some("t1")
    })
}

/// M/31 opens this fixed M/40 public help page in the system browser.
pub(in crate::client) fn purchased_quality_guide_url(vivid: bool) -> &'static str {
    if vivid {
        "https://music.163.com/g/st-vip-audio-quality"
    } else {
        "https://y.music.163.com/m/at/648fba3cbe7c73d87cbc2e81"
    }
}
