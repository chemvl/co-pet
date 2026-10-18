//! MCP client identity and setup. HTTP clients prove a bearer token; stdio clients are known by
//! their key and get a row (writes not approved) on first contact. "Add to …" writes a `copet`
//! entry into the config file of one of the [`KNOWN`] clients; Remove takes it out again.
use std::ffi::OsString;
use std::fs;
use std::io::{self, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::db::{new_id, Db};
use crate::error::{AppError, AppResult};
use crate::model::McpClientView;

pub const KEY_MAX: usize = 80;
pub const NAME_MAX: usize = 100;
/// The name of Co-pet's entry in every client config.
const ENTRY: &str = "copet";

/// The `client` of a socket request. `token` is set in HTTP mode only.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientRef {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub token: Option<String>,
}

/// A live `mcp_clients` row.
#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    pub id: String,
    pub name: String,
    pub write_approved: bool,
}

fn map_client(r: &Row<'_>) -> rusqlite::Result<Client> {
    Ok(Client { id: r.get(0)?, name: r.get(1)?, write_approved: r.get(2)? })
}

/// Hex sha256 of an HTTP bearer token, as stored in `mcp_clients.token_hash`.
pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Byte equality whose time depends only on the length.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The live client whose `token_hash` matches `token`. Every live hash is compared.
fn by_token(conn: &Connection, token: &str) -> AppResult<Option<Client>> {
    let hash = hash_token(token);
    let mut stmt = conn.prepare(
        "SELECT id, name, write_approved, token_hash FROM mcp_clients
         WHERE deleted_at IS NULL AND token_hash IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| Ok((map_client(r)?, r.get::<_, String>(3)?)))?;
    let mut found = None;
    for row in rows {
        let (client, stored) = row?;
        if ct_eq(stored.as_bytes(), hash.as_bytes()) {
            found = Some(client);
        }
    }
    Ok(found)
}

fn by_key(conn: &Connection, key: &str) -> AppResult<Option<Client>> {
    Ok(conn
        .query_row(
            "SELECT id, name, write_approved FROM mcp_clients WHERE key = ?1 AND deleted_at IS NULL",
            [key],
            map_client,
        )
        .optional()?)
}

fn valid_text(s: &str, max: usize) -> bool {
    let n = s.trim().chars().count();
    n > 0 && n <= max
}

/// The client making a call; every call sets its `last_call_at`. With a token (HTTP): the live
/// row whose `token_hash` matches, else `unauthorized`. Without (stdio): the live row of `key`,
/// inserted with writes not approved on first contact; `http:` keys always need a token.
/// `invalid` for an empty or oversized key or name.
pub fn resolve(conn: &Connection, r: &ClientRef, now: i64) -> AppResult<Client> {
    let client = match &r.token {
        Some(token) => by_token(conn, token)?.ok_or(AppError::Code("unauthorized"))?,
        None => {
            if !valid_text(&r.key, KEY_MAX) || !valid_text(&r.name, NAME_MAX) {
                return Err(AppError::Code("invalid"));
            }
            let key = r.key.trim();
            if key.starts_with("http:") {
                return Err(AppError::Code("unauthorized"));
            }
            match by_key(conn, key)? {
                Some(c) => c,
                None => {
                    let c = Client { id: new_id(), name: r.name.trim().to_owned(), write_approved: false };
                    conn.execute(
                        "INSERT INTO mcp_clients (id, key, name, write_approved, created_at, updated_at)
                         VALUES (?1, ?2, ?3, 0, ?4, ?4)",
                        params![c.id, key, c.name, now],
                    )?;
                    c
                }
            }
        }
    };
    conn.execute(
        "UPDATE mcp_clients SET last_call_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![client.id, now],
    )?;
    Ok(client)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Format {
    /// `mcpServers.copet`; `typed` adds `"type": "stdio"`.
    Json { typed: bool },
    /// `[mcp_servers.copet]`.
    Toml,
}

/// A client "Add to …" configures: its key, display name and user-level config file.
#[derive(Debug)]
pub struct Known {
    pub key: &'static str,
    pub name: &'static str,
    /// Relative to the home folder.
    file: &'static str,
    format: Format,
}

pub const KNOWN: [Known; 4] = [
    Known { key: "claude-code", name: "Claude Code", file: ".claude.json", format: Format::Json { typed: true } },
    Known { key: "cursor", name: "Cursor", file: ".cursor/mcp.json", format: Format::Json { typed: true } },
    Known { key: "codex", name: "Codex", file: ".codex/config.toml", format: Format::Toml },
    Known { key: "gemini-cli", name: "Gemini CLI", file: ".gemini/settings.json", format: Format::Json { typed: false } },
];

pub fn known(key: &str) -> Option<&'static Known> {
    KNOWN.iter().find(|k| k.key == key)
}

