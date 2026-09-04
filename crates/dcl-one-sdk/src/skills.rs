//! Agent skills carried inside the binary and written into a scene offline.
//!
//! The source of truth is `skills/<name>/` at the crate root (what gets
//! upstreamed, and what `~/.claude/skills/<name>` symlinks to while it is being
//! written); build.rs generates the `EMBEDDED` table from it, so a released
//! binary needs neither the checkout nor the network. Skills land in
//! `<scene>/.claude/skills/<name>/` because that is the directory Claude Code
//! walks and matches `SKILL.md` frontmatter against — unlike `dclcontext/`,
//! which only helps once someone points an agent at it.

use crate::ux::{TrySteps, UserError};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Where a scene's project-local skills live, relative to the scene root.
pub const SKILLS_DIR: &str = ".claude/skills";

pub struct EmbeddedSkill {
    /// Directory name, and the frontmatter `name:` — Claude Code requires the
    /// two to agree.
    pub name: &'static str,
    /// (path relative to the skill directory, contents).
    pub files: &'static [(&'static str, &'static str)],
}

impl EmbeddedSkill {
    pub fn bytes(&self) -> usize {
        self.files.iter().map(|(_, body)| body.len()).sum()
    }
}

include!(concat!(env!("OUT_DIR"), "/skills_embedded.rs"));

/// Writes every embedded skill into `<root>/.claude/skills/`, returning the
/// paths written relative to `root`. Each skill directory is removed first:
/// `SKILL.md` names its reference files, so a file dropped between releases
/// must not outlive it. Other skills beside ours are untouched.
pub fn install(root: &Path) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for skill in EMBEDDED {
        let dir = root.join(SKILLS_DIR).join(skill.name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| skill_io_error(&dir, e))?;
        }
        for (rel, body) in skill.files {
            let path = dir.join(rel);
            let parent = path.parent().unwrap_or(&dir);
            std::fs::create_dir_all(parent).map_err(|e| skill_io_error(parent, e))?;
            std::fs::write(&path, body).map_err(|e| skill_io_error(&path, e))?;
            #[cfg(unix)]
            if rel.ends_with(".sh") {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| skill_io_error(&path, e))?;
            }
            written.push(PathBuf::from(SKILLS_DIR).join(skill.name).join(rel));
        }
    }
    Ok(written)
}

fn skill_io_error(path: &Path, e: std::io::Error) -> anyhow::Error {
    UserError::new(
        format!("cannot write the bundled skill into {}", path.display()),
        TrySteps::one("check write permission on the project directory")
            .and("close any program holding files under .claude/skills/ open"),
    )
    .caused_by(e)
    .into()
}

/// The `name:` line of a leading `SKILL.md` YAML frontmatter block.
#[cfg(test)]
fn frontmatter_name(body: &str) -> Option<&str> {
    let rest = body.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    rest[..end]
        .lines()
        .find_map(|l| l.strip_prefix("name:"))
        .map(str::trim)
}

#[cfg(test)]
pub(crate) mod test_tree {
    use std::path::PathBuf;

    /// A fresh temp directory named by `tag` and the pid, removed on drop; tags
    /// must be unique across the crate's tests.
    pub struct TempTree(pub PathBuf);

    impl TempTree {
        pub fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("dcl-one-sdk-test-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempTree(dir)
        }

        pub fn write(&self, rel: &str, contents: &[u8]) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contents).unwrap();
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_tree::TempTree;
    use super::*;

    fn source_dir(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("skills")
            .join(name)
    }

    fn files_under(root: &Path) -> Vec<String> {
        let mut on_disk = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path.strip_prefix(root).unwrap();
                    on_disk.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        on_disk.sort();
        on_disk
    }

