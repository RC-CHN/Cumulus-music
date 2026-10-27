//! A graphics probe waits for the parent before creating any WebView workers.
//! This job then bounds the complete WebView2 process tree, including timeouts.
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::Child,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};

pub(super) fn contain(child: &Child) -> io::Result<OwnedHandle> {
    // SAFETY: unnamed private job; the resulting handle is owned exactly once.
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateJobObjectW returned a new valid owned HANDLE.
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: matching information class, correctly sized initialized buffer;
    // both process/job handles remain live for the entire calls.
    let configured = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        ) != 0
            && AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) != 0
    };
    if !configured {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}
