use std::{
    ffi::c_void,
    ptr::null_mut,
    slice,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Media::Audio::{
            AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_E_UNSUPPORTED_FORMAT, AUDCLNT_S_BUFFER_EMPTY,
            AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            AUDCLNT_STREAMFLAGS_LOOPBACK, DEVICE_STATE_ACTIVE, IAudioCaptureClient, IAudioClient,
            IMMDevice, IMMDeviceEnumerator, IMMEndpoint, MMDeviceEnumerator, WAVEFORMATEX,
            WAVEFORMATEXTENSIBLE, eRender,
        },
        System::{
            Com::{
                CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance,
                CoInitializeEx, CoTaskMemFree, CoUninitialize,
            },
            Threading::{CreateEventW, WaitForSingleObject},
        },
    },
    core::{Interface, PCWSTR},
};

use crate::policy::{
    Budget, Cancellation, Fault, FixtureEndpoint, Format, MAX_FRAMES_PER_PACKET, Packet,
    PacketInfo, SILENT, Stereo, negotiate, next_wait, read_packet, validate_request,
};

const _: () = {
    assert!(std::mem::size_of::<WAVEFORMATEX>() == 18);
    assert!(std::mem::size_of::<WAVEFORMATEXTENSIBLE>() == 40);
    assert!(AUDCLNT_STREAMFLAGS_EVENTCALLBACK == 0x40000);
    assert!(AUDCLNT_STREAMFLAGS_LOOPBACK == 0x20000);
};

fn native_error(operation: &'static str, error: windows::core::Error) -> Fault {
    if error.code() == AUDCLNT_E_DEVICE_INVALIDATED {
        Fault::DeviceChanged
    } else {
        Fault::Native(operation, error.code().0)
    }
}

struct Com;
impl Com {
    fn open() -> Result<Self, Fault> {
        // SAFETY: initialized and torn down on this same synchronous worker thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|err| native_error("CoInitializeEx", err))?;
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: one corresponding successful initialization, including S_FALSE.
        unsafe { CoUninitialize() }
    }
}

struct ComMemory(*const c_void);
impl Drop for ComMemory {
    fn drop(&mut self) {
        // SAFETY: pointer was returned by IMMDevice::GetId or IAudioClient::GetMixFormat.
        unsafe { CoTaskMemFree(Some(self.0)) }
    }
}

fn validate_endpoint(endpoint: &FixtureEndpoint, device: &IMMDevice) -> Result<(), Fault> {
    // SAFETY: COM apartment and interfaces outlive calls; owned ID is freed below.
    unsafe {
        let active = device
            .GetState()
            .map_err(|err| native_error("GetState", err))?
            == DEVICE_STATE_ACTIVE;
        let render = device
            .cast::<IMMEndpoint>()
            .map_err(|err| native_error("IMMEndpoint", err))?
            .GetDataFlow()
            .map_err(|err| native_error("GetDataFlow", err))?
            == eRender;
        let observed = device.GetId().map_err(|err| native_error("GetId", err))?;
        let _owned = ComMemory(observed.0.cast());
        if observed.is_null() || !active || !render {
            return Err(Fault::Endpoint);
        }
        let expected = endpoint.id();
        // Read one unit at a time; return at first mismatch or earlier NUL.
        // IMMDevice guarantees a terminated string on success.
        for (idx, unit) in expected.iter().copied().chain(Some(0)).enumerate() {
            if *observed.0.add(idx) != unit {
                return Err(Fault::Endpoint);
            }
        }
        endpoint.verify(
            slice::from_raw_parts(observed.0, expected.len()),
            active,
            render,
        )
    }
}

fn open_audio_client(device: &IMMDevice) -> Result<IAudioClient, Fault> {
    // SAFETY: live COM interface in its owning apartment.
    unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(|err| native_error("Activate", err))
}

