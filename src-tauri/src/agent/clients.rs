//! MCP client identity. HTTP clients prove a bearer token; stdio clients are known by their key
//! and get a row (writes not approved) on first contact.
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::db::new_id;
use crate::error::{AppError, AppResult};

pub const KEY_MAX: usize = 80;
pub const NAME_MAX: usize = 100;

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
