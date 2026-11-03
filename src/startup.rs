//! "Start at login": a value under HKCU\Software\Microsoft\Windows\CurrentVersion\Run.
//! Per-user, needs no admin rights, and is visible/removable in Task Manager > Startup apps.

use std::path::Path;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, RRF_RT_REG_SZ,
};
use windows::core::HSTRING;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "orderup";

/// Run `exe --background` at every login.
pub fn enable(exe: &Path) -> Result<(), String> {
    set_value(RUN_KEY, VALUE, &format!("\"{}\" --background", exe.display()))
}

/// Stop starting at login. Fine if it wasn't enabled.
pub fn disable() -> Result<(), String> {
    delete_value(RUN_KEY, VALUE)
}

pub fn is_enabled() -> bool {
    get_value(RUN_KEY, VALUE).is_some()
}

fn check(what: &str, code: WIN32_ERROR) -> Result<(), String> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("{what} failed: {}", windows::core::Error::from(code.to_hresult())))
    }
}

fn set_value(key: &str, name: &str, data: &str) -> Result<(), String> {
    let wide: Vec<u16> = data.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 buffer and the byte length passed matches it.
    let code = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            REG_SZ.0,
            Some(wide.as_ptr().cast()),
            (wide.len() * 2) as u32,
        )
    };
    check("registering startup", code)
}

fn delete_value(key: &str, name: &str) -> Result<(), String> {
    // SAFETY: plain registry call with valid wide-string arguments.
    let code = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name)) };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    check("removing startup entry", code)
}

fn get_value(key: &str, name: &str) -> Option<String> {
    let mut size = 0u32;
    // SAFETY: first call only asks for the required size (null data pointer).
    let code = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), RRF_RT_REG_SZ, None, None, Some(&mut size))
    };
    if code != ERROR_SUCCESS || size == 0 {
        return None;
    }
    let mut buf = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY: `buf` has at least `size` bytes of room.
    let code = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            &HSTRING::from(name),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if code != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same code path as enable/disable, but on a throwaway key instead of the Run key.
    #[test]
    fn roundtrip_on_scratch_key() {
        let key = r"Software\orderup-test";
        let data = r#""C:\Program Files\order up\orderup.exe" --background"#;
        set_value(key, "v", data).unwrap();
        assert_eq!(get_value(key, "v").as_deref(), Some(data));
        delete_value(key, "v").unwrap();
        assert_eq!(get_value(key, "v"), None);
        delete_value(key, "v").unwrap(); // deleting twice is fine
        // remove the now-empty scratch key itself
        let _ = std::process::Command::new("reg").args(["delete", &format!(r"HKCU\{key}"), "/f"]).output();
    }
}
