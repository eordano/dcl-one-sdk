use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, mpsc, oneshot};

pub(super) struct Session {
    pub id: u64,
    pub session_id: Option<String>,
    pub device_name: Option<String>,
    pub connected_at: String,
    pub ended: Option<Instant>,
    pub disconnected_at: Option<String>,
    pub count: u64,
    pub send: Option<mpsc::Sender<Value>>,
    pub pending: HashMap<String, oneshot::Sender<Value>>,
}
impl Session {
    pub fn json(&self) -> Value {
        json!({"id":self.id,"sessionId":self.session_id,"deviceName":self.device_name,"connectedAt":self.connected_at,"disconnectedAt":self.disconnected_at,"status":if self.ended.is_none(){"active"}else{"ended"},"messageCount":self.count})
    }
}

#[derive(Default)]
pub(super) struct Store {
    pub sessions: BTreeMap<u64, Session>,
    pub next_session: u64,
    seq: u64,
    pub history: VecDeque<(usize, Arc<Value>)>,
    history_bytes: usize,
}

pub(super) struct DebugState {
    pub token: String,
    pub store: Mutex<Store>,
    pub events: broadcast::Sender<Arc<Value>>,
}
impl Default for DebugState {
    fn default() -> Self {
        Self {
            token: format!("{:032x}", rand::random::<u128>()),
            store: Mutex::new(Store::default()),
            events: broadcast::channel(8).0,
        }
    }
}
impl DebugState {
    pub fn publish(&self, mut event: Value) {
        let mut store = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.seq += 1;
        event["seq"] = json!(store.seq);
        let bytes = event.to_string().len();
        let event = Arc::new(event);
        store.history_bytes += bytes;
        store.history.push_back((bytes, event.clone()));
        while store.history_bytes > 32 * 1024 * 1024 || store.history.len() > 512 {
            if let Some((bytes, _)) = store.history.pop_front() {
                store.history_bytes -= bytes;
            } else {
                break;
            }
        }
        let _ = self.events.send(event);
    }
    pub fn sessions(&self) -> Vec<Value> {
        let mut store = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        store.sessions.retain(|_, s| {
            !s.ended
                .is_some_and(|at| at.elapsed() > Duration::from_secs(60))
        });
        store.sessions.values().map(Session::json).collect()
    }
    pub fn changed(&self) {
        self.publish(json!({"type":"sessions","sessions":self.sessions()}));
    }
    pub fn connect(&self, send: mpsc::Sender<Value>) -> Option<u64> {
        let _ = self.sessions();
        let mut store = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if store
            .sessions
            .values()
            .filter(|s| s.ended.is_none())
            .count()
            >= 16
        {
            return None;
        }
        store.next_session += 1;
        let id = store.next_session;
        store.sessions.insert(
            id,
            Session {
                id,
                session_id: None,
                device_name: None,
                connected_at: chrono::Utc::now().to_rfc3339(),
                ended: None,
                disconnected_at: None,
                count: 0,
                send: Some(send),
                pending: HashMap::new(),
            },
        );
        drop(store);
        self.changed();
        Some(id)
    }
    pub fn disconnect(&self, id: u64) {
        let mut store = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(s) = store.sessions.get_mut(&id) {
            s.ended = Some(Instant::now());
            s.disconnected_at = Some(chrono::Utc::now().to_rfc3339());
            s.send = None;
            for (_, pending) in s.pending.drain() {
                let _ = pending.send(json!({"ok":false,"data":{"error":"session disconnected"}}));
            }
        }
        drop(store);
        self.changed();
    }
    pub fn receive(&self, id: u64, message: Value) {
        let mut store = self
            .store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(session) = store.sessions.get_mut(&id) else {
            return;
        };
        if message["type"] == "SCENE_INSPECTOR_CMD_ACK" {
            if let Some(id) = message["id"].as_str() {
                if let Some(pending) = session.pending.remove(id) {
                    let _=pending.send(json!({"ok":message["ok"]==true,"data":message.get("data").cloned().unwrap_or(json!({}))}));
                }
            }
            return;
        }
        if message["type"] != "SCENE_INSPECTOR" {
            return;
        }
        let payload = &message["payload"];
        if session.session_id.is_none() {
            session.session_id = payload["sessionId"]
                .as_str()
                .map(|s| s.chars().take(128).collect());
        }
        let Some(entries) = payload["entries"].as_array() else {
            return;
        };
        let entries: Vec<Value> = entries.iter().filter(|v| v.is_object()).cloned().collect();
        for entry in &entries {
            if entry["type"] == "session_start" {
                session.device_name = entry["device_name"]
                    .as_str()
                    .map(|s| s.chars().take(128).collect());
            }
        }
        session.count += entries.len() as u64;
        drop(store);
        if !entries.is_empty() {
            self.publish(json!({"type":"entries","sessionId":id,"entries":entries}));
        }
        self.changed();
    }
}
