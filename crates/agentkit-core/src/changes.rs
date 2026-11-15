//! Bounded before/after working-file comparison, never a claim of exclusive authorship.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    path::Path,
    process::Stdio,
};
use tokio::{io::AsyncReadExt, process::Command};

pub async fn snapshot(workspace: &str) -> Value {
    let result = async {
        let mut child = Command::new("git")
            .args([
                "-c",
                "core.fsmonitor=false",
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ])
            .current_dir(workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .unwrap()
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > 256 * 1024 {
            let _ = child.kill().await;
            return Err(anyhow::anyhow!("Git file list exceeds comparison limit"));
        }
        if !child.wait().await?.success() {
            return Err(anyhow::anyhow!(
                "Git comparison unavailable for this project"
            ));
        }
        let workspace = workspace.to_owned();
        tokio::task::spawn_blocking(move||{
            let mut files=BTreeMap::new();let mut total=0usize;let mut truncated=false;
            for name in bytes.split(|b|*b==0).filter(|b|!b.is_empty()) {
                if files.len()>=2000 {truncated=true;break;}
                let Ok(name)=std::str::from_utf8(name) else {truncated=true;continue;};
                let path=Path::new(name);if path.is_absolute()||path.components().any(|c|matches!(c,std::path::Component::ParentDir)){continue;}
                let full=Path::new(&workspace).join(path);
                let Ok(meta)=std::fs::symlink_metadata(&full) else {continue;};
                if !meta.is_file()||meta.file_type().is_symlink(){continue;}
                // Do not follow a symlinked directory outside the selected workspace.
                let Ok(resolved)=full.canonicalize() else {continue;};
                if !resolved.starts_with(Path::new(&workspace)){continue;}
                let mut hash=std::collections::hash_map::DefaultHasher::new();
                meta.len().hash(&mut hash);meta.modified().ok().hash(&mut hash);
                let content=if meta.len()<=128*1024&&total+meta.len() as usize<=2*1024*1024 {
                    match std::fs::read(&full){Ok(data)=>{total+=data.len();data.hash(&mut hash);String::from_utf8(data).ok()},Err(_)=>None}
                }else{truncated=true;None};
                files.insert(name.to_owned(),json!({"fingerprint":format!("{:016x}",hash.finish()),"text":content,"bytes":meta.len()}));
            }
            json!({"available":true,"files":files,"truncated":truncated})
        }).await.map_err(anyhow::Error::from)
    };
    match tokio::time::timeout(std::time::Duration::from_secs(5), result).await {
        Ok(Ok(value)) => value,
        Ok(Err(e)) => json!({"available":false,"note":e.to_string()}),
        Err(_) => json!({"available":false,"note":"Git comparison timed out"}),
    }
}
pub fn diff(before: &Value, after: &Value) -> Value {
    if before["available"] != true || after["available"] != true {
        return json!({"available":false,"files":[],"note":"Git comparison unavailable; consult the worker's reported outputs."});
    }
    let empty = serde_json::Map::new();
    let b = before["files"].as_object().unwrap_or(&empty);
    let a = after["files"].as_object().unwrap_or(&empty);
    let names = b.keys().chain(a.keys()).cloned().collect::<BTreeSet<_>>();
    let mut files = vec![];
    let mut budget = 128 * 1024;
    let mut truncated = before["truncated"] == true || after["truncated"] == true;
    for name in names {
        if files.len() >= 200 {
            truncated = true;
            break;
        }
        let old = b.get(&name);
        let new = a.get(&name);
        if (old.is_none() && before["truncated"] == true)
            || (new.is_none() && after["truncated"] == true)
        {
            truncated = true;
            continue;
        }
        if old.and_then(|v| v["fingerprint"].as_str())
            == new.and_then(|v| v["fingerprint"].as_str())
        {
            continue;
        }
        let old_text = old.map(|v| v["text"].as_str()).unwrap_or(Some(""));
        let new_text = new.map(|v| v["text"].as_str()).unwrap_or(Some(""));
        if old_text.is_some() && old_text == new_text {
            continue;
        }
        let mut patch = Value::Null;
        if let (Some(old), Some(new)) = (old_text, new_text) {
            if old.len() + new.len() < 16000 && old.len() + new.len() < budget {
                let mut text = format!(
                    "--- before/{name}\n+++ after/{name}\n@@ -1,{} +1,{} @@\n",
                    old.lines().count(),
                    new.lines().count()
                );
                for line in old.lines() {
                    text.push('-');
                    text.push_str(line);
                    text.push('\n');
                }
                for line in new.lines() {
                    text.push('+');
                    text.push_str(line);
                    text.push('\n');
                }
                budget = budget.saturating_sub(text.len());
                patch = json!(text);
            } else {
                truncated = true;
            }
        }
        files.push(json!({"path":name,"status":if old.is_none(){"added"}else if new.is_none(){"deleted"}else{"modified"},"diff":patch}));
    }
    json!({"available":true,"files":files,"truncated":truncated,"note":"Compared working files immediately before and after this task. Existing modifications are the baseline; external edits during execution cannot be attributed exclusively to the agent."})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compares_against_dirty_baseline() {
        let before =
            json!({"available":true,"files":{"a.txt":{"fingerprint":"a","text":"already dirty"}}});
        let same =
            json!({"available":true,"files":{"a.txt":{"fingerprint":"b","text":"already dirty"}}});
        assert_eq!(diff(&before, &same)["files"].as_array().unwrap().len(), 0);
        let after =
            json!({"available":true,"files":{"a.txt":{"fingerprint":"c","text":"new result"}}});
        assert_eq!(diff(&before, &after)["files"][0]["status"], "modified");
        assert!(diff(&before, &after)["files"][0]["diff"]
            .as_str()
            .unwrap()
            .contains("-already dirty"));
        assert_eq!(diff(&Value::Null, &after)["available"], false);
    }
}
