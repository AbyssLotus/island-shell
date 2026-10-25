//! Keys and tokens the user typed in, encrypted for this Windows account only (DPAPI), as base64.
//! Exactly what 0.1.x did in src-tauri/src/util.rs (`CryptProtectData`, no entropy, no flags), so a
//! module can read the files it takes over from 0.1.x.

#[cfg(windows)]
pub fn protect(plain: &str) -> String {
    if plain.is_empty() {
        return String::new();
    }
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        if CryptProtectData(&input, None, None, None, None, 0, &mut out).is_err() {
            return String::new();
        }
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        B64.encode(bytes)
    }
}

#[cfg(windows)]
pub fn unprotect(enc: &str) -> String {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    let Ok(bytes) = B64.decode(enc) else { return String::new() };
    if bytes.is_empty() {
        return String::new();
    }
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        if CryptUnprotectData(&input, None, None, None, None, 0, &mut out).is_err() {
            return String::new();
        }
        let plain = String::from_utf8_lossy(std::slice::from_raw_parts(out.pbData, out.cbData as usize)).into_owned();
        let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        plain
    }
}

/// Off Windows there's no DPAPI; secrets can't be stored (tests use these to see that).
#[cfg(not(windows))]
pub fn protect(_plain: &str) -> String {
    String::new()
}

#[cfg(not(windows))]
pub fn unprotect(_enc: &str) -> String {
    String::new()
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn round_trip() {
        let enc = super::protect("s3cret value");
        assert!(!enc.is_empty() && !enc.contains("s3cret"));
        assert_eq!(super::unprotect(&enc), "s3cret value");
        assert_eq!(super::unprotect(""), "");
    }
}
