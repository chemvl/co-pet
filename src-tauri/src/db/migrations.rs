//! Schema v1. Every id is a UUIDv7 string, every row has `updated_at`, deletes are soft.
use std::sync::LazyLock;

use rusqlite_migration::{Migrations, M};

const V1: &str = r#"
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE categories (
  id TEXT PRIMARY KEY,
  position INTEGER NOT NULL, exclude_from_ai INTEGER NOT NULL DEFAULT 0, exclude_from_mcp INTEGER NOT NULL DEFAULT 0,
  updated_at INTEGER NOT NULL);
CREATE TABLE items (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK (kind IN ('file','image','url','text','richtext','code','screenshot')),
  title TEXT NOT NULL, summary TEXT, tags TEXT NOT NULL DEFAULT '[]',
  payload_text TEXT, payload_html TEXT, url TEXT,
  file_path TEXT, file_name TEXT, mime TEXT, size_bytes INTEGER, thumb_path TEXT,
  source_app TEXT, source_bundle_id TEXT, source_kind TEXT NOT NULL CHECK (source_kind IN ('drop','clipboard','screenshot','note','selection')),
  extracted_text TEXT, extract_status TEXT NOT NULL DEFAULT 'pending' CHECK (extract_status IN ('pending','done','failed','skipped')),
  category_id TEXT NOT NULL DEFAULT 'unsorted' REFERENCES categories(id),
  triage_status TEXT NOT NULL DEFAULT 'pending' CHECK (triage_status IN ('pending','done')),
  triage_source TEXT CHECK (triage_source IN ('rule','ai','user','none')), triage_confidence REAL, triage_reason TEXT,
  suggestions TEXT NOT NULL DEFAULT '[]',
  sensitive INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'inbox' CHECK (status IN ('inbox','archived','trashed')),
  pinned INTEGER NOT NULL DEFAULT 0, content_hash TEXT NOT NULL,
  captured_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER);
