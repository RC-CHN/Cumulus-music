use std::{marker::PhantomData, rc::Rc};
use windows::Win32::{
    Media::Audio::{
        Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia,
        eRender,
    },
    System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    },
};

struct Apartment(PhantomData<Rc<()>>);

impl Apartment {
    fn initialize() -> Option<Self> {
        // SAFETY: on the blocking worker; both S_OK and S_FALSE are balanced
        // on the same thread, and failure never creates a lease.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .ok()?;
        Some(Self(PhantomData))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: this guard is used only in read(), after its COM objects
        // have been dropped on the thread that initialized COM.
        unsafe { CoUninitialize() };
    }
}

pub(super) fn read() -> Option<(f32, bool)> {
    let _apartment = Apartment::initialize()?;
    // SAFETY: the calling apartment is initialized; no aggregation or
    // renderer-supplied device identifier is involved.
    let devices: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.ok()?;
    // SAFETY: this is the recovered default rendering role, not the app's
    // configured output device, and the interface owns its returned references.
    let device = unsafe { devices.GetDefaultAudioEndpoint(eRender, eMultimedia) }.ok()?;
    // SAFETY: IAudioEndpointVolume is the documented interface for this
    // activation; it does not require activation parameters.
    let endpoint: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None) }.ok()?;
    // SAFETY: the interface is live. The generated bindings own the out slots.
    let scalar = unsafe { endpoint.GetMasterVolumeLevelScalar() }.ok()?;
    // SAFETY: same interface and apartment; this reads the mute flag only.
    let muted = unsafe { endpoint.GetMute() }.ok()?.as_bool();
    Some((scalar, muted))
}
