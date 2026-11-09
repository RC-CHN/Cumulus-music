#[cfg(windows)]
use std::{marker::PhantomData, rc::Rc};
#[cfg(windows)]
use windows::Win32::{
    Media::Audio::{
        ERole, Endpoints::IAudioEndpointVolume, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
        eMultimedia, eRender,
    },
    System::Com::{
        CLSCTX_ALL, COINIT, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
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
pub(super) struct Apartment(PhantomData<Rc<()>>);

#[cfg(windows)]
impl Apartment {
    pub(super) fn initialize(model: COINIT) -> Result<Self, (ReadStage, Option<u32>)> {
        // SAFETY: on the blocking worker; both S_OK and S_FALSE are balanced
        // on the same thread, and failure never creates a lease.
        unsafe { CoInitializeEx(None, model) }
            .ok()
            .map_err(|err| (ReadStage::InitializeCom, Some(err.code().0 as u32)))?;
        Ok(Self(PhantomData))
    }
}

#[cfg(windows)]
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: COM objects are dropped before this guard, on the thread
        // that initialized COM.
        unsafe { CoUninitialize() };
    }
}

#[cfg(windows)]
pub(super) fn default_render_endpoint(
    _apartment: &Apartment,
    role: ERole,
) -> Result<IMMDevice, (ReadStage, Option<u32>)> {
    // SAFETY: the calling apartment is initialized; no aggregation or
    // renderer-supplied device identifier is involved.
    let devices: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|err| (ReadStage::CreateEnumerator, Some(err.code().0 as u32)))?;
    // SAFETY: the interface owns its returned references. The caller holds
    // this thread's initialized COM apartment until those references drop.
    unsafe { devices.GetDefaultAudioEndpoint(eRender, role) }
        .map_err(|err| (ReadStage::FindDefaultEndpoint, Some(err.code().0 as u32)))
}

#[cfg(windows)]
pub(super) fn read() -> Result<(f32, bool), (ReadStage, Option<u32>)> {
    let apartment = Apartment::initialize(COINIT_MULTITHREADED)?;
    let device = default_render_endpoint(&apartment, eMultimedia)?;
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
