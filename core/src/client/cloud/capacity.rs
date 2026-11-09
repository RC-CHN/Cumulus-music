//! The read-only capacity status from official M/247 and M/3100.
use super::{CloudCapacityKind, CloudCapacityStatus, CloudPageView, Context};
use crate::{membership::MembershipSnapshot, protocol::context::RequestContext};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

const CONFIG_KEY: &str = "preload#cloudDisk";
const DAY_MS: f64 = 86_400_000.0;
const FULL: &str = "网盘已满，请清除部分文件，才能继续上传";
const FULL_WITH_OPTIONS: &str = "网盘已满，升级SVIP或购买容量后可继续上传歌曲";
const PERMANENT: &str = "歌曲永久保存，随时随地多端畅听";

pub(super) struct Messages {
    active: Option<String>,
    expiring: Option<String>,
    offer: Option<String>,
}

pub(super) enum Config {
    Classic,
    New(Messages),
    Unknown,
}

impl Config {
    pub(super) fn read(context: Option<&RequestContext>) -> Self {
        let snapshot = context.and_then(|context| context.pc_snapshot().ok().flatten());
        Self::from_data(snapshot.as_ref().and_then(|state| state.data.as_ref()))
    }

    fn from_data(data: Option<&Value>) -> Self {
        let Some(config) = data.and_then(|data| data.get(CONFIG_KEY)) else {
            // M/247's PC defaults apply before an accepted override.
            return Self::New(Messages {
                active: Some("黑胶SVIP尊享6TB超大空间".into()),
                // The bundled string is a prefix next to the native cashier
                // action. Complete the read-only notice without an action.
                expiring: Some("黑胶SVIP6TB超大空间即将到期".into()),
                offer: Some("黑胶SVIP6TB超大空间已失效".into()),
            });
        };
        let message = |key| {
            config[key]
                .as_str()
                .filter(|text| {
                    !text.trim().is_empty()
                        && text.chars().count() <= 160
                        && !text.chars().any(char::is_control)
                })
                .map(str::to_owned)
        };
        // M/247 shallow-merges top-level keys; the accepted object replaces
        // the entire default. Absent or malformed text is never synthesized.
        match config["useNewFeat"].as_bool() {
            Some(false) => Self::Classic,
            Some(true) => Self::New(Messages {
                active: message("svipSimpleText"),
                expiring: message("svipRemainText"),
                offer: message("svipExpiredText"),
            }),
            None => Self::Unknown,
        }
    }
}

struct MemberTime {
    plus: bool,
    expiry: f64,
    server_ms: f64,
    wall_ms: f64,
}

impl MemberTime {
    fn new(value: MembershipSnapshot) -> Option<Self> {
        let wall_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs_f64()
            * 1000.0;
        Some(Self {
            plus: value.state.is_plus_vip,
            expiry: value.state.plus_vip_expire_time,
            server_ms: value.server_now,
            wall_ms,
        })
    }

    fn valid(&self) -> bool {
        self.expiry.is_finite()
            && self.expiry >= 0.0
            && self.server_ms.is_finite()
            && self.server_ms > 0.0
            && self.wall_ms.is_finite()
            && self.wall_ms > 0.0
            && self.plus == (self.expiry > self.server_ms)
    }
}

fn status(
    config: &Config,
    used: Option<u64>,
    max: Option<u64>,
    has_tracks: bool,
    membership: Option<MemberTime>,
) -> Option<CloudCapacityStatus> {
    let full = used? >= max.filter(|max| *max > 0)?;
    if !has_tracks {
        return None;
    }
    let text = |kind, text: &str| {
        Some(CloudCapacityStatus {
            kind,
            text: text.into(),
        })
    };
    let member = membership.filter(MemberTime::valid);
    if full {
        return text(
            CloudCapacityKind::Full,
            if matches!(config, Config::New(_)) && member.as_ref().is_some_and(|state| !state.plus)
            {
                FULL_WITH_OPTIONS
            } else {
                FULL
            },
        );
    }
    match config {
        Config::Classic => text(CloudCapacityKind::Permanent, PERMANENT),
        Config::Unknown => None,
        Config::New(messages) => {
            let member = member?;
            if member.plus {
                if ((member.expiry - member.server_ms) / DAY_MS).ceil() <= 7.0 {
                    text(
                        CloudCapacityKind::SvipExpiring,
                        messages.expiring.as_deref()?,
                    )
                } else {
                    text(CloudCapacityKind::SvipActive, messages.active.as_deref()?)
                }
            } else if !(member.expiry > 0.0 && member.expiry <= member.wall_ms) {
                // The original predicate is deliberately negated: an already
                // expired positive timestamp produces no under-full notice.
                text(CloudCapacityKind::SvipOffer, messages.offer.as_deref()?)
            } else {
                None
            }
        }
    }
}

