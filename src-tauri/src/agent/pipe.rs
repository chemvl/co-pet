//! Windows transport for the `copet-mcp` sidecar: a named pipe per Windows user, restricted to
//! that user's SID. Same line protocol as the Unix socket (`socket::serve`).

/// `\\.\pipe\copet-mcp-<hex of the UTF-8 user name>`; `src-mcp/bridge.ts` builds the same name.
pub fn pipe_name(user: &str) -> String {
    format!(r"\\.\pipe\copet-mcp-{}", hex::encode(user.as_bytes()))
}

#[cfg(windows)]
pub(super) mod server {
    use std::time::Duration;

    use tauri::AppHandle;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{
        GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// `D:P(A;;GA;;;<user SID>)`: only this user, no inherited ACEs.
    fn sddl() -> Option<String> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
            let mut len = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            // u64 words so the buffer is aligned for `TOKEN_USER`.
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            let ok = GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len).is_ok();
            let _ = CloseHandle(token);
            if !ok {
                return None;
            }
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut sid = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut sid).ok()?;
            let s = sid.to_string().ok();
            let _ = LocalFree(Some(HLOCAL(sid.0.cast())));
            s.map(|s| format!("D:P(A;;GA;;;{s})"))
        }
    }

    fn create(name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
        let sddl = sddl().ok_or_else(|| std::io::Error::other("no user SID"))?;
        let w = crate::platform::wide(&sddl);
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(w.as_ptr()), SDDL_REVISION_1, &mut sd, None)
                .map_err(std::io::Error::other)?;
            let mut attrs = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: false.into(),
            };
            let r = ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(name, (&mut attrs as *mut SECURITY_ATTRIBUTES).cast());
            let _ = LocalFree(Some(HLOCAL(sd.0)));
            r
        }
    }

    /// Serves pipe connections until the app quits; a failure to create the first instance
    /// (another Co-pet of this user owns the name) is logged.
    pub fn start(app: &AppHandle, name: String) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut server = match create(&name, true) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[agent] pipe {name}: {e}");
                    return;
                }
            };
            loop {
                if let Err(e) = server.connect().await {
                    eprintln!("[agent] pipe connect: {e}");
                    // Reset the instance before retrying.
                    let _ = server.disconnect();
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
                let next = match create(&name, false) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("[agent] pipe instance: {e}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                let connected = std::mem::replace(&mut server, next);
                let (read, write) = tokio::io::split(connected);
                let app = app.clone();
                tauri::async_runtime::spawn(super::super::socket::serve(read, write, move |line| {
                    super::super::socket::handle(app.clone(), line)
                }));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pipe_name_is_per_user_and_matches_the_sidecar() {
        assert_eq!(super::pipe_name("Cường"), r"\\.\pipe\copet-mcp-43c6b0e1bb9d6e67");
        assert_ne!(super::pipe_name("a"), super::pipe_name("A"));
    }
}