/// How "Add to Claude Code" reaches the `claude` CLI, which must make the change while Claude
/// Code runs (it rewrites `~/.claude.json`). Injectable for tests.
pub trait ClaudeCli: Sync {
    /// `claude` on the login-shell PATH, if any.
    fn find(&self) -> Option<PathBuf>;
    /// Runs `claude <args>`; a failure carries its stderr.
    fn run(&self, claude: &Path, args: &[OsString]) -> AppResult<()>;
}

/// How long the login-shell probe or one `claude mcp` call may run before it is killed.
const SHELL_TIMEOUT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(50);

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    })
}

/// `cmd.output()` (stdin null) that kills the child after `timeout` and fails with `TimedOut`.
/// On a timeout the pipe readers are left to end on their own, so a grandchild still holding
/// a pipe can't hold up the caller.
fn output_within(mut cmd: Command, timeout: Duration) -> io::Result<Output> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(ErrorKind::TimedOut, format!("timed out after {} s", timeout.as_secs_f32())));
        }
        thread::sleep(POLL);
    };
    Ok(Output { status, stdout: stdout.join().unwrap_or_default(), stderr: stderr.join().unwrap_or_default() })
}

/// The user's login shell (`/bin/zsh -l`) on macOS, so the PATH matches their terminal's; on
/// Windows, Co-pet's own PATH. Each call is killed after [`SHELL_TIMEOUT`].
pub struct LoginShell;

#[cfg(unix)]
impl ClaudeCli for LoginShell {
    fn find(&self) -> Option<PathBuf> {
        let mut cmd = Command::new("/bin/zsh");
        cmd.args(["-lc", "command -v claude"]);
        let out = output_within(cmd, SHELL_TIMEOUT).ok()?;
        let stdout = String::from_utf8(out.stdout).ok()?;
        let path = PathBuf::from(stdout.lines().last()?.trim());
        (out.status.success() && path.is_absolute() && path.is_file()).then_some(path)
    }

    fn run(&self, claude: &Path, args: &[OsString]) -> AppResult<()> {
        // Through the login shell too: `claude` is a node script that needs its PATH.
        let mut cmd = Command::new("/bin/zsh");
        cmd.args(["-lc", r#"exec "$0" "$@""#]).arg(claude).args(args);
        let out = output_within(cmd, SHELL_TIMEOUT).map_err(|e| AppError::msg(format!("claude failed: {e}")))?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(AppError::msg(format!("claude failed ({}): {}", out.status, stderr.trim())))
    }
}

/// `CREATE_NO_WINDOW`: `claude.exe` and the `cmd.exe` behind a `.cmd` shim are console
/// programs, which would otherwise flash a console window over Co-pet.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The first `claude.exe` in the folders of `path` (a `PATH` value), else the first
/// `claude.cmd` (npm shim).
#[cfg(any(windows, test))]
fn claude_on_path(path: &std::ffi::OsStr) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = std::env::split_paths(path).filter(|d| !d.as_os_str().is_empty()).collect();
    ["claude.exe", "claude.cmd"].iter().find_map(|name| dirs.iter().map(|d| d.join(name)).find(|p| p.is_file()))
}

#[cfg(windows)]
impl ClaudeCli for LoginShell {
    /// Searched in-process: `where.exe` prints in the console code page, which mangles
    /// non-ASCII folder names.
    fn find(&self) -> Option<PathBuf> {
        claude_on_path(&std::env::var_os("PATH")?)
    }

    /// Runs `claude` directly; for a `.cmd` shim the standard library goes through `cmd.exe`
    /// with batch-safe quoting (a plain `cmd /C` would mangle quoted paths with spaces).
    fn run(&self, claude: &Path, args: &[OsString]) -> AppResult<()> {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new(claude);
        cmd.args(args).creation_flags(CREATE_NO_WINDOW);
        let out = output_within(cmd, SHELL_TIMEOUT).map_err(|e| AppError::msg(format!("claude failed: {e}")))?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(AppError::msg(format!("claude failed ({}): {}", out.status, stderr.trim())))
    }
}

/// Where "Add to …" writes: the home folder, the bundled `copet-mcp` and the Claude Code CLI.
pub struct Setup<'a> {
    pub home: PathBuf,
    pub command: PathBuf,
    pub claude: &'a dyn ClaudeCli,
}

fn unreadable() -> AppError {
    AppError::Code("configUnreadable")
}

/// The file's text; `None` when it doesn't exist. Any other failure is `configUnreadable`.
fn read_config(path: &Path) -> AppResult<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err(unreadable()),
    }
}

/// A missing or blank file is `{}`; anything but a JSON object is `configUnreadable`.
fn parse_json(text: Option<&str>) -> AppResult<Map<String, Value>> {
    match text.map(str::trim) {
        None | Some("") => Ok(Map::new()),
        Some(t) => match serde_json::from_str(t) {
            Ok(Value::Object(root)) => Ok(root),
            _ => Err(unreadable()),
        },
    }
}

