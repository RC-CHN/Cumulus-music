#[cfg(windows)]
use std::{marker::PhantomData, rc::Rc};
#[cfg(windows)]
use windows::Win32::{
    Media::Audio::{
        Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia,
        eRender,
    },
    System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReadStage {
    InitializeCom,
    CreateEnumerator,
    FindDefaultEndpoint,
    ActivateVolume,
    GetScalar,
    InvalidScalar,
    GetMute,
}

#[cfg(windows)]
struct Apartment(PhantomData<Rc<()>>);

#[cfg(windows)]
impl Apartment {
    fn initialize() -> Result<Self, (ReadStage, Option<u32>)> {
        // SAFETY: on the blocking worker; both S_OK and S_FALSE are balanced
        // on the same thread, and failure never creates a lease.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|err| (ReadStage::InitializeCom, Some(err.code().0 as u32)))?;
        Ok(Self(PhantomData))
    }
}

#[cfg(windows)]
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: this guard is used only in read(), after its COM objects
        // have been dropped on the thread that initialized COM.
        unsafe { CoUninitialize() };
    }
}

#[cfg(windows)]
pub(super) fn read() -> Result<(f32, bool), (ReadStage, Option<u32>)> {
    let _apartment = Apartment::initialize()?;
    // SAFETY: the calling apartment is initialized; no aggregation or
    // renderer-supplied device identifier is involved.
    let devices: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|err| (ReadStage::CreateEnumerator, Some(err.code().0 as u32)))?;
    // SAFETY: this is the recovered default rendering role, not the app's
    // configured output device, and the interface owns its returned references.
    let device = unsafe { devices.GetDefaultAudioEndpoint(eRender, eMultimedia) }
        .map_err(|err| (ReadStage::FindDefaultEndpoint, Some(err.code().0 as u32)))?;
    // SAFETY: IAudioEndpointVolume is the documented interface for this
    // activation; it does not require activation parameters.
    let endpoint: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None) }
        .map_err(|err| (ReadStage::ActivateVolume, Some(err.code().0 as u32)))?;
    // SAFETY: the interface is live. The generated bindings own the out slots.
    let scalar = unsafe { endpoint.GetMasterVolumeLevelScalar() }
        .map_err(|err| (ReadStage::GetScalar, Some(err.code().0 as u32)))?;
    // SAFETY: same interface and apartment; this reads the mute flag only.
    let muted = unsafe { endpoint.GetMute() }
        .map_err(|err| (ReadStage::GetMute, Some(err.code().0 as u32)))?
        .as_bool();
    if !(0.0..=1.0).contains(&scalar) {
        return Err((ReadStage::InvalidScalar, None));
    }
    Ok((scalar, muted))
}
