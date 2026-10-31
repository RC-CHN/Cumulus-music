//! Verification-only raw PCM output through the real libmpv filter chain.
//! Production playback uses core::MpvEngine; no application IPC exposes these options.
use cumulus_core::{Error, Result, playback::engine::MpvRuntime};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::Path,
    time::{Duration, Instant},
};
#[repr(C)]
struct Event {
    id: c_int,
    error: c_int,
    userdata: u64,
    data: *mut c_void,
}
// mpv/client.h: MPV_EVENT_LOG_MESSAGE data, borrowed until the next wait.
#[repr(C)]
struct LogMessage {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
    _log_level: c_int,
}
type Handle = *mut c_void;
type Create = unsafe extern "C" fn() -> Handle;
type Initialize = unsafe extern "C" fn(Handle) -> c_int;
type OptionString = unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int;
type Command = unsafe extern "C" fn(Handle, *const *const c_char) -> c_int;
type Wait = unsafe extern "C" fn(Handle, f64) -> *const Event;
type Destroy = unsafe extern "C" fn(Handle);
type Property = unsafe extern "C" fn(Handle, *const c_char) -> *mut c_char;
type Free = unsafe extern "C" fn(*mut c_void);
type RequestLog = unsafe extern "C" fn(Handle, *const c_char) -> c_int;
struct Player {
    _runtime: MpvRuntime,
    handle: Handle,
    destroy: Destroy,
}
impl Drop for Player {
    fn drop(&mut self) {
        eprintln!("PCM proof: handle_closing");
        unsafe { (self.destroy)(self.handle) }
        eprintln!("PCM proof: handle_closed");
    }
}
fn c(value: impl AsRef<str>) -> Result<CString> {
    CString::new(value.as_ref()).map_err(|_| Error::InvalidInput("NUL in proof argument"))
}
fn check(code: i32) -> Result<()> {
    if code < 0 {
        Err(Error::Playback {
            operation: "PCM proof",
            code,
        })
    } else {
        Ok(())
    }
}
pub fn capture(input: &Path, output: &Path, graph: &str, rate: u32, channels: usize) -> Result<()> {
    eprintln!("PCM proof: runtime_open rate={rate} channels={channels}");
    if output.exists() {
        return Err(Error::InvalidInput("PCM proof output already exists"));
    }
    let names: &[&str] = if cfg!(target_os = "windows") {
        &["libmpv-2.dll", "mpv-2.dll"]
    } else if cfg!(target_os = "macos") {
        &["libmpv.2.dylib", "libmpv.dylib"]
    } else {
        &["libmpv.so.2"]
    };
    // SAFETY: same documented libmpv ABI as the production wrapper. All symbols
    // and the handle remain library-owned. Event pointers are read before wait.
    unsafe {
        let library = names
            .iter()
            .find_map(|n| MpvRuntime::open(n.as_ref()).ok())
            .ok_or(Error::State("libmpv unavailable"))?;
        let create = *library
            .get::<Create>(b"mpv_create\0")
            .map_err(|_| Error::State("mpv ABI"))?;
        let initialize = *library
            .get::<Initialize>(b"mpv_initialize\0")
            .map_err(|_| Error::State("mpv ABI"))?;
        let option = *library
            .get::<OptionString>(b"mpv_set_option_string\0")
            .map_err(|_| Error::State("mpv ABI"))?;
        let command = *library
            .get::<Command>(b"mpv_command\0")
            .map_err(|_| Error::State("mpv ABI"))?;
        let wait = *library
            .get::<Wait>(b"mpv_wait_event\0")
            .map_err(|_| Error::State("mpv ABI"))?;
        let destroy = *library
            .get::<Destroy>(b"mpv_terminate_destroy\0")
            .map_err(|_| Error::State("mpv ABI"))?;
        let property = *library
            .get::<Property>(b"mpv_get_property_string\0")
            .map_err(|_| Error::State("mpv ABI"))?;
        let free = *library
            .get::<Free>(b"mpv_free\0")
            .map_err(|_| Error::State("mpv ABI"))?;
        let request_log = *library
            .get::<RequestLog>(b"mpv_request_log_messages\0")
            .map_err(|_| Error::State("mpv ABI"))?;
        let handle = create();
        if handle.is_null() {
            return Err(Error::State("mpv allocation"));
        }
        let _player = Player {
            _runtime: library,
            handle,
            destroy,
        };
        eprintln!("PCM proof: handle_created");
        for (key, value) in [
            ("config", "no"),
            ("load-scripts", "no"),
            ("ytdl", "no"),
            // stdout is the structured success report. Consume diagnostics
            // through the API and preserve them on stderr instead.
            ("terminal", "no"),
            ("msg-level", "all=warn"),
            ("vo", "null"),
            ("ao", "pcm"),
            ("ao-pcm-waveheader", "no"),
            ("audio-format", "float"),
            ("volume", "100"),
            ("idle", "yes"),
            ("keep-open", "no"),
            ("pause", "no"),
        ] {
            check(option(handle, c(key)?.as_ptr(), c(value)?.as_ptr()))?;
        }
        check(option(
            handle,
            c"ao-pcm-file".as_ptr(),
            c(output.to_str().ok_or(Error::InvalidInput("proof path"))?)?.as_ptr(),
        ))?;
        check(option(
            handle,
            c"audio-samplerate".as_ptr(),
            c(rate.to_string())?.as_ptr(),
        ))?;
        check(option(
            handle,
            c"audio-channels".as_ptr(),
            c(match channels {
                1 => "mono",
                2 => "stereo",
                6 => "5.1",
                _ => return Err(Error::InvalidInput("proof channels")),
            })?
            .as_ptr(),
        ))?;
        let filter = format!("@cumulus-geq:lavfi=graph=%{}%{}", graph.len(), graph);
        check(option(handle, c"af".as_ptr(), c(filter)?.as_ptr()))?;
        check(initialize(handle))?;
        check(request_log(handle, c"warn".as_ptr()))?;
        eprintln!("PCM proof: initialized");
        let args = [
            c"loadfile".to_owned(),
            c(input.to_str().ok_or(Error::InvalidInput("proof path"))?)?,
        ];
        let pointers = [args[0].as_ptr(), args[1].as_ptr(), std::ptr::null()];
        check(command(handle, pointers.as_ptr()))?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut loaded = false;
        while Instant::now() < deadline {
            let event = wait(handle, 0.1);
            if event.is_null() {
                continue;
            }
            match (*event).id {
                2 => {
                    if (*event).data.is_null() {
                        return Err(Error::State("missing native log data"));
                    }
                    let message = &*(*event).data.cast::<LogMessage>();
                    if !message.prefix.is_null()
                        && !message.level.is_null()
                        && !message.text.is_null()
                    {
                        eprint!(
                            "libmpv[{}/{}]: {}",
                            CStr::from_ptr(message.prefix).to_string_lossy(),
                            CStr::from_ptr(message.level).to_string_lossy(),
                            CStr::from_ptr(message.text).to_string_lossy()
                        );
                    }
                }
                8 => loaded = true,
                7 => {
                    // end-file data begins with reason:int, error:int.
                    if (*event).data.is_null() {
                        return Err(Error::State("missing end-file data"));
                    }
                    let data = (*event).data.cast::<c_int>();
                    if *data != 0 || *data.add(1) < 0 || !loaded {
                        eprintln!(
                            "PCM capture ended before verified EOF: reason={} error={} loaded={loaded}",
                            *data,
                            *data.add(1)
                        );
                        return Err(Error::State("capture did not reach EOF"));
                    }
                    eprintln!("PCM proof: eof_verified");
                    return Ok(());
                }
                1 => return Err(Error::State("unexpected mpv shutdown")),
                _ => {}
            }
        }
        let p = property(handle, c"idle-active".as_ptr());
        if !p.is_null() {
            eprintln!(
                "capture timeout idle={}",
                CStr::from_ptr(p).to_string_lossy()
            );
            free(p.cast());
        }
        Err(Error::State("PCM proof timed out"))
    }
}
