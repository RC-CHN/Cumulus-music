use super::windows::{Apartment, default_render_endpoint};
use windows::Win32::{
    Media::Audio::{ISpatialAudioClient, eConsole},
    System::Com::{CLSCTX_ALL, COINIT_APARTMENTTHREADED},
};

pub(super) fn read() -> Option<bool> {
    let apartment = Apartment::initialize(COINIT_APARTMENTTHREADED).ok()?;
    // The recovered spatial query uses the console render endpoint, not the
    // multimedia endpoint used by the system master-volume diagnostic.
    let device = default_render_endpoint(&apartment, eConsole).ok()?;
    // SAFETY: the documented interface requires no activation parameters.
    // It is dropped on this same apartment before the guard is released.
    let spatial: ISpatialAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }.ok()?;
    // SAFETY: the interface is live and the typed binding owns the output
    // slot. This queries capacity without activating an audio stream.
    unsafe { spatial.GetMaxDynamicObjectCount() }
        .ok()
        .map(|objects| objects > 0)
}
