//! The scene's server-side storage: what `@dcl/sdk/server`'s `Storage` and
//! `EnvVar` read and write. One SQLite file per project at
//! `.dcl-one/storage.sqlite`, opened per request by the preview server, the
//! `storage` CLI and the host isolate's HTTP client alike, so no process
//! holds a stale copy and two writers queue on SQLite's own lock instead of
//! resurrecting each other's deleted keys.
//!
//! The same file remembers where the preview points storage at
//! ([`Target`]): this database, the public storage service, or a custom
//! service URL. A remote target turns the preview into a signing proxy
//! (`storage_remote`); the tables here are then untouched.

use crate::ux::{TrySteps, UserError};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DB_FILE: &str = "storage.sqlite";
/// The pre-0.26 host's JSON store, imported once into a fresh database.
pub const LEGACY_FILE: &str = "storage.json";
/// Longest key accepted on any route. Upstream's preview accepts anything
/// non-empty; a bound keeps a runaway scene from growing the index.
pub const MAX_KEY: usize = 255;
/// The production service's per-value ceilings (its `.env.default`):
/// a scene value, a player value, an environment variable.
pub const MAX_WORLD_VALUE_BYTES: usize = 512 * 1024;
pub const MAX_PLAYER_VALUE_BYTES: usize = 100 * 1024;
pub const MAX_ENV_VALUE_BYTES: usize = 10 * 1024;
/// And its totals: per world, per player in a world, per world's env.
pub const MAX_WORLD_TOTAL_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_PLAYER_TOTAL_BYTES: usize = 1024 * 1024;
pub const MAX_ENV_TOTAL_BYTES: usize = 256 * 1024;
/// The most keys one page lists, and how many a page lists unasked.
pub const MAX_LIMIT: usize = 100;
/// Writes the activity log keeps; older rows are trimmed on every write.
pub const ACTIVITY_KEEP: usize = 200;

pub const ORG_URL: &str = "https://storage.decentraland.org";
pub const ZONE_URL: &str = "https://storage.decentraland.zone";

/// A snapshot of every table, in upstream's `server-storage.json` shape
/// (`env`, `world`, `players`): what export writes, import reads, and the
/// legacy JSON file is parsed into.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Store {
    /// Runtime environment overrides; `.env` fills in what is absent.
    pub env: BTreeMap<String, String>,
    /// Scene-scoped values (`Storage.get/set`).
    pub world: BTreeMap<String, Value>,
    /// Player-scoped values (`Storage.player.get/set`), keyed by lowercase
    /// wallet address.
    pub players: BTreeMap<String, BTreeMap<String, Value>>,
}

impl Store {
    /// Parses a JSON snapshot, accepting the pre-0.26 host's `{ world, player }`
    /// shape (string values, singular player map) alongside upstream's.
    pub fn from_value(mut value: Value) -> Result<Store> {
        let Some(map) = value.as_object_mut() else {
            anyhow::bail!("the storage snapshot is not a JSON object");
        };
        if !map.contains_key("players") {
            if let Some(legacy) = map.remove("player") {
                map.insert("players".to_string(), legacy);
            }
        }
        if let Some(env) = map.get_mut("env").and_then(Value::as_object_mut) {
            for v in env.values_mut() {
                if !v.is_string() {
                    *v = Value::String(env_text(v));
                }
            }
        }
        let mut store: Store = serde_json::from_value(value)
            .context("the storage snapshot has an unexpected shape")?;
        let players = std::mem::take(&mut store.players);
        for (address, values) in players {
            store
                .players
                .entry(address.to_lowercase())
                .or_default()
                .extend(values);
        }
        Ok(store)
    }

    pub fn is_empty(&self) -> bool {
        self.env.is_empty() && self.world.is_empty() && self.players.is_empty()
    }
}

/// The value as `EnvVar.get` answers it: strings verbatim, anything else as
/// its JSON text.
fn env_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Where the preview sends storage traffic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// This project's SQLite file.
    Local,
    /// The public storage service, mainnet.
    Org,
    /// The public storage service, testnet.
    Zone,
    /// Any service serving the same routes: a self-hosted catalyrst stack, say.
    Custom(String),
}

