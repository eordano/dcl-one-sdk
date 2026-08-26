//! The contract `cargo test -p dcl-one-sdk` is supposed to keep, pinned.
//!
//! Three claims, each of which was false before this file existed:
//!
//! 1. A test that could not run is **visible**. Either the harness prints
//!    `ignored, <reason>`, or — when the runtime opt-out is open — a `SKIPPED`
//!    line reaches the operator. libtest discards the captured stderr of every
//!    passing test, and a skip passes by construction, so a notice printed with
//!    `eprintln!` is seen by nobody: the operator gets `test x ... ok` and a
//!    tally that counts a test which asserted nothing.
//! 2. Asking for a gated suite and not getting it is a **failure**, naming the
//!    variable — not a green run.
//! 3. Every variable that gates real coverage is written down where a
//!    contributor will find it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A requirement no machine can satisfy, and deliberately outside every prefix
/// [`documented_prefixes`] treats as a gate: this is the harness testing itself,
/// not a knob anyone should set.
const IMPOSSIBLE: &str = "A_RESOURCE_THIS_MACHINE_DOES_NOT_HAVE";

/// Runs only as a child of the tests below, which pass `--include-ignored`.
#[test]
#[ignore = "probe for the tests in this file; it exists to be skipped, not to assert"]
fn testgate_probe_that_can_never_find_its_dependency() {
    let Some(_) = catalyrst_testgate::require_env(IMPOSSIBLE) else {
        return;
    };
    panic!("{IMPOSSIBLE} was actually set; this probe is meaningless");
}

fn run_probe(opt_out: Option<&str>) -> Output {
    let mut cmd = Command::new(std::env::current_exe().expect("current exe"));
    cmd.args([
        "testgate_probe_that_can_never_find_its_dependency",
        "--exact",
        "--include-ignored",
        "--test-threads=1",
    ]);
    cmd.env_remove(IMPOSSIBLE);
    cmd.env_remove(catalyrst_testgate::SKIP_LOG);
    match opt_out {
        Some(v) => cmd.env(catalyrst_testgate::OPT_OUT, v),
        None => cmd.env_remove(catalyrst_testgate::OPT_OUT),
    };
    cmd.output().expect("re-running this test binary")
}

#[test]
fn a_skip_is_announced_where_libtest_cannot_swallow_it() {
    let out = run_probe(Some("1"));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stdout.contains("1 passed"),
        "the probe was supposed to skip and report ok\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("SKIPPED"),
        "a skip reported `ok` and said nothing about it. This is the whole defect: \
         libtest captures a passing test's stderr and throws it away, so the notice has to \
         go to the stderr fd directly.\nstdout: {stdout}\nstderr: {stderr:?}"
    );
    assert!(
        stderr.contains("testgate_probe_that_can_never_find_its_dependency"),
        "the notice must name the test that did not run\nstderr: {stderr}"
    );
    assert!(
        stderr.contains(IMPOSSIBLE),
        "the notice must name the missing requirement\nstderr: {stderr}"
    );
}

#[test]
fn without_the_opt_out_a_missing_dependency_fails_and_names_the_variable() {
    let out = run_probe(None);
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !out.status.success(),
        "a gated test with no dependency and no opt-out reported success\n{all}"
    );
    assert!(all.contains(IMPOSSIBLE), "{all}");
    assert!(all.contains(catalyrst_testgate::OPT_OUT), "{all}");
}

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn test_sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![tests_dir()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("reading tests/") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let name = path
                    .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")))
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                out.push((
                    name,
                    std::fs::read_to_string(&path).expect("reading a test"),
                ));
            }
        }
    }
    assert!(out.len() > 5, "found almost no test sources: {out:?}");
    out
}

#[test]
fn every_ignored_test_names_a_reason() {
    let mut bare = Vec::new();
    for (name, src) in test_sources() {
        for (i, line) in src.lines().enumerate() {
            if line.trim() == "#[ignore]" {
                bare.push(format!("{name}:{}", i + 1));
            }
        }
    }
    assert!(
        bare.is_empty(),
        "`#[ignore]` with no reason prints a bare `ignored` and tells nobody what is missing \
         or how to run it. Use `#[ignore = \"needs <VAR>: <why>; see docs/testing.md\"]`.\n  {}",
        bare.join("\n  ")
    );
}

/// Prefixes that mark a variable as gating real coverage rather than being a
/// product knob a test happens to set.
fn documented_prefixes() -> [&'static str; 5] {
    [
        "DCL_ONE_SDK_TEST_",
        "DCL1_TUNNEL_",
        "ALLOW_SKIPPED_INTEGRATION",
        "CATALYRST_TESTGATE_",
        "UPDATE_GOLDEN",
    ]
}

fn gating_vars_used() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for (_, src) in test_sources() {
        let bytes: Vec<char> = src.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_uppercase() {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == '_') {
                    i += 1;
                }
                let word: String = bytes[start..i].iter().collect();
                if documented_prefixes().iter().any(|p| word.starts_with(p))
                    && !found.contains(&word)
                {
                    found.push(word);
                }
            } else {
                i += 1;
            }
        }
    }
    found.sort();
    found
}

#[test]
fn every_gating_env_var_is_documented() {
    let doc_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("testing.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("{} must exist: {e}", doc_path.display()));
    let missing: Vec<String> = gating_vars_used()
        .into_iter()
        .filter(|v| !doc.contains(v.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "these variables gate coverage in tests/ but are invisible to a contributor \
         — document them in {}:\n  {}",
        doc_path.display(),
        missing.join("\n  ")
    );
    assert!(
        !gating_vars_used().is_empty(),
        "the scanner found no gating variables at all, so this test asserts nothing"
    );
}
