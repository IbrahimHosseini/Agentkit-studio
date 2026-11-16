//! Durable Studio entities. SQLite is accessed only by the owning daemon.
use crate::store::{self, Store};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("Missing string parameter: {key}"))
}
pub fn migrate(db: &mut Connection, root: &Path, existed: bool) -> Result<()> {
    let version: u32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > 1 {
        bail!("History was created by a newer Studio version");
    }
    if version == 1 {
        return Ok(());
    }
    if existed {
        let backup = root.join(format!("history.pre-studio-v1-{}.sqlite3", id()));
        db.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
    }
    let tx = db.transaction()?;
    tx.execute_batch("CREATE TABLE studio_entities(kind TEXT NOT NULL,id TEXT NOT NULL,parent TEXT NOT NULL DEFAULT '',data TEXT NOT NULL, PRIMARY KEY(kind,id)); CREATE INDEX studio_parent ON studio_entities(kind,parent); PRAGMA user_version=1;")?;
    let legacy = {
        let mut q = tx.prepare("SELECT id,run,input FROM runs ORDER BY rowid")?;
        let rows = q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    let mut projects = std::collections::HashMap::new();
    for (run_id, raw, input) in legacy {
        let run: Value = serde_json::from_str(&raw)?;
        let input: Value = serde_json::from_str(&input)?;
        let workspace = run["workspace"].as_str().unwrap_or("");
        let project_id = projects
            .entry(workspace.to_owned())
            .or_insert_with(id)
            .clone();
        let project = json!({"id":project_id,"path":workspace,"name":Path::new(workspace).file_name().unwrap_or_default().to_string_lossy(),"created_at":run["created_at"]});
        let conversation = json!({"id":run_id,"project_id":project_id,"title":run["prompt"].as_str().unwrap_or("Imported run").chars().take(100).collect::<String>(),"status":run["status"],"created_at":run["created_at"],"updated_at":run["updated_at"],"legacy_run_id":run_id,"request_id":null,"plan":null});
        let message = json!({"id":format!("legacy-{run_id}"),"conversation_id":run_id,"role":"user","text":run["prompt"],"attachments":input["attachments"].as_array().cloned().unwrap_or_default(),"created_at":run["created_at"]});
        let task = json!({"id":run_id,"conversation_id":run_id,"run_id":run_id,"request_id":"legacy","agent_id":input["role"],"provider":run["provider"],"model":input["model"],"effort":input["effort"],"title":"Imported run","prompt":run["prompt"],"status":run["status"],"kind":"legacy","read_only":false,"dependencies":[],"files":[],"reason":"Imported history","result":"","error":null,"created_at":run["created_at"]});
        for (kind, key, parent, data) in [
            ("project", project_id.as_str(), "", project),
            (
                "conversation",
                run_id.as_str(),
                project_id.as_str(),
                conversation,
            ),
            (
                "message",
                message["id"].as_str().unwrap(),
                run_id.as_str(),
                message.clone(),
            ),
            ("task", run_id.as_str(), run_id.as_str(), task),
        ] {
            tx.execute(
                "INSERT OR IGNORE INTO studio_entities(kind,id,parent,data) VALUES(?1,?2,?3,?4)",
                params![kind, key, parent, data.to_string()],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}
impl Store {
    pub(crate) fn import_run(&self, rid: &str) -> Result<()> {
        if self.entity("conversation", rid).is_ok() {
            return Ok(());
        }
        let run = self.get(rid)?;
        let input = self.input(rid)?;
        let project=self.entities("project",None)?.into_iter().find(|p|p["path"]==run.workspace).unwrap_or_else(||json!({"id":id(),"path":run.workspace,"name":Path::new(&run.workspace).file_name().unwrap_or_default().to_string_lossy(),"created_at":run.created_at}));
        let pid = string(&project, "id")?;
        let tx = self.db.unchecked_transaction()?;
        self.put("project", "", &project)?;
        self.put("conversation",pid,&json!({"id":rid,"project_id":pid,"title":run.prompt.chars().take(100).collect::<String>(),"status":run.status,"created_at":run.created_at,"updated_at":run.updated_at,"legacy_run_id":rid,"request_id":"legacy","plan":null}))?;
        self.message(
            rid,
            "user",
            &run.prompt,
            serde_json::to_value(&input.attachments)?,
        )?;
        self.put("task",rid,&json!({"id":rid,"conversation_id":rid,"run_id":rid,"request_id":"legacy","agent_id":input.role,"provider":run.provider,"model":input.model,"effort":input.effort,"title":"CLI run","prompt":run.prompt,"status":run.status,"kind":"legacy","read_only":input.read_only,"dependencies":[],"files":[],"reason":"Started from CLI","result":"","error":null,"created_at":run.created_at}))?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn entity(&self, kind: &str, key: &str) -> Result<Value> {
        let raw: String = self
            .db
            .query_row(
                "SELECT data FROM studio_entities WHERE kind=?1 AND id=?2",
                params![kind, key],
                |r| r.get(0),
            )
            .with_context(|| format!("{kind} not found"))?;
        let mut value: Value = serde_json::from_str(&raw)?;
        if kind == "conversation" && value.get("permission_mode").is_none() {
            value["permission_mode"] = json!("ask");
        }
        Ok(value)
    }
    pub(crate) fn entities(&self, kind: &str, parent: Option<&str>) -> Result<Vec<Value>> {
        let mut q=self.db.prepare("SELECT data FROM studio_entities WHERE kind=?1 AND (?2 IS NULL OR parent=?2) ORDER BY rowid")?;
        let rows = q.query_map(params![kind, parent], |r| r.get::<_, String>(0))?;
        let mut out = vec![];
        for row in rows {
            let mut value: Value = serde_json::from_str(&row?)?;
            if kind == "conversation" && value.get("permission_mode").is_none() {
                value["permission_mode"] = json!("ask");
            }
            out.push(value);
        }
        Ok(out)
    }
    pub(crate) fn put(&self, kind: &str, parent: &str, value: &Value) -> Result<()> {
        self.db.execute("INSERT INTO studio_entities(kind,id,parent,data) VALUES(?1,?2,?3,?4) ON CONFLICT(kind,id) DO UPDATE SET parent=excluded.parent,data=excluded.data",params![kind,string(value,"id")?,parent,value.to_string()])?;
        Ok(())
    }
    pub(crate) fn remove_entity(&self, kind: &str, key: &str) -> Result<()> {
        self.db.execute("DELETE FROM studio_entities WHERE kind=?1 AND id=?2", params![kind, key])?;
        Ok(())
    }
    /// Delete history atomically, including all provider attempts, so CLI imports cannot resurrect it.
    pub(crate) fn delete_conversation(&self, cid: &str) -> Result<()> {
        self.entity("conversation", cid)?;
        let tx = self.db.unchecked_transaction()?;
        let mut runs = std::collections::HashSet::<String>::new();
        for task in self.entities("task", Some(cid))? {
            if let Some(rid) = task["run_id"].as_str() { runs.insert(rid.into()); }
            if let Some(attempts) = task["attempts"].as_array() {
                for attempt in attempts { if let Some(rid) = attempt["run_id"].as_str() { runs.insert(rid.into()); } }
            }
        }
        for rid in runs {
            self.db.execute("DELETE FROM events WHERE run_id=?1", [&rid])?;
            self.db.execute("DELETE FROM runs WHERE id=?1", [&rid])?;
        }
        self.db.execute("DELETE FROM studio_entities WHERE kind IN ('message','task') AND parent=?1", [cid])?;
        self.remove_entity("conversation", cid)?;
        tx.commit()?;
        Ok(())
    }
    /// Catalog records use the existing versioned entity store; no parallel database owner.
    pub(crate) fn seed_model_catalog(&self, workspace: &str) -> Result<()> {
        let settings = self.settings()?;
        let mut definitions = crate::registry::discover(workspace)?;
        let defaults = crate::registry::superagent_defaults();
        definitions.push(json!({"provider":defaults["provider"],"model":defaults["model"],"effort":defaults["effort"],"available":true}));
        for definition in definitions {
            let Some(provider) = definition["provider"].as_str() else { continue; };
            if !crate::scheduler::provider_enabled(&settings, provider) || definition["available"] != true { continue; }
            let mut catalog = self.entity("model_catalog", provider).unwrap_or_else(|_| json!({"id":provider,"provider":provider,"models":[],"updated_at":store::now()}));
            let original = catalog.clone();
            let Some(model) = definition["model"].as_str().filter(|m| !m.is_empty()) else {
                if let Some(effort) = definition["effort"].as_str() {
                    if !catalog["runtimeDefaultEfforts"].is_array() { catalog["runtimeDefaultEfforts"] = json!([]); }
                    let efforts = catalog["runtimeDefaultEfforts"].as_array_mut().unwrap();
                    if !efforts.contains(&json!(effort)) { efforts.push(json!(effort)); }
                    if catalog != original { self.put("model_catalog", "", &catalog)?; }
                }
                continue;
            };
            let models = catalog["models"].as_array_mut().context("Invalid stored model catalog")?;
            if let Some(existing) = models.iter_mut().find(|m| m["model"] == model) {
                if existing["verification"] == "installed" {
                    if let Some(effort) = definition["effort"].as_str() {
                        let efforts = existing["installedEfforts"].as_array_mut().context("Invalid installed efforts")?;
                        if !efforts.contains(&json!(effort)) { efforts.push(json!(effort)); }
                    }
                }
            } else {
                models.push(json!({"provider":provider,"id":model,"model":model,"displayName":model,"verification":"installed","defaultReasoningEffort":null,"supportedReasoningEfforts":[],"installedEfforts":definition["effort"].as_str().map(|e|vec![e]).unwrap_or_default()}));
            }
            if catalog != original { self.put("model_catalog", "", &catalog)?; }
        }
        Ok(())
    }
    pub(crate) fn save_model_catalog(&self, provider: &str, value: &Value) -> Result<()> {
        let previous = self.entity("model_catalog", provider).ok();
        let mut catalog = if value["catalog_error"].is_string() {
            previous.clone().unwrap_or_else(||value.clone())
        } else { value.clone() };
        if value["catalog_error"].is_string() {
            catalog["catalog_error"] = value["catalog_error"].clone();
            catalog["stale"] = json!(previous.is_some());
            catalog["detail"] = json!("Refresh failed; retained the last stored catalog. Availability is not verified.");
        } else { catalog["stale"] = json!(false); }
        if let Some(previous) = previous { catalog["runtimeDefaultEfforts"] = previous["runtimeDefaultEfforts"].clone(); }
        catalog["id"] = json!(provider);
        catalog["provider"] = json!(provider);
        catalog["updated_at"] = json!(store::now());
        for model in catalog["models"].as_array_mut().context("Invalid model catalog")? {
            model["provider"] = json!(provider);
        }
        self.put("model_catalog", "", &catalog)
    }
    pub(crate) fn model_catalog(&self, workspace: &str) -> Result<Value> {
        self.seed_model_catalog(workspace)?;
        let settings = self.settings()?;
        let mut models = Vec::new();
        let mut providers = Vec::new();
        for catalog in self.entities("model_catalog", None)? {
            let provider = string(&catalog, "provider")?;
            if !crate::scheduler::provider_enabled(&settings, provider) { continue; }
            models.extend(catalog["models"].as_array().cloned().unwrap_or_default());
            providers.push(json!({"provider":provider,"updated_at":catalog["updated_at"],"catalog_error":catalog["catalog_error"],"detail":catalog["detail"],"stale":catalog["stale"],"runtimeDefaultEfforts":catalog["runtimeDefaultEfforts"]}));
        }
        Ok(json!({"models":models,"providers":providers}))
    }
    pub(crate) fn message(
        &self,
        cid: &str,
        role: &str,
        text: &str,
        attachments: Value,
    ) -> Result<Value> {
        let value = json!({"id":id(),"conversation_id":cid,"role":role,"text":text,"attachments":attachments,"created_at":store::now()});
        self.put("message", cid, &value)?;
        Ok(value)
    }
    pub(crate) fn settings(&self) -> Result<Value> {
        let mut defaults = crate::registry::superagent_defaults();
        if !defaults.is_object() {
            defaults = json!({});
        }
        if defaults.get("provider").is_none() {
            defaults["provider"] = json!("codex");
        }
        let previous = self.entity("settings", "main").ok();
        let mut settings=previous.clone().unwrap_or_else(||json!({"id":"main","concurrency":4,"superagent":defaults,"agent_overrides":{},"mcp_enabled":false,"provider_connections":{"codex":false,"claude":false}}));
        if settings.get("claude_auth_mode").is_none() {
            settings["claude_auth_mode"] = json!("configured");
        }
        if settings.get("provider_connections").is_none() { settings["provider_connections"] = json!({"codex":true,"claude":true}); }
        if previous.as_ref() != Some(&settings) { self.put("settings", "", &settings)?; }
        Ok(settings)
    }
    pub(crate) fn snapshot(&self, cid: &str) -> Result<Value> {
        let conversation = self.entity("conversation", cid)?;
        let mut messages = self.entities("message", Some(cid))?;
        let mut tasks = self.entities("task", Some(cid))?;
        let total_messages = messages.len();
        let total_tasks = tasks.len();
        if messages.len() > 100 {
            messages.drain(..messages.len() - 100);
        }
        if tasks.len() > 80 {
            tasks.drain(..tasks.len() - 80);
        }
        for t in &mut tasks {
            if let Some(o) = t.as_object_mut() {
                o.remove("instructions");
                o.remove("prompt");
            }
        }
        let mut out = json!({"plan":conversation["plan"],"conversation":conversation,"messages":messages,"tasks":tasks,"total_messages":total_messages,"total_tasks":total_tasks});
        while serde_json::to_vec(&out)?.len() > 1400000 {
            if out["messages"].as_array().unwrap().len() > 1 {
                out["messages"].as_array_mut().unwrap().remove(0);
            } else if out["tasks"].as_array().unwrap().len() > 1 {
                out["tasks"].as_array_mut().unwrap().remove(0);
            } else {
                bail!("Conversation snapshot exceeds the transport limit");
            }
            out["truncated"] = json!(true);
        }
        Ok(out)
    }
    pub(crate) fn message_page(
        &self,
        cid: &str,
        before: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        self.entity("conversation", cid)?;
        let messages = self.entities("message", Some(cid))?;
        let end = match before {
            Some(id) => messages
                .iter()
                .position(|m| m["id"] == id)
                .context("Message cursor does not belong to conversation")?,
            None => messages.len(),
        };
        let mut start = end.saturating_sub(limit);
        let mut output = messages[start..end].to_vec();
        while serde_json::to_vec(&output)?.len() > 1000000 && output.len() > 1 {
            output.remove(0);
            start += 1;
        }
        Ok(json!({"messages":output,"has_more":start>0}))
    }
    pub(crate) fn task_page(&self, cid: &str, before: Option<&str>, limit: usize) -> Result<Value> {
        self.entity("conversation", cid)?;
        let tasks = self.entities("task", Some(cid))?;
        let end = match before {
            Some(id) => tasks
                .iter()
                .position(|t| t["id"] == id)
                .context("Task cursor does not belong to conversation")?,
            None => tasks.len(),
        };
        let mut start = end.saturating_sub(limit);
        let mut output = tasks[start..end].to_vec();
        for t in &mut output {
            if let Some(o) = t.as_object_mut() {
                o.remove("instructions");
                o.remove("prompt");
            }
        }
        while serde_json::to_vec(&output)?.len() > 1000000 && output.len() > 1 {
            output.remove(0);
            start += 1;
        }
        Ok(json!({"tasks":output,"has_more":start>0}))
    }
    pub(crate) fn referenced(&self, path: &str) -> Result<bool> {
        for message in self.entities("message", None)? {
            if message["attachments"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x["path"] == path))
            {
                return Ok(true);
            }
        }
        let count:u64=self.db.query_row("SELECT COUNT(*) FROM runs,json_each(runs.input,'$.attachments') AS a WHERE json_extract(a.value,'$.path')=?1",[path],|r|r.get(0))?;
        Ok(count > 0)
    }
}
pub fn recover(store: &Store) -> Result<()> {
    for mut task in store.entities("task", None)? {
        if !store::terminal(task["status"].as_str().unwrap_or("")) {
            task["status"] = json!("interrupted");
            task["error"] = json!("Engine restarted. Work was not automatically repeated.");
            let cid = string(&task, "conversation_id")?.to_owned();
            store.put("task", &cid, &task)?;
        }
    }
    for mut c in store.entities("conversation", None)? {
        if matches!(
            c["status"].as_str(),
            Some("planning" | "running" | "queued" | "awaiting_approval" | "stopping")
        ) {
            c["status"] = json!("interrupted");
            c["plan"] = Value::Null;
            store.put("conversation", c["project_id"].as_str().unwrap_or(""), &c)?;
        }
    }
    Ok(())
}