impl Target {
    /// `local`, `org`, `zone`, or an http(s) URL.
    pub fn parse(raw: &str) -> Result<Target, String> {
        let raw = raw.trim();
        match raw.to_ascii_lowercase().as_str() {
            "" | "local" | "sqlite" => return Ok(Target::Local),
            "org" | "decentraland.org" => return Ok(Target::Org),
            "zone" | "decentraland.zone" => return Ok(Target::Zone),
            _ => {}
        }
        if raw == ORG_URL {
            return Ok(Target::Org);
        }
        if raw == ZONE_URL {
            return Ok(Target::Zone);
        }
        let parsed = url::Url::parse(raw).map_err(|_| {
            format!("{raw} is not a storage target: expected local, org, zone or an http(s) URL")
        })?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(format!("{raw} is not an http(s) URL"));
        }
        Ok(Target::Custom(raw.trim_end_matches('/').to_string()))
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Target::Local => "local",
            Target::Org => "org",
            Target::Zone => "zone",
            Target::Custom(_) => "custom",
        }
    }

    /// The service base URL; `None` for the local database.
    pub fn url(&self) -> Option<&str> {
        match self {
            Target::Local => None,
            Target::Org => Some(ORG_URL),
            Target::Zone => Some(ZONE_URL),
            Target::Custom(url) => Some(url),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Target::Local)
    }

    pub fn label(&self) -> String {
        match self {
            Target::Local => "local SQLite".to_string(),
            Target::Org => "storage.decentraland.org".to_string(),
            Target::Zone => "storage.decentraland.zone".to_string(),
            Target::Custom(url) => url.clone(),
        }
    }

    /// The spelling `Target::parse` reads back.
    pub fn to_arg(&self) -> String {
        match self {
            Target::Custom(url) => url.clone(),
            other => other.kind().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvSource {
    Runtime,
    DotEnv,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvEntry {
    pub key: String,
    pub value: String,
    pub source: EnvSource,
}

/// One stored value and who last wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub value: Value,
    /// Unix milliseconds.
    pub updated_at: i64,
    /// `scene`, `cli`, `ui`, `import`… whatever the writer declared.
    pub source: String,
}

/// One page of a listing, in upstream's `{ data, pagination }` shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub data: Vec<Entry>,
    pub limit: usize,
    pub offset: usize,
    pub total: usize,
}

impl Page {
    /// The wire shape, and nothing the production service would not send:
    /// a scene reading `getValues()` here sees what it sees there.
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "data": self.data.iter().map(|e| serde_json::json!({ "key": e.key, "value": e.value })).collect::<Vec<_>>(),
            "pagination": { "limit": self.limit, "offset": self.offset, "total": self.total }
        })
    }
}

/// One write, as the activity log remembers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub at: i64,
    pub scope: String,
    pub address: String,
    pub key: String,
    pub op: String,
    pub source: String,
}

/// Which namespace a key lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope<'a> {
    Scene,
    Player(&'a str),
    Env,
}

impl Scope<'_> {
    pub fn name(&self) -> &'static str {
        match self {
            Scope::Scene => "scene",
            Scope::Player(_) => "player",
            Scope::Env => "env",
        }
    }

    /// The row's address column: the player's address lowercased, or empty.
    pub fn address(&self) -> String {
        match self {
            Scope::Player(address) => address.to_lowercase(),
            _ => String::new(),
        }
    }

    /// The service's ceilings for the scope: per value, then in total.
    pub fn limits(&self) -> (usize, usize) {
        match self {
            Scope::Scene => (MAX_WORLD_VALUE_BYTES, MAX_WORLD_TOTAL_BYTES),
            Scope::Player(_) => (MAX_PLAYER_VALUE_BYTES, MAX_PLAYER_TOTAL_BYTES),
            Scope::Env => (MAX_ENV_VALUE_BYTES, MAX_ENV_TOTAL_BYTES),
        }
    }
}

/// A `limit` that lists everything: what export and the env merge pass.
pub const ALL: usize = usize::MAX;

/// SQLite's `LIMIT`: at least one row, or `-1` for all of them.
fn sql_limit(limit: usize) -> i64 {
    if limit == ALL {
        -1
    } else {
        limit.clamp(1, i64::MAX as usize) as i64
    }
}

pub fn path(root: &Path) -> PathBuf {
    root.join(".dcl-one").join(DB_FILE)
}

pub fn legacy_path(root: &Path) -> PathBuf {
    root.join(".dcl-one").join(LEGACY_FILE)
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS kv (
    scope TEXT NOT NULL,
    address TEXT NOT NULL DEFAULT '',
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    source TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (scope, address, key)
);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at INTEGER NOT NULL,
    scope TEXT NOT NULL,
    address TEXT NOT NULL DEFAULT '',
    key TEXT NOT NULL,
    op TEXT NOT NULL,
    source TEXT NOT NULL
);
";

/// One open connection. Cheap to open, so callers open one per request or
/// per command and let it drop. The mutex only makes `&Db` shareable across
/// an await (a connection is Send but not Sync); nothing holds it long.
pub struct Db {
    conn: std::sync::Mutex<Connection>,
}

