//! Skip tsc when nothing it would read has changed: node plus tsc spend
//! ~200 ms confirming an unchanged scene. The stamp records every file the
//! last passing program read (from tsc's own tsbuildinfo), the tsconfig
//! chain, the compiler, and the set of sources under the project; it is
//! written only by a pass and deleted by a failure.

use crate::scene::Project;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Under a dot-dir so the watcher skips it and tsc cannot feed back into
/// the rebuild that started it.
pub const TSBUILDINFO: &str = ".dcl-cache/tsbuildinfo";
const STAMP: &str = ".dcl-cache/tscheck.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct Stamp {
    /// Size and mtime of tsc.js; hashing 9 MB every build is not worth it.
    tsc: String,
    files: Vec<Seen>,
    candidates: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq)]
struct Seen {
    rel: String,
    size: u64,
    mtime_ns: u128,
    hash: String,
}

fn stamp_path(root: &Path) -> PathBuf {
    root.join(STAMP)
}

/// `path` relative to `root` with `/` separators; a path outside stays absolute.
fn rel_str(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

fn mtime_ns(meta: &std::fs::Metadata) -> Option<u128> {
    let since = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(since.as_nanos())
}

fn fingerprint(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    Some(format!("{}:{}", meta.len(), mtime_ns(&meta)?))
}

fn hash_file(path: &Path) -> Option<String> {
    let digest = Sha256::digest(std::fs::read(path).ok()?);
    Some(digest.iter().map(|b| format!("{b:02x}")).collect())
}

fn seen(root: &Path, rel: &str) -> Option<Seen> {
    let path = root.join(rel);
    let meta = std::fs::metadata(&path).ok()?;
    Some(Seen {
        rel: rel.to_string(),
        size: meta.len(),
        mtime_ns: mtime_ns(&meta)?,
        hash: hash_file(&path)?,
    })
}

/// Same size and mtime, or same size and hash (a save without an edit).
fn still_same(root: &Path, was: &Seen) -> bool {
    let path = root.join(&was.rel);
    let Ok(meta) = std::fs::metadata(&path) else {
        return false;
    };
    meta.len() == was.size
        && (mtime_ns(&meta) == Some(was.mtime_ns)
            || hash_file(&path).as_deref() == Some(was.hash.as_str()))
}

/// Every TypeScript or JavaScript file under the project outside
/// dependencies, build output and dot-prefixed paths, sorted — a file added
/// since the pass is not in the stamp's file list, so the set is compared.
fn candidates(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                if !matches!(&*name, "node_modules" | "bin") {
                    walk(root, &path, out);
                }
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs")
            ) {
                out.push(rel_str(root, &path));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// tsbuildinfo's `fileNames`, relative to its own directory, re-based onto
/// the root; a file outside the root stays absolute.
fn program_files(root: &Path) -> Option<Vec<String>> {
    let info_path = root.join(TSBUILDINFO);
    let info: serde_json::Value = serde_json::from_slice(&std::fs::read(&info_path).ok()?).ok()?;
    let base = info_path.parent()?;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let names = info
        .get("fileNames")
        .or_else(|| info.get("program").and_then(|p| p.get("fileNames")))?
        .as_array()?;
    names
        .iter()
        .map(|name| {
            let abs = base.join(name.as_str()?);
            Some(rel_str(&root, &abs.canonicalize().unwrap_or(abs)))
        })
        .collect()
}

/// tsconfig.json and everything it extends; None when a link does not
/// resolve, and then tsc runs.
fn tsconfig_chain(project: &Project) -> Option<Vec<String>> {
    let mut chain = Vec::new();
    let mut current = project.root.join("tsconfig.json");
    for _ in 0..8 {
        chain.push(rel_str(&project.root, &current));
        let doc = crate::jsjson::parse(&std::fs::read_to_string(&current).ok()?).ok()?;
        let Some(next) = doc.get("extends").and_then(|e| e.as_str()) else {
            return Some(chain);
        };
        current = match next.starts_with('.') || next.starts_with('/') {
            true => current.parent()?.join(next),
            false => project
                .node_module(next)
                .or_else(|| project.node_module(&format!("{next}/tsconfig.json")))?,
        };
        if !current.is_file() {
            return None;
        }
    }
    None
}

pub fn unchanged(project: &Project, tsc: &Path) -> bool {
    let root = &project.root;
    let Ok(text) = std::fs::read(stamp_path(root)) else {
        return false;
    };
    let Ok(stamp) = serde_json::from_slice::<Stamp>(&text) else {
        return false;
    };
    fingerprint(tsc).as_deref() == Some(stamp.tsc.as_str())
        && !stamp.files.is_empty()
        && stamp.files.iter().all(|f| still_same(root, f))
        && candidates(root) == stamp.candidates
}

/// Best-effort: a stamp that cannot be written only costs the next build
/// its shortcut.
pub fn record(project: &Project, tsc: &Path) {
    let root = &project.root;
    let stamp = (|| {
        let mut files = program_files(root)?;
        for link in tsconfig_chain(project)? {
            if !files.contains(&link) {
                files.push(link);
            }
        }
        Some(Stamp {
            tsc: fingerprint(tsc)?,
            files: files
                .iter()
                .map(|rel| seen(root, rel))
                .collect::<Option<_>>()?,
            candidates: candidates(root),
        })
    })();
    let Some(stamp) = stamp else {
        return forget(root);
    };
    if let Ok(json) = serde_json::to_vec(&stamp) {
        let path = stamp_path(root);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, json);
    }
}

pub fn forget(root: &Path) {
    let _ = std::fs::remove_file(stamp_path(root));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(tag: &str) -> Project {
        let root =
            std::env::temp_dir().join(format!("dcl-one-sdk-stamp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in [
            "src",
            "node_modules/typescript/lib",
            "node_modules/@dcl/sdk/types",
            ".dcl-cache",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for (rel, text) in [
            ("node_modules/typescript/lib/tsc.js", "// tsc"),
            (
                "node_modules/@dcl/sdk/types/tsconfig.ecs7.json",
                r#"{ "compilerOptions": { "strict": true } }"#,
            ),
            (
                "tsconfig.json",
                r#"{ "extends": "@dcl/sdk/types/tsconfig.ecs7.json", "include": ["src/**/*.ts"] }"#,
            ),
            ("src/index.ts", "export const a = 1\n"),
            (
                TSBUILDINFO,
                r#"{ "fileNames": ["../src/index.ts"], "version": "6.0.3" }"#,
            ),
            ("scene.json", "{}"),
        ] {
            std::fs::write(root.join(rel), text).unwrap();
        }
        Project {
            root,
            scene_json: serde_json::json!({}),
        }
    }

    fn tsc(p: &Project) -> PathBuf {
        p.root.join("node_modules/typescript/lib/tsc.js")
    }

    #[test]
    fn the_stamp_stands_until_something_tsc_would_read_moves() {
        let p = scene("stands");
        assert!(!unchanged(&p, &tsc(&p)), "no stamp, no shortcut");
        record(&p, &tsc(&p));
        assert!(unchanged(&p, &tsc(&p)));

        let src = p.root.join("src/index.ts");
        let text = std::fs::read_to_string(&src).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        std::fs::write(&src, &text).unwrap();
        assert!(unchanged(&p, &tsc(&p)), "a touch is not a change");

        std::fs::write(&src, "export const a = 2\n").unwrap();
        assert!(!unchanged(&p, &tsc(&p)), "an edit runs tsc");
        record(&p, &tsc(&p));
        assert!(unchanged(&p, &tsc(&p)));

        std::fs::write(p.root.join("src/new.ts"), "export {}\n").unwrap();
        assert!(!unchanged(&p, &tsc(&p)), "a new source runs tsc");
        std::fs::remove_file(p.root.join("src/new.ts")).unwrap();
        assert!(unchanged(&p, &tsc(&p)));

        std::fs::write(
            p.root
                .join("node_modules/@dcl/sdk/types/tsconfig.ecs7.json"),
            r#"{ "compilerOptions": { "strict": false } }"#,
        )
        .unwrap();
        assert!(!unchanged(&p, &tsc(&p)), "the extended tsconfig counts");
        record(&p, &tsc(&p));

        std::fs::write(tsc(&p), "// tsc 2").unwrap();
        assert!(
            !unchanged(&p, &tsc(&p)),
            "another compiler is another check"
        );
        record(&p, &tsc(&p));
        assert!(unchanged(&p, &tsc(&p)));

        forget(&p.root);
        assert!(!unchanged(&p, &tsc(&p)), "a failure leaves no stamp");
        let _ = std::fs::remove_dir_all(&p.root);
    }

    #[test]
    fn only_sources_count_as_candidates() {
        let p = scene("candidates");
        record(&p, &tsc(&p));
        for rel in [
            "bin/index.js",
            ".dcl-one-scratch.ts",
            ".dcl-one/entrypoint.ts",
            "node_modules/x.ts",
        ] {
            let path = p.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        assert!(unchanged(&p, &tsc(&p)), "{:?}", candidates(&p.root));
        std::fs::create_dir_all(p.root.join("assets/scene")).unwrap();
        std::fs::write(p.root.join("assets/scene/entity-names.ts"), "").unwrap();
        assert!(!unchanged(&p, &tsc(&p)));
        let _ = std::fs::remove_dir_all(&p.root);
    }
}
