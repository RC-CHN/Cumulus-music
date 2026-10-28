use windows::{
    Media::SystemMediaTransportControls,
    Win32::{
        Foundation::HWND,
        System::WinRT::{
            ISystemMediaTransportControlsInterop, RO_INIT_MULTITHREADED, RoInitialize,
            RoUninitialize,
        },
    },
    core::{Result, factory},
};
// Kept on the dedicated worker; cannot be moved to another apartment/thread.
pub(super) struct Apartment(std::marker::PhantomData<std::rc::Rc<()>>);
impl Apartment {
    pub(super) fn open() -> Result<Self> {
        // SAFETY: called on a new dedicated native worker, with balanced teardown.
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        Ok(Self(std::marker::PhantomData))
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: all controls and callbacks are released before this same-thread guard.
        unsafe {
            RoUninitialize();
        }
    }
}
pub(super) fn open(hwnd: usize) -> Result<SystemMediaTransportControls> {
    let interop: ISystemMediaTransportControlsInterop =
        factory::<SystemMediaTransportControls, _>()?;
    // SAFETY: the handle comes from our own live top-level Tauri window. A close
    // race is an OS error; no raw window memory is dereferenced here.
    let controls: SystemMediaTransportControls =
        unsafe { interop.GetForWindow(HWND(hwnd as *mut _))? };
    controls.SetIsEnabled(false)?;
    Ok(controls)
}