/// Opens (creating on first use) the project's storage database. A fresh
/// database absorbs the legacy `storage.json` if one is there.
pub fn open(root: &Path) -> Result<Db> {
    let dir = crate::scene::work_dir(root)
        .with_context(|| format!("creating {}", root.join(".dcl-one").display()))?;
    let file = dir.join(DB_FILE);
    let fresh = !file.exists();
    let conn = Connection::open(&file).map_err(|e| {
        UserError::new(
            format!("could not open {}", file.display()),
            TrySteps::one("check the file is writable")
                .and("if it is corrupt, move it aside; the preview recreates an empty one"),
        )
        .why(e.to_string())
    })?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("setting the storage busy timeout")?;
    // WAL lets the preview answer reads while the CLI writes; a database that
    // cannot switch (a network mount, say) still works on the rollback journal.
    let _ = conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()));
    conn.execute_batch(SCHEMA)
        .with_context(|| format!("preparing {}", file.display()))?;
    let db = Db {
        conn: std::sync::Mutex::new(conn),
    };
    if fresh {
        db.import_legacy(root)?;
    }
    Ok(db)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn decode(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}

impl Db {
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn import_legacy(&self, root: &Path) -> Result<()> {
        let legacy = legacy_path(root);
        let Ok(text) = std::fs::read_to_string(&legacy) else {
            return Ok(());
        };
        if text.trim().is_empty() {
            return Ok(());
        }
        let store = serde_json::from_str::<Value>(&text)
            .map_err(anyhow::Error::new)
            .and_then(Store::from_value)
            .map_err(|e| {
                UserError::new(
                    format!("{} could not be imported", legacy.display()),
                    TrySteps::one("fix the JSON or move the file aside, then retry"),
                )
                .why(e.to_string())
            })?;
        self.import(&store, false, "import")?;
        let kept = legacy.with_extension("json.imported");
        std::fs::rename(&legacy, &kept)
            .with_context(|| format!("renaming {} after importing it", legacy.display()))?;
        Ok(())
    }

    pub fn get(&self, scope: Scope<'_>, key: &str) -> Result<Option<Value>> {
        let text: Option<String> = self
            .conn()
            .query_row(
                "SELECT value FROM kv WHERE scope = ?1 AND address = ?2 AND key = ?3",
                params![scope.name(), scope.address(), key],
                |row| row.get(0),
            )
            .optional()
            .context("reading a storage value")?;
        Ok(text.as_deref().map(decode))
    }

    pub fn set(&self, scope: Scope<'_>, key: &str, value: &Value, source: &str) -> Result<()> {
        let now = now_ms();
        self.conn()
            .execute(
                "INSERT INTO kv (scope, address, key, value, updated_at, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (scope, address, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at, source = excluded.source",
                params![scope.name(), scope.address(), key, value.to_string(), now, source],
            )
            .context("writing a storage value")?;
        self.log(now, scope, key, "set", source)
    }

    /// True when the key existed.
    pub fn delete(&self, scope: Scope<'_>, key: &str, source: &str) -> Result<bool> {
        let removed = self
            .conn()
            .execute(
                "DELETE FROM kv WHERE scope = ?1 AND address = ?2 AND key = ?3",
                params![scope.name(), scope.address(), key],
            )
            .context("deleting a storage value")?;
        if removed > 0 {
            self.log(now_ms(), scope, key, "delete", source)?;
        }
        Ok(removed > 0)
    }