CREATE INDEX items_list ON items(status, captured_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX items_cat ON items(category_id, captured_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX items_hash ON items(content_hash) WHERE deleted_at IS NULL;
CREATE VIRTUAL TABLE items_fts USING fts5(item_id UNINDEXED, body, tokenize = 'unicode61 remove_diacritics 2');
CREATE TABLE rules (
  id TEXT PRIMARY KEY, category_id TEXT NOT NULL REFERENCES categories(id),
  kind TEXT NOT NULL CHECK (kind IN ('domain','extension','item_kind','source_kind','regex')),
  pattern TEXT NOT NULL, builtin INTEGER NOT NULL DEFAULT 0, position INTEGER NOT NULL,
  updated_at INTEGER NOT NULL, deleted_at INTEGER);
CREATE TABLE category_examples (
  id TEXT PRIMARY KEY, item_id TEXT NOT NULL REFERENCES items(id), category_id TEXT NOT NULL,
  title TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER);
CREATE TABLE item_events (
  id TEXT PRIMARY KEY, item_id TEXT NOT NULL REFERENCES items(id), at INTEGER NOT NULL,
  actor TEXT NOT NULL CHECK (actor IN ('user','triage')),
  action TEXT NOT NULL CHECK (action IN ('captured','recategorize','pin','unpin','archive','unarchive','trash','restore')),
  before_json TEXT, after_json TEXT, undone_at INTEGER, updated_at INTEGER NOT NULL);

-- Seed: categories
INSERT INTO categories (id, position, updated_at) VALUES
  ('todo', 0, 0),
  ('read', 1, 0),
  ('reference', 2, 0),
  ('code', 3, 0),
  ('design', 4, 0),
  ('idea', 5, 0),
  ('contact', 6, 0),
  ('meeting', 7, 0),
  ('unsorted', 8, 0);
-- Seed: built-in rules, ascending priority by position
INSERT INTO rules (id, category_id, kind, pattern, builtin, position, updated_at) VALUES
  ('01a143b9-9c00-7001-9e37-79b97f4a7c15', 'idea', 'source_kind', 'note', 1, 0, 0),
  ('01a143b9-9c00-7002-bc6e-f372fe94f82a', 'meeting', 'extension', 'ics', 1, 1, 0),
  ('01a143b9-9c00-7003-9aa6-6d2c7ddf743f', 'contact', 'extension', 'vcf', 1, 2, 0),
  ('01a143b9-9c00-7004-b8dd-e6e5fd29f054', 'design', 'domain', 'figma.com', 1, 3, 0),
  ('01a143b9-9c00-7005-9715-609f7c746c69', 'design', 'domain', 'dribbble.com', 1, 4, 0),
  ('01a143b9-9c00-7006-b54c-da58fbbee87e', 'design', 'domain', 'behance.net', 1, 5, 0),
  ('01a143b9-9c00-7007-9384-54127b096493', 'code', 'domain', 'github.com', 1, 6, 0),
  ('01a143b9-9c00-7008-b1bb-cdcbfa53e0a8', 'code', 'domain', 'gitlab.com', 1, 7, 0),
  ('01a143b9-9c00-7009-8ff3-4785799e5cbd', 'code', 'domain', 'stackoverflow.com', 1, 8, 0),
  ('01a143b9-9c00-700a-ae2a-c13ef8e8d8d2', 'code', 'domain', 'codepen.io', 1, 9, 0),
  ('01a143b9-9c00-700b-8c62-3af8783354e7', 'code', 'domain', 'codesandbox.io', 1, 10, 0),
  ('01a143b9-9c00-700c-aa99-b4b1f77dd0fc', 'meeting', 'domain', 'zoom.us', 1, 11, 0),
  ('01a143b9-9c00-700d-88d1-2e6b76c84d11', 'meeting', 'domain', 'meet.google.com', 1, 12, 0),
  ('01a143b9-9c00-700e-a708-a824f612c926', 'meeting', 'domain', 'teams.microsoft.com', 1, 13, 0),
  ('01a143b9-9c00-700f-8540-21de755d453b', 'code', 'item_kind', 'code', 1, 14, 0),
  ('01a143b9-9c00-7010-a377-9b97f4a7c150', 'code', 'extension', 'ts', 1, 15, 0),
  ('01a143b9-9c00-7011-81af-155173f23d65', 'code', 'extension', 'tsx', 1, 16, 0),
  ('01a143b9-9c00-7012-9fe6-8f0af33cb97a', 'code', 'extension', 'js', 1, 17, 0),
  ('01a143b9-9c00-7013-be1e-08c47287358f', 'code', 'extension', 'jsx', 1, 18, 0),
  ('01a143b9-9c00-7014-9c55-827df1d1b1a4', 'code', 'extension', 'mjs', 1, 19, 0),
  ('01a143b9-9c00-7015-ba8c-fc37711c2db9', 'code', 'extension', 'rs', 1, 20, 0),
  ('01a143b9-9c00-7016-98c4-75f0f066a9ce', 'code', 'extension', 'py', 1, 21, 0),
  ('01a143b9-9c00-7017-b6fb-efaa6fb125e3', 'code', 'extension', 'go', 1, 22, 0),
  ('01a143b9-9c00-7018-9533-6963eefba1f8', 'code', 'extension', 'java', 1, 23, 0),
  ('01a143b9-9c00-7019-b36a-e31d6e461e0d', 'code', 'extension', 'kt', 1, 24, 0),
  ('01a143b9-9c00-701a-91a2-5cd6ed909a22', 'code', 'extension', 'swift', 1, 25, 0),
  ('01a143b9-9c00-701b-afd9-d6906cdb1637', 'code', 'extension', 'c', 1, 26, 0),
  ('01a143b9-9c00-701c-8e11-5049ec25924c', 'code', 'extension', 'h', 1, 27, 0),
  ('01a143b9-9c00-701d-ac48-ca036b700e61', 'code', 'extension', 'cpp', 1, 28, 0),
  ('01a143b9-9c00-701e-8a80-43bceaba8a76', 'code', 'extension', 'cs', 1, 29, 0),
  ('01a143b9-9c00-701f-a8b7-bd766a05068b', 'code', 'extension', 'rb', 1, 30, 0),
  ('01a143b9-9c00-7020-86ef-372fe94f82a0', 'code', 'extension', 'php', 1, 31, 0),
  ('01a143b9-9c00-7021-a526-b0e96899feb5', 'code', 'extension', 'css', 1, 32, 0),
  ('01a143b9-9c00-7022-835e-2aa2e7e47aca', 'code', 'extension', 'scss', 1, 33, 0),
  ('01a143b9-9c00-7023-a195-a45c672ef6df', 'code', 'extension', 'html', 1, 34, 0),
  ('01a143b9-9c00-7024-bfcd-1e15e67972f4', 'code', 'extension', 'json', 1, 35, 0),
  ('01a143b9-9c00-7025-9e04-97cf65c3ef09', 'code', 'extension', 'yaml', 1, 36, 0),
  ('01a143b9-9c00-7026-bc3c-1188e50e6b1e', 'code', 'extension', 'yml', 1, 37, 0),
  ('01a143b9-9c00-7027-9a73-8b426458e733', 'code', 'extension', 'toml', 1, 38, 0),
  ('01a143b9-9c00-7028-b8ab-04fbe3a36348', 'code', 'extension', 'sh', 1, 39, 0),
  ('01a143b9-9c00-7029-96e2-7eb562eddf5d', 'code', 'extension', 'sql', 1, 40, 0),
  ('01a143b9-9c00-702a-b519-f86ee2385b72', 'reference', 'extension', 'pdf', 1, 41, 0),
  ('01a143b9-9c00-702b-9351-72286182d787', 'reference', 'extension', 'doc', 1, 42, 0),
  ('01a143b9-9c00-702c-b188-ebe1e0cd539c', 'reference', 'extension', 'docx', 1, 43, 0),
  ('01a143b9-9c00-702d-8fc0-659b6017cfb1', 'reference', 'extension', 'ppt', 1, 44, 0),
  ('01a143b9-9c00-702e-adf7-df54df624bc6', 'reference', 'extension', 'pptx', 1, 45, 0),
  ('01a143b9-9c00-702f-8c2f-590e5eacc7db', 'reference', 'extension', 'xls', 1, 46, 0),
  ('01a143b9-9c00-7030-aa66-d2c7ddf743f0', 'reference', 'extension', 'xlsx', 1, 47, 0),
  ('01a143b9-9c00-7031-889e-4c815d41c005', 'reference', 'extension', 'odt', 1, 48, 0),
  ('01a143b9-9c00-7032-a6d5-c63adc8c3c1a', 'reference', 'extension', 'key', 1, 49, 0),
  ('01a143b9-9c00-7033-850d-3ff45bd6b82f', 'reference', 'extension', 'pages', 1, 50, 0),
  ('01a143b9-9c00-7034-a344-b9addb213444', 'reference', 'extension', 'numbers', 1, 51, 0),
  ('01a143b9-9c00-7035-817c-33675a6bb059', 'reference', 'extension', 'epub', 1, 52, 0),
  ('01a143b9-9c00-7036-9fb3-ad20d9b62c6e', 'design', 'extension', 'png', 1, 53, 0),
  ('01a143b9-9c00-7037-bdeb-26da5900a883', 'design', 'extension', 'jpg', 1, 54, 0),
  ('01a143b9-9c00-7038-9c22-a093d84b2498', 'design', 'extension', 'jpeg', 1, 55, 0),
  ('01a143b9-9c00-7039-ba5a-1a4d5795a0ad', 'design', 'extension', 'gif', 1, 56, 0),
  ('01a143b9-9c00-703a-9891-9406d6e01cc2', 'design', 'extension', 'webp', 1, 57, 0),
  ('01a143b9-9c00-703b-b6c9-0dc0562a98d7', 'design', 'extension', 'svg', 1, 58, 0),
  ('01a143b9-9c00-703c-9500-8779d57514ec', 'design', 'extension', 'heic', 1, 59, 0),
  ('01a143b9-9c00-703d-b338-013354bf9101', 'design', 'extension', 'fig', 1, 60, 0),
  ('01a143b9-9c00-703e-916f-7aecd40a0d16', 'design', 'extension', 'sketch', 1, 61, 0),
  ('01a143b9-9c00-703f-afa6-f4a65354892b', 'contact', 'regex', '^\s*(mailto:)?[\w.+-]+@[\w-]+\.[\w.-]+\s*$', 1, 62, 0),
  ('01a143b9-9c00-7040-8dde-6e5fd29f0540', 'contact', 'regex', '^\s*\+?[\d\s().-]{8,}\s*$', 1, 63, 0),
  ('01a143b9-9c00-7041-ac15-e81951e98155', 'read', 'item_kind', 'url', 1, 64, 0);
-- Seed: default settings (values are JSON)
INSERT INTO settings (key, value, updated_at) VALUES
  ('ui.language', '"system"', 0),
  ('pet.name', '"Miso"', 0),
  ('pet.style', '"soft"', 0),
  ('pet.skin', '"peach"', 0),
  ('pet.size', '80', 0),
  ('pet.positions', '{}', 0),
  ('pet.paused', 'false', 0),
  ('pet.hiddenUntil', 'null', 0),
  ('pet.sleepHours', '{"start":"23:00","end":"07:00"}', 0),
  ('capture.removeOriginal', 'false', 0),
  ('capture.excludedApps', '["com.1password.1password","com.agilebits.onepassword7","com.bitwarden.desktop","com.apple.keychainaccess","com.apple.Passwords","org.keepassxc.keepassxc","com.lastpass.LastPass","com.dashlane.dashlanephonefinal"]', 0),
  ('triage.confidenceThreshold', '0.6', 0),
  ('ai.enabled', 'true', 0),
  ('ai.modelId', 'null', 0),
  ('privacy.localOnly', 'false', 0),
  ('privacy.secretDetection', 'true', 0),
  ('general.openAtLogin', 'true', 0),
  ('hotkeys', '{"focusPet":"CommandOrControl+Shift+Space","sendSelection":"CommandOrControl+Shift+C","captureScreen":"CommandOrControl+Shift+S","quickNote":"CommandOrControl+Shift+N"}', 0),
  ('onboarding.completed', 'false', 0);
"#;

/// Schema v2: editable categories, agent attribution, task suggestions. `item_events` and
/// `rules` are rebuilt to widen their CHECKs (no table references them).
const V2: &str = r#"
ALTER TABLE categories ADD COLUMN name TEXT;      -- NULL: built-in i18n name `category.<id>`
ALTER TABLE categories ADD COLUMN icon TEXT;      -- NULL: icon named like the id; else a ds IconName
ALTER TABLE categories ADD COLUMN color TEXT;     -- NULL: colour of the id; else one of the nine built-in ids
ALTER TABLE categories ADD COLUMN deleted_at INTEGER;
ALTER TABLE items ADD COLUMN added_by TEXT;       -- mcp_clients.id of the agent that added it; NULL = the user
ALTER TABLE items ADD COLUMN task_suggestion TEXT; -- AI-suggested task title for actionable items; NULL = none
CREATE TABLE item_events_v2 (
  id TEXT PRIMARY KEY, item_id TEXT NOT NULL REFERENCES items(id), at INTEGER NOT NULL,
  actor TEXT NOT NULL CHECK (actor IN ('user','triage','agent')),
  client_id TEXT,
  action TEXT NOT NULL CHECK (action IN ('captured','recategorize','pin','unpin','archive','unarchive','trash','restore','tag')),
  before_json TEXT, after_json TEXT, undone_at INTEGER, updated_at INTEGER NOT NULL);
INSERT INTO item_events_v2 (id, item_id, at, actor, client_id, action, before_json, after_json, undone_at, updated_at)
  SELECT id, item_id, at, actor, NULL, action, before_json, after_json, undone_at, updated_at FROM item_events;
DROP TABLE item_events;
ALTER TABLE item_events_v2 RENAME TO item_events;
CREATE INDEX item_events_item ON item_events(item_id, at DESC);
CREATE TABLE rules_v2 (
  id TEXT PRIMARY KEY, category_id TEXT NOT NULL REFERENCES categories(id),
  kind TEXT NOT NULL CHECK (kind IN ('domain','extension','item_kind','source_kind','source_app','regex')),
  pattern TEXT NOT NULL, builtin INTEGER NOT NULL DEFAULT 0, position INTEGER NOT NULL,
  updated_at INTEGER NOT NULL, deleted_at INTEGER);
INSERT INTO rules_v2 (id, category_id, kind, pattern, builtin, position, updated_at, deleted_at)
  SELECT id, category_id, kind, pattern, builtin, position, updated_at, deleted_at FROM rules;
DROP TABLE rules;
ALTER TABLE rules_v2 RENAME TO rules;
"#;

/// Schema v3: local tasks and their history.
const V3: &str = r#"
CREATE TABLE tasks (
  id TEXT PRIMARY KEY, title TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','done')),
  due_date TEXT,                 -- 'YYYY-MM-DD' local calendar date
  due_time TEXT,                 -- 'HH:MM' local; NULL = any time that day
  priority INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 3),
  estimate_min INTEGER,
  first_step TEXT,
  source_item_id TEXT REFERENCES items(id),
  added_by TEXT,                 -- mcp_clients.id; NULL = the user
  snoozed_until INTEGER, done_at INTEGER,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER);
CREATE INDEX tasks_open ON tasks(status, due_date) WHERE deleted_at IS NULL;
CREATE INDEX tasks_source ON tasks(source_item_id) WHERE deleted_at IS NULL;
CREATE TABLE task_events (
  id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES tasks(id), at INTEGER NOT NULL,
  actor TEXT NOT NULL CHECK (actor IN ('user','agent','next')), client_id TEXT,
  action TEXT NOT NULL CHECK (action IN ('created','edited','done','reopened','snoozed','trashed','restored')),
  before_json TEXT, after_json TEXT, undone_at INTEGER, updated_at INTEGER NOT NULL);
CREATE INDEX task_events_task ON task_events(task_id, at DESC);
"#;

pub static MIGRATIONS: LazyLock<Migrations<'static>> =
    LazyLock::new(|| Migrations::new(vec![M::up(V1), M::up(V2), M::up(V3)]));
