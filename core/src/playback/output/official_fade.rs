//! Recovered Windows DirectSound control ramp. This computes logical volume
//! commands; it does not open an output, change mixer state, or process PCM.
//! Current Cumulus output backends do not consume this DirectSound policy.

pub const TICK_MILLISECONDS: u32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FadeTransition {
    Resume,
    SeekIn,
    ManualTrackIn,
    AutomaticTrackIn,
    Pause,
    SeekOut,
    Stop,
    ManualTrackOut,
    AutomaticTrackOut,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FadeDurations([u32; 9]);

impl Default for FadeDurations {
    fn default() -> Self {
        // cloudmusic.dll 1B4820 / 1B4870, in the order of 1B4B60.
        Self([600, 0, 1000, 600, 600, 0, 600, 600, 600])
    }
}

impl FadeDurations {
    /// The native override ignores zero and negative values. The bridge
    /// normally sends only the switch; per-transition keys are optional.
    pub fn apply_override(&mut self, transition: FadeTransition, milliseconds: i32) {
        if milliseconds > 0 {
            self.0[transition as usize] = milliseconds as u32;
        }
    }

    /// The native factory only constructs a transition when it is enabled,
    /// the selected backend supports it, and its configured value exceeds 20.
    /// The two runtime gates belong to the caller, not the duration table.
    pub fn effective_duration(&self, transition: FadeTransition) -> Option<u32> {
        let value = self.0[transition as usize];
        (value > TICK_MILLISECONDS).then_some(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FadeTick {
    pub logical_volume: f64,
    pub terminal: bool,
}

/// Iterator starts with the immediate tick, then yields one value per native
/// scheduled step. It leaves actual clock and output ownership to its caller.
#[derive(Clone, Debug)]
pub struct FadeRamp {
    from: f64,
    to: f64,
    delta: f64,
    steps: u32,
    index: u32,
}

impl FadeRamp {
    pub fn new(from: f64, to: f64, milliseconds: u32) -> Option<Self> {
        if !from.is_finite()
            || !to.is_finite()
            || from < 0.0
            || to < 0.0
            || (from - to).abs() < 0.01
            || !(TICK_MILLISECONDS..=(i32::MAX as u32 - 19)).contains(&milliseconds)
        {
            return None;
        }
        // 1B4570 uses signed (milliseconds + 19) / 20. Reject inputs
        // that would overflow that addition in the recovered ABI.
        let steps = milliseconds.div_ceil(TICK_MILLISECONDS);
        Some(Self {
            from,
            to,
            delta: (to - from) / f64::from(steps),
            steps,
            index: 0,
        })
    }

    /// The native force-completion entry (1B47E0) snaps to the final value
    /// exactly once. A later scheduler wake must not emit another command.
    pub fn finish(&mut self) -> Option<FadeTick> {
        if self.index > self.steps {
            return None;
        }
        self.index = self.steps;
        self.next()
    }
}

impl Iterator for FadeRamp {
    type Item = FadeTick;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index > self.steps {
            return None;
        }
        let terminal = self.index == self.steps;
        let logical_volume = if terminal {
            self.to
        } else {
            // Preserve two f64 operations; the native callback is not fused.
            self.from + f64::from(self.index) * self.delta
        };
        self.index += 1;
        Some(FadeTick {
            logical_volume,
            terminal,
        })
    }
}