    /// The service's list semantics: filter by prefix, count, then `offset`
    /// skipped and `limit` taken; see [`page_limit`] for the bounds.
    pub fn list(
        &self,
        scope: Scope<'_>,
        prefix: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Page> {
        let prefix = prefix.unwrap_or_default();
        let address = scope.address();
        let total: i64 = self
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM kv WHERE scope = ?1 AND address = ?2 AND (?3 = '' OR substr(key, 1, length(?3)) = ?3)",
                params![scope.name(), address, prefix],
                |row| row.get(0),
            )
            .context("counting storage values")?;
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT key, value, updated_at, source FROM kv WHERE scope = ?1 AND address = ?2 AND (?3 = '' OR substr(key, 1, length(?3)) = ?3)
                 ORDER BY key LIMIT ?4 OFFSET ?5",
            )
            .context("listing storage values")?;
        let data = stmt
            .query_map(
                params![
                    scope.name(),
                    address,
                    prefix,
                    sql_limit(limit),
                    offset as i64
                ],
                |row| {
                    Ok(Entry {
                        key: row.get(0)?,
                        value: decode(&row.get::<_, String>(1)?),
                        updated_at: row.get(2)?,
                        source: row.get(3)?,
                    })
                },
            )
            .context("listing storage values")?
            .collect::<Result<Vec<_>, _>>()
            .context("reading storage values")?;
        Ok(Page {
            data,
            limit,
            offset,
            total: total.max(0) as usize,
        })
    }

    /// How many bytes the scope's values take, as the service counts them:
    /// the JSON text of every value.
    pub fn usage(&self, scope: Scope<'_>) -> Result<usize> {
        let used: i64 = self
            .conn()
            .query_row(
                "SELECT COALESCE(SUM(length(CAST(value AS BLOB))), 0) FROM kv WHERE scope = ?1 AND address = ?2",
                params![scope.name(), scope.address()],
                |row| row.get(0),
            )
            .context("measuring storage usage")?;
        Ok(used.max(0) as usize)
    }

    /// The service's total-size rule: the scope's bytes with `key` replaced
    /// by `size` more must stay within the scope's ceiling.
    pub fn check_fits(
        &self,
        scope: Scope<'_>,
        key: &str,
        size: usize,
    ) -> Result<Result<(), String>> {
        let existing: i64 = self
            .conn()
            .query_row(
                "SELECT COALESCE(length(CAST(value AS BLOB)), 0) FROM kv WHERE scope = ?1 AND address = ?2 AND key = ?3",
                params![scope.name(), scope.address(), key],
                |row| row.get(0),
            )
            .optional()
            .context("measuring a storage value")?
            .unwrap_or(0);
        let used = self.usage(scope)?;
        let (_, max_total) = scope.limits();
        let projected = used - (existing.max(0) as usize).min(used) + size;
        Ok(if projected > max_total {
            Err(format!(
                "Total storage size would exceed the maximum allowed ({max_total} bytes). Current usage: {used} bytes. Delete existing data to free up space"
            ))
        } else {
            Ok(())
        })
    }

    /// How many keys the scope held.
    pub fn clear(&self, scope: Scope<'_>, source: &str) -> Result<usize> {
        let removed = self
            .conn()
            .execute(
                "DELETE FROM kv WHERE scope = ?1 AND address = ?2",
                params![scope.name(), scope.address()],
            )
            .context("clearing storage values")?;
        if removed > 0 {
            self.log(now_ms(), scope, "*", "clear", source)?;
        }
        Ok(removed)
    }

    /// `DELETE /players`: every player's values at once. How many rows went.
    pub fn clear_all_players(&self, source: &str) -> Result<usize> {
        let removed = self
            .conn()
            .execute("DELETE FROM kv WHERE scope = 'player'", [])
            .context("clearing player storage")?;
        if removed > 0 {
            let now = now_ms();
            self.conn()
                .execute(
                    "INSERT INTO activity (at, scope, address, key, op, source) VALUES (?1, 'players', '', '*', 'clear', ?2)",
                    params![now, source],
                )
                .context("logging a storage write")?;
        }
        Ok(removed)
    }

    /// `GET /players`: the addresses with values, alphabetically, one page
    /// at a time, and how many there are.
    pub fn player_addresses(&self, limit: usize, offset: usize) -> Result<(Vec<String>, usize)> {
        let conn = self.conn();
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT address) FROM kv WHERE scope = 'player'",
                [],
                |row| row.get(0),
            )
            .context("counting players")?;
        let mut stmt = conn
            .prepare("SELECT DISTINCT address FROM kv WHERE scope = 'player' ORDER BY address LIMIT ?1 OFFSET ?2")
            .context("listing players")?;
        let rows = stmt
            .query_map(params![sql_limit(limit), offset as i64], |row| {
                row.get::<_, String>(0)
            })
            .context("listing players")?
            .collect::<Result<Vec<_>, _>>()
            .context("reading players")?;
        Ok((rows, total.max(0) as usize))
    }

    /// Every address with player values, and how many each holds.
    pub fn players(&self) -> Result<Vec<(String, usize)>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT address, COUNT(*) FROM kv WHERE scope = 'player' GROUP BY address ORDER BY address")
            .context("listing players")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })
            .context("listing players")?
            .collect::<Result<Vec<_>, _>>()
            .context("reading players")?;
        Ok(rows)
    }

    /// The runtime override, if any; `.env` is layered on by [`env_value`].
    pub fn env_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.get(Scope::Env, key)?.map(|v| env_text(&v)))
    }

    pub fn env_set(&self, key: &str, value: &str, source: &str) -> Result<()> {
        self.set(Scope::Env, key, &Value::String(value.to_string()), source)
    }

    pub fn env_runtime(&self) -> Result<BTreeMap<String, String>> {
        Ok(self
            .list(Scope::Env, None, ALL, 0)?
            .data
            .into_iter()
            .map(|e| (e.key, env_text(&e.value)))
            .collect())
    }

    /// `(scene, player, env)` key counts.
    pub fn counts(&self) -> Result<(usize, usize, usize)> {
        let count = |scope: &str| -> Result<usize> {
            let n: i64 = self
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM kv WHERE scope = ?1",
                    params![scope],
                    |row| row.get(0),
                )
                .context("counting storage values")?;
            Ok(n.max(0) as usize)
        };
        Ok((count("scene")?, count("player")?, count("env")?))
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        self.conn()
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .context("reading a storage setting")
    }

    pub fn set_setting(&self, key: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(value) => self.conn().execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            ),
            None => self
                .conn()
                .execute("DELETE FROM settings WHERE key = ?1", params![key]),
        }
        .context("writing a storage setting")?;
        Ok(())
    }

    /// Where the preview points storage at; the local database until told
    /// otherwise, and again if the remembered spelling stopped parsing.
    pub fn target(&self) -> Result<Target> {
        Ok(self
            .setting("target")?
            .and_then(|raw| Target::parse(&raw).ok())
            .unwrap_or(Target::Local))
    }

    pub fn set_target(&self, target: &Target) -> Result<()> {
        self.set_setting("target", Some(&target.to_arg()))
    }

    fn log(&self, at: i64, scope: Scope<'_>, key: &str, op: &str, source: &str) -> Result<()> {
        self.conn()
            .execute(
                "INSERT INTO activity (at, scope, address, key, op, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![at, scope.name(), scope.address(), key, op, source],
            )
            .context("logging a storage write")?;
        self.conn()
            .execute(
                "DELETE FROM activity WHERE id <= (SELECT id FROM activity ORDER BY id DESC LIMIT 1 OFFSET ?1)",
                params![ACTIVITY_KEEP as i64],
            )
            .context("trimming the storage log")?;
        Ok(())
    }

    /// The newest writes first.
    pub fn activity(&self, limit: usize) -> Result<Vec<Activity>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT at, scope, address, key, op, source FROM activity ORDER BY id DESC LIMIT ?1")
            .context("reading the storage log")?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(Activity {
                    at: row.get(0)?,
                    scope: row.get(1)?,
                    address: row.get(2)?,
                    key: row.get(3)?,
                    op: row.get(4)?,
                    source: row.get(5)?,
                })
            })
            .context("reading the storage log")?
            .collect::<Result<Vec<_>, _>>()
            .context("reading the storage log")?;
        Ok(rows)
    }

    pub fn export(&self) -> Result<Store> {
        let mut store = Store {
            env: self.env_runtime()?,
            ..Store::default()
        };
        for e in self.list(Scope::Scene, None, ALL, 0)?.data {
            store.world.insert(e.key, e.value);
        }
        for (address, _) in self.players()? {
            let values = self.list(Scope::Player(&address), None, ALL, 0)?.data;
            store.players.insert(
                address,
                values.into_iter().map(|e| (e.key, e.value)).collect(),
            );
        }
        Ok(store)
    }

    /// Replaces every table with the snapshot, or with `merge` lays it over
    /// what is there (the snapshot's keys win). One transaction either way.
    pub fn import(&self, store: &Store, merge: bool, source: &str) -> Result<()> {
        let conn = self.conn();
        let tx = conn
            .unchecked_transaction()
            .context("starting the import")?;
        if !merge {
            tx.execute("DELETE FROM kv", [])
                .context("clearing the tables before the import")?;
        }
        let now = now_ms();
        let mut upsert = tx
            .prepare(
                "INSERT INTO kv (scope, address, key, value, updated_at, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (scope, address, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at, source = excluded.source",
            )
            .context("preparing the import")?;
        for (key, value) in &store.env {
            upsert
                .execute(params![
                    "env",
                    "",
                    key,
                    Value::String(value.clone()).to_string(),
                    now,
                    source
                ])
                .context("importing an environment variable")?;
        }
        for (key, value) in &store.world {
            upsert
                .execute(params!["scene", "", key, value.to_string(), now, source])
                .context("importing a scene value")?;
        }
        for (address, values) in &store.players {
            for (key, value) in values {
                upsert
                    .execute(params![
                        "player",
                        address.to_lowercase(),
                        key,
                        value.to_string(),
                        now,
                        source
                    ])
                    .context("importing a player value")?;
            }
        }
        drop(upsert);
        tx.execute(
            "INSERT INTO activity (at, scope, address, key, op, source) VALUES (?1, 'all', '', '*', ?2, ?3)",
            params![now, if merge { "merge" } else { "import" }, source],
        )
        .context("logging the import")?;
        tx.commit().context("committing the import")
    }
}

