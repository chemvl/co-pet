//! Windows secret store: one DPAPI-encrypted file per account (user scope) under
//! `%APPDATA%\com.copet.app\secrets\`. Credential Manager is not used: its 2560-byte blob cap is
//! smaller than a Supabase session.
use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

fn root() -> PathBuf {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    appdata.join("com.copet.app").join("secrets")
}

fn file_name(account: &str) -> String {
    format!("{}.bin", hex::encode(account))
}

fn protect(data: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        let r = if decrypt {
            CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        r.map_err(|e| e.to_string())?;
        if out.pbData.is_null() {
            return Ok(Vec::new());
        }
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        Ok(bytes)
    }
}

fn get_in(dir: &Path, account: &str) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(dir.join(file_name(account))) {
        Ok(enc) => protect(&enc, true).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn set_in(dir: &Path, account: &str, value: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let enc = protect(value, false)?;
    let path = dir.join(file_name(account));
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, enc).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn delete_in(dir: &Path, account: &str) -> Result<(), String> {
    match std::fs::remove_file(dir.join(file_name(account))) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

pub fn get(account: &str) -> Result<Option<Vec<u8>>, String> {
    get_in(&root(), account)
}

pub fn set(account: &str, value: &[u8]) -> Result<(), String> {
    set_in(&root(), account, value)
}

pub fn delete(account: &str) -> Result<(), String> {
    delete_in(&root(), account)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_and_delete_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(get_in(dir.path(), "notion").unwrap(), None);
        set_in(dir.path(), "notion", b"{\"t\":1}").unwrap();
        assert_eq!(get_in(dir.path(), "notion").unwrap().as_deref(), Some(&b"{\"t\":1}"[..]));
        // DPAPI output is not the plain text.
        let raw = std::fs::read(dir.path().join(file_name("notion"))).unwrap();
        assert!(!raw.windows(5).any(|w| w == b"{\"t\":"));
        delete_in(dir.path(), "notion").unwrap();
        delete_in(dir.path(), "notion").unwrap();
        assert_eq!(get_in(dir.path(), "notion").unwrap(), None);
    }

    #[test]
    fn large_values_fit() {
        let dir = tempfile::tempdir().unwrap();
        let big = vec![b'x'; 16 * 1024];
        set_in(dir.path(), "supabase.session", &big).unwrap();
        assert_eq!(get_in(dir.path(), "supabase.session").unwrap().unwrap().len(), big.len());
    }
}
