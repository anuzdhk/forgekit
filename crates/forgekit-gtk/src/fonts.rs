//! Forgekit's typeface, Mona Sans (SIL Open Font License, see
//! data/fonts/OFL.txt). It ships inside the binary and is registered for this
//! process only, so nothing gets installed on the system.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;

const MONA_SANS: &[u8] = include_bytes!("../data/fonts/MonaSans.ttf");

#[link(name = "fontconfig")]
unsafe extern "C" {
    fn FcConfigAppFontAddFile(config: *mut std::ffi::c_void, file: *const std::os::raw::c_char) -> i32;
}

/// Make Mona Sans available to this app. Call before GTK lays out any text.
pub fn register() {
    // fontconfig wants a file; keep one in the cache, rewritten when it changes.
    let path = forgekit_core::paths::cache_dir().join("fonts/MonaSans.ttf");
    if std::fs::metadata(&path).map(|m| m.len()).ok() != Some(MONA_SANS.len() as u64) {
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        if let Err(e) = std::fs::write(&path, MONA_SANS) {
            eprintln!("fonts: {e}");
            return;
        }
    }
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else { return };
    // SAFETY: a NULL config means the current one; the path is a valid C string.
    if unsafe { FcConfigAppFontAddFile(std::ptr::null_mut(), c.as_ptr()) } == 0 {
        eprintln!("fonts: couldn't load {}", path.display());
    }
}
