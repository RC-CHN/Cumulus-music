//! Native helpers shared by the two explicit local WMI probe processes.
use std::{
    fmt,
    ptr::{null, null_mut},
};
use windows::{
    Win32::System::{
        Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            CoInitializeSecurity, CoSetProxyBlanket, CoUninitialize, EOAC_NONE,
            RPC_C_AUTHN_LEVEL_CALL, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
        },
        Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE},
        Variant::{VARIANT, VARIANT_0_0, VT_BSTR, VariantClear},
        Wmi::{
            IWbemClassObject, IWbemContext, IWbemLocator, IWbemServices,
            WBEM_FLAG_CONNECT_USE_MAX_WAIT, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
            WBEM_GENERIC_FLAG_TYPE, WBEM_S_TIMEDOUT, WbemLocator,
        },
    },
    core::{BSTR, PCWSTR},
};

const CP_ACP: u32 = 0;
const MAX_BSTR_UNITS: usize = 4_096;
pub(super) const MAX_CONVERSION_UNITS: usize = MAX_BSTR_UNITS * 4 + 1;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn WideCharToMultiByte(
        cp: u32,
        flags: u32,
        source: *const u16,
        len: i32,
        out: *mut u8,
        cap: i32,
        default_char: *const u8,
        used_default_char: *mut i32,
    ) -> i32;
}

struct ComThread;

impl Drop for ComThread {
    fn drop(&mut self) {
        // SAFETY: successful CoInitializeEx ran on this thread. Interfaces and
        // VARIANTs were dropped before the worker's COM guard.
        unsafe { CoUninitialize() };
    }
}

pub(super) struct Property {
    value: VARIANT,
    owner: IWbemClassObject,
}

impl Drop for Property {
    fn drop(&mut self) {
        // SAFETY: the VARIANT was initialized, and only VariantClear owns
        // the BSTR or SAFEARRAY carried by the active tag. Keep even an
        // unexpected BYREF provider alive while clearing the tag.
        let _ = unsafe { VariantClear(&mut self.value) };
    }
}

impl Property {
    pub(super) fn tagged(&self) -> &VARIANT_0_0 {
        // SAFETY: Get returned a Windows VARIANT; this arm exposes the tag.
        unsafe { &self.value.Anonymous.Anonymous }
    }

