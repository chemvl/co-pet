//! The Unix socket the `copet-mcp` sidecar talks to (`Paths::sock`, mode 0600). One JSON object
//! per line each way: `{id, client, tool, args}` in; `{id, ok: true, result}` or
//! `{id, ok: false, error, message}` out. Calls on one connection run concurrently and answer
//! in completion order, matched by `id`.
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

use crate::agent::clients::ClientRef;
use crate::agent::tools::{self, ToolError};
use crate::agent::AppHost;
use crate::paths::Paths;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    id: Value,
    client: ClientRef,
    tool: String,
    #[serde(default)]
    args: Value,
}

/// Binds the socket and serves connections until the app quits. A bind failure is logged.
pub fn start(app: &AppHandle) {
    let path = app.state::<Paths>().sock.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let listener = match bind(&path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[agent] socket {}: {e}", path.display());
                return;
            }
        };
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    tauri::async_runtime::spawn(serve(app.clone(), stream));
                }
                Err(e) => {
                    eprintln!("[agent] accept: {e}");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });
}

/// Removes a stale socket file, binds, then restricts the socket to this user.
fn bind(path: &Path) -> std::io::Result<UnixListener> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

async fn serve(app: AppHandle, stream: UnixStream) {
    let (read, write) = stream.into_split();
    let write = Arc::new(Mutex::new(write));
    let mut lines = BufReader::new(read).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => {
                eprintln!("[agent] read: {e}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let (app, write) = (app.clone(), write.clone());
        tauri::async_runtime::spawn(async move {
            let mut out = handle(app, &line).await.to_string();
            out.push('\n');
            if let Err(e) = write.lock().await.write_all(out.as_bytes()).await {
                eprintln!("[agent] write: {e}");
            }
        });
    }
}

fn error_reply(id: Value, e: &ToolError) -> Value {
    json!({ "id": id, "ok": false, "error": e.code, "message": e.message })
}

/// One request line → one response. The call runs on the blocking pool (SQLite, file copies).
async fn handle(app: AppHandle, line: &str) -> Value {
    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            let id = serde_json::from_str::<Value>(line).ok().and_then(|v| v.get("id").cloned());
            return error_reply(id.unwrap_or(Value::Null), &ToolError::invalid(format!("Bad request: {e}.")));
        }
    };
    let Request { id, client, tool, args } = req;
    let host = AppHost(app);
    match tauri::async_runtime::spawn_blocking(move || tools::dispatch(&host, &client, &tool, args)).await {
        Ok(Ok(result)) => json!({ "id": id, "ok": true, "result": result }),
        Ok(Err(e)) => error_reply(id, &e),
        Err(e) => {
            eprintln!("[agent] call panicked: {e}");
            error_reply(id, &ToolError::internal())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bind_replaces_stale_socket_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.sock");
        std::fs::write(&path, "stale").unwrap();
        let listener = bind(&path).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let mut client = UnixStream::connect(&path).await.unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        client.write_all(b"ping\n").await.unwrap();
        let mut line = String::new();
        BufReader::new(&mut server).read_line(&mut line).await.unwrap();
        assert_eq!(line, "ping\n");
    }
}
