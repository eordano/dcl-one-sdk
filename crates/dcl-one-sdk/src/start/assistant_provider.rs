use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(super) const PROVIDERS: [(&str, &str, &str); 4] = [
    ("codex", "Codex", "codex"),
    ("claude", "Claude", "claude"),
    ("cursor", "Cursor", "cursor-agent"),
    ("gemini", "Gemini", "gemini"),
];

pub(super) fn executable(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| {
            let Ok(meta) = std::fs::metadata(p) else {
                return false;
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                meta.is_file() && meta.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                meta.is_file()
            }
        })
}

pub(super) struct Invocation {
    pub args: Vec<String>,
    pub stdin: String,
    pub overlays: Vec<ConfigOverlay>,
}

pub(super) fn invocation(
    provider: &str,
    prompt: &str,
    session: Option<&str>,
    model: Option<&str>,
    root: &Path,
    mcp: Option<&str>,
) -> Result<Invocation, String> {
    let mut args: Vec<String> = match provider {
        "codex" => {
            let mut v = vec!["exec".into()];
            if let Some(session) = session {
                v.extend(["resume".into(), session.into()]);
            }
            v.extend(["--json".into(), "--skip-git-repo-check".into()]);
            v
        }
        "claude" => vec!["-p", "--output-format", "stream-json", "--verbose"]
            .into_iter()
            .map(String::from)
            .collect(),
        "cursor" => vec!["-p", "--output-format", "stream-json"]
            .into_iter()
            .map(String::from)
            .collect(),
        "gemini" => vec!["-o", "stream-json"]
            .into_iter()
            .map(String::from)
            .collect(),
        _ => return Err("Unknown assistant provider".into()),
    };
    if let Some(session) = session {
        if provider == "gemini" {
            return Err(
                "Gemini turns cannot resume a stable session; include prior context in the prompt"
                    .into(),
            );
        }
        if provider != "codex" {
            args.extend(["--resume".into(), session.into()]);
        }
    }
    if let Some(model) = model.filter(|m| *m != "default") {
        args.extend(["--model".into(), model.into()]);
    }
    let mut overlays = Vec::new();
    if let Some(url) = mcp {
        match provider {
            "codex" => {
                args.extend([
                    "-c".into(),
                    format!("mcp_servers.creator-hub.url={}", json!(url)),
                ]);
            }
            "claude" => {
                let config = json!({"mcpServers":{"creator-hub":{"type":"http","url":url}}});
                let path = std::env::temp_dir()
                    .canonicalize()
                    .unwrap_or_else(|_| std::env::temp_dir())
                    .join(format!(
                        "dcl-one-assistant-{:x}.json",
                        rand::random::<u64>()
                    ));
                overlays.push(ConfigOverlay::write(path.clone(), config, false)?);
                args.extend(["--mcp-config".into(), path.to_string_lossy().into_owned()]);
            }
            "cursor" | "gemini" => {
                let (path, server) = if provider == "cursor" {
                    (root.join(".cursor/mcp.json"), json!({"url":url}))
                } else {
                    (root.join(".gemini/settings.json"), json!({"httpUrl":url}))
                };
                overlays.push(ConfigOverlay::write(
                    path,
                    json!({"mcpServers":{"creator-hub":server}}),
                    true,
                )?);
            }
            _ => {}
        }
    }
    let instructions="You are editing this Decentraland SDK scene. Work in the current project. Use its existing dcl-one-sdk build/watch workflow. Edit authored source and composites, not generated bin/ or .dcl-one/ output. The SDK watcher rebuilds saved files. Use available creator-hub MCP scene tools to inspect the running editor; call session_status before scene tools. Explain permission or authentication failures clearly.";
    let text = format!("{instructions}\n\n{prompt}");
    let stdin = match provider {
        "codex" => {
            args.push("-".into());
            text
        }
        "claude" => text,
        "cursor" => {
            args.push(text);
            String::new()
        }
        "gemini" => {
            args.extend(["-p".into(), prompt.into()]);
            instructions.into()
        }
        _ => unreachable!(),
    };
    Ok(Invocation {
        args,
        stdin,
        overlays,
    })
}

