//! Offline reference for the ordinary MAudio/WASAPI submitted-byte clock.
//! This models the counted branch of cloudmusic 3.1.42.205594 `10917F0`.
//! It has no device, source, registration, file or player side effects.
//!
//! Run independently: `rustc --edition 2024 --test scripts/research/wasapi_submitted_clock.rs -o /tmp/wasapi-clock-test`
//! Then `/tmp/wasapi-clock-test`. See docs/core-effects-modern-limiter-host-clock.txt.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockError {
    Geometry,
    ClockRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Submitted {
    pub weighted_bytes: u64,
    pub device_us: u64,
}

/// Owns the software counter, not a hardware presentation cursor. The caller
/// must supply actual post-effect/post-tempo bytes from a successful native
/// counted write, and the speed argument that accompanied that write.
#[derive(Debug)]
pub struct WasapiSubmittedClock {
    frame_bytes: u16,
    sample_rate: u32,
    submitted: Submitted,
}

impl WasapiSubmittedClock {
    pub fn new(sample_rate: u32, frame_bytes: u16) -> Result<Self, ClockError> {
        if sample_rate == 0 || frame_bytes == 0 {
            return Err(ClockError::Geometry);
        }
        Ok(Self {
            frame_bytes,
            sample_rate,
            submitted: Submitted {
                weighted_bytes: 0,
                device_us: 0,
            },
        })
    }

    /// 10920E0–1092192. Each write truncates weighted *bytes*, and only the
    /// accumulated byte count is divided by the negotiated frame alignment.
    pub fn counted_write(
        &mut self,
        actual_output_bytes: u32,
        speed: f64,
    ) -> Result<Submitted, ClockError> {
        if actual_output_bytes == 0
            || actual_output_bytes > i32::MAX as u32
            || !actual_output_bytes.is_multiple_of(u32::from(self.frame_bytes))
        {
            return Err(ClockError::Geometry);
        }
        let weighted = f64::from(actual_output_bytes) * speed;
        if !speed.is_finite() || speed <= 0.0 || weighted >= 2.0_f64.powi(64) {
            return Err(ClockError::ClockRange);
        }
        let weighted_bytes = self
            .submitted
            .weighted_bytes
            .checked_add(weighted.trunc() as u64)
            .ok_or(ClockError::ClockRange)?;
        let frames = weighted_bytes / u64::from(self.frame_bytes);
        let device_us = frames
            .checked_mul(1_000_000)
            .ok_or(ClockError::ClockRange)?
            / u64::from(self.sample_rate);
        self.submitted = Submitted {
            weighted_bytes,
            device_us,
        };
        Ok(self.submitted)
    }

    /// WASAPI +98 (`1092520`) clears +1C0 in states 1 and 2. It leaves the
    /// previous +1B8 cache intact until another counted write. That stale
    /// cache is a native observation, not a new generation receipt.
    pub fn reset_seek_counter(&mut self, device_state: u32) {
        if matches!(device_state, 1 | 2) {
            self.submitted.weighted_bytes = 0;
        }
    }

    /// WASAPI +90 (`108F400`) returns zero for native state 3, otherwise
    /// the last device time cached by the counted branch.
    pub fn reported_device_us(&self, device_state: u32) -> u64 {
        if device_state == 3 {
            0
        } else {
            self.submitted.device_us
        }
    }
}