fn parse_toml(text: Option<&str>) -> AppResult<toml_edit::DocumentMut> {
    text.unwrap_or("").parse().map_err(|_| unreadable())
}

fn json_has_entry(root: &Map<String, Value>) -> bool {
    root.get("mcpServers").and_then(Value::as_object).is_some_and(|s| s.contains_key(ENTRY))
}

fn toml_has_entry(doc: &toml_edit::DocumentMut) -> bool {
    doc.get("mcp_servers").and_then(|s| s.as_table_like()).is_some_and(|s| s.contains_key(ENTRY))
}

/// Whether `path` has the `copet` entry; `configUnreadable` when it can't be read or parsed.
fn has_entry(path: &Path, format: Format) -> AppResult<bool> {
    let text = read_config(path)?;
    Ok(match format {
        Format::Json { .. } => json_has_entry(&parse_json(text.as_deref())?),
        Format::Toml => toml_has_entry(&parse_toml(text.as_deref())?),
    })
}

/// Whether each of the [`KNOWN`] clients' config files has the `copet` entry.
pub fn installed(home: &Path) -> [bool; 4] {
    KNOWN.each_ref().map(|k| has_entry(&home.join(k.file), k.format).unwrap_or(false))
}

/// `<file>.copet-backup`, made from the file before Co-pet first changes it.
fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".copet-backup");
    path.with_file_name(name)
}

fn backup_once(path: &Path) -> AppResult<()> {
    let backup = backup_path(path);
    if path.is_file() && !backup.exists() {
        fs::copy(path, backup)?;
    }
    Ok(())
}

