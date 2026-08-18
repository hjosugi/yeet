//! Registry reads and writes for the Windows integration points.
//!
//! These used to shell out to `reg.exe`. From the GUI subsystem every one of
//! those spawns flashes a console window on screen, and the theme lookup runs
//! on every realize and map of the shelf and of each edge strip — so a launch
//! opened and closed a console window roughly ten times before the shelf
//! appeared. The same work in-process needs no process, no window, and no
//! measurable time.

use std::io;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_DWORD, RegCloseKey, RegDeleteValueW,
    RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows::core::PCWSTR;

/// Read a `REG_DWORD` under `HKEY_CURRENT_USER`.
///
/// A missing key or value is `None` rather than an error: the settings read
/// through here are all "unset means the Windows default".
pub fn current_user_dword(subkey: &str, value: &str) -> Option<u32> {
    let subkey = wide(subkey);
    let value = wide(value);
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated for the call, and the output
    // buffer is a `u32` described by `size`.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast()),
            Some(&mut size),
        )
    };
    (status == ERROR_SUCCESS).then_some(data)
}

/// Write a `REG_SZ` under `HKEY_CURRENT_USER`, creating no keys.
pub fn set_current_user_string(subkey: &str, value: &str, data: &str) -> io::Result<()> {
    let key = OpenKey::for_writing(subkey)?;
    let value = wide(value);
    // `REG_SZ` data is counted in bytes and includes the terminating NUL.
    let data: Vec<u8> = wide(data).into_iter().flat_map(u16::to_le_bytes).collect();
    // SAFETY: `key` is open for `KEY_SET_VALUE`, and both buffers outlive the call.
    let status =
        unsafe { RegSetValueExW(key.0, PCWSTR(value.as_ptr()), None, REG_SZ, Some(&data)) };
    result(status)
}

/// Remove a value under `HKEY_CURRENT_USER`; an absent one is already removed.
pub fn delete_current_user_value(subkey: &str, value: &str) -> io::Result<()> {
    let key = OpenKey::for_writing(subkey)?;
    let value = wide(value);
    // SAFETY: `key` is open for `KEY_SET_VALUE`, which covers value deletion.
    let status = unsafe { RegDeleteValueW(key.0, PCWSTR(value.as_ptr())) };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    result(status)
}

/// An open registry key that closes itself.
struct OpenKey(HKEY);

impl OpenKey {
    fn for_writing(subkey: &str) -> io::Result<Self> {
        let subkey = wide(subkey);
        let mut key = HKEY::default();
        // SAFETY: the subkey is NUL-terminated and `key` receives the handle,
        // which `Drop` closes.
        let status = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                KEY_SET_VALUE,
                &mut key,
            )
        };
        result(status).map(|()| Self(key))
    }
}

impl Drop for OpenKey {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from a successful `RegOpenKeyExW` and is closed once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// A NUL-terminated UTF-16 copy, as the `…W` entry points expect.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn result(status: WIN32_ERROR) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status.0 as i32))
    }
}