/// `KEY=VALUE` lines of the scene's `.env`, the file upstream's preview
/// reads: blank lines and `#` comments skipped, an `export ` prefix
/// tolerated, one layer of matching quotes stripped.
pub fn parse_dotenv(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        let value = match value.as_bytes() {
            [q, .., e] if (*q == b'"' || *q == b'\'') && q == e => &value[1..value.len() - 1],
            _ => value,
        };
        out.insert(key.to_string(), value.to_string());
    }
    out
}

pub fn dotenv(root: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(root.join(".env"))
        .map(|text| parse_dotenv(&text))
        .unwrap_or_default()
}

/// The runtime override wins over `.env`, as upstream merges them.
pub fn env_value(root: &Path, db: &Db, key: &str) -> Result<Option<String>> {
    Ok(db.env_get(key)?.or_else(|| dotenv(root).remove(key)))
}

/// Every variable a scene could read, each tagged with where it comes from.
pub fn env_entries(root: &Path, db: &Db) -> Result<Vec<EnvEntry>> {
    let mut merged: BTreeMap<String, EnvEntry> = dotenv(root)
        .into_iter()
        .map(|(key, value)| {
            let entry = EnvEntry {
                key: key.clone(),
                value,
                source: EnvSource::DotEnv,
            };
            (key, entry)
        })
        .collect();
    for (key, value) in db.env_runtime()? {
        merged.insert(
            key.clone(),
            EnvEntry {
                key,
                value,
                source: EnvSource::Runtime,
            },
        );
    }
    Ok(merged.into_values().collect())
}

