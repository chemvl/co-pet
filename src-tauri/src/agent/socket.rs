//! The Unix socket the `copet-mcp` sidecar talks to (`Paths::sock`, mode 0600). One JSON object
//! per line each way: `{id, client, tool, args}` in; `{id, ok: true, result}` or
//! `{id, ok: false, error, message}` out. Calls on one connection run concurrently (at most
//! [`MAX_IN_FLIGHT`]) and answer in completion order, matched by `id`. A line over
//! [`MAX_LINE`] bytes gets one `invalid` reply and the connection is closed.
use std::future::Future;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{Mutex, Semaphore};

use crate::agent::clients::ClientRef;
use crate::agent::tools::{self, ToolError};
use crate::agent::AppHost;
use crate::events;
use crate::paths::Paths;

/// Longest request line in bytes, without its newline.
const MAX_LINE: usize = 1024 * 1024;
/// Calls one connection may have running; further lines wait to be read.
const MAX_IN_FLIGHT: usize = 8;

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
                    let app = app.clone();
                    let (read, write) = stream.into_split();
                    tauri::async_runtime::spawn(serve(read, write, move |line| handle(app.clone(), line)));
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

/// Reads request lines and answers each with `handle`'s reply.
async fn serve<R, W, F, Fut>(read: R, write: W, handle: F)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
    F: Fn(String) -> Fut,
    Fut: Future<Output = Value> + Send + 'static,
{
    let write = Arc::new(Mutex::new(write));
    let slots = Arc::new(Semaphore::new(MAX_IN_FLIGHT));
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        // One byte past the cap tells a too-long line from one that just fits.
        match (&mut reader).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("[agent] read: {e}");
                break;
            }
        }
        if buf.last() == Some(&b'\n') {
            buf.pop();
        } else if buf.len() > MAX_LINE {
            let reply = error_reply(Value::Null, &ToolError::invalid("Request too large."));
            let mut w = write.lock().await;
            if let Err(e) = send(&mut *w, &reply).await {
                eprintln!("[agent] write: {e}");
            }
            let _ = w.shutdown().await;
            break;
        }
        if buf.trim_ascii().is_empty() {
            continue;
        }
        let Ok(slot) = slots.clone().acquire_owned().await else { break };
        let reply = match String::from_utf8(std::mem::take(&mut buf)) {
            Ok(line) => handle(line),
            Err(_) => {
                let reply = error_reply(Value::Null, &ToolError::invalid("Bad request: not UTF-8."));
                let mut w = write.lock().await;
                if let Err(e) = send(&mut *w, &reply).await {
                    eprintln!("[agent] write: {e}");
                }
                continue;
            }
        };
        let write = write.clone();
        tokio::spawn(async move {
            let reply = reply.await;
            if let Err(e) = send(&mut *write.lock().await, &reply).await {
                eprintln!("[agent] write: {e}");
            }
            drop(slot);
        });
    }
}

async fn send<W: AsyncWrite + Unpin>(w: &mut W, reply: &Value) -> std::io::Result<()> {
    let mut out = reply.to_string();
    out.push('\n');
    w.write_all(out.as_bytes()).await
}

fn error_reply(id: Value, e: &ToolError) -> Value {
    json!({ "id": id, "ok": false, "error": e.code, "message": e.message })
}

/// One request line → one response. The call runs on the blocking pool (SQLite, file copies).
async fn handle(app: AppHandle, line: String) -> Value {
    let req: Request = match serde_json::from_str(&line) {
        Ok(r) => r,
        Err(e) => {
            let id = serde_json::from_str::<Value>(&line).ok().and_then(|v| v.get("id").cloned());
            return error_reply(id.unwrap_or(Value::Null), &ToolError::invalid(format!("Bad request: {e}.")));
        }
    };
    let Request { id, client, tool, args } = req;
    let host = AppHost(app.clone());
    let done = tauri::async_runtime::spawn_blocking(move || tools::dispatch(&host, &client, &tool, args)).await;
    // The call logged a row and set the client's `last_call_at`.
    events::agents_changed_throttled(&app);
    match done {
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::net::UnixStream;

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

    /// `serve` on one end of a socket pair, answering `{"echo": <line>}`.
    fn echo_server() -> UnixStream {
        let (client, server) = UnixStream::pair().unwrap();
        let (read, write) = server.into_split();
        tokio::spawn(serve(read, write, |line| async move { json!({ "echo": line }) }));
        client
    }

    #[tokio::test]
    async fn oversized_line_is_rejected_and_closes() {
        let client = echo_server();
        let (read, mut write) = client.into_split();
        let mut lines = BufReader::new(read).lines();
        let fits = "a".repeat(MAX_LINE);
        write.write_all(format!("{fits}\n").as_bytes()).await.unwrap();
        let reply: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(reply["echo"].as_str().map(str::len), Some(MAX_LINE));
        let writer = tokio::spawn(async move {
            // The server stops reading part-way, so this write may fail.
            let _ = write.write_all(&vec![b'b'; MAX_LINE + 10]).await;
        });
        let reply: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!((reply["error"].as_str(), reply["message"].as_str()), (Some("invalid"), Some("Request too large.")));
        assert_eq!(lines.next_line().await.unwrap(), None);
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn in_flight_calls_are_bounded() {
        let (client, server) = UnixStream::pair().unwrap();
        let started = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Semaphore::new(0));
        let (read, write) = server.into_split();
        let (s, g) = (started.clone(), gate.clone());
        tokio::spawn(serve(read, write, move |line| {
            let (s, g) = (s.clone(), g.clone());
            async move {
                s.fetch_add(1, Ordering::SeqCst);
                g.acquire().await.unwrap().forget();
                json!({ "echo": line })
            }
        }));
        let (read, mut write) = client.into_split();
        for i in 0..MAX_IN_FLIGHT + 3 {
            write.write_all(format!("{i}\n").as_bytes()).await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(started.load(Ordering::SeqCst), MAX_IN_FLIGHT);
        gate.add_permits(MAX_IN_FLIGHT + 3);
        let mut lines = BufReader::new(read).lines();
        for _ in 0..MAX_IN_FLIGHT + 3 {
            lines.next_line().await.unwrap().unwrap();
        }
        assert_eq!(started.load(Ordering::SeqCst), MAX_IN_FLIGHT + 3);
    }
}
