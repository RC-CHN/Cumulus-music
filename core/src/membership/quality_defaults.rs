//! M/116 resetListen after a fresh, verified M/198 member observation.
use super::{MembershipCommit, Quality};
use crate::protocol::context::RequestContext;
use serde_json::Value;

pub(crate) const MUSIC_PACKAGE_HIRES: &str = "preload#musicPackageCanUseHiresAndLoseless";

pub(crate) fn music_package_hires(data: Option<&Value>) -> Option<bool> {
    match data.and_then(|data| data.get(MUSIC_PACKAGE_HIRES)) {
        // M/247 merges verified remote settings over its built-in PC defaults.
        None => Some(true),
        Some(value) => value.as_bool(),
    }
}

fn ceiling(
    vinyl: bool,
    plus: bool,
    music_package: bool,
    package_config: Option<bool>,
) -> Option<Quality> {
    if plus {
        None
    } else if vinyl {
        Some(Quality::JyEffect)
    } else if music_package {
        // The live service established the package, but only an initialized
        // native PC snapshot can tell whether the M/247 override permits it.
        package_config.map(|enabled| {
            if enabled {
                Quality::HiRes
            } else {
                Quality::ExHigh
            }
        })
    } else {
        Some(Quality::ExHigh)
    }
}

pub(crate) fn membership_default_ceiling(
    commit: &MembershipCommit,
    context: Option<&RequestContext>,
) -> Option<Quality> {
    let [vinyl, plus, music_package, _, _] =
        commit.expiries.map(|expiry| expiry > commit.server_now);
    let package_config = if music_package && !plus && !vinyl {
        context
            .and_then(|context| context.pc_snapshot().ok().flatten())
            .and_then(|snapshot| music_package_hires(snapshot.data.as_ref()))
    } else {
        None
    };
    ceiling(vinyl, plus, music_package, package_config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_ceiling_follows_m116_priorities_and_preserves_unknown_package_policy() {
        use Quality::{ExHigh, HiRes, JyEffect};
        for (vinyl, plus, package, config, expected) in [
            (true, true, true, Some(true), None),
            (false, true, false, None, None),
            (true, false, true, Some(true), Some(JyEffect)),
            (true, false, false, None, Some(JyEffect)),
            (false, false, true, Some(true), Some(HiRes)),
            (false, false, true, Some(false), Some(ExHigh)),
            (false, false, true, None, None),
            (false, false, false, None, Some(ExHigh)),
        ] {
            assert_eq!(ceiling(vinyl, plus, package, config), expected);
        }
    }
}
