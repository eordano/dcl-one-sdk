use super::{project_bridge, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{path::Path as FsPath, sync::Arc, time::Duration};

type Failure = (StatusCode, String);
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EntityContext {
    pub id: String,
    pub name: String,
}

pub(super) fn prompt(prompt: &str, selection: &[EntityContext]) -> Result<String, Failure> {
    if selection.len() > 100
        || selection.iter().any(|e| {
            e.id.len() > 64
                || e.id.is_empty()
                || !e.id.bytes().all(|b| b.is_ascii_digit())
                || e.name.len() > 256
        })
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Select at most 100 entities with valid IDs and names".into(),
        ));
    }
    if selection.is_empty() {
        return Ok(prompt.into());
    }
    Ok(format!("{prompt}\n\nEditor selection (context only; verify these current entities with scene tools before editing):\n{}", serde_json::to_string(selection).unwrap()))
}

pub(super) struct History {
    conn: Connection,
}
fn failure(error: impl std::fmt::Display) -> Failure {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Assistant history: {error}"),
    )
}
impl History {
    pub fn open(root: &FsPath) -> Result<Self, Failure> {
        let dir = root.join(".dcl-one");
        if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(failure("project state directory is a symlink"));
        }
        let dir = crate::scene::work_dir(root).map_err(failure)?;
        let path = dir.join("assistant.sqlite");
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(failure("history file is a symlink"));
        }
        let conn = Connection::open(&path).map_err(failure)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(failure)?;
        }
        conn.busy_timeout(Duration::from_secs(5)).map_err(failure)?;
        conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS conversations(id TEXT PRIMARY KEY, provider TEXT NOT NULL, title TEXT NOT NULL, updated INTEGER NOT NULL, resume TEXT, bytes INTEGER NOT NULL DEFAULT 0, truncated INTEGER NOT NULL DEFAULT 0); CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY, conversation TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE, data TEXT NOT NULL);").map_err(failure)?;
        Ok(Self { conn })
    }
    pub fn resolve(
        &self,
        id: Option<&str>,
        provider: &str,
        legacy: Option<&str>,
    ) -> Result<Option<(String, Option<String>)>, Failure> {
        let found: Option<(String, String, Option<String>)> = match (id, legacy) {
            (Some(id), _) => self.conn.query_row("SELECT id,provider,resume FROM conversations WHERE id=?1", [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional(),
            (None, Some(resume)) => self.conn.query_row("SELECT id,provider,resume FROM conversations WHERE resume=?1 AND provider=?2 ORDER BY updated DESC LIMIT 1", params![resume,provider], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional(),
            _ => return Ok(None),
        }.map_err(failure)?;
        let (id, owner, resume) = found.ok_or((
            StatusCode::NOT_FOUND,
            "Conversation does not belong to this project".into(),
        ))?;
        if owner != provider {
            return Err((
                StatusCode::CONFLICT,
                "Start a new conversation to change provider".into(),
            ));
        }
        Ok(Some((id, resume)))
    }
    pub fn begin(
        &mut self,
        id: &str,
        provider: &str,
        prompt: &str,
        selected: &[EntityContext],
    ) -> Result<(), Failure> {
        let title: String = prompt.trim().chars().take(80).collect();
        let now = chrono::Utc::now().timestamp_millis();
        let tx = self.conn.transaction().map_err(failure)?;
        tx.execute("INSERT INTO conversations(id,provider,title,updated) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET updated=excluded.updated", params![id,provider,title,now]).map_err(failure)?;
        tx.execute("DELETE FROM conversations WHERE id NOT IN (SELECT id FROM conversations ORDER BY updated DESC,rowid DESC LIMIT 20)", []).map_err(failure)?;
        tx.commit().map_err(failure)?;
        self.record(
            id,
            &json!({"type":"user","text":prompt,"selectedEntities":selected}),
        )
    }
    pub fn record(&mut self, id: &str, event: &Value) -> Result<(), Failure> {
        let tx = self.conn.transaction().map_err(failure)?;
        if event["type"] == "session" {
            let resume = event["sessionId"]
                .as_str()
                .filter(|id| super::assistant::identifier(id))
                .ok_or_else(|| failure("provider returned an invalid resume identifier"))?;
            tx.execute(
                "UPDATE conversations SET resume=?2 WHERE id=?1",
                params![id, resume],
            )
            .map_err(failure)?;
        }
        if matches!(
            event["type"].as_str(),
            Some("user" | "text" | "tool" | "error" | "done")
        ) {
            let data = event.to_string();
            let bytes: i64 = tx
                .query_row("SELECT bytes FROM conversations WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .map_err(failure)?;
            if bytes as usize + data.len() <= MAX_BYTES {
                tx.execute(
                    "INSERT INTO events(conversation,data) VALUES(?1,?2)",
                    params![id, data],
                )
                .map_err(failure)?;
                tx.execute(
                    "UPDATE conversations SET bytes=bytes+?2,updated=?3 WHERE id=?1",
                    params![id, data.len() as i64, chrono::Utc::now().timestamp_millis()],
                )
                .map_err(failure)?;
            } else {
                tx.execute("UPDATE conversations SET truncated=1 WHERE id=?1", [id])
                    .map_err(failure)?;
            }
        }
        tx.commit().map_err(failure)
    }
    pub fn list(&self) -> Result<Value, Failure> {
        let mut stmt = self.conn.prepare("SELECT id,provider,title,updated,truncated FROM conversations ORDER BY updated DESC,rowid DESC").map_err(failure)?;
        let rows = stmt.query_map([], |r| Ok(json!({"id":r.get::<_,String>(0)?,"provider":r.get::<_,String>(1)?,"title":r.get::<_,String>(2)?,"updatedAt":r.get::<_,i64>(3)?,"truncated":r.get::<_,bool>(4)?}))).map_err(failure)?;
        Ok(json!({"conversations":rows.collect::<Result<Vec<_>,_>>().map_err(failure)?}))
    }
    pub fn get(&self, id: &str) -> Result<Value, Failure> {
        let mut summary = self.list()?["conversations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == id)
            .cloned()
            .ok_or((
                StatusCode::NOT_FOUND,
                "Conversation does not belong to this project".into(),
            ))?;
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM events WHERE conversation=?1 ORDER BY seq")
            .map_err(failure)?;
        let rows = stmt
            .query_map([id], |r| r.get::<_, String>(0))
            .map_err(failure)?;
        let events = rows
            .map(|r| {
                r.map_err(failure)
                    .and_then(|s| serde_json::from_str::<Value>(&s).map_err(failure))
            })
            .collect::<Result<Vec<_>, _>>()?;
        summary["events"] = json!(events);
        summary["resumable"] = json!(summary["provider"] != "gemini");
        Ok(summary)
    }
    fn remove(&self, id: &str) -> Result<(), Failure> {
        self.conn
            .execute("DELETE FROM conversations WHERE id=?1", [id])
            .map_err(failure)?;
        Ok(())
    }
}

pub(super) async fn record_stream(
    history: &mut History,
    id: &str,
    mut received: tokio::sync::mpsc::Receiver<Value>,
    tx: &tokio::sync::mpsc::Sender<Value>,
) {
    loop {
        let event = tokio::select! { event = received.recv() => event, _ = tx.closed() => None };
        let Some(event) = event else {
            break;
        };
        if let Err((_, message)) = history.record(id, &event) {
            let _ = super::assistant::emit(tx, json!({"type":"error","message":message})).await;
            break;
        }
        if super::assistant::emit(tx, event).await.is_err() {
            break;
        }
    }
}

pub(super) async fn list(State(st): State<Arc<AppState>>) -> Result<Json<Value>, Failure> {
    Ok(Json(
        History::open(&project_bridge::single_project(&st)?.root)?.list()?,
    ))
}
pub(super) async fn get(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Failure> {
    Ok(Json(
        History::open(&project_bridge::single_project(&st)?.root)?.get(&id)?,
    ))
}
pub(super) async fn remove(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, Failure> {
    let active = st
        .assistant
        .active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.is_some() {
        return Err((
            StatusCode::CONFLICT,
            "Wait for the active assistant turn to finish".into(),
        ));
    }
    History::open(&project_bridge::single_project(&st)?.root)?.remove(&id)?;
    Ok(Json(json!({"deleted":true})))
}

#[cfg(test)]
#[path = "assistant_history_tests.rs"]
mod tests;