fn open_stream(
    device: &IMMDevice,
    check: impl Fn() -> Result<Duration, Fault>,
) -> Result<(Format, IAudioClient), Fault> {
    check()?;
    let mix_client = open_audio_client(device)?;
    // SAFETY: COM owns the object; caller owns the CoTaskMem-allocated WAVE data.
    let mix =
        unsafe { mix_client.GetMixFormat() }.map_err(|err| native_error("GetMixFormat", err))?;
    let _owned = ComMemory(mix.cast());
    if mix.is_null() {
        return Err(Fault::Format);
    }
    // SAFETY: GetMixFormat guarantees a complete WAVEFORMATEX and declared extra bytes.
    // Reject unsupported extra sizes without reading them.
    let raw = unsafe {
        let length = match (*mix).cbSize {
            0 => 18,
            22 => 40,
            _ => 0,
        };
        slice::from_raw_parts(mix.cast::<u8>(), length)
    };
    negotiate(raw, |candidate| {
        check()?;
        // Discard the previously attempted IAudioClient, preserving the exact same endpoint.
        let client = open_audio_client(device)?;
        check()?;
        // SAFETY: validated native WAVEFORMATEX/WAVEFORMATEXTENSIBLE, both packed (align=1).
        // Initialize consumes the format during this call and does not retain its pointer.
        unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_LOOPBACK,
                50_000_000,
                0,
                candidate.as_ptr().cast::<WAVEFORMATEX>(),
                None,
            )
        }
        .map_err(|err| {
            if err.code() == AUDCLNT_E_UNSUPPORTED_FORMAT {
                Fault::Format
            } else {
                native_error("Initialize", err)
            }
        })?;
        Ok(client)
    })
}

struct Event(HANDLE);
impl Event {
    fn open() -> Result<Self, Fault> {
        // SAFETY: private unnamed nonsignaled auto-reset event; scoped to this worker.
        unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
            .map(Self)
            .map_err(|err| native_error("CreateEventW", err))
    }
    fn wait(&self, timeout: Duration) -> Result<(), Fault> {
        let ms = timeout.as_nanos().div_ceil(1_000_000) as u32;
        // SAFETY: valid live handle and wait is bounded by the policy's 10 ms.
        match unsafe { WaitForSingleObject(self.0, ms) } {
            WAIT_OBJECT_0 | WAIT_TIMEOUT => Ok(()),
            _ => Err(native_error(
                "WaitForSingleObject",
                windows::core::Error::from_win32(),
            )),
        }
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: unique owned handle, after the audio client has been dropped.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct Started<'a>(Option<&'a IAudioClient>);
impl Started<'_> {
    fn stop(&mut self) -> Result<(), Fault> {
        if let Some(client) = self.0.take() {
            // SAFETY: successful Start on this thread. This guard attempts Stop once.
            unsafe { client.Stop() }.map_err(|err| native_error("Stop", err))?;
        }
        Ok(())
    }
}
impl Drop for Started<'_> {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct PacketLease<'a> {
    client: &'a IAudioCaptureClient,
    info: PacketInfo,
    data: *const u8,
    frame_bytes: usize,
    armed: bool,
}
impl<'a> PacketLease<'a> {
    fn acquire(client: &'a IAudioCaptureClient, format: Format) -> Result<Option<Self>, Fault> {
        let (mut data, mut frames, mut flags) = (null_mut(), 0, 0);
        // SAFETY: valid out pointers; raw vtable preserves the success value for empty
        // packets. Timestamp out pointers are optional and not consumed by the prototype.
        let hr = unsafe {
            (client.vtable().GetBuffer)(
                client.as_raw(),
                &mut data,
                &mut frames,
                &mut flags,
                null_mut(),
                null_mut(),
            )
        };
        if hr == AUDCLNT_S_BUFFER_EMPTY {
            return Ok(None);
        }
        hr.ok().map_err(|err| native_error("GetBuffer", err))?;
        Ok(Some(Self {
            client,
            info: PacketInfo { frames, flags },
            data,
            frame_bytes: format.frame_bytes(),
            armed: true,
        }))
    }
    fn release_once(&mut self) -> Result<(), Fault> {
        self.armed = false;
        // SAFETY: matching acquired packet on the same thread; pass the full native frame count.
        unsafe { self.client.ReleaseBuffer(self.info.frames) }
            .map_err(|err| native_error("ReleaseBuffer", err))
    }
}
impl Packet for PacketLease<'_> {
    fn info(&self) -> PacketInfo {
        self.info
    }
    fn bytes(&self, length: usize) -> Option<&[u8]> {
        if self.data.is_null()
            || self.info.flags & SILENT != 0
            || self.info.frames > MAX_FRAMES_PER_PACKET
            || length > self.info.frames as usize * self.frame_bytes
        {
            return None;
        }
        // SAFETY: a non-silent successful GetBuffer provides this memory until ReleaseBuffer.
        // The checked maximum uses the initialized format's exact WAVE block alignment.
        Some(unsafe { slice::from_raw_parts(self.data, length) })
    }
    fn release(mut self) -> Result<(), Fault> {
        self.release_once()
    }
}
impl Drop for PacketLease<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.release_once();
        }
    }
}

