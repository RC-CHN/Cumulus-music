//! M/128: fixed half-hour cadence following the delayed startup sweep.
use chrono::{DateTime, Local, TimeDelta, TimeZone, Timelike, Utc};
use std::time::Duration;
use tokio::time::{self, Instant, Interval, MissedTickBehavior};

const PERIOD: Duration = Duration::from_secs(30 * 60);

fn until_boundary<Tz: TimeZone>(now: DateTime<Tz>) -> Option<Duration> {
    let is_first_half = now.minute() < 30;
    let mut target =
        now.date_naive()
            .and_hms_opt(now.hour(), if is_first_half { 30 } else { 0 }, 0)?;
    if !is_first_half {
        target = target.checked_add_signed(TimeDelta::hours(1))?;
    }
    // JavaScript picks the earlier occurrence of a duplicated wall time.
    // Its elapsed delay can be negative on the second fold; setTimeout then
    // queues the callback immediately. Normalize nonexistent boundaries to
    // the next valid half-hour (including the ordinary one-hour DST gap).
    for _ in 0..=48 {
        if let Some(boundary) = now.timezone().from_local_datetime(&target).earliest() {
            return Some(
                boundary
                    .signed_duration_since(now)
                    .to_std()
                    .unwrap_or_default(),
            );
        }
        target = target.checked_add_signed(TimeDelta::minutes(30))?;
    }
    None
}

fn jitter(sample: f64) -> Duration {
    debug_assert!((0.0..1.0).contains(&sample));
    Duration::from_millis((sample * 10.0 * 60.0 * 1000.0).floor() as u64)
}

/// One native scheduler after a completed, positively configured startup
/// check. There is no renderer timer command or synthetic system-resume hook.
pub struct DailyNotificationTimer(Interval);

impl DailyNotificationTimer {
    pub fn start() -> Option<Self> {
        until_boundary(Utc::now().with_timezone(&Local)).map(Self::aligned)
    }

    fn aligned(delay: Duration) -> Self {
        let mut interval = time::interval_at(Instant::now() + delay, PERIOD);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Self(interval)
    }

    pub async fn tick(&mut self) {
        self.tick_with(|| jitter(rand::random::<f64>())).await;
    }

    async fn tick_with(&mut self, mut sample: impl FnMut() -> Duration) {
        loop {
            let anchor = self.0.tick().await;
            if anchor.elapsed() >= PERIOD {
                continue;
            }
            let delay = sample();
            if !delay.is_zero() {
                time::sleep(delay).await;
            }
            if anchor.elapsed() < PERIOD {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, LocalResult, NaiveDate, NaiveDateTime, Offset};

    #[test]
    fn normal_local_boundaries_and_exact_jitter_bounds() {
        let tz = FixedOffset::east_opt(8 * 3600).unwrap();
        let at = |h, m, s, ms| {
            tz.from_local_datetime(
                &NaiveDate::from_ymd_opt(2025, 11, 1)
                    .unwrap()
                    .and_hms_milli_opt(h, m, s, ms)
                    .unwrap(),
            )
            .unwrap()
        };
        for (wall, ms) in [
            (at(9, 0, 0, 0), 1_800_000),
            (at(9, 29, 59, 999), 1),
            (at(9, 30, 0, 0), 1_800_000),
            (at(23, 59, 59, 999), 1),
        ] {
            assert_eq!(until_boundary(wall), Some(Duration::from_millis(ms)));
        }
        for (sample, ms) in [
            (0.0, 0),
            (0.099999, 59_999),
            (0.1, 60_000),
            (1.0 - f64::EPSILON / 2.0, 599_999),
        ] {
            assert_eq!(jitter(sample), Duration::from_millis(ms));
        }
    }

    // A sealed 2025 transition table, independent of global TZ/environment and
    // the host zone database. Expected delays were compared with Date setters
    // under America/New_York, not derived from this scheduler.
    #[derive(Clone)]
    struct NewYork2025;

    impl TimeZone for NewYork2025 {
        type Offset = FixedOffset;

        fn from_offset(_: &Self::Offset) -> Self {
            Self
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> LocalResult<Self::Offset> {
            self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<Self::Offset> {
            let offsets = [-4, -5].map(|h| FixedOffset::east_opt(h * 3600).unwrap());
            let possible = offsets.map(|offset| {
                let utc = *local - TimeDelta::seconds(offset.fix().local_minus_utc().into());
                (self.offset_from_utc_datetime(&utc) == offset).then_some(offset)
            });
            match possible {
                [Some(first), Some(second)] => LocalResult::Ambiguous(first, second),
                [Some(offset), None] | [None, Some(offset)] => LocalResult::Single(offset),
                [None, None] => LocalResult::None,
            }
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> Self::Offset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
            let spring = NaiveDate::from_ymd_opt(2025, 3, 9)
                .unwrap()
                .and_hms_opt(7, 0, 0)
                .unwrap();
            let fall = NaiveDate::from_ymd_opt(2025, 11, 2)
                .unwrap()
                .and_hms_opt(6, 0, 0)
                .unwrap();
            FixedOffset::east_opt(
                if (*utc >= spring) && (*utc < fall) {
                    -4
                } else {
                    -5
                } * 3600,
            )
            .unwrap()
        }
    }

    #[test]
    fn daylight_saving_fold_and_gap_match_the_js_oracle() {
        for (iso, ms) in [
            ("2025-11-02T01:10:00-05:00", 0),
            ("2025-11-02T01:45:00-04:00", 4_500_000),
            ("2025-03-09T01:45:00-05:00", 900_000),
        ] {
            let now = DateTime::parse_from_rfc3339(iso)
                .unwrap()
                .with_timezone(&NewYork2025);
            assert_eq!(until_boundary(now), Some(Duration::from_millis(ms)));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn independent_jitter_preserves_first_anchor_and_does_not_replay_missed_ticks() {
        let start = Instant::now();
        let delay = Duration::from_secs(1);
        let mut timer = DailyNotificationTimer::aligned(delay);
        timer.tick_with(|| Duration::from_millis(500)).await;
        assert_eq!(start.elapsed(), delay + Duration::from_millis(500));
        timer.tick_with(|| Duration::from_secs(2)).await;
        assert_eq!(start.elapsed(), delay + PERIOD + Duration::from_secs(2));

        time::advance(2 * PERIOD).await;
        let mut draws = 0;
        timer
            .tick_with(|| {
                draws += 1;
                Duration::ZERO
            })
            .await;
        assert_eq!(draws, 1);
        assert_eq!(start.elapsed(), delay + 4 * PERIOD);
    }
}
