//! What the server stores: rows sealed with the record key under `"<tbl>\0<id>"` (so a payload
//! can't stand in for another row), files sealed with the blob key under their library path, and
//! file names as an HMAC of that path (so the server can't match known files). The keys come
//! from the master key through HKDF.
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::Sha256;

use crate::account::crypto::{hkdf32, open, seal};
use crate::db;
use crate::error::{AppError, AppResult};

/// Version of the sealed row JSON (`{"v":1,"row":{…}}`).
const ROW_VERSION: u64 = 1;

/// The keys derived from the master key `mk`.
pub(crate) struct Keys {
    pub record: [u8; 32],
    pub blob: [u8; 32],
    pub name: [u8; 32],
}

impl Keys {
    pub fn derive(mk: &[u8; 32]) -> Keys {
        Keys {
            record: hkdf32(mk, b"", "copet records v1"),
            blob: hkdf32(mk, b"", "copet blobs v1"),
            name: hkdf32(mk, b"", "copet blob names v1"),
        }
    }
}

#[derive(Serialize)]
struct SealedRow<'a> {
    v: u64,
    row: &'a Map<String, Value>,
}

#[derive(Deserialize)]
struct OpenedRow {
    v: u64,
    row: Map<String, Value>,
}

fn row_aad(tbl: &str, id: &str) -> Vec<u8> {
    format!("{tbl}\0{id}").into_bytes()
}

/// `base64(seal(record_key, "<tbl>\0<id>", {"v":1,"row":row}))`.
pub(crate) fn seal_row(record_key: &[u8; 32], tbl: &str, id: &str, row: &Map<String, Value>) -> String {
    let plain = serde_json::to_vec(&SealedRow { v: ROW_VERSION, row }).expect("a JSON object always serializes");
    STANDARD.encode(seal(record_key, &row_aad(tbl, id), &plain))
}

/// Reverses [`seal_row`]; `decrypt` for a wrong key, another row's payload or an unknown version.
pub(crate) fn open_row(record_key: &[u8; 32], tbl: &str, id: &str, payload: &str) -> AppResult<Map<String, Value>> {
    let sealed = STANDARD.decode(payload).map_err(|_| AppError::Code("decrypt"))?;
    let plain = open(record_key, &row_aad(tbl, id), &sealed)?;
    let opened: OpenedRow = serde_json::from_slice(&plain).map_err(|_| AppError::Code("decrypt"))?;
    if opened.v != ROW_VERSION {
        return Err(AppError::Code("decrypt"));
    }
    Ok(opened.row)
}

/// The object name of library file `rel`: `hex(HMAC-SHA256(name_key, rel))`.
pub(crate) fn blob_name(name_key: &[u8; 32], rel: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(name_key).expect("HMAC takes a key of any length");
    mac.update(rel.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Every column of the row whose `key` column is `id` (`SELECT * FROM <tbl> …`); `None` when
/// there is no such row. `tbl` and `key` must come from `SYNC_TABLES`.
pub(crate) fn read_row(conn: &Connection, tbl: &str, key: &str, id: &str) -> AppResult<Option<Map<String, Value>>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT * FROM {tbl} WHERE {key} = ?1"))?;
    let names: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();
    let mut rows = stmt.query([id])?;
    rows.next()?.map(|row| db::row_to_json(row, &names)).transpose()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::db::Db;

    #[test]
    fn rows_open_only_with_their_key_and_place() {
        let keys = Keys::derive(&[1; 32]);
        let row = json!({ "id": "t1", "title": "Ship it", "updated_at": 5 }).as_object().unwrap().clone();
        let payload = seal_row(&keys.record, "tasks", "t1", &row);
        assert_eq!(open_row(&keys.record, "tasks", "t1", &payload).unwrap(), row);
        assert!(!String::from_utf8_lossy(&STANDARD.decode(&payload).unwrap()).contains("Ship it"));
        let other = Keys::derive(&[2; 32]);
        for (key, tbl, id) in [(&other.record, "tasks", "t1"), (&keys.record, "tasks", "t2"), (&keys.record, "items", "t1")] {
            assert!(matches!(open_row(key, tbl, id, &payload), Err(AppError::Code("decrypt"))));
        }
        assert!(matches!(open_row(&keys.record, "tasks", "t1", "not base64!"), Err(AppError::Code("decrypt"))));
    }

    #[test]
    fn keys_and_blob_names_are_distinct_and_stable() {
        let keys = Keys::derive(&[1; 32]);
        assert!(keys.record != keys.blob && keys.blob != keys.name && keys.record != keys.name);
        let name = blob_name(&keys.name, "ab/abcd.pdf");
        assert_eq!(name.len(), 64);
        assert!(name.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(name, blob_name(&Keys::derive(&[1; 32]).name, "ab/abcd.pdf"));
        assert_ne!(name, blob_name(&keys.name, "ab/abce.pdf"));
        assert_ne!(name, blob_name(&Keys::derive(&[2; 32]).name, "ab/abcd.pdf"));
    }

    #[test]
    fn read_row_returns_every_column() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        let row = read_row(&c, "settings", "key", "pet.name").unwrap().unwrap();
        assert_eq!(row["key"], "pet.name");
        assert_eq!(row["value"], "\"Miso\"");
        assert_eq!(row["updated_at"], 0);
        assert!(read_row(&c, "settings", "key", "nope").unwrap().is_none());
    }
}