type Capture = Result<(Format, Vec<Stereo>), Fault>;

fn capture_fixture(
    endpoint: &FixtureEndpoint,
    frames: u32,
    timeout: Duration,
    cancel: &Cancellation,
) -> Capture {
    cancel.check()?;
    validate_request(frames, timeout)?;
    let started_at = Instant::now();
    let check = || next_wait(timeout, started_at.elapsed(), cancel);
    let _com = Com::open()?;
    check()?;
    // SAFETY: MMDeviceEnumerator is in-process COM, no aggregation or default-device lookup.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
            .map_err(|err| native_error("CoCreateInstance", err))?;
    check()?;
    let explicit_id = endpoint.nul_terminated();
    // SAFETY: stable NUL-terminated identifier from a fixture-only capability.
    let device = unsafe { enumerator.GetDevice(PCWSTR(explicit_id.as_ptr())) }
        .map_err(|err| native_error("GetDevice", err))?;
    check()?;
    validate_endpoint(endpoint, &device)?;
    check()?;
    let event = Event::open()?;
    let (format, client) = open_stream(&device, check)?;
    let mut budget = Budget::new(format, frames, timeout)?;
    budget.next_wait(started_at.elapsed(), cancel)?;
    // SAFETY: initialized live client; event outlives both client interfaces.
    let capture: IAudioCaptureClient =
        unsafe { client.GetService() }.map_err(|err| native_error("GetService", err))?;
    budget.next_wait(started_at.elapsed(), cancel)?;
    // SAFETY: valid handle kept alive until after both COM interfaces are released.
    unsafe { client.SetEventHandle(event.0) }.map_err(|err| native_error("SetEventHandle", err))?;
    budget.next_wait(started_at.elapsed(), cancel)?;
    // SAFETY: endpoint and event are set on this initialized client.
    unsafe { client.Start() }.map_err(|err| native_error("Start", err))?;
    let mut running = Started(Some(&client));
    let mut result = Vec::new();
    while !budget.complete() {
        let wait = budget.next_wait(started_at.elapsed(), cancel)?;
        // SAFETY: running capture client on the original COM apartment/thread.
        let announced = unsafe { capture.GetNextPacketSize() }
            .map_err(|err| native_error("GetNextPacketSize", err))?;
        if announced != 0
            && let Some(packet) = PacketLease::acquire(&capture, format)?
        {
            result.push(read_packet(packet, announced, format, &mut budget, cancel)?);
            continue;
        }
        event.wait(wait)?;
    }
    budget.next_wait(started_at.elapsed(), cancel)?;
    running.stop()?;
    validate_endpoint(endpoint, &device)?;
    budget.next_wait(started_at.elapsed(), cancel)?;
    Ok((format, result))
}

#[test]
fn native_link_and_pre_cancel_without_opening_com() {
    let capture = std::hint::black_box(capture_fixture);
    let cancel = Cancellation::default();
    cancel.cancel();
    // This is deliberately the only call. It returns before creating any COM
    // object or looking for a device. The fixed synthetic vector is not a driver.
    assert_eq!(
        capture(
            &FixtureEndpoint::synthetic_vector(),
            1,
            Duration::from_secs(1),
            &cancel
        ),
        Err(Fault::Cancelled),
    );
}
