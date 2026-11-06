//! Process lifetime for the native SDK; media handles still close individually.
use crate::{Error, Result};
use libloading::{Library, Symbol};
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    sync::{Arc, Mutex, OnceLock},
};
#[cfg(windows)]
#[path = "ffi/com.rs"]
mod com;

/// Load only from the chosen Windows DLL's directory and System32. Neither the
/// working directory nor PATH is an implicit source for the SDK or its imports.
unsafe fn load(path: &OsStr) -> Result<Library> {
    let unavailable = |_| Error::Playback {
        operation: "library",
        code: -1,
    };
    #[cfg(windows)]
    {
        use libloading::os::windows::{
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
            Library as WindowsLibrary,
        };
        if !std::path::Path::new(path).is_absolute() {
            return Err(Error::InvalidInput(
                "libmpv requires an absolute Windows path",
            ));
        }
        // SAFETY: the caller selected the trusted file; dependency lookup is
        // restricted to the file's own directory and Windows System32.
        unsafe {
            WindowsLibrary::load_with_flags(
                path,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        }
        .map(Library::from)
        .map_err(unavailable)
    }
    #[cfg(not(windows))]
    {
        // SAFETY: the caller selects the trusted system or host SDK.
        unsafe { Library::new(path) }.map_err(unavailable)
    }
}

type Libraries = Mutex<HashMap<OsString, Arc<Library>>>;
static LIBRARIES: OnceLock<Libraries> = OnceLock::new();

/// Retains the exact loaded code without a native media handle or MTA lease.
#[derive(Clone)]
pub(super) struct MpvModule(Arc<Library>);
impl MpvModule {
    pub(super) fn acquire(&self) -> Result<MpvRuntime> {
        Ok(MpvRuntime {
            #[cfg(windows)]
            _mta: com::Mta::acquire()?,
            library: self.0.clone(),
        })
    }
}

/// Native host lease for libmpv and its dependency modules. This carries no
/// account, decoder, output device, source, or playback state.
///
/// A dependency such as GGML can install a process-global C++ terminate handler
/// without restoring it on unload. Retain each trusted SDK until process exit;
/// unloading/reloading it between media handles can abort the process. The
/// Windows MTA lease still ends after this owner's media handles are closed.
pub struct MpvRuntime {
    #[cfg(windows)]
    _mta: com::Mta,
    library: Arc<Library>,
}

impl MpvRuntime {
    /// Open the packaged SDK, or use a trusted host configuration.
    ///
    /// # Safety
    /// On Windows the executable directory and the native launch environment
    /// (including MPV_DLL_DIR_CUMULUS) must be trusted. On Unix the dynamic
    /// loader's configured system search directories must be trusted.
    pub unsafe fn open_default() -> Result<Self> {
        let missing = || Error::Playback {
            operation: "library unavailable",
            code: -1,
        };
        #[cfg(windows)]
        {
            let path = crate::media::runtime::windows_tool(&["libmpv-2.dll", "mpv-2.dll"])
                .ok_or_else(missing)?;
            // SAFETY: only the trusted locations documented above are accepted.
            unsafe { Self::open(path.as_os_str()) }
        }
        #[cfg(not(windows))]
        {
            let names: &[&str] = if cfg!(target_os = "macos") {
                &["libmpv.2.dylib", "libmpv.dylib"]
            } else {
                &["libmpv.so.2", "libmpv.so.1"]
            };
            names
                .iter()
                // SAFETY: uses the documented trusted Unix loader search.
                .find_map(|name| unsafe { Self::open(OsStr::new(name)) }.ok())
                .ok_or_else(missing)
        }
    }

    /// Open or retain the host-selected SDK. Failed loads are not cached.
    /// Changing native SDK binaries requires restarting the application.
    ///
    /// # Safety
    /// `path` must identify a trusted native libmpv SDK. Loading a library runs
    /// its native initialization code; never accept this path from IPC/API data.
    pub unsafe fn open(path: &OsStr) -> Result<Self> {
        let mut modules = LIBRARIES
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        let library = if let Some(library) = modules.get(path) {
            library.clone()
        } else {
            // SAFETY: the native caller supplies a trusted SDK path.
            let library = Arc::new(unsafe { load(path) }?);
            modules.insert(path.to_owned(), library.clone());
            library
        };
        drop(modules);
        MpvModule(library).acquire()
    }

    pub(super) fn module(&self) -> MpvModule {
        MpvModule(self.library.clone())
    }

    /// Resolve an SDK symbol, with the borrowed library lifetime preserved.
    ///
    /// # Safety
    /// `T` must match the exact C ABI for `name`. Copied function pointers and
    /// every media handle must stop being used before this lease is dropped.
    pub unsafe fn get<T>(&self, name: &[u8]) -> Result<Symbol<'_, T>> {
        // SAFETY: the caller supplies the documented C ABI and symbol identity.
        unsafe { self.library.get(name) }.map_err(|_| Error::Playback {
            operation: "ABI",
            code: -1,
        })
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn windows_sdk_rejects_an_unqualified_name_before_loading() {
        // SAFETY: an unqualified name is rejected before Windows loading.
        assert!(matches!(
            unsafe { MpvRuntime::open(OsStr::new("libmpv-2.dll")) },
            Err(Error::InvalidInput(_))
        ));
    }
}
