use super::{Error, Family, Result};
use windows::{
    Win32::{Globalization::GetUserDefaultLocaleName, Graphics::DirectWrite::*},
    core::{BOOL, PCWSTR},
};
fn native() -> windows::core::Result<Vec<Family>> {
    // SAFETY: DirectWrite owns COM objects via reference-counted wrappers; all
    // out parameters and UTF-16 buffers remain live for each synchronous call.
    // No raw pointer or platform object leaves this worker.
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let mut collection = None;
        factory.GetSystemFontCollection(&mut collection, true)?;
        let collection = collection.ok_or_else(|| {
            windows::core::Error::from_hresult(windows::core::HRESULT(0x8000ffffu32 as i32))
        })?;
        let count = collection.GetFontFamilyCount();
        if count > 32768 {
            return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                0x80070057u32 as i32,
            )));
        }
        let mut locale = [0u16; 85];
        let has_locale = GetUserDefaultLocaleName(&mut locale) > 0;
        let mut families = Vec::new();
        for i in 0..count {
            let strings = collection.GetFontFamily(i)?.GetFamilyNames()?;
            let size = strings.GetCount();
            if size == 0 || size > 1024 {
                continue;
            }
            let mut index = 0;
            let mut exists = BOOL(0);
            if has_locale {
                strings.FindLocaleName(PCWSTR(locale.as_ptr()), &mut index, &mut exists)?;
            }
            if !exists.as_bool() {
                strings.FindLocaleName(windows::core::w!("en-us"), &mut index, &mut exists)?;
            }
            if !exists.as_bool() {
                index = 0;
            }
            let mut names = Vec::new();
            let mut preferred = String::new();
            for j in 0..size {
                let length = strings.GetStringLength(j)?;
                if length > 256 {
                    continue;
                }
                let mut buffer = vec![0u16; length as usize + 1];
                strings.GetString(j, &mut buffer)?;
                if let Ok(name) = String::from_utf16(&buffer[..length as usize]) {
                    if j == index {
                        preferred = name.clone();
                    }
                    names.push(name);
                }
            }
            families.push(Family {
                name: preferred,
                aliases: names,
            });
        }
        Ok(families)
    }
}
pub(super) fn enumerate() -> Result<Vec<Family>> {
    native().map_err(|_| Error::State("system font enumeration failed"))
}