/// Backs the file up (once), creates its folder, then replaces it in one rename (through a
/// symlink, to its target) keeping its permissions.
fn save(path: &Path, text: &str) -> AppResult<()> {
    backup_once(path)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let mut tmp_name = OsString::from(".");
    tmp_name.push(target.file_name().unwrap_or_default());
    tmp_name.push(".copet-tmp");
    let tmp = target.with_file_name(tmp_name);
    let written = (|| -> std::io::Result<()> {
        fs::write(&tmp, text)?;
        if let Ok(meta) = fs::metadata(&target) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        fs::rename(&tmp, &target)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(written?)
}

fn json_text(root: &Map<String, Value>) -> AppResult<String> {
    Ok(serde_json::to_string_pretty(root)? + "\n")
}

fn toml_entry(command: &str, key: &str) -> toml_edit::Table {
    let mut env = toml_edit::InlineTable::new();
    env.insert("COPET_CLIENT", key.into());
    let mut t = toml_edit::Table::new();
    t["command"] = toml_edit::value(command);
    t["args"] = toml_edit::value(toml_edit::Array::from_iter(["--stdio"]));
    t["env"] = toml_edit::value(env);
    t
}

/// The text of the config with the `copet` entry set for `known`.
fn with_entry(text: Option<&str>, known: &Known, command: &Path) -> AppResult<String> {
    let command = command.to_string_lossy();
    match known.format {
        Format::Json { typed } => {
            let mut root = parse_json(text)?;
            let servers = root.entry("mcpServers").or_insert_with(|| json!({}));
            let servers = servers.as_object_mut().ok_or_else(unreadable)?;
            let mut entry = Map::new();
            if typed {
                entry.insert("type".into(), json!("stdio"));
            }
            entry.insert("command".into(), json!(command));
            entry.insert("args".into(), json!(["--stdio"]));
            entry.insert("env".into(), json!({ "COPET_CLIENT": known.key }));
            servers.insert(ENTRY.into(), Value::Object(entry));
            json_text(&root)
        }
        Format::Toml => {
            let mut doc = parse_toml(text)?;
            let servers = doc.entry("mcp_servers").or_insert_with(|| {
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                toml_edit::Item::Table(t)
            });
            let entry = toml_entry(&command, known.key);
            match servers {
                toml_edit::Item::Table(servers) => match servers.get_mut(ENTRY).and_then(|e| e.as_table_mut()) {
                    // In place, so its position and any keys the user added stay.
                    Some(existing) => existing.extend(entry),
                    None => {
                        servers.insert(ENTRY, toml_edit::Item::Table(entry));
                    }
                },
                toml_edit::Item::Value(toml_edit::Value::InlineTable(servers)) => {
                    servers.insert(ENTRY, toml_edit::Value::InlineTable(entry.into_inline_table()));
                }
                _ => return Err(unreadable()),
            }
            Ok(doc.to_string())
        }
    }
}

/// The text of the config without the `copet` entry; `None` when it has none.
fn without_entry(text: Option<&str>, format: Format) -> AppResult<Option<String>> {
    match format {
        Format::Json { .. } => {
            let mut root = parse_json(text)?;
            let removed = root.get_mut("mcpServers").and_then(Value::as_object_mut).and_then(|s| s.remove(ENTRY));
            removed.map(|_| json_text(&root)).transpose()
        }
        Format::Toml => {
            let mut doc = parse_toml(text)?;
            let removed = doc.get_mut("mcp_servers").and_then(|s| s.as_table_like_mut()).and_then(|s| s.remove(ENTRY));
            Ok(removed.map(|_| doc.to_string()))
        }
    }
}

fn os_args(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// Writes the `copet` entry for `known` (Claude Code: through `claude mcp add` when the CLI is
/// on the PATH) and returns the config file's path. `configUnreadable` when the file can't be
/// read or parsed; it is left unchanged.
pub fn install(setup: &Setup, known: &Known) -> AppResult<PathBuf> {
    let path = setup.home.join(known.file);
    if known.key == "claude-code" {
        if let Some(claude) = setup.claude.find() {
            let present = has_entry(&path, known.format)?;
            backup_once(&path)?;
            if present {
                // `mcp add` refuses an existing name; the command path may have moved.
                setup.claude.run(&claude, &os_args(&["mcp", "remove", "--scope", "user", ENTRY]))?;
            }
            let mut args = os_args(&["mcp", "add", "--scope", "user", "--transport", "stdio", ENTRY]);
            args.extend(os_args(&["--env", "COPET_CLIENT=claude-code", "--"]));
            args.push(setup.command.clone().into_os_string());
            args.push("--stdio".into());
            setup.claude.run(&claude, &args)?;
            return Ok(path);
        }
    }
    let text = read_config(&path)?;
    let out = with_entry(text.as_deref(), known, &setup.command)?;
    if text.as_deref() != Some(out.as_str()) {
        save(&path, &out)?;
    }
    Ok(path)
}

/// Removes only the `copet` entry from `path` (Claude Code: through `claude mcp remove` when
/// the CLI is on the PATH). A missing file or entry is fine; `configUnreadable` as in
/// [`install`].
pub fn uninstall(setup: &Setup, known: &Known, path: &Path) -> AppResult<()> {
    let text = read_config(path)?;
    let Some(out) = without_entry(text.as_deref(), known.format)? else { return Ok(()) };
    if known.key == "claude-code" {
        if let Some(claude) = setup.claude.find() {
            backup_once(path)?;
            return setup.claude.run(&claude, &os_args(&["mcp", "remove", "--scope", "user", ENTRY]));
        }
    }
    save(path, &out)
}

/// Lowercase ASCII letters and digits, other runs as `-`, at most 60 characters.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(60);
    out.trim_end_matches('-').to_owned()
}

const VIEW_COLS: &str =
    "id, key, name, write_approved, token_hash IS NOT NULL, created_at, approved_at, last_call_at";

fn map_view(r: &Row<'_>) -> rusqlite::Result<McpClientView> {
    Ok(McpClientView {
        id: Some(r.get(0)?),
        key: r.get(1)?,
        name: r.get(2)?,
        write_approved: r.get(3)?,
        has_token: r.get(4)?,
        config_installed: false,
        created_at: r.get(5)?,
        approved_at: r.get(6)?,
        last_call_at: r.get(7)?,
    })
}

fn view(conn: &Connection, id: &str) -> AppResult<McpClientView> {
    conn.query_row(&format!("SELECT {VIEW_COLS} FROM mcp_clients WHERE id = ?1"), [id], map_view)
        .optional()?
        .ok_or(AppError::Code("notFound"))
}

/// The [`KNOWN`] clients first, in order, live row or not (`installed` from [`installed`]),
/// then the other live clients, most recent call first.
pub fn list(conn: &Connection, installed: &[bool; 4]) -> AppResult<Vec<McpClientView>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {VIEW_COLS} FROM mcp_clients WHERE deleted_at IS NULL
         ORDER BY last_call_at IS NULL, last_call_at DESC, created_at DESC"
    ))?;
    let rows = stmt.query_map([], map_view)?.collect::<Result<Vec<_>, _>>()?;
    let mut out: Vec<McpClientView> = KNOWN
        .iter()
        .zip(installed)
        .map(|(k, &config_installed)| {
            let row = rows.iter().find(|r| r.key == k.key).cloned();
            McpClientView {
                config_installed,
                ..row.unwrap_or_else(|| McpClientView {
                    id: None,
                    key: k.key.to_owned(),
                    name: k.name.to_owned(),
                    write_approved: false,
                    has_token: false,
                    config_installed,
                    created_at: None,
                    approved_at: None,
                    last_call_at: None,
                })
            }
        })
        .collect();
    out.extend(rows.into_iter().filter(|r| known(&r.key).is_none()));
    Ok(out)
}

