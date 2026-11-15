//! Read installed definitions without modifying native configuration or executing it.
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}
fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".codex"))
}
fn claude_home() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"))
}
fn bounded_read(path: &Path) -> Result<String> {
    if fs::metadata(path)?.len() > 256 * 1024 {
        bail!("Definition exceeds 256 KiB");
    }
    fs::read_to_string(path).context("Definition must be UTF-8")
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}
// The installed role format uses strings, not general TOML expressions. Unsupported
// syntax on a selected key is an explicit error rather than a guessed default.
fn string_value(raw: &str) -> Result<String> {
    let raw = raw.trim();
    let mut quote = None;
    let mut escaped = false;
    let mut end = raw.len();
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == '#' {
            end = index;
            break;
        }
    }
    let raw = raw[..end].trim();
    if raw.starts_with('"') {
        return serde_json::from_str(raw)
            .context("Expected a quoted string (unsupported TOML escape or expression)");
    }
    if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
        return Ok(raw[1..raw.len() - 1].into());
    }
    bail!("Expected a quoted string")
}
fn toml_fields(text: &str) -> Result<BTreeMap<String, String>> {
    let selected = [
        "model",
        "model_reasoning_effort",
        "developer_instructions",
        "description",
    ];
    let mut out = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !selected.contains(&key) {
            continue;
        }
        let raw = raw.trim();
        let value = if raw.starts_with("\"\"\"") || raw.starts_with("'''") {
            let delim = &raw[..3];
            let mut body = raw[3..].to_owned();
            while !body.contains(delim) {
                body.push('\n');
                body.push_str(lines.next().context("Unterminated multiline string")?);
            }
            let end = body.find(delim).unwrap();
            let rest = body[end + 3..].trim();
            if !rest.is_empty() && !rest.starts_with('#') {
                bail!("Unexpected text after multiline string");
            }
            let body = body[..end].strip_prefix('\n').unwrap_or(&body[..end]);
            if delim == "'''" {
                body.to_owned()
            } else {
                // JSON decodes TOML's common basic-string escapes. Escape actual newlines only.
                serde_json::from_str::<String>(&format!(
                    "\"{}\"",
                    body.replace('\n', "\\n").replace('\r', "\\r")
                ))
                .context("Unsupported multiline basic string")?
            }
        } else {
            string_value(raw)?
        };
        if out.insert(key.to_owned(), value).is_some() {
            bail!("Duplicate definition key: {key}");
        }
    }
    Ok(out)
}
fn yaml_scalar(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.starts_with('"') {
        return serde_json::from_str(raw).context("Invalid quoted frontmatter value");
    }
    if raw.starts_with('\'') && raw.ends_with('\'') {
        return Ok(raw[1..raw.len() - 1].replace("''", "'"));
    }
    if raw.is_empty() || raw.starts_with(['[', '{', '&', '*', '!', '|', '>']) {
        bail!("Unsupported frontmatter scalar");
    }
    Ok(raw.split(" #").next().unwrap_or(raw).trim().to_owned())
}
fn claude_fields(text: &str) -> Result<(BTreeMap<String, String>, String)> {
    let text = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .context("Missing YAML frontmatter")?;
    let (front, body) = text
        .split_once("\n---")
        .context("Unclosed YAML frontmatter")?;
    let mut out = BTreeMap::new();
    let mut lines = front.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with(char::is_whitespace) || line.trim().starts_with('#') {
            continue;
        }
        if let Some((key, raw)) = line.split_once(':') {
            if !["name", "description", "model", "effort"].contains(&key) {
                continue;
            }
            let value = if key == "description" && [">", ">-", "|", "|-"].contains(&raw.trim()) {
                let mut parts = Vec::new();
                while lines
                    .peek()
                    .is_some_and(|line| line.starts_with(char::is_whitespace) || line.is_empty())
                {
                    parts.push(lines.next().unwrap().trim().to_owned());
                }
                parts.join(if raw.trim().starts_with('>') {
                    " "
                } else {
                    "\n"
                })
            } else {
                yaml_scalar(raw)?
            };
            if out.insert(key.into(), value).is_some() {
                bail!("Duplicate frontmatter key: {key}");
            }
        }
    }
    Ok((out, body.trim_start_matches(['\r', '\n']).to_owned()))
}
fn definition(path: &Path, provider: &str, scope: &str) -> Value {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("invalid");
    let parsed = (|| -> Result<Value> {
        let text = bounded_read(path)?;
        let (fields, instructions) = if provider == "codex" {
            let fields = toml_fields(&text)?;
            let instructions = fields
                .get("developer_instructions")
                .cloned()
                .unwrap_or_default();
            (fields, instructions)
        } else {
            claude_fields(&text)?
        };
        let name = fields.get("name").map(String::as_str).unwrap_or(stem);
        if !valid_name(name) {
            bail!("Role name must contain only letters, numbers, underscores and hyphens");
        }
        let model = fields.get("model").filter(|m| m.as_str() != "inherit");
        let effort = fields.get(if provider == "codex" {
            "model_reasoning_effort"
        } else {
            "effort"
        });
        Ok(
            json!({"id":format!("{provider}:{name}"),"name":name,"provider":provider,"description":fields.get("description").cloned().unwrap_or_else(||format!("Installed {name} agent")),"source":path,"scope":scope,"model":model,"effort":effort,"instructions":instructions,"available":true,"status":"definition_loaded","warnings":[],"verification":"definition_only"}),
        )
    })();
    parsed.unwrap_or_else(|error|json!({"id":format!("{provider}:{stem}"),"name":stem,"provider":provider,"description":"Invalid definition","source":path,"scope":scope,"model":null,"effort":null,"instructions":"","available":false,"status":"invalid_definition","warnings":[error.to_string()],"verification":"unverified"}))
}
pub fn discover(workspace: &str) -> Result<Vec<Value>> {
    let mut paths = vec![
        ("codex", codex_home().join("agents"), "user"),
        ("claude", claude_home().join("agents"), "user"),
    ];
    paths.extend(project_paths(workspace));
    discover_paths(&paths)
}
fn project_paths(workspace: &str) -> Vec<(&'static str, PathBuf, &'static str)> {
    if workspace.is_empty() { return vec![]; }
    vec![
        ("codex", Path::new(workspace).join(".codex/agents"), "project"),
        ("claude", Path::new(workspace).join(".claude/agents"), "project"),
    ]
}
fn discover_paths(paths: &[(&str, PathBuf, &str)]) -> Result<Vec<Value>> {
    let mut agents = BTreeMap::<String, Value>::new();
    for (provider, dir, scope) in paths {
        if !dir.exists() {
            continue;
        }
        let mut entries = fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let ext = if *provider == "codex" { "toml" } else { "md" };
            if path.extension().and_then(|s| s.to_str()) != Some(ext) || !path.is_file() {
                continue;
            }
            let value = definition(&path, provider, scope);
            agents.insert(value["id"].as_str().unwrap().into(), value);
        }
    }
    Ok(agents.into_values().collect())
}
pub fn superagent_defaults() -> Value {
    let path = codex_home().join("config.toml");
    match bounded_read(&path).and_then(|s| toml_fields(&s)) {
        Ok(fields) => {
            json!({"provider":"codex","model":fields.get("model"),"effort":fields.get("model_reasoning_effort")})
        }
        Err(_) => json!({"provider":"codex","model":null,"effort":null}),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_workspace_does_not_inherit_current_directory_agents() {
        assert!(project_paths("").is_empty());
        let paths = project_paths("/selected/project");
        assert_eq!(paths[0].1, PathBuf::from("/selected/project/.codex/agents"));
        assert_eq!(paths[1].1, PathBuf::from("/selected/project/.claude/agents"));
    }
    #[test]
    fn strings_and_frontmatter() {
        let f=toml_fields("model = \"gpt-example\"\nmodel_reasoning_effort = \"high\"\ndeveloper_instructions = \"Read\\n carefully\"\n[other]\nmodel=\"wrong\"").unwrap();
        assert_eq!(f["model"], "gpt-example");
        assert_eq!(f["developer_instructions"], "Read\n carefully");
        let (f, body) =
            claude_fields("---\nname: tester\nmodel: inherit\nskills:\n  - test\n---\nDo work")
                .unwrap();
        assert_eq!(f["model"], "inherit");
        assert_eq!(body, "Do work");
        assert!(toml_fields("model = [\"not a scalar\"]").is_err());
        assert!(claude_fields("---\nname: x\nmodel: [sonnet]\n---\nbody").is_err());
    }
    #[test]
    fn project_override_preserves_provider_identity() {
        let root = std::env::temp_dir().join(format!("studio-registry-{}", uuid::Uuid::new_v4()));
        for d in ["user", "project", "claude"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        fs::write(root.join("user/backend.toml"), "model=\"old\"").unwrap();
        fs::write(root.join("project/backend.toml"), "model=\"new\"").unwrap();
        fs::write(
            root.join("claude/backend.md"),
            "---\nname: backend\nmodel: sonnet\n---\nrole",
        )
        .unwrap();
        let a = discover_paths(&[
            ("codex", root.join("user"), "user"),
            ("claude", root.join("claude"), "user"),
            ("codex", root.join("project"), "project"),
        ])
        .unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(
            a.iter().find(|a| a["id"] == "codex:backend").unwrap()["model"],
            "new"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