/// The service's key rule (1 to 255 characters), in its words. NUL never
/// reaches a database there either; here it is refused up front.
pub fn check_key(key: &str) -> Result<(), String> {
    let chars = key.chars().count();
    if chars == 0 || chars > MAX_KEY {
        return Err(format!("Key must be between 1 and {MAX_KEY} characters"));
    }
    if key.contains('\0') {
        return Err("Key must not contain the \\u0000 (NUL) character".to_string());
    }
    Ok(())
}

/// A wallet address, lowercased so one spelling names the player, and
/// refused in the service's words when it is not one.
pub fn normalize_address(address: &str) -> Result<String, String> {
    let address = address.trim();
    let hex = address
        .strip_prefix("0x")
        .or_else(|| address.strip_prefix("0X"));
    match hex {
        Some(hex) if hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(address.to_ascii_lowercase())
        }
        _ => Err("Invalid player address".to_string()),
    }
}

/// The value's size as the service measures it: its JSON text in bytes.
pub fn value_size(value: &Value) -> usize {
    value.to_string().len()
}

/// The service's per-value rules for the scope, in its words: a size
/// ceiling, and no NUL (its database cannot hold one).
pub fn check_value(scope: Scope<'_>, value: &Value) -> Result<(), String> {
    let (max_value, _) = scope.limits();
    let size = value_size(value);
    if size > max_value {
        return Err(format!(
            "Value size ({size} bytes) exceeds the maximum allowed size ({max_value} bytes)"
        ));
    }
    if value.to_string().contains("\\u0000") {
        return Err("Values must not contain the \\u0000 (NUL) character".to_string());
    }
    Ok(())
}

