//! `get-context-files` — the AI context a scene hands to an agent, in two
//! halves: the embedded skills `src/skills.rs` writes straight out of the
//! binary, then Decentraland's `ai-sdk-context` corpus pulled from the GitHub
//! contents API into `dclcontext/`. A download failure does not fail the
//! command (hard-failing on an unreachable registry is the npm behaviour this
//! crate replaces), and `dclcontext/` is only wiped once the listing is in
//! hand, so a failed run never leaves an empty directory where a good corpus
//! had been. `--offline` skips the request outright.

use crate::ux::{self, TrySteps, UserError};
use anyhow::Result;
use serde_json::Value;
use std::path::Path;

pub const DEFAULT_API: &str =
    "https://api.github.com/repos/decentraland/documentation/contents/ai-sdk-context";

struct RemoteFile {
    name: String,
    path: String,
    download_url: String,
}

pub async fn get_context_files(dir: &Path, api_base: &str, offline: bool) -> Result<()> {
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
    println!("\u{2713} {kind} project");

    let written = crate::skills::install(&root)?;
    let bytes: usize = crate::skills::EMBEDDED.iter().map(|s| s.bytes()).sum();
    println!(
        "\u{2713} Installed {} skills into {}/ ({} files, {:.1} MB) — bundled, no network",
        crate::skills::EMBEDDED.len(),
        crate::skills::SKILLS_DIR,
        written.len(),
        bytes as f64 / (1024.0 * 1024.0),
    );
    debug_assert_eq!(
        written.len(),
        crate::skills::EMBEDDED
            .iter()
            .map(|s| s.files.len())
            .sum::<usize>()
    );

    if offline {
        ux::note("--offline: skipping the GitHub ai-sdk-context download");
        return Ok(());
    }

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
    let files = match list_files(&client, api_base).await {
        Ok(files) => files,
        Err(e) => {
            println!("\u{2717} Could not reach the ai-sdk-context corpus: {e}");
            ux::note("dclcontext/ was left untouched — the bundled skill above is installed");
            ux::note("re-run dcl-one-sdk get-context-files when the network is back");
            ux::note("or pass --offline to skip the download and only refresh the skill");
            return Ok(());
        }
    };

    let out_dir = root.join("dclcontext");
    if out_dir.exists() {
        ux::note("dclcontext/ exists — removing old files");
        std::fs::remove_dir_all(&out_dir).map_err(|e| out_dir_error(&out_dir, e))?;
    }
    std::fs::create_dir_all(&out_dir).map_err(|e| out_dir_error(&out_dir, e))?;
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

fn str_of<'a>(item: &'a Value, key: &str) -> Option<&'a str> {
    item.get(key).and_then(|v| v.as_str())
}

async fn list_files(client: &reqwest::Client, api_base: &str) -> Result<Vec<RemoteFile>> {
    let mut queue = vec![api_base.to_string()];
    let mut out = Vec::new();
    while let Some(url) = queue.pop() {
        let items = fetch_listing(client, &url).await?;
        for item in items.as_array().map(|a| a.as_slice()).unwrap_or_default() {
            let name = str_of(item, "name").unwrap_or_default();
            let path = str_of(item, "path").unwrap_or(name).to_string();
            match str_of(item, "type").unwrap_or_default() {
                "file" => {
                    if !is_safe_basename(name) {
                        ux::note(format!("skipping context file with an unsafe name: {path}"));
                        continue;
                    }
                    if let Some(dl) = str_of(item, "download_url") {
                        out.push(RemoteFile {
                            name: name.to_string(),
                            path,
                            download_url: dl.to_string(),
                        });
                    }
                }
                "dir" => {
                    if let Some(sub) = str_of(item, "url") {
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

async fn fetch_listing(client: &reqwest::Client, url: &str) -> Result<Value> {
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
    use crate::skills::test_tree::TempTree;
    use std::path::PathBuf;

    /// Refuses instantly, so no timeout and no risk of hitting the real GitHub.
    const DEAD_API: &str = "http://127.0.0.1:1/contents";

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

    fn scene(tag: &str) -> TempTree {
        let t = TempTree::new(tag);
        t.write("package.json", b"{}");
        t.write("scene.json", b"{}");
        t
    }

    fn skill_md(scene: &TempTree) -> PathBuf {
        scene
            .0
            .join(crate::skills::SKILLS_DIR)
            .join(crate::skills::EMBEDDED[0].name)
            .join("SKILL.md")
    }

    #[tokio::test]
    async fn download_failure_still_installs_the_skill_and_succeeds() {
        let scene = scene("nonet");
        get_context_files(&scene.0, DEAD_API, false).await.unwrap();
        assert!(skill_md(&scene).is_file());
        assert!(
            !scene.0.join("dclcontext").exists(),
            "a failed listing must not leave an empty dclcontext/"
        );
    }

    #[tokio::test]
    async fn download_failure_does_not_wipe_an_existing_dclcontext() {
        let scene = scene("keep");
        scene.write("dclcontext/old.md", b"corpus");
        get_context_files(&scene.0, DEAD_API, false).await.unwrap();
        assert_eq!(
            std::fs::read(scene.0.join("dclcontext/old.md")).unwrap(),
            b"corpus"
        );
    }

    #[tokio::test]
    async fn offline_writes_the_skill_and_never_dials_out() {
        let scene = scene("offline");
        get_context_files(&scene.0, "http://198.51.100.1/contents", true)
            .await
            .unwrap();
        assert!(skill_md(&scene).is_file());
    }

    /// The command is scene-scoped: a `.claude/skills/` dropped in a random cwd
    /// is litter, not help.
    #[tokio::test]
    async fn non_project_directory_writes_nothing() {
        let dir = TempTree::new("bare");
        get_context_files(&dir.0, DEAD_API, false).await.unwrap();
        assert!(!dir.0.join(".claude").exists());
    }
}