/// "Add to …": writes the config (no database lock held meanwhile), then upserts the client's
/// row with its `config_path`. `invalid` for an unknown key, `configUnreadable`.
pub fn add(db: &Db, setup: &Setup, key: &str, now: i64) -> AppResult<McpClientView> {
    let k = known(key).ok_or(AppError::Code("invalid"))?;
    let path = install(setup, k)?;
    let path = path.to_string_lossy();
    let conn = db.conn();
    let id = match by_key(&conn, k.key)? {
        Some(c) => {
            conn.execute(
                "UPDATE mcp_clients SET config_path = ?2, updated_at = ?3 WHERE id = ?1",
                params![c.id, path, now],
            )?;
            c.id
        }
        None => {
            let id = new_id();
            conn.execute(
                "INSERT INTO mcp_clients (id, key, name, write_approved, config_path, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 0, ?4, ?5, ?5)",
                params![id, k.key, k.name, path, now],
            )?;
            id
        }
    };
    Ok(McpClientView { config_installed: true, ..view(&conn, &id)? })
}

/// Approves writes and issues a new HTTP token, returned once (only its hash is stored).
/// `notFound`.
pub fn approve(conn: &Connection, id: &str, now: i64) -> AppResult<String> {
    let token = crate::export::oauth::random_b64()?;
    let n = conn.execute(
        "UPDATE mcp_clients SET write_approved = 1, approved_at = ?2, token_hash = ?3, updated_at = ?2
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, now, hash_token(&token)],
    )?;
    if n == 0 {
        return Err(AppError::Code("notFound"));
    }
    Ok(token)
}

/// Turns writes off and clears the token. `notFound`.
pub fn revoke(conn: &Connection, id: &str, now: i64) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE mcp_clients SET write_approved = 0, approved_at = NULL, token_hash = NULL, updated_at = ?2
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, now],
    )?;
    if n == 0 {
        return Err(AppError::Code("notFound"));
    }
    Ok(())
}

