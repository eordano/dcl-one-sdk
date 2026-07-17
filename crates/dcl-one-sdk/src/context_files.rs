use crate::ux::{self, TrySteps, UserError};
use anyhow::Result;
use std::path::Path;

pub const DEFAULT_API: &str =
    "https://api.github.com/repos/decentraland/documentation/contents/ai-sdk-context";

struct RemoteFile {
    name: String,
    path: String,
    download_url: String,
}

pub async fn get_context_files(dir: &Path, api_base: &str) -> Result<()> {
    let root = dunce::canonicalize(dir).map_err(|e| {
        UserError::new(
            format!("the directory {} does not exist", dir.display()),
            TrySteps::one("check the path passed to --dir")
                .and("run the command from inside your project folder"),
        )
        .caused_by(e)
    })?;
    let Some(kind) = project_kind(&root) else {
        ux::note(
            "not a Decentraland project (needs package.json plus scene.json or wearable.json) — nothing to fetch",
        );
        ux::note("run this inside a project folder, or scaffold one with: dcl-one-sdk init");
        return Ok(());
    };
    println!("\u{2713} Valid {kind} project");
    let out_dir = root.join("dclcontext");
    if out_dir.exists() {
        ux::note("dclcontext/ exists — removing old files");
        std::fs::remove_dir_all(&out_dir).map_err(|e| out_dir_error(&out_dir, e))?;
    }
    std::fs::create_dir_all(&out_dir).map_err(|e| out_dir_error(&out_dir, e))?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("dcl-one-sdk/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| {
            UserError::new(
                "could not initialize the HTTP client",
                TrySteps::one("re-run with --verbose for the underlying cause"),
            )
            .caused_by(e)
        })?;
    let files = list_files(&client, api_base).await?;
    let mut saved = 0usize;
    let mut failed: Vec<String> = Vec::new();
    for file in &files {
        match fetch_bytes(&client, &file.download_url).await {
            Ok(bytes) => match std::fs::write(out_dir.join(&file.name), bytes) {
                Ok(()) => {
                    println!("\u{2713} Saved {}", file.path);
                    saved += 1;
                }
                Err(e) => {
                    println!("\u{2717} Failed to save {}: {e}", file.path);
                    failed.push(file.path.clone());
                }
            },
            Err(e) => {
                println!("\u{2717} Failed to download {}: {e:#}", file.path);
                failed.push(file.path.clone());
            }
        }
    }
    println!(
        "Download complete: {saved} successful, {} failed",
        failed.len()
    );
    if !failed.is_empty() {
        for f in &failed {
            ux::note(format!("  failed: {f}"));
        }
        ux::note("re-run dcl-one-sdk get-context-files to retry the failed files");
    }
    Ok(())
}

fn out_dir_error(path: &Path, e: std::io::Error) -> anyhow::Error {
    UserError::new(
        format!("cannot rewrite the context directory {}", path.display()),
        TrySteps::one("check write permission on the project directory")
            .and("close any program holding files under dclcontext/ open"),
    )
    .caused_by(e)
    .into()
}

fn is_safe_basename(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

fn project_kind(root: &Path) -> Option<&'static str> {
    if !root.join("package.json").is_file() {
        return None;
    }
    if root.join("wearable.json").is_file() {
        return Some("Smart Wearable");
    }
    if root.join("scene.json").is_file() {
        return Some("Scene");
    }
    None
}

async fn list_files(client: &reqwest::Client, api_base: &str) -> Result<Vec<RemoteFile>> {
    let mut queue = vec![api_base.to_string()];
    let mut out = Vec::new();
    while let Some(url) = queue.pop() {
        let items = fetch_listing(client, &url).await?;
        for item in items.as_array().map(|a| a.as_slice()).unwrap_or_default() {
            let kind = item
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let name = item
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let path = item
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(name)
                .to_string();
            match kind {
                "file" => {
                    if !is_safe_basename(name) {
                        ux::note(format!("skipping context file with an unsafe name: {path}"));
                        continue;
                    }
                    if let Some(dl) = item.get("download_url").and_then(|v| v.as_str()) {
                        out.push(RemoteFile {
                            name: name.to_string(),
                            path,
                            download_url: dl.to_string(),
                        });
                    }
                }
                "dir" => {
                    if let Some(sub) = item.get("url").and_then(|v| v.as_str()) {
                        queue.push(sub.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

async fn fetch_listing(client: &reqwest::Client, url: &str) -> Result<serde_json::Value> {
    let listing_error = |why: String| {
        UserError::new(
            "could not list the AI context files",
            TrySteps::one("check the network connection and retry").and(
                "the corpus lives in the decentraland/documentation repo under ai-sdk-context — download it manually if GitHub is unreachable",
            ),
        )
        .why(why)
    };
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| listing_error(format!("GET {url} failed")).caused_by(e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(listing_error(format!("GET {url} \u{2192} HTTP {status}")).into());
    }
    resp.json().await.map_err(|e| {
        listing_error(format!("GET {url} returned unparseable JSON"))
            .caused_by(e)
            .into()
    })
}

async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = client.get(url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("HTTP {status}");
    }
    Ok(resp.bytes().await?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_basename_rejects_traversal_and_separators() {
        assert!(is_safe_basename("scene-context.md"));
        assert!(is_safe_basename("ecs7.d.ts"));
        assert!(!is_safe_basename(""));
        assert!(!is_safe_basename("."));
        assert!(!is_safe_basename(".."));
        assert!(!is_safe_basename("../../etc/passwd"));
        assert!(!is_safe_basename("sub/dir.md"));
        assert!(!is_safe_basename("evil\\..\\x"));
        assert!(!is_safe_basename("/abs.md"));
    }
}