    pub(super) fn bounded_bstr(&self) -> Result<&[u16], Failure> {
        let tagged = self.tagged();
        if tagged.vt != VT_BSTR {
            return Err(Failure(ReadState::NotBstr, Some(i32::from(tagged.vt.0))));
        }
        // SAFETY: exact non-BYREF VT_BSTR, borrowed with the VARIANT alive.
        let value = unsafe { &tagged.Anonymous.bstrVal };
        if value.len() > MAX_BSTR_UNITS {
            return Err(Failure(ReadState::TooLong, None));
        }
        if value.is_empty() || value[0] == 0 {
            return Err(Failure(ReadState::Empty, None));
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum ReadState {
    QueryFailed,
    NextFailed,
    TimedOut,
    NoObject,
    BadObjectCount,
    PropertyFailed,
    NotBstr,
    TooLong,
    Empty,
    Present,
}

pub(super) struct Failure(pub(super) ReadState, pub(super) Option<i32>);

pub(super) fn first_property(
    services: &IWbemServices,
    query: &'static str,
    property: PCWSTR,
) -> Result<Property, Failure> {
    let flags = WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_RETURN_IMMEDIATELY.0 | WBEM_FLAG_FORWARD_ONLY.0);
    // SAFETY: in-thread proxy on a fixed local namespace. Both callers pass
    // hardcoded WQL strings and property names with no external input.
    let enumerator = unsafe {
        services.ExecQuery(
            &BSTR::from("WQL"),
            &BSTR::from(query),
            flags,
            None::<&IWbemContext>,
        )
    }
    .map_err(|error| Failure(ReadState::QueryFailed, Some(error.code().0)))?;
    let mut first: [Option<IWbemClassObject>; 1] = [None];
    let mut count = 0;
    // SAFETY: a single live writable object slot and count, no retries.
    let status = unsafe { enumerator.Next(1_500, &mut first, &mut count) };
    if status.is_err() {
        return Err(Failure(ReadState::NextFailed, Some(status.0)));
    }
    let [object] = first;
    let object = match (count, object) {
        (1, Some(object)) => object,
        (0, None) if status.0 == WBEM_S_TIMEDOUT.0 => {
            return Err(Failure(ReadState::TimedOut, Some(status.0)));
        }
        (0, None) => return Err(Failure(ReadState::NoObject, Some(status.0))),
        _ => return Err(Failure(ReadState::BadObjectCount, Some(status.0))),
    };
    let mut value = Property {
        value: VARIANT::default(),
        owner: object,
    };
    // SAFETY: initialized output, fixed NUL-terminated property, live object.
    unsafe { value.owner.Get(property, 0, &mut value.value, None, None) }
        .map_err(|error| Failure(ReadState::PropertyFailed, Some(error.code().0)))?;
    Ok(value)
}

/// D02970 for a bounded UTF-16 value. Includes the API's terminator and the
/// extra NUL at the returned size. Never tries another encoding or locale.
pub(super) fn acp_with_terminal(units: &[u16]) -> Option<Vec<u8>> {
    if units.len() > MAX_BSTR_UNITS {
        return None;
    }
    let mut wide = units.to_vec();
    wide.push(0);
    // SAFETY: bounded NUL-terminated input, null/zero for sizing, or a
    // correctly sized writable output. Both default-character pointers null.
    let encode = |out, capacity| unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            wide.as_ptr(),
            -1,
            out,
            capacity,
            null(),
            null_mut(),
        )
    };
    let required = encode(null_mut(), 0);
    let count = usize::try_from(required).ok()?;
    if !(1..=MAX_CONVERSION_UNITS).contains(&count) {
        return None;
    }
    let mut narrow = vec![0xA5; count + 1];
    if encode(narrow.as_mut_ptr(), required) != required || narrow[count - 1] != 0 {
        return None;
    }
    narrow[count] = 0;
    Some(narrow)
}

pub(super) struct Startup {
    stage: &'static str,
    code: i32,
}
impl fmt::Display for Startup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stage={} hresult=0x{:08x}", self.stage, self.code)
    }
}

pub(super) fn with_local_wmi<T>(inspect: impl FnOnce(&IWbemServices) -> T) -> Result<T, Startup> {
    let stage = |stage| {
        move |error: windows::core::Error| Startup {
            stage,
            code: error.code().0,
        }
    };
    // SAFETY: fresh worker owns an MTA; S_OK and S_FALSE both require
    // a matching CoUninitialize.
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(stage("com"))?;
    let _com = ComThread;
    // SAFETY: this is a dedicated probe process, not the app/GUI. The
    // descriptor and flags are CF7D80's defaults.
    unsafe {
        CoInitializeSecurity(
            None,
            -1,
            None,
            None,
            RPC_C_AUTHN_LEVEL_DEFAULT,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
            None,
        )
    }
    .map_err(stage("security"))?;
    // SAFETY: in-process WbemLocator on the initialized worker, no aggregation.
    let locator: IWbemLocator =
        unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }
            .map_err(stage("locator"))?;
    let empty = BSTR::new();
    // SAFETY: fixed local namespace and no explicit credentials/remote
    // context. The OS wait flag supplements the outer process deadline.
    let services = unsafe {
        locator.ConnectServer(
            &BSTR::from(r"ROOT\CIMV2"),
            &empty,
            &empty,
            &empty,
            WBEM_FLAG_CONNECT_USE_MAX_WAIT.0,
            &empty,
            None::<&IWbemContext>,
        )
    }
    .map_err(stage("local_namespace"))?;
    // SAFETY: CF7D80's 10/0/3/3 applies only to this proxy. Null principal
    // and auth info; no change to system policy or the caller's process.
    unsafe {
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
    .map_err(stage("proxy"))?;
    Ok(inspect(&services))
}
