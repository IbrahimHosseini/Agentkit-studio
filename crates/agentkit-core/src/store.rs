use agentkit_protocol::{ProviderEvent, Run, RunEvent, RunInput};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;

pub struct Store {
    pub(crate) db: Connection,
}
pub fn now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}
pub fn terminal(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled" | "interrupted")
}
const PAYLOAD_LIMIT: usize = 65_536;
const CONTROL_FIELDS: &[&str] = &[
    "retry",
    "request_id",
    "session_id",
    "status",
    "decision",
    "agent_id",
    "parent_agent_id",
    "task_id",
    "run_id",
    "thread_id",
    "turn_id",
    "method",
    "id",
];
fn prefix(value: &str, bytes: usize) -> &str {
    let mut end = bytes.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
fn bounded_payload(value: serde_json::Value) -> Result<serde_json::Value> {
    use serde_json::{json, Map, Value};
    let raw = serde_json::to_string(&value)?;
    if raw.len() <= PAYLOAD_LIMIT {
        return Ok(value);
    }
    let mut output = Map::new();
    output.insert("truncated".into(), json!(true));
    output.insert("original_bytes".into(), json!(raw.len()));
    if let Some(object) = value.as_object() {
        for key in CONTROL_FIELDS {
            if let Some(value) = object.get(*key) {
                output.insert((*key).into(), value.clone());
            }
        }
        let used = serde_json::to_vec(&output)?.len();
        if used > PAYLOAD_LIMIT - 256 {
            bail!("Provider control metadata exceeds the 64 KiB event limit");
        }
        let mut available = PAYLOAD_LIMIT - used - 256;
        for (key, value) in object {
            if CONTROL_FIELDS.contains(&key.as_str())
                || matches!(key.as_str(), "truncated" | "original_bytes")
            {
                continue;
            }
            if key.len() > 256 {
                continue;
            }
            let encoded = serde_json::to_string(value)?;
            let bounded = if encoded.len() <= 4096 {
                value.clone()
            } else if let Some(text) = value.as_str() {
                Value::String(format!("{}… [truncated]", prefix(text, 2048)))
            } else {
                json!({"truncated":true,"preview":prefix(&encoded,2048),"original_bytes":encoded.len()})
            };
            let size = serde_json::to_vec(&bounded)?.len() + serde_json::to_string(key)?.len() + 2;
            if size <= available {
                available -= size;
                output.insert(key.clone(), bounded);
            }
        }
    } else {
        output.insert("preview".into(), json!(prefix(&raw, 4096)));
    }
    Ok(Value::Object(output))
}

impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        let existed = root.join("history.sqlite3").exists();
        let mut db = Connection::open(root.join("history.sqlite3"))?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY, input TEXT NOT NULL, run TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events (seq INTEGER PRIMARY KEY AUTOINCREMENT, run_id TEXT NOT NULL REFERENCES runs(id), event TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS events_run_seq ON events(run_id,seq);")?;
        crate::studio::migrate(&mut db, root, existed)?;
        Ok(Self { db })
    }
    pub fn recover(&mut self) -> Result<()> {
        let ids = {
            let mut query=self.db.prepare("SELECT id FROM runs WHERE json_extract(run, '$.status') NOT IN ('completed','failed','cancelled','interrupted')")?;
            let rows = query.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for id in ids {
            self.event(&id, ProviderEvent::new("status", serde_json::json!({"status":"interrupted","reason":"Engine restarted; this run was not automatically retried"})))?;
        }
        crate::studio::recover(self)?;
        Ok(())
    }
    pub fn create(&mut self, input: &RunInput) -> Result<Run> {
        let time = now();
        let run = Run {
            id: uuid::Uuid::new_v4().to_string(),
            provider: input.provider.clone(),
            workspace: input.workspace.clone(),
            prompt: input.prompt.clone(),
            status: "queued".into(),
            created_at: time.clone(),
            updated_at: time,
            provider_session_id: None,
        };
        self.db.execute(
            "INSERT INTO runs(id,input,run) VALUES(?1,?2,?3)",
            params![
                run.id,
                serde_json::to_string(input)?,
                serde_json::to_string(&run)?
            ],
        )?;
        self.event(
            &run.id,
            ProviderEvent::new("status", serde_json::json!({"status":"queued"})),
        )?;
        Ok(run)
    }
    pub fn get(&self, id: &str) -> Result<Run> {
        let value: String = self
            .db
            .query_row("SELECT run FROM runs WHERE id=?1", [id], |r| r.get(0))
            .context("Run not found")?;
        Ok(serde_json::from_str(&value)?)
    }
    pub fn input(&self, id: &str) -> Result<RunInput> {
        let value: String = self
            .db
            .query_row("SELECT input FROM runs WHERE id=?1", [id], |r| r.get(0))
            .context("Run not found")?;
        Ok(serde_json::from_str(&value)?)
    }
    pub fn list(&self, limit: usize) -> Result<Vec<Run>> {
        let mut query = self
            .db
            .prepare("SELECT run FROM runs ORDER BY rowid DESC LIMIT ?1")?;
        let rows = query.query_map([limit as u64], |r| r.get::<_, String>(0))?;
        let mut output = Vec::new();
        let mut bytes = 0usize;
        for row in rows {
            let row = row?;
            bytes += row.len();
            if bytes > 1024 * 1024 {
                break;
            }
            output.push(serde_json::from_str(&row)?);
        }
        Ok(output)
    }
    pub fn event(&mut self, run_id: &str, mut event: ProviderEvent) -> Result<RunEvent> {
        let mut run = self.get(run_id)?;
        let time = now();
        if event.kind == "status" {
            if let Some(status) = event.payload.get("status").and_then(|v| v.as_str()) {
                if matches!(
                    status,
                    "queued"
                        | "running"
                        | "awaiting_approval"
                        | "stopping"
                        | "completed"
                        | "failed"
                        | "cancelled"
                        | "interrupted"
                        | "unknown"
                ) {
                    run.status = status.to_owned();
                }
            }
        }
        if event.kind == "session" {
            run.provider_session_id = event
                .payload
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
        }
        // Truncate display data without discarding identifiers used by approval controls.
        event.payload = bounded_payload(event.payload)?;
        run.updated_at = time.clone();
        let mut output = RunEvent {
            seq: 0,
            run_id: run_id.into(),
            timestamp: time,
            agent_id: event.agent_id,
            parent_agent_id: event.parent_agent_id,
            kind: event.kind,
            payload: event.payload,
        };
        let tx = self.db.transaction()?;
        tx.execute(
            "INSERT INTO events(run_id,event) VALUES(?1,?2)",
            params![run_id, serde_json::to_string(&output)?],
        )?;
        output.seq = tx.last_insert_rowid() as u64;
        tx.execute(
            "UPDATE runs SET run=?1 WHERE id=?2",
            params![serde_json::to_string(&run)?, run_id],
        )?;
        tx.commit()?;
        Ok(output)
    }
    pub fn events(&self, id: &str, after: u64, limit: usize) -> Result<Vec<RunEvent>> {
        self.get(id)?;
        let mut query = self.db.prepare(
            "SELECT seq,event FROM events WHERE run_id=?1 AND seq>?2 ORDER BY seq LIMIT ?3",
        )?;
        let rows = query.query_map(params![id, after, limit as u64], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (seq, raw) = row?;
            let mut event: RunEvent = serde_json::from_str(&raw)?;
            event.seq = seq;
            events.push(event);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("ak-store-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn input() -> RunInput {
        RunInput {
            provider: "mock".into(),
            workspace: std::env::temp_dir().to_string_lossy().into_owned(),
            prompt: "test".into(),
            role: None,
            attachments: vec![],
            resume_session: None,
            model: None,
            effort: None,
            instructions: None,
            read_only: false,
            claude_auth_mode: None,
            permission_mode: Default::default(),
            devtools_conversation_id: None,
            devtools_cli: None,
        }
    }
    #[test]
    fn recovery_marks_only_unfinished_runs_and_retains_cursor_history() {
        let scratch = Scratch::new();
        let mut store = Store::open(&scratch.0).unwrap();
        let unfinished = store.create(&input()).unwrap();
        let complete = store.create(&input()).unwrap();
        store
            .event(
                &complete.id,
                ProviderEvent::new("status", serde_json::json!({"status":"completed"})),
            )
            .unwrap();
        let first = store.events(&unfinished.id, 0, 10).unwrap();
        let cursor = first[0].seq;
        drop(store);
        let mut store = Store::open(&scratch.0).unwrap();
        store.recover().unwrap();
        assert_eq!(store.get(&unfinished.id).unwrap().status, "interrupted");
        assert_eq!(store.get(&complete.id).unwrap().status, "completed");
        let next = store.events(&unfinished.id, cursor, 10).unwrap();
        assert_eq!(next.len(), 1);
        assert!(next[0].seq > cursor);
        assert_eq!(next[0].payload["status"], "interrupted");
    }
    #[test]
    fn large_unicode_events_are_bounded_and_do_not_break_json() {
        let scratch = Scratch::new();
        let mut store = Store::open(&scratch.0).unwrap();
        let run = store.create(&input()).unwrap();
        let event = store
            .event(
                &run.id,
                ProviderEvent::new("text", serde_json::json!({"text":"فارسی".repeat(20000)})),
            )
            .unwrap();
        assert_eq!(event.payload["truncated"], true);
        assert!(serde_json::to_string(&event.payload).unwrap().len() < 65536);
        assert_eq!(store.events(&run.id, 0, 1).unwrap().len(), 1);
    }
    #[test]
    fn truncation_preserves_session_and_status_metadata() {
        for (key, value) in [
            ("session_id", serde_json::json!("session-123")),
            ("status", serde_json::json!("awaiting_approval")),
        ] {
            let mut payload = serde_json::json!({"raw":"x".repeat(100000)});
            payload[key] = value.clone();
            let bounded = bounded_payload(payload).unwrap();
            assert_eq!(bounded[key], value);
            assert!(serde_json::to_vec(&bounded).unwrap().len() <= PAYLOAD_LIMIT);
        }
        assert!(
            bounded_payload(serde_json::json!({"request_id":"x".repeat(PAYLOAD_LIMIT+1)})).is_err()
        );
    }
}
