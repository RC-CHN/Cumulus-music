//! M/247 and M/128: the first traditional Daily exposure from startup.
use crate::protocol::pc_experiments::PcExperimentGroups;
use chrono::{DateTime, Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

pub(super) const MAX_DAYS: usize = 366;
const DAY_MS: u64 = 86_400_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Policy {
    pub delay: Duration,
    pub days: u16,
    pub count: usize,
    pub descriptions: Vec<String>,
    pub daily_v3: bool,
}

impl Policy {
    pub fn read(groups: &PcExperimentGroups, config: &Value) -> Option<Self> {
        let group = groups.group("PH-PC-push")?;
        // Copyright has first refusal in its treatment cohort, while visual
        // treatment uses a different native window. Neither is this lane.
        if !matches!(group, "t1" | "t2" | "t3")
            || !matches!(groups.group("PH-PC-push2"), Some("" | "c"))
            || !matches!(groups.group("PH-PC-Pop-Visual"), Some("" | "c"))
        {
            return None;
        }
        let push = config.get("preload#push")?;
        let minutes = push[group]
            .as_f64()
            .filter(|n| (0.0..=1440.0).contains(n) && *n > 0.0)?;
        let delay = Duration::try_from_secs_f64(minutes * 60.0).ok()?;
        if delay.is_zero() {
            return None;
        }
        let days = push["day"]
            .as_u64()
            .filter(|n| (1..=MAX_DAYS as u64).contains(n))?;
        let count = push["count"]
            .as_u64()
            .filter(|n| (1..=MAX_DAYS as u64).contains(n))?;
        let descriptions = push["desc"]
            .as_array()
            .filter(|items| items.len() <= 32)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|text| text.len() <= 512)
            .map(str::to_owned)
            .collect();
        Some(Self {
            delay,
            days: days as u16,
            count: count as usize,
            descriptions,
            daily_v3: groups.group("PH-PC-Blacklist") == Some("t1"),
        })
    }

    pub fn first_exposure(&self, history: &History, local_ms: i64, server_ms: i64) -> bool {
        if !history.enabled || history.shown.len() > MAX_DAYS + 1 {
            return false;
        }
        let Some((local_date, server_date)) = local_date(local_ms).zip(local_date(server_ms))
        else {
            return false;
        };
        // M/128 first checks the machine's current day, then the synchronized
        // date for the per-type frequency. This lane never displays a second
        // exposure that would depend on the copyright/duration experiments.
        if history
            .shown
            .iter()
            .any(|t| self::local_date(*t).is_none_or(|day| day == local_date || day == server_date))
        {
            return false;
        }
        // In M/128 the empty-exposure shortcut precedes the snooze check. A
        // valid local snooze always follows a recorded presentation.
        if history.shown.is_empty() {
            return true;
        }
        if history
            .snoozed_at
            .is_some_and(|at| day_distance(at, server_ms).is_none_or(|days| days < 7))
        {
            return false;
        }
        history
            .shown
            .iter()
            .rev()
            .take_while(|at| {
                day_distance(**at, server_ms).is_some_and(|days| days < u64::from(self.days))
            })
            .count()
            < self.count
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct History {
    pub enabled: bool,
    pub snoozed_at: Option<i64>,
    pub shown: Vec<i64>,
}
impl Default for History {
    fn default() -> Self {
        Self {
            enabled: true, // M/116:372
            snoozed_at: None,
            shown: vec![],
        }
    }
}

fn local_date(ms: i64) -> Option<NaiveDate> {
    Some(
        DateTime::from_timestamp_millis(ms)?
            .with_timezone(&Local)
            .date_naive(),
    )
}

fn midnight(ms: i64) -> Option<i64> {
    let date = local_date(ms)?.and_hms_opt(0, 0, 0)?;
    Some(
        Local
            .from_local_datetime(&date)
            .earliest()?
            .timestamp_millis(),
    )
}

fn day_distance(left: i64, right: i64) -> Option<u64> {
    // Recovered JS takes ceil(abs(local midnight difference) / 24h).
    Some(midnight(left)?.abs_diff(midnight(right)?).div_ceil(DAY_MS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn groups(push: &str, priority: &str, visual: &str) -> PcExperimentGroups {
        PcExperimentGroups::refreshed(&json!({"data":[
            {"expName":"PH-PC-push","expGroupName":push},
            {"expName":"PH-PC-push2","expGroupName":priority},
            {"expName":"PH-PC-Pop-Visual","expGroupName":visual},
            {"expName":"PH-PC-Blacklist","expGroupName":"t1"}
        ]}))
        .unwrap()
    }

    #[test]
    fn never_arms_default_control_immersive_or_copyright_first_cohorts() {
        let cfg = json!({"preload#push":{"day":3,"count":1,"t1":3.5,"t2":1,"desc":["合成"]}});
        let p = Policy::read(&groups("t1", "c", "c"), &cfg).unwrap();
        assert_eq!(p.delay, Duration::from_secs(210));
        assert_eq!(p.descriptions, ["合成"]);
        assert!(p.daily_v3);
        for (push, priority, visual) in [
            ("c", "c", "c"),
            ("t1", "t1", "c"),
            ("t1", "c", "t1"),
            ("t3", "c", "c"),
        ] {
            assert!(Policy::read(&groups(push, priority, visual), &cfg).is_none());
        }
        assert!(
            Policy::read(
                &groups("t1", "c", "c"),
                &json!({"preload#push":{"day":0,"count":0,"t1":0}})
            )
            .is_none()
        );
    }

    fn at_date(day: u32) -> i64 {
        Local
            .with_ymd_and_hms(2026, 2, day, 13, 0, 0)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn separates_first_daily_exposure_frequency_and_seven_day_snooze() {
        let p = Policy::read(
            &groups("t1", "c", "c"),
            &json!({"preload#push":{"day":3,"count":1,"t1":1}}),
        )
        .unwrap();
        let mut h = History::default();
        assert!(p.first_exposure(&h, at_date(2), at_date(2)));
        h.shown.push(at_date(2));
        for (local, server) in [(2, 3), (3, 2), (3, 3), (4, 4)] {
            assert!(!p.first_exposure(&h, at_date(local), at_date(server)));
        }
        assert!(p.first_exposure(&h, at_date(5), at_date(5)));
        h.snoozed_at = Some(at_date(2));
        assert!(!p.first_exposure(&h, at_date(8), at_date(8)));
        assert!(p.first_exposure(&h, at_date(9), at_date(9)));
        h.enabled = false;
        assert!(!p.first_exposure(&h, at_date(10), at_date(10)));
    }
}