/// `limit` as the service reads it: 1 to [`MAX_LIMIT`], the maximum when
/// absent or out of range.
pub fn page_limit(requested: Option<usize>) -> usize {
    match requested {
        Some(n) if n > 0 && n <= MAX_LIMIT => n,
        _ => MAX_LIMIT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dcl-one-sdk-storage-{tag}-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_fresh_project_opens_an_empty_ignored_database() {
        let root = tmp("fresh");
        let db = open(&root).unwrap();
        assert_eq!(db.get(Scope::Scene, "nothing").unwrap(), None);
        assert_eq!(db.counts().unwrap(), (0, 0, 0));
        assert_eq!(db.target().unwrap(), Target::Local);
        assert!(path(&root).is_file());
        assert!(root.join(".dcl-one/.gitignore").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn values_round_trip_as_json_and_deletes_report_existence() {
        let root = tmp("roundtrip");
        let db = open(&root).unwrap();
        db.set(
            Scope::Scene,
            "obj",
            &json!({ "a": [1, true, null] }),
            "scene",
        )
        .unwrap();
        db.set(Scope::Scene, "n", &json!(7), "cli").unwrap();
        assert_eq!(
            db.get(Scope::Scene, "obj").unwrap(),
            Some(json!({ "a": [1, true, null] }))
        );
        assert_eq!(db.get(Scope::Scene, "n").unwrap(), Some(json!(7)));
        // a second open sees the same rows: nothing lives in memory
        let again = open(&root).unwrap();
        assert_eq!(again.get(Scope::Scene, "n").unwrap(), Some(json!(7)));
        assert!(again.delete(Scope::Scene, "n", "ui").unwrap());
        assert!(!again.delete(Scope::Scene, "n", "ui").unwrap());
        assert_eq!(db.get(Scope::Scene, "n").unwrap(), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn listing_filters_by_prefix_then_pages_and_remembers_the_writer() {
        let root = tmp("paging");
        let db = open(&root).unwrap();
        for (k, v) in [("a1", 1), ("a2", 2), ("a3", 3), ("b1", 4)] {
            db.set(Scope::Scene, k, &json!(v), "scene").unwrap();
        }
        let page = db.list(Scope::Scene, Some("a"), 2, 1).unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.offset, 1);
        assert_eq!(
            page.data.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            ["a2", "a3"]
        );
        assert_eq!(page.data[0].source, "scene");
        assert!(page.data[0].updated_at > 0);
        assert_eq!(
            page.to_json(),
            json!({ "data": [{ "key": "a2", "value": 2 }, { "key": "a3", "value": 3 }], "pagination": { "limit": 2, "offset": 1, "total": 3 } })
        );
        assert_eq!(db.list(Scope::Scene, None, ALL, 0).unwrap().data.len(), 4);
        assert_eq!(db.list(Scope::Scene, Some("zz"), ALL, 0).unwrap().total, 0);
        assert_eq!(db.clear(Scope::Scene, "ui").unwrap(), 4);
        assert_eq!(db.counts().unwrap().0, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn players_are_scoped_by_lowercase_address() {
        let root = tmp("players");
        let db = open(&root).unwrap();
        db.set(Scope::Player("0xABC"), "score", &json!(3), "scene")
            .unwrap();
        db.set(Scope::Player("0xdef"), "score", &json!(9), "scene")
            .unwrap();
        assert_eq!(
            db.get(Scope::Player("0xabc"), "score").unwrap(),
            Some(json!(3))
        );
        assert_eq!(db.get(Scope::Scene, "score").unwrap(), None);
        assert_eq!(
            db.players().unwrap(),
            vec![("0xabc".to_string(), 1), ("0xdef".to_string(), 1)]
        );
        assert_eq!(db.clear(Scope::Player("0xABC"), "ui").unwrap(), 1);
        assert_eq!(db.players().unwrap(), vec![("0xdef".to_string(), 1)]);
        assert_eq!(db.counts().unwrap(), (0, 1, 0));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn runtime_env_wins_over_dotenv_and_both_are_listed_with_their_source() {
        let root = tmp("env");
        std::fs::write(
            root.join(".env"),
            "# comment\nexport A=\"from file\"\nB='b'\nC=plain\nbad line\n",
        )
        .unwrap();
        let db = open(&root).unwrap();
        db.env_set("A", "runtime", "cli").unwrap();
        assert_eq!(
            env_value(&root, &db, "A").unwrap().as_deref(),
            Some("runtime")
        );
        assert_eq!(env_value(&root, &db, "B").unwrap().as_deref(), Some("b"));
        assert_eq!(env_value(&root, &db, "Z").unwrap(), None);
        let entries = env_entries(&root, &db).unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|e| (e.key.as_str(), e.value.as_str(), e.source))
                .collect::<Vec<_>>(),
            [
                ("A", "runtime", EnvSource::Runtime),
                ("B", "b", EnvSource::DotEnv),
                ("C", "plain", EnvSource::DotEnv),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_legacy_json_store_is_imported_once_into_a_fresh_database() {
        let root = tmp("legacy");
        std::fs::create_dir_all(root.join(".dcl-one")).unwrap();
        std::fs::write(
            legacy_path(&root),
            json!({ "world": { "hi": "there" }, "player": { "0xABC": { "n": "1" } } }).to_string(),
        )
        .unwrap();
        let db = open(&root).unwrap();
        assert_eq!(db.get(Scope::Scene, "hi").unwrap(), Some(json!("there")));
        assert_eq!(
            db.get(Scope::Player("0xabc"), "n").unwrap(),
            Some(json!("1"))
        );
        assert!(
            !legacy_path(&root).exists(),
            "the file is renamed, not re-read"
        );
        assert!(root.join(".dcl-one/storage.json.imported").is_file());
        assert_eq!(db.activity(5).unwrap()[0].op, "import");
        // a corrupt legacy file is an error naming it, never silently dropped
        let root2 = tmp("legacy-bad");
        std::fs::create_dir_all(root2.join(".dcl-one")).unwrap();
        std::fs::write(legacy_path(&root2), "{ not json").unwrap();
        let err = open(&root2)
            .err()
            .expect("corrupt legacy file refused")
            .to_string();
        assert!(err.contains("could not be imported"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    #[test]
    fn export_and_import_round_trip_in_upstreams_snapshot_shape() {
        let root = tmp("snapshot");
        let db = open(&root).unwrap();
        db.set(Scope::Scene, "w", &json!([1, 2]), "scene").unwrap();
        db.set(Scope::Player("0xAA"), "p", &json!("v"), "scene")
            .unwrap();
        db.env_set("E", "1", "cli").unwrap();
        let snapshot = db.export().unwrap();
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap(),
            json!({ "env": { "E": "1" }, "world": { "w": [1, 2] }, "players": { "0xaa": { "p": "v" } } })
        );
        let mut merged = snapshot.clone();
        merged.world.insert("extra".into(), json!(true));
        merged.world.remove("w");
        db.import(&merged, true, "cli").unwrap();
        assert_eq!(
            db.get(Scope::Scene, "w").unwrap(),
            Some(json!([1, 2])),
            "merge keeps"
        );
        assert_eq!(db.get(Scope::Scene, "extra").unwrap(), Some(json!(true)));
        db.import(&merged, false, "cli").unwrap();
        assert_eq!(db.get(Scope::Scene, "w").unwrap(), None, "replace drops");
        assert_eq!(db.counts().unwrap(), (1, 1, 1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_target_is_remembered_and_parsed_strictly() {
        let root = tmp("target");
        let db = open(&root).unwrap();
        db.set_target(&Target::Zone).unwrap();
        assert_eq!(open(&root).unwrap().target().unwrap(), Target::Zone);
        db.set_target(&Target::Custom("http://localhost:5199/storage/".into()))
            .unwrap();
        assert_eq!(
            open(&root).unwrap().target().unwrap(),
            Target::Custom("http://localhost:5199/storage".into()),
            "a trailing slash is not remembered"
        );
        db.set_setting("target", Some("garbage")).unwrap();
        assert_eq!(
            db.target().unwrap(),
            Target::Local,
            "an unparsable memory is local"
        );
        assert_eq!(Target::parse("ORG").unwrap(), Target::Org);
        assert_eq!(Target::parse(ZONE_URL).unwrap(), Target::Zone);
        assert_eq!(Target::parse(" local ").unwrap(), Target::Local);
        assert_eq!(
            Target::parse("http://127.0.0.1:5199/x/").unwrap(),
            Target::Custom("http://127.0.0.1:5199/x".into())
        );
        assert!(Target::parse("ftp://x").is_err());
        assert!(Target::parse("storage.decentraland.org").is_err());
        assert_eq!(Target::Custom("http://h".into()).to_arg(), "http://h");
        assert_eq!(Target::Org.url(), Some(ORG_URL));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_activity_log_is_newest_first_and_bounded() {
        let root = tmp("activity");
        let db = open(&root).unwrap();
        for i in 0..(ACTIVITY_KEEP + 20) {
            db.set(Scope::Scene, &format!("k{i}"), &json!(i), "scene")
                .unwrap();
        }
        db.delete(Scope::Player("0xAB"), "missing", "ui").unwrap();
        db.delete(Scope::Scene, "k3", "ui").unwrap();
        let log = db.activity(1000).unwrap();
        assert_eq!(log.len(), ACTIVITY_KEEP);
        assert_eq!(
            (
                log[0].op.as_str(),
                log[0].key.as_str(),
                log[0].source.as_str()
            ),
            ("delete", "k3", "ui")
        );
        assert_eq!(log[1].key, format!("k{}", ACTIVITY_KEEP + 19));
        assert!(
            log.iter().all(|a| a.scope == "scene"),
            "a no-op delete is not logged"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keys_addresses_and_values_are_validated_in_the_services_words() {
        assert_eq!(
            check_key(""),
            Err("Key must be between 1 and 255 characters".to_string())
        );
        assert!(check_key(&"k".repeat(MAX_KEY + 1)).is_err());
        assert!(
            check_key(&"é".repeat(MAX_KEY)).is_ok(),
            "characters, not bytes"
        );
        assert!(check_key("a\0b").is_err());
        assert!(check_key("a b\n").is_ok(), "the service takes any text");
        assert_eq!(
            normalize_address(&format!(" 0x{} ", "AB".repeat(20))).unwrap(),
            format!("0x{}", "ab".repeat(20))
        );
        assert_eq!(
            normalize_address("0xabc"),
            Err("Invalid player address".to_string())
        );
        assert!(normalize_address("").is_err());
        assert!(check_value(Scope::Scene, &json!("x".repeat(MAX_WORLD_VALUE_BYTES))).is_err());
        assert!(check_value(Scope::Env, &json!("x".repeat(MAX_ENV_VALUE_BYTES - 2))).is_ok());
        assert!(check_value(Scope::Env, &json!("x".repeat(MAX_ENV_VALUE_BYTES))).is_err());
        assert!(check_value(Scope::Scene, &json!("a\u{0}b")).is_err());
        assert!(check_value(Scope::Scene, &json!({ "ok": true })).is_ok());
        assert_eq!(page_limit(None), MAX_LIMIT);
        assert_eq!(page_limit(Some(0)), MAX_LIMIT);
        assert_eq!(page_limit(Some(500)), MAX_LIMIT);
        assert_eq!(page_limit(Some(7)), 7);
    }

    #[test]
    fn dotenv_parsing_matches_upstreams_reader() {
        let env = parse_dotenv("A=1\n#c\n\nexport B = \"two\"\nC='3'\n=x\nD\nE=\"unterminated\n");
        assert_eq!(
            env.into_iter().collect::<Vec<_>>(),
            [
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "two".to_string()),
                ("C".to_string(), "3".to_string()),
                ("E".to_string(), "\"unterminated".to_string()),
            ]
        );
    }

    #[test]
    fn a_legacy_snapshot_is_read_in_either_shape() {
        let store = Store::from_value(json!({
            "env": { "N": 1 },
            "world": { "k": "v" },
            "player": { "0xAbC": { "a": "1" }, "0xabc": { "b": "2" } }
        }))
        .unwrap();
        assert_eq!(store.env["N"], "1");
        assert_eq!(
            store.players["0xabc"].len(),
            2,
            "spellings of one address merge"
        );
        assert!(Store::from_value(json!([])).is_err());
        assert!(Store::default().is_empty());
    }
}
