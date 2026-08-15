//! Agent skills carried inside the binary, and written into a scene offline.
//!
//! The source of truth is `skills/<name>/` at the crate root — ordinary files,
//! because that directory is what gets upstreamed and what
//! `~/.claude/skills/<name>` symlinks to while it is being written. Nothing
//! here rewrites or generates it; `include_str!` only takes a copy at compile
//! time so a released binary needs neither the checkout nor the network.
//!
//! **Where it lands: `<scene>/.claude/skills/<name>/`.** That is the directory
//! Claude Code walks when it opens a project, and the frontmatter `name` +
//! `description` in `SKILL.md` are what it matches a request against — so a
//! skill dropped there is discovered by the agent without the user knowing it
//! exists, which is the whole point of shipping it. `dclcontext/`, where
//! `get-context-files` puts the official corpus, has no such contract: it is a
//! flat pile of `.md` that only helps once someone thinks to point an agent at
//! it. Both land in the same command; only one of them is discoverable.
//!
//! The file list below is hand-written, and `embedded_matches_the_source_tree`
//! is what keeps it honest — add a reference file to `skills/` without adding
//! it here and that test fails rather than shipping a skill whose `SKILL.md`
//! points at a file the user does not have.

use crate::ux::{TrySteps, UserError};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Where a scene's project-local skills live, relative to the scene root.
pub const SKILLS_DIR: &str = ".claude/skills";

pub struct EmbeddedSkill {
    /// Directory name, and the frontmatter `name:` — Claude Code requires the
    /// two to agree (`skill_name_matches_its_directory` checks it).
    pub name: &'static str,
    /// (path relative to the skill directory, contents).
    pub files: &'static [(&'static str, &'static str)],
}

impl EmbeddedSkill {
    pub fn bytes(&self) -> usize {
        self.files.iter().map(|(_, body)| body.len()).sum()
    }
}

pub const EMBEDDED: &[EmbeddedSkill] = &[EmbeddedSkill {
    name: "migrate-smart-items-to-code",
    files: &[
        (
            "SKILL.md",
            include_str!("../skills/migrate-smart-items-to-code/SKILL.md"),
        ),
        (
            "references/actions.md",
            include_str!("../skills/migrate-smart-items-to-code/references/actions.md"),
        ),
        (
            "references/triggers.md",
            include_str!("../skills/migrate-smart-items-to-code/references/triggers.md"),
        ),
    ],
}];

/// Write every embedded skill into `<root>/.claude/skills/`, returning the
/// paths written relative to `root`.
///
/// Each skill directory is removed first. A reference file dropped between two
/// releases would otherwise survive forever in a scene that keeps re-running
/// this, and a stale reference is worse than a missing one: `SKILL.md` tells
/// the agent which reference files exist, so the two have to move together.
/// Nothing outside `.claude/skills/<name>/` is touched — a user's own skills
/// sit beside ours untouched.
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

/// The `name:` line of a `SKILL.md` YAML frontmatter block, if there is one.
///
/// Test-only: the frontmatter is fixed at compile time by `include_str!`, so
/// the invariant it guards (frontmatter `name` == directory name) is settled
/// when `cargo test` runs, and re-parsing it at install time would only be able
/// to fail on a build that never shipped.
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
mod tests {
    use super::*;

    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dcl-one-sdk-skills-test-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempTree(dir)
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn source_dir(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("skills")
            .join(name)
    }

    /// The `files` lists are hand-written. This is what stops one from going
    /// stale: every `.md` under `skills/<name>/` must be embedded, and every
    /// embedded entry must still exist on disk with byte-identical contents.
    #[test]
    fn embedded_matches_the_source_tree() {
        for skill in EMBEDDED {
            let root = source_dir(skill.name);
            let mut on_disk = Vec::new();
            let mut stack = vec![root.clone()];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(&dir).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path.extension().is_some_and(|e| e == "md") {
                        let rel = path.strip_prefix(&root).unwrap();
                        on_disk.push(rel.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
            on_disk.sort();
            let mut embedded: Vec<String> =
                skill.files.iter().map(|(rel, _)| rel.to_string()).collect();
            embedded.sort();
            assert_eq!(
                embedded, on_disk,
                "{}: skills/ and EMBEDDED disagree — add the file to src/skills.rs",
                skill.name
            );
            for (rel, body) in skill.files {
                let disk = std::fs::read_to_string(root.join(rel)).unwrap();
                assert_eq!(&disk, body, "{}/{rel} is not what is embedded", skill.name);
            }
        }
    }

    /// Claude Code keys discovery on the frontmatter, and refuses a skill whose
    /// `name` does not match its directory.
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

    /// Every reference file `SKILL.md` names has to be one we write. This is
    /// the failure the pruning in `install()` exists to prevent, checked
    /// statically instead of waiting for an agent to hit a missing path.
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