pub(super) struct ConfigOverlay {
    path: PathBuf,
    before: Option<Vec<u8>>,
    written: Vec<u8>,
    permissions: Option<std::fs::Permissions>,
}
impl ConfigOverlay {
    fn write(path: PathBuf, patch: Value, merge: bool) -> Result<Self, String> {
        for ancestor in path.ancestors().take_while(|p| p.parent().is_some()) {
            if std::fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err("MCP configuration cannot follow symbolic links".into());
            }
        }
        let before = match std::fs::read(&path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        let permissions = std::fs::metadata(&path).ok().map(|m| m.permissions());
        let mut value = if merge {
            before
                .as_ref()
                .map(|b| serde_json::from_slice::<Value>(b))
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or(json!({}))
        } else {
            json!({})
        };
        if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
            return Err("Existing MCP configuration must contain JSON objects".into());
        }
        if value.get("mcpServers").is_none() {
            value["mcpServers"] = json!({});
        }
        value["mcpServers"]["creator-hub"] = patch["mcpServers"]["creator-hub"].clone();
        let written = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temporary =
            path.with_file_name(format!(".assistant-config-{:x}.tmp", rand::random::<u64>()));
        let outcome = (|| -> std::io::Result<()> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            use std::io::Write;
            let mut file = options.open(&temporary)?;
            file.write_all(&written)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &path)
        })();
        if let Err(error) = outcome {
            let _ = std::fs::remove_file(temporary);
            return Err(error.to_string());
        }
        Ok(Self {
            path,
            before,
            written,
            permissions,
        })
    }
}
impl Drop for ConfigOverlay {
    fn drop(&mut self) {
        if std::fs::read(&self.path).ok().as_ref() != Some(&self.written) {
            return;
        }
        if let Some(before) = &self.before {
            let _ = std::fs::write(&self.path, before);
            if let Some(permissions) = &self.permissions {
                let _ = std::fs::set_permissions(&self.path, permissions.clone());
            }
        } else {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub(super) fn events(provider: &str, line: &str) -> Vec<Value> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![];
    };
    let mut out = Vec::new();
    let kind = v["type"].as_str().unwrap_or("");
    let session = if provider == "codex" && kind == "thread.started" {
        v["thread_id"].as_str()
    } else if matches!(kind, "system" | "init" | "result") {
        v["session_id"].as_str()
    } else {
        None
    };
    if let Some(id) = session {
        out.push(json!({"type":"session","sessionId":id}));
    }
    if provider == "codex" && kind == "item.completed" {
        let item = &v["item"];
        match item["type"].as_str().unwrap_or("") {
            "agent_message" => out.push(json!({"type":"text","text":item["text"]})),
            "error" => out.push(json!({"type":"error","message":item["message"]})),
            "command_execution" => {
                out.push(json!({"type":"tool","name":"Run","detail":item["command"]}))
            }
            "file_change" => {
                out.push(json!({"type":"tool","name":"Edit","detail":item["changes"].to_string()}))
            }
            "mcp_tool_call" => {
                out.push(json!({"type":"tool","name":item["tool"],"detail":item["server"]}))
            }
            _ => {}
        }
    }
    if matches!(provider, "claude" | "cursor") && kind == "assistant" {
        if let Some(blocks) = v["message"]["content"].as_array() {
            for b in blocks {
                match b["type"].as_str() {
                    Some("text") => out.push(json!({"type":"text","text":b["text"]})),
                    Some("tool_use") => out.push(
                        json!({"type":"tool","name":b["name"],"detail":b["input"].to_string()}),
                    ),
                    _ => {}
                }
            }
        }
    }
    if provider == "cursor" && kind == "tool_call" && v["subtype"] == "started" {
        if let Some(tools) = v["tool_call"].as_object() {
            for (name, detail) in tools {
                out.push(json!({"type":"tool","name":name,"detail":detail.to_string()}));
            }
        }
    }
    if provider == "gemini" {
        if kind == "message" && v["role"] == "assistant" {
            out.push(json!({"type":"text","text":v["content"]}));
        }
        if kind == "tool_use" {
            out.push(
                json!({"type":"tool","name":v["tool_name"],"detail":v["parameters"].to_string()}),
            );
        }
    }
    if kind == "error"
        || kind == "turn.failed"
        || (kind == "result" && (v["is_error"] == true || v["status"] == "error"))
    {
        let message = v["message"]
            .as_str()
            .or(v["error"]["message"].as_str())
            .or(v["result"].as_str())
            .unwrap_or("Assistant turn failed");
        out.push(json!({"type":"error","message":message}));
    }
    out
}