pub(super) fn apply(page: &mut CloudPageView, context: &Context, config: &Config) {
    let member = if matches!(config, Config::New(_))
        && page.uploaded_count > 0
        && page.size.is_some()
        && page.max_size.is_some_and(|max| max > 0)
    {
        crate::membership::MembershipManager::new(context.session.clone())
            .and_then(|manager| manager.snapshot())
            .ok()
            .and_then(MemberTime::new)
    } else {
        None
    };
    page.capacity_status = status(
        config,
        page.size,
        page.max_size,
        page.uploaded_count > 0,
        member,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CLOCK: f64 = 100.0 * DAY_MS;

    fn member(delta: f64) -> MemberTime {
        MemberTime {
            plus: delta > 0.0,
            expiry: CLOCK + delta,
            server_ms: CLOCK,
            wall_ms: CLOCK,
        }
    }

    fn rendered(
        config: &Config,
        member: Option<MemberTime>,
    ) -> Option<(CloudCapacityKind, String)> {
        status(config, Some(9), Some(10), true, member).map(|value| (value.kind, value.text))
    }

    #[test]
    fn default_quota_uses_server_days_and_distinct_expiry_clock() {
        let config = Config::from_data(None);
        assert_eq!(
            rendered(&config, Some(member(7.0 * DAY_MS + 1.0))),
            Some((
                CloudCapacityKind::SvipActive,
                "黑胶SVIP尊享6TB超大空间".into(),
            ))
        );
        assert_eq!(
            rendered(&config, Some(member(7.0 * DAY_MS))),
            Some((
                CloudCapacityKind::SvipExpiring,
                "黑胶SVIP6TB超大空间即将到期".into(),
            ))
        );
        assert!(rendered(&config, Some(member(-1.0))).is_none());
        assert!(rendered(&config, Some(member(0.0))).is_none());
        let wall_skew = MemberTime {
            wall_ms: CLOCK - 2.0,
            ..member(-1.0)
        };
        assert_eq!(
            rendered(&config, Some(wall_skew)).unwrap().0,
            CloudCapacityKind::SvipOffer
        );
        assert_eq!(
            rendered(&config, Some(member(-CLOCK))).unwrap().0,
            CloudCapacityKind::SvipOffer
        );
        assert!(rendered(&config, None).is_none());
    }

    #[test]
    fn full_only_uses_known_capacity_and_never_guesses_membership() {
        let config = Config::from_data(None);
        let full = |member| status(&config, Some(10), Some(10), true, member).unwrap();
        assert_eq!(full(None).text, FULL);
        assert_eq!(full(Some(member(DAY_MS))).text, FULL);
        assert_eq!(full(Some(member(-1.0))).text, FULL_WITH_OPTIONS);
        assert_eq!(
            full(Some(MemberTime {
                expiry: f64::NAN,
                ..member(-1.0)
            }))
            .text,
            FULL
        );
        assert!(status(&config, Some(10), Some(10), false, None).is_none());
        assert!(status(&config, None, Some(10), true, None).is_none());
        assert!(status(&config, Some(10), Some(0), true, None).is_none());
    }

    #[test]
    fn accepted_config_is_a_whole_key_and_bad_copy_never_becomes_a_tier_claim() {
        let classic = Config::from_data(Some(&json!({CONFIG_KEY:{"useNewFeat":false}})));
        assert_eq!(rendered(&classic, None).unwrap().1, PERMANENT);
        let custom = Config::from_data(Some(&json!({CONFIG_KEY: {
            "useNewFeat":true, "svipSimpleText":"合成容量提示", "svipRemainText":"a\nb"
        }})));
        assert_eq!(
            rendered(&custom, Some(member(8.0 * DAY_MS))).unwrap().1,
            "合成容量提示"
        );
        assert!(rendered(&custom, Some(member(7.0 * DAY_MS))).is_none());
        assert!(rendered(&custom, Some(member(-CLOCK))).is_none());
        let malformed = Config::from_data(Some(&json!({CONFIG_KEY:{"useNewFeat":"true"}})));
        assert!(rendered(&malformed, Some(member(8.0 * DAY_MS))).is_none());
    }
}
