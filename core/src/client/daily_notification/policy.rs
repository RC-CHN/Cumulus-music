//! M/247 and M/128: shared startup and half-hour exposure policy.
use crate::protocol::pc_experiments::PcExperimentGroups;
use chrono::{DateTime, Local, NaiveDate, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

pub(super) const MAX_DAYS: usize = 366;
const DAY_MS: u64 = 86_400_000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Trigger {
    Startup,
    HalfHour,
}

#[derive(Clone, Copy)]
pub(super) struct Attempt {
    pub trigger: Trigger,
    pub local_ms: i64,
}

impl Attempt {
    pub fn still_today(self, current_local_ms: i64) -> bool {
        local_date(self.local_ms).is_some_and(|day| local_date(current_local_ms) == Some(day))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Policy {
    pub delay: Duration,
    pub days: u16,
    pub count: usize,
    pub descriptions: Vec<String>,
    pub daily_v3: bool,
    pub daily: bool,
    pub copyright: bool,
    first_hour: Option<u32>,
    second_hour: Option<u32>,
    duration_limit_ms: Option<u64>,
}

fn tested_value(groups: &PcExperimentGroups, name: &str, config: &Value) -> Option<f64> {
    let group = groups.group(name)?;
    matches!(group, "t1" | "t2" | "t3")
        .then(|| number_or_zero(&config[group]))
        .flatten()
}

fn number_or_zero(value: &Value) -> Option<f64> {
    // M/128 uses config value || 0; do not coerce unexpected types into
    // permission. A verified experiment with a missing value is still valid.
    if value.is_null() {
        Some(0.0)
    } else {
        value.as_f64()
    }
}

fn hour(value: f64) -> Option<u32> {
    ((0.0..=23.0).contains(&value) && value.fract() == 0.0).then_some(value as u32)
}

impl Policy {
    pub fn read(groups: &PcExperimentGroups, config: &Value) -> Option<Self> {
        let group = groups.group("PH-PC-push")?;
        let copyright = groups.group("PH-PC-push2") == Some("t1");
        let daily = matches!(groups.group("PH-PC-Pop-Visual"), Some("" | "c"));
        if !matches!(group, "t1" | "t2" | "t3")
            || !matches!(groups.group("PH-PC-push2"), Some("" | "c" | "t1"))
            || (!daily && !copyright)
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
            daily,
            copyright,
            first_hour: tested_value(groups, "PH-PC-push-firstTime", &push["firstTime"])
                .and_then(hour),
            second_hour: number_or_zero(&push["secondPushTime"]).and_then(hour),
            duration_limit_ms: tested_value(
                groups,
                "PH-PC-push-durationLimit",
                &push["durationLimit"],
            )
            .filter(|minutes| (0.0..=1440.0).contains(minutes))
            .map(|minutes| (minutes * 60_000.0) as u64),
        })
    }

    pub fn eligible(
        &self,
        history: &History,
        sibling: &History,
        attempt: Attempt,
        current_local_ms: i64,
        server_ms: i64,
    ) -> bool {
        if !history.enabled
            || history.shown.len() > MAX_DAYS + 1
            || sibling.shown.len() > MAX_DAYS + 1
        {
            return false;
        }
        let Some(local) =
            DateTime::from_timestamp_millis(attempt.local_ms).map(|dt| dt.with_timezone(&Local))
        else {
            return false;
        };
        let day = local.date_naive();
        let Some(server_date) = local_date(server_ms) else {
            return false;
        };
        // Keep the admitted wall minute through asynchronous reads and Shown,
        // but never carry that permission across the device's local midnight.
        if !attempt.still_today(current_local_ms) {
            return false;
        }
        let mut latest = None;
        for at in history.shown.iter().chain(&sibling.shown) {
            let Some(date) = local_date(*at) else {
                return false;
            };
            if date == day && latest.replace(*at).is_some() {
                return false;
            }
        }
        let shared = match latest {
            None => {
                attempt.trigger == Trigger::Startup
                    || (Some(local.hour()) == self.first_hour && local.minute() == 0)
            }
            Some(last) => {
                self.second_hour
                    .zip(self.duration_limit_ms)
                    .is_some_and(|(hour, duration)| {
                        local.hour() >= hour
                            && attempt.local_ms.checked_sub(last).is_some_and(|elapsed| {
                                u64::try_from(elapsed).is_ok_and(|elapsed| elapsed > duration)
                            })
                    })
            }
        };
        if !shared
            || history
                .shown
                .iter()
                .any(|at| local_date(*at).is_none_or(|date| date == server_date))
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
    fn arms_only_the_recovered_startup_candidates() {
        let cfg = json!({"preload#push":{"day":3,"count":1,"t1":3.5,"t2":1,"desc":["合成"]}});
        let p = Policy::read(&groups("t1", "c", "c"), &cfg).unwrap();
        assert_eq!(p.delay, Duration::from_secs(210));
        assert_eq!(p.descriptions, ["合成"]);
        assert!(p.daily_v3);
        assert!(p.daily && !p.copyright);
        let priority = Policy::read(&groups("t1", "t1", "c"), &cfg).unwrap();
        assert!(priority.daily && priority.copyright);
        let immersive = Policy::read(&groups("t1", "t1", "t1"), &cfg).unwrap();
        assert!(!immersive.daily && immersive.copyright);
        for (push, priority, visual) in [("c", "c", "c"), ("t1", "c", "t1"), ("t3", "c", "c")] {
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

    fn eligible(
        policy: &Policy,
        history: &History,
        sibling: &History,
        at: i64,
        server: i64,
    ) -> bool {
        policy.eligible(
            history,
            sibling,
            Attempt {
                trigger: Trigger::Startup,
                local_ms: at,
            },
            at,
            server,
        )
    }

    #[test]
    fn second_exposure_strictly_exceeds_delay_for_either_trigger() {
        let mut p = Policy::read(
            &groups("t1", "t1", "c"),
            &json!({"preload#push":{"day":3,"count":1,"t1":1}}),
        )
        .unwrap();
        assert_eq!(p.duration_limit_ms, None);
        p.first_hour = Some(9);
        p.second_hour = Some(17);
        p.duration_limit_ms = Some(1_800_000);
        let at = |h, m| {
            Local
                .with_ymd_and_hms(2026, 2, 2, h, m, 0)
                .unwrap()
                .timestamp_millis()
        };
        let empty = History::default();
        let sibling = History {
            shown: vec![at(16, 30)],
            ..History::default()
        };
        let check = |trigger, own: &History, other: &History, sampled, now| {
            p.eligible(
                own,
                other,
                Attempt {
                    trigger,
                    local_ms: sampled,
                },
                now,
                now,
            )
        };
        assert!(check(
            Trigger::HalfHour,
            &empty,
            &empty,
            at(9, 0) + 59_999,
            at(9, 1)
        ));
        assert!(!check(
            Trigger::HalfHour,
            &empty,
            &empty,
            at(9, 1),
            at(9, 1)
        ));
        assert!(!check(
            Trigger::HalfHour,
            &empty,
            &empty,
            at(9, 30),
            at(9, 30)
        ));
        assert!(!check(
            Trigger::HalfHour,
            &empty,
            &empty,
            at(9, 0),
            at(9, 0) + 86_400_000
        ));
        for trigger in [Trigger::Startup, Trigger::HalfHour] {
            for time in [at(16, 59) + 59_999, at(17, 0)] {
                assert!(!check(trigger, &empty, &sibling, time, time));
            }
            let later = at(17, 0) + 1;
            assert!(check(trigger, &empty, &sibling, later, later));
            assert!(!check(trigger, &sibling, &empty, later, later));
        }
        p.duration_limit_ms = None;
        let later = at(17, 0) + 1;
        assert!(!p.eligible(
            &empty,
            &sibling,
            Attempt {
                trigger: Trigger::Startup,
                local_ms: later
            },
            later,
            later
        ));
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
        assert!(eligible(
            &p,
            &h,
            &History::default(),
            at_date(2),
            at_date(2)
        ));
        h.shown.push(at_date(2));
        for (local, server) in [(2, 3), (3, 2), (3, 3), (4, 4)] {
            assert!(!eligible(
                &p,
                &h,
                &History::default(),
                at_date(local),
                at_date(server)
            ));
        }
        assert!(eligible(
            &p,
            &h,
            &History::default(),
            at_date(5),
            at_date(5)
        ));
        h.snoozed_at = Some(at_date(2));
        assert!(!eligible(
            &p,
            &h,
            &History::default(),
            at_date(8),
            at_date(8)
        ));
        assert!(eligible(
            &p,
            &h,
            &History::default(),
            at_date(9),
            at_date(9)
        ));
        h.enabled = false;
        assert!(!eligible(
            &p,
            &h,
            &History::default(),
            at_date(10),
            at_date(10)
        ));
    }
    #[test]
    fn shares_local_day_across_types_without_sharing_snooze_or_frequency() {
        let p = Policy::read(
            &groups("t1", "t1", "c"),
            &json!({"preload#push":{"day":3,"count":1,"t1":1}}),
        )
        .unwrap();
        let daily = History::default();
        let other = History {
            enabled: false,
            snoozed_at: Some(at_date(2)),
            shown: vec![at_date(2)],
        };
        assert!(!eligible(&p, &daily, &other, at_date(2), at_date(3)));
        assert!(eligible(&p, &daily, &other, at_date(3), at_date(2)));
        assert!(eligible(&p, &daily, &other, at_date(3), at_date(3)));
    }
}