    /// Guards the build.rs walk going stale: every file under `skills/<name>/`
    /// must be embedded byte-identical.
    #[test]
    fn embedded_matches_the_source_tree() {
        let skills_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
        let mut on_disk_skills: Vec<String> = std::fs::read_dir(&skills_root)
            .unwrap()
            .flatten()
            .filter(|e| e.path().join("SKILL.md").is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        on_disk_skills.sort();
        let embedded_skills: Vec<String> = EMBEDDED.iter().map(|s| s.name.to_string()).collect();
        assert_eq!(
            embedded_skills, on_disk_skills,
            "skills/ and EMBEDDED disagree — rebuild, and check build.rs if it persists"
        );
        for skill in EMBEDDED {
            let root = source_dir(skill.name);
            let mut embedded: Vec<String> =
                skill.files.iter().map(|(rel, _)| rel.to_string()).collect();
            embedded.sort();
            assert_eq!(
                embedded,
                files_under(&root),
                "{}: skills/ and EMBEDDED disagree — rebuild, and check build.rs if it persists",
                skill.name
            );
            for (rel, body) in skill.files {
                let disk = std::fs::read_to_string(root.join(rel)).unwrap();
                assert_eq!(&disk, body, "{}/{rel} is not what is embedded", skill.name);
            }
        }
    }

    /// Claude Code refuses a skill whose frontmatter `name` differs from its
    /// directory.
    #[test]
    fn skill_name_matches_its_directory() {
        for skill in EMBEDDED {
            let (rel, body) = skill.files[0];
            assert_eq!(rel, "SKILL.md", "{} must lead with SKILL.md", skill.name);
            assert_eq!(frontmatter_name(body), Some(skill.name));
            assert!(
                body.contains("description:"),
                "{}: no frontmatter description — the agent cannot match it",
                skill.name
            );
        }
    }

    #[test]
    fn frontmatter_name_only_reads_a_leading_block() {
        assert_eq!(frontmatter_name("---\nname: a\n---\n# x"), Some("a"));
        assert_eq!(frontmatter_name("---\ndescription: d\n---\n"), None);
        assert_eq!(frontmatter_name("# x\n---\nname: a\n---\n"), None);
        assert_eq!(frontmatter_name(""), None);
    }

    #[test]
    fn skill_md_only_references_files_that_ship() {
        for skill in EMBEDDED {
            let body = skill.files[0].1;
            let shipped: Vec<&str> = skill.files.iter().map(|(rel, _)| *rel).collect();
            for token in body.split(|c: char| !(c.is_alphanumeric() || "._/-".contains(c))) {
                if token.starts_with("references/") && token.ends_with(".md") {
                    assert!(
                        shipped.contains(&token),
                        "{}: SKILL.md names {token}, which is not embedded",
                        skill.name
                    );
                }
            }
        }
    }

    #[test]
    fn install_writes_the_skill_under_dot_claude() {
        let tree = TempTree::new("install");
        let written = install(&tree.0).unwrap();
        assert!(!written.is_empty());
        for rel in &written {
            assert!(
                rel.starts_with(SKILLS_DIR),
                "{} escaped .claude",
                rel.display()
            );
            assert!(tree.0.join(rel).is_file());
        }
        let skill_md = tree
            .0
            .join(SKILLS_DIR)
            .join(EMBEDDED[0].name)
            .join("SKILL.md");
        assert_eq!(
            std::fs::read_to_string(skill_md).unwrap(),
            EMBEDDED[0].files[0].1
        );
    }

    #[test]
    fn install_prunes_stale_files_and_leaves_other_skills_alone() {
        let tree = TempTree::new("prune");
        let ours = tree.0.join(SKILLS_DIR).join(EMBEDDED[0].name);
        let theirs = tree.0.join(SKILLS_DIR).join("someone-elses-skill");
        std::fs::create_dir_all(ours.join("references")).unwrap();
        std::fs::create_dir_all(&theirs).unwrap();
        std::fs::write(ours.join("references/dropped.md"), b"old").unwrap();
        std::fs::write(theirs.join("SKILL.md"), b"mine").unwrap();

        install(&tree.0).unwrap();

        assert!(!ours.join("references/dropped.md").exists());
        assert!(ours.join("SKILL.md").is_file());
        assert_eq!(std::fs::read(theirs.join("SKILL.md")).unwrap(), b"mine");
    }

    #[test]
    fn install_is_idempotent() {
        let tree = TempTree::new("idem");
        let first = install(&tree.0).unwrap();
        let second = install(&tree.0).unwrap();
        assert_eq!(first, second);
    }
}