/// Removes the `copet` entry from the config Co-pet wrote (if any), then soft-deletes the
/// row; when the config can't be changed the row stays. `notFound`, `configUnreadable`.
pub fn remove(db: &Db, setup: &Setup, id: &str, now: i64) -> AppResult<()> {
    let (key, config_path): (String, Option<String>) = db
        .conn()
        .query_row(
            "SELECT key, config_path FROM mcp_clients WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::Code("notFound"))?;
    if let (Some(k), Some(path)) = (known(&key), config_path) {
        uninstall(setup, k, Path::new(&path))?;
    }
    db.conn().execute(
        "UPDATE mcp_clients SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
        params![id, now],
    )?;
    Ok(())
}

/// A client of the local HTTP server: key `http:<slug of name>` (`-2`, `-3`… when taken), writes
/// approved, with a token returned once. `invalid` for an empty or oversized name.
pub fn create_http(conn: &Connection, name: &str, now: i64) -> AppResult<(McpClientView, String)> {
    if !valid_text(name, NAME_MAX) {
        return Err(AppError::Code("invalid"));
    }
    let name = name.trim();
    let slug = slug(name);
    let base = format!("http:{}", if slug.is_empty() { "client" } else { &slug });
    let mut key = base.clone();
    let mut n = 2;
    while by_key(conn, &key)?.is_some() {
        key = format!("{base}-{n}");
        n += 1;
    }
    let token = crate::export::oauth::random_b64()?;
    let id = new_id();
    conn.execute(
        "INSERT INTO mcp_clients (id, key, name, write_approved, token_hash, created_at, approved_at, updated_at)
         VALUES (?1, ?2, ?3, 1, ?4, ?5, ?5, ?5)",
        params![id, key, name, hash_token(&token), now],
    )?;
    Ok((view(conn, &id)?, token))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// The bundled `copet-mcp` in a made-up install folder.
    fn command() -> PathBuf {
        std::env::temp_dir().join("Co-pet").join(format!("copet-mcp{}", std::env::consts::EXE_SUFFIX))
    }

    #[cfg(unix)]
    #[test]
    fn shell_calls_are_killed_after_the_timeout() {
        // The shell's `sleep` child keeps the pipes open after the shell is killed.
        let mut slow = Command::new("/bin/sh");
        slow.args(["-c", "echo started; sleep 30"]);
        let start = Instant::now();
        let err = output_within(slow, Duration::from_millis(300)).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(5), "returned after {:?}", start.elapsed());

        let mut quick = Command::new("/bin/sh");
        quick.args(["-c", "echo out; echo err >&2; exit 3"]);
        let out = output_within(quick, Duration::from_secs(5)).unwrap();
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(out.stdout, b"out\n");
        assert_eq!(out.stderr, b"err\n");
    }

    #[test]
    fn claude_on_path_prefers_the_exe_over_the_npm_shim() {
        let root = tempfile::tempdir().unwrap();
        let (npm, bin, empty) = (root.path().join("npm Cường"), root.path().join("bin"), root.path().join("empty"));
        for d in [&npm, &bin, &empty] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(npm.join("claude.cmd"), "").unwrap();
        let path = |dirs: &[&PathBuf]| std::env::join_paths(dirs).unwrap();
        assert_eq!(claude_on_path(&path(&[&empty, &npm, &bin])), Some(npm.join("claude.cmd")));
        fs::write(bin.join("claude.exe"), "").unwrap();
        assert_eq!(claude_on_path(&path(&[&empty, &npm, &bin])), Some(bin.join("claude.exe")), "the .exe wins");
        assert_eq!(claude_on_path(&path(&[&empty])), None);
        fs::create_dir_all(empty.join("claude.exe")).unwrap();
        assert_eq!(claude_on_path(&path(&[&empty])), None, "a folder isn't a program");
    }

    /// A `claude` CLI that records its calls; `path: None` = not on the PATH.
    struct FakeClaude {
        path: Option<PathBuf>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl FakeClaude {
        fn new(path: Option<&str>) -> Self {
            FakeClaude { path: path.map(PathBuf::from), calls: Mutex::new(Vec::new()) }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().iter().map(|c| c.join(" ")).collect()
        }
    }

    impl ClaudeCli for FakeClaude {
        fn find(&self) -> Option<PathBuf> {
            self.path.clone()
        }

        fn run(&self, claude: &Path, args: &[OsString]) -> AppResult<()> {
            assert_eq!(Some(claude), self.path.as_deref());
            self.calls.lock().unwrap().push(args.iter().map(|a| a.to_string_lossy().into_owned()).collect());
            Ok(())
        }
    }

    fn setup<'a>(home: &Path, claude: &'a FakeClaude) -> Setup<'a> {
        Setup { home: home.to_owned(), command: command(), claude }
    }

    fn k(key: &str) -> &'static Known {
        known(key).unwrap()
    }

    fn json_at(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn creates_missing_json_configs_with_the_entry() {
        let home = tempfile::tempdir().unwrap();
        let no_cli = FakeClaude::new(None);
        let s = setup(home.path(), &no_cli);
        let path = install(&s, k("cursor")).unwrap();
        assert_eq!(path, home.path().join(".cursor/mcp.json"));
        assert_eq!(
            json_at(&path),
            json!({ "mcpServers": { "copet": {
                "type": "stdio", "command": command().to_str().unwrap(), "args": ["--stdio"],
                "env": { "COPET_CLIENT": "cursor" }
            } } })
        );
        assert!(!backup_path(&path).exists(), "nothing to back up");
        let gemini = install(&s, k("gemini-cli")).unwrap();
        assert_eq!(
            json_at(&gemini)["mcpServers"]["copet"],
            json!({ "command": command().to_str().unwrap(), "args": ["--stdio"], "env": { "COPET_CLIENT": "gemini-cli" } })
        );
        // Without the CLI, Claude Code's file is edited directly.
        let claude = install(&s, k("claude-code")).unwrap();
        assert_eq!(claude, home.path().join(".claude.json"));
        assert_eq!(json_at(&claude)["mcpServers"]["copet"]["env"], json!({ "COPET_CLIENT": "claude-code" }));
        assert_eq!(installed(home.path()), [true, true, false, true]);
    }

    #[test]
    fn json_keeps_other_keys_and_backs_up_once() {
        let home = tempfile::tempdir().unwrap();
        let no_cli = FakeClaude::new(None);
        let s = setup(home.path(), &no_cli);
        let path = home.path().join(".gemini/settings.json");
        let original = r#"{"theme":"dark","mcpServers":{"other":{"command":"x"}},"zeta":1}"#;
        write(&path, original);
        install(&s, k("gemini-cli")).unwrap();
        let root = json_at(&path);
        assert_eq!(root["theme"], "dark");
        assert_eq!(root["mcpServers"]["other"], json!({ "command": "x" }));
        let keys: Vec<&String> = root.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["theme", "mcpServers", "zeta"], "key order kept");
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), original);

        // Later writes keep the first backup.
        uninstall(&s, k("gemini-cli"), &path).unwrap();
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), original);
        let root = json_at(&path);
        assert_eq!(root["mcpServers"], json!({ "other": { "command": "x" } }), "only copet removed");
        assert_eq!(root["zeta"], 1);
    }

    #[test]
    fn unparsable_config_is_left_untouched() {
        let home = tempfile::tempdir().unwrap();
        let no_cli = FakeClaude::new(None);
        let s = setup(home.path(), &no_cli);
        let cases = [
            ("cursor", ".cursor/mcp.json", "{ not json"),
            ("cursor", ".cursor/mcp.json", "[1, 2]"),
            ("cursor", ".cursor/mcp.json", r#"{"mcpServers": 3}"#),
            ("codex", ".codex/config.toml", "[mcp_servers\nbroken = "),
            ("codex", ".codex/config.toml", "mcp_servers = 3\n"),
        ];
        for (key, file, text) in cases {
            let path = home.path().join(file);
            write(&path, text);
            assert!(matches!(install(&s, k(key)), Err(AppError::Code("configUnreadable"))), "{text}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
            assert!(!backup_path(&path).exists());
        }
        // Removing from an unparsable file fails the same way.
        let path = home.path().join(".cursor/mcp.json");
        write(&path, "{ not json");
        assert!(matches!(uninstall(&s, k("cursor"), &path), Err(AppError::Code("configUnreadable"))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        assert_eq!(installed(home.path()), [false; 4]);
    }

    #[test]
    fn toml_keeps_comments_and_formatting() {
        let home = tempfile::tempdir().unwrap();
        let no_cli = FakeClaude::new(None);
        let s = setup(home.path(), &no_cli);
        let path = home.path().join(".codex/config.toml");
        let original = "# my codex settings\nmodel = \"o3\"  # the model\n\n[mcp_servers.other]\ncommand = \"x\"\n";
        write(&path, original);
        install(&s, k("codex")).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(original), "{text}");
        let doc: toml_edit::DocumentMut = text.parse().unwrap();
        let entry = &doc["mcp_servers"]["copet"];
        assert!(entry.is_table(), "a [mcp_servers.copet] table: {text}");
        assert_eq!(entry["command"].as_str(), command().to_str());
        assert_eq!(entry["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect::<Vec<_>>(), ["--stdio"]);
        assert_eq!(entry["env"]["COPET_CLIENT"].as_str(), Some("codex"));
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), original);
        assert_eq!(installed(home.path()), [false, false, true, false]);

        // Adding again changes nothing; removing restores the original text.
        install(&s, k("codex")).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        uninstall(&s, k("codex"), &path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn toml_created_when_missing() {
        let home = tempfile::tempdir().unwrap();
        let no_cli = FakeClaude::new(None);
        let path = install(&setup(home.path(), &no_cli), k("codex")).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("[mcp_servers.copet]\n"), "{text}");
        assert!(!text.contains("[mcp_servers]\n"), "no empty parent table: {text}");
    }

    #[test]
    fn claude_code_goes_through_the_cli_when_present() {
        let home = tempfile::tempdir().unwrap();
        let cli = FakeClaude::new(Some("/opt/bin/claude"));
        let s = setup(home.path(), &cli);
        let path = home.path().join(".claude.json");
        let original = r#"{"projects":{}}"#;
        write(&path, original);
        assert_eq!(install(&s, k("claude-code")).unwrap(), path);
        let add = format!(
            "mcp add --scope user --transport stdio copet --env COPET_CLIENT=claude-code -- {} --stdio",
            command().display()
        );
        assert_eq!(cli.calls(), [add.clone()]);
        assert_eq!(fs::read_to_string(&path).unwrap(), original, "the CLI edits it, not Co-pet");
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), original);

        // Already configured (by the CLI): re-adding removes the old entry first.
        write(&path, r#"{"projects":{},"mcpServers":{"copet":{"command":"/old/copet-mcp"}}}"#);
        install(&s, k("claude-code")).unwrap();
        let remove = "mcp remove --scope user copet".to_owned();
        assert_eq!(cli.calls(), [add.clone(), remove.clone(), add]);
        uninstall(&s, k("claude-code"), &path).unwrap();
        assert_eq!(cli.calls().last(), Some(&remove));
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), original, "backup kept");
    }

    #[test]
    fn uninstall_without_an_entry_or_file_does_nothing() {
        let home = tempfile::tempdir().unwrap();
        let cli = FakeClaude::new(Some("/opt/bin/claude"));
        let s = setup(home.path(), &cli);
        uninstall(&s, k("claude-code"), &home.path().join(".claude.json")).unwrap();
        let path = home.path().join(".cursor/mcp.json");
        write(&path, r#"{"mcpServers":{"other":{}}}"#);
        uninstall(&s, k("cursor"), &path).unwrap();
        assert!(cli.calls().is_empty());
        assert!(!home.path().join(".claude.json").exists());
        assert!(!backup_path(&path).exists(), "not written");
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_a_symlink() {
        let home = tempfile::tempdir().unwrap();
        let no_cli = FakeClaude::new(None);
        let real = home.path().join("dotfiles/mcp.json");
        write(&real, "{}");
        fs::create_dir_all(home.path().join(".cursor")).unwrap();
        let link = home.path().join(".cursor/mcp.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        install(&setup(home.path(), &no_cli), k("cursor")).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert!(json_at(&real)["mcpServers"]["copet"].is_object());
    }

    #[test]
    fn client_rows_follow_the_agents_commands() {
        let db = Db::open_in_memory().unwrap();
        let home = tempfile::tempdir().unwrap();
        let no_cli = FakeClaude::new(None);
        let s = setup(home.path(), &no_cli);

        let views = list(&db.conn(), &[false; 4]).unwrap();
        let keys: Vec<&str> = views.iter().map(|v| v.key.as_str()).collect();
        assert_eq!(keys, ["claude-code", "cursor", "codex", "gemini-cli"]);
        assert!(views.iter().all(|v| v.id.is_none() && !v.write_approved && v.created_at.is_none()));
        assert_eq!(views[3].name, "Gemini CLI");

        let cursor = add(&db, &s, "cursor", 10).unwrap();
        assert!(cursor.id.is_some() && cursor.config_installed && !cursor.write_approved);
        assert_eq!((cursor.name.as_str(), cursor.created_at), ("Cursor", Some(10)));
        let path: Option<String> = db
            .conn()
            .query_row("SELECT config_path FROM mcp_clients WHERE key = 'cursor'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(path, Some(home.path().join(".cursor/mcp.json").to_string_lossy().into_owned()));
        assert_eq!(add(&db, &s, "cursor", 11).unwrap().id, cursor.id, "upserted");
        assert!(matches!(add(&db, &s, "other:x", 12), Err(AppError::Code("invalid"))));
        let id = cursor.id.clone().unwrap();

        // Approve: writes on, a token whose hash identifies the client over HTTP.
        let token = approve(&db.conn(), &id, 20).unwrap();
        assert_eq!(token.len(), 43);
        let http = ClientRef { key: "x".into(), name: "x".into(), token: Some(token.clone()) };
        assert_eq!(resolve(&db.conn(), &http, 21).unwrap().id, id);
        let v = list(&db.conn(), &installed(home.path())).unwrap().remove(1);
        assert!(v.write_approved && v.has_token && v.config_installed);
        assert_eq!((v.approved_at, v.last_call_at), (Some(20), Some(21)));

        revoke(&db.conn(), &id, 30).unwrap();
        let v = list(&db.conn(), &[false; 4]).unwrap().remove(1);
        assert!(!v.write_approved && !v.has_token && v.approved_at.is_none());
        assert!(matches!(resolve(&db.conn(), &http, 31), Err(AppError::Code("unauthorized"))));

        // Other and HTTP clients follow the known four, latest call first.
        let other = ClientRef { key: "other:zed".into(), name: "Zed".into(), token: None };
        resolve(&db.conn(), &other, 40).unwrap();
        let (h1, t1) = create_http(&db.conn(), "  My script ", 41).unwrap();
        let (h2, _) = create_http(&db.conn(), "My script", 42).unwrap();
        assert_eq!((h1.key.as_str(), h1.name.as_str(), h2.key.as_str()), ("http:my-script", "My script", "http:my-script-2"));
        assert!(h1.write_approved && h1.has_token && h1.approved_at == Some(41));
        let by_token = ClientRef { key: "x".into(), name: "x".into(), token: Some(t1) };
        resolve(&db.conn(), &by_token, 43).unwrap();
        let keys: Vec<String> = list(&db.conn(), &[false; 4]).unwrap().into_iter().skip(4).map(|v| v.key).collect();
        assert_eq!(keys, ["http:my-script", "other:zed", "http:my-script-2"]);
        assert!(matches!(create_http(&db.conn(), "  ", 44), Err(AppError::Code("invalid"))));
        assert_eq!(create_http(&db.conn(), "Tiện ích", 45).unwrap().0.key, "http:ti-n-ch");
        assert_eq!(create_http(&db.conn(), "日本", 46).unwrap().0.key, "http:client");

        // Remove takes the entry out of the config and soft-deletes the row.
        remove(&db, &s, &id, 50).unwrap();
        assert_eq!(installed(home.path()), [false; 4]);
        let v = list(&db.conn(), &[false; 4]).unwrap().remove(1);
        assert!(v.id.is_none());
        assert!(matches!(remove(&db, &s, &id, 51), Err(AppError::Code("notFound"))));
        assert!(matches!(approve(&db.conn(), &id, 52), Err(AppError::Code("notFound"))));
        assert!(matches!(revoke(&db.conn(), &id, 52), Err(AppError::Code("notFound"))));
    }

    #[test]
    fn remove_keeps_the_row_when_the_config_is_unreadable() {
        let db = Db::open_in_memory().unwrap();
        let home = tempfile::tempdir().unwrap();
        let no_cli = FakeClaude::new(None);
        let s = setup(home.path(), &no_cli);
        let id = add(&db, &s, "codex", 1).unwrap().id.unwrap();
        let path = home.path().join(".codex/config.toml");
        fs::write(&path, "not = = toml").unwrap();
        assert!(matches!(remove(&db, &s, &id, 2), Err(AppError::Code("configUnreadable"))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "not = = toml");
        assert_eq!(list(&db.conn(), &[false; 4]).unwrap()[2].id.as_deref(), Some(id.as_str()));
    }
}
