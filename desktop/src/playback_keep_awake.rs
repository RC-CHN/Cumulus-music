//! Explicit Windows adapter for the recovered playback power requests.
//! No startup hook, command, or setting uses this adapter.
#[cfg(any(test, windows))]
use cumulus_core::playback::keep_awake::PlaybackPowerRequest;

#[cfg(any(test, windows))]
const fn execution_flags(request: PlaybackPowerRequest) -> u32 {
    // Exact flags recovered from the Windows x64 handler. The host never
    // generates away-mode or the partial native RPC request combinations.
    match request {
        PlaybackPowerRequest::Clear => 0x8000_0000,
        PlaybackPowerRequest::KeepSystemAndDisplayAwake => 0x8000_0003,
    }
}

#[cfg(windows)]
#[derive(Default)]
pub struct WindowsPlaybackKeepAwake {
    holding: bool,
    // SetThreadExecutionState is thread-specific. A future host must own
    // this adapter on one dedicated thread and must not move it to a worker.
    thread_affine: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(windows)]
impl WindowsPlaybackKeepAwake {
    /// Apply the request once, on the owning thread. Construction is inert.
    pub fn apply(&mut self, request: PlaybackPowerRequest) -> std::io::Result<()> {
        use windows_sys::Win32::System::Power::SetThreadExecutionState;

        // SAFETY: the flags are a supported mask with ES_CONTINUOUS, the
        // function takes no pointers, and the non-Send guard is thread-affine.
        let previous = unsafe { SetThreadExecutionState(execution_flags(request)) };
        if previous == 0 {
            return Err(std::io::Error::last_os_error());
        }
        self.holding = request == PlaybackPowerRequest::KeepSystemAndDisplayAwake;
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for WindowsPlaybackKeepAwake {
    fn drop(&mut self) {
        if self.holding {
            // Best-effort host cleanup, not an extra playback-state retry.
            let _ = self.apply(PlaybackPowerRequest::Clear);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_requests_match_the_recovered_windows_masks() {
        assert_eq!(execution_flags(PlaybackPowerRequest::Clear), 0x8000_0000);
        assert_eq!(
            execution_flags(PlaybackPowerRequest::KeepSystemAndDisplayAwake),
            0x8000_0003
        );
    }
}
