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

type Libraries = Mutex<HashMap<OsString, Arc<Library>>>;
static LIBRARIES: OnceLock<Libraries> = OnceLock::new();

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
            let library = Arc::new(unsafe { Library::new(path) }.map_err(|_| Error::Playback {
                operation: "library",
                code: -1,
            })?);
            modules.insert(path.to_owned(), library.clone());
            library
        };
        drop(modules);
        #[cfg(windows)]
        let mta = com::Mta::acquire()?;
        Ok(Self {
            #[cfg(windows)]
            _mta: mta,
            library,
        })
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