/// Source base used in the ordinary seek path (1017F0C / 10B00FF), for
/// finite nonnegative application positions. This does not seek a decoder.
pub fn seek_base_ms(position_seconds: f64, clip_origin_seconds: f64) -> Result<u64, ClockError> {
    if !position_seconds.is_finite()
        || !clip_origin_seconds.is_finite()
        || position_seconds < 0.0
        || clip_origin_seconds < 0.0
    {
        return Err(ClockError::ClockRange);
    }
    let ms = (position_seconds + clip_origin_seconds) * 1_000.0;
    if ms >= 2.0_f64.powi(63) {
        return Err(ClockError::ClockRange);
    }
    Ok(ms.trunc() as u64)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progress {
    pub absolute_ms: u64,
    pub relative_seconds: f64,
}

/// `1014BA0` composes integer milliseconds; `1017330` converts, subtracts
/// clip origin, and lower clamps the progress event's current position.
pub fn ordinary_progress(
    source_base_ms: u64,
    reported_device_us: u64,
    clip_origin_seconds: f64,
) -> Result<Progress, ClockError> {
    let absolute_ms = source_base_ms
        .checked_add(reported_device_us / 1_000)
        .filter(|ms| *ms <= i64::MAX as u64)
        .ok_or(ClockError::ClockRange)?;
    if !clip_origin_seconds.is_finite() || clip_origin_seconds < 0.0 {
        return Err(ClockError::ClockRange);
    }
    let relative_seconds = (absolute_ms as f64 / 1_000.0 - clip_origin_seconds).max(0.0);
    Ok(Progress {
        absolute_ms,
        relative_seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Frozen from executing original cloudmusic x64 excerpts in a sealed
    // machine. The four input counts came from the original SDK graph;
    // they are not claims about actual device period scheduling.
    #[test]
    fn original_counted_outputs_and_relative_progress() {
        let cases = [
            (
                44_100,
                [1829, 2048, 2048, 226],
                [41473, 87913, 134353, 139478],
                3514,
                3.139,
            ),
            (
                44_100,
                [1829, 1829, 1829, 226],
                [41473, 82947, 124421, 129546],
                3504,
                3.129,
            ),
            (
                48_000,
                [1810, 2048, 2048, 245],
                [37708, 80375, 123041, 128145],
                3503,
                3.128,
            ),
            (
                48_000,
                [1810, 1810, 1810, 245],
                [37708, 75416, 113125, 118229],
                3493,
                3.118,
            ),
        ];
        for (rate, blocks, clock_us, expected_ms, expected_seconds) in cases {
            let mut clock = WasapiSubmittedClock::new(rate, 8).unwrap();
            for (frames, us) in blocks.into_iter().zip(clock_us) {
                assert_eq!(clock.counted_write(frames * 8, 1.0).unwrap().device_us, us);
            }
            let source = seek_base_ms(3.0, 0.375).unwrap();
            assert_eq!(source, 3375);
            assert_eq!(
                ordinary_progress(source, clock.reported_device_us(1), 0.375).unwrap(),
                Progress {
                    absolute_ms: expected_ms,
                    relative_seconds: expected_seconds,
                }
            );
        }
    }

    #[test]
    fn seek_counter_resets_without_manufacturing_a_new_observation() {
        let mut clock = WasapiSubmittedClock::new(44_100, 8).unwrap();
        assert_eq!(clock.counted_write(1829 * 8, 1.0).unwrap().device_us, 41473);
        assert_eq!(clock.reported_device_us(3), 0);
        clock.reset_seek_counter(2);
        assert_eq!(clock.reported_device_us(2), 41473);
        assert_eq!(
            clock.counted_write(13 * 8, 1.25).unwrap(),
            Submitted {
                weighted_bytes: 130,
                device_us: 362,
            }
        );
        assert_eq!(
            ordinary_progress(3375, 362, 0.375)
                .unwrap()
                .relative_seconds,
            3.0
        );
        assert_eq!(
            ordinary_progress(0, 0, 0.375).unwrap().relative_seconds,
            0.0
        );
    }

    #[test]
    fn counted_writes_round_bytes_per_write_and_reject_invalid_input_atomically() {
        let mut clock = WasapiSubmittedClock::new(44_100, 4).unwrap();
        for _ in 0..8 {
            clock.counted_write(4, 1.125).unwrap();
        }
        assert_eq!(clock.reported_device_us(1), 181);
        let mut combined = WasapiSubmittedClock::new(44_100, 4).unwrap();
        assert_eq!(
            combined.counted_write(32, 1.125).unwrap(),
            Submitted {
                weighted_bytes: 36,
                device_us: 204,
            }
        );
        assert_eq!(clock.counted_write(3, 1.0), Err(ClockError::Geometry));
        assert_eq!(
            clock.counted_write(4, f64::NAN),
            Err(ClockError::ClockRange)
        );
        assert_eq!(clock.reported_device_us(1), 181);
        assert_eq!(
            ordinary_progress(u64::MAX, 1_000, 0.0),
            Err(ClockError::ClockRange)
        );
    }
}
