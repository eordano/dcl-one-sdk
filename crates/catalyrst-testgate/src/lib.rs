pub const OPT_OUT: &str = "ALLOW_SKIPPED_INTEGRATION";
pub const SKIP_LOG: &str = "CATALYRST_TESTGATE_SKIPLOG";
pub const SHARED_PG: &str = "CATALYRST_TEST_PG";

pub fn opt_out_is_set(raw: Option<&str>) -> bool {
    match raw {
        Some(v) => !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"),
        None => false,
    }
}

pub fn skips_allowed() -> bool {
    opt_out_is_set(std::env::var(OPT_OUT).ok().as_deref())
}

pub fn current_test() -> String {
    std::thread::current()
        .name()
        .filter(|n| *n != "main")
        .unwrap_or("<unnamed test>")
        .to_string()
}

pub fn refusal(requirement: &str, detail: &str) -> String {
    format!(
        "integration dependency unavailable: {requirement}\n  {detail}\n  \
         this test asserts nothing without it, so it fails instead of passing.\n  \
         provide {requirement}, or set {OPT_OUT}=1 to let it skip on a machine that cannot run it."
    )
}

pub fn breakage(requirement: &str, detail: &str) -> String {
    format!(
        "integration dependency configured but unusable: {requirement}\n  {detail}\n  \
         {OPT_OUT} does not cover this: a dependency you explicitly pointed the suite at \
         must work."
    )
}

pub fn unavailable<T>(requirement: &str, detail: &str) -> Option<T> {
    if !skips_allowed() {
        panic!("{}", refusal(requirement, detail));
    }
    record_skip(requirement, detail);
    None
}

pub fn unusable<T>(var: &str, detail: &str) -> Option<T> {
    if std::env::var_os(var).is_some() {
        panic!("{}", breakage(var, detail));
    }
    unavailable(var, detail)
}

pub fn env_value(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

pub fn require_env(var: &str) -> Option<String> {
    match env_value(var) {
        Some(v) => Some(v),
        None => unavailable(var, "the variable is unset"),
    }
}

pub fn require_env_or(var: &str, fallback: &str) -> String {
    env_value(var).unwrap_or_else(|| fallback.to_string())
}

pub fn pg_requirement(var: &str) -> String {
    format!("{var} (or the workspace-wide {SHARED_PG})")
}

pub fn require_pg(var: &str) -> Option<String> {
    match env_value(var).or_else(|| env_value(SHARED_PG)) {
        Some(v) => Some(v),
        None => unavailable(&pg_requirement(var), "neither variable is set"),
    }
}

pub fn require_pg_or(var: &str, fallback: &str) -> String {
    env_value(var)
        .or_else(|| env_value(SHARED_PG))
        .unwrap_or_else(|| fallback.to_string())
}

pub fn pg_unusable<T>(var: &str, detail: &str) -> Option<T> {
    if env_value(var).is_some() || env_value(SHARED_PG).is_some() {
        panic!("{}", breakage(&pg_requirement(var), detail));
    }
    unavailable(&pg_requirement(var), detail)
}

/// The one line an operator gets in place of an assertion. It has to name the
/// test, because the harness line right after it says `ok` and that is the only
/// other thing on screen.
pub fn skip_notice(test: &str, requirement: &str, detail: &str) -> String {
    format!(
        "SKIPPED {test}: {requirement} unavailable ({detail}); \
         {OPT_OUT} is set, so this test asserted NOTHING and still reports ok\n"
    )
}

/// Writes straight to the stderr *file descriptor*, bypassing the thread-local
/// sink libtest installs.
///
/// `eprintln!` goes through `std::io::_eprint`, which libtest redirects into a
/// per-test capture buffer and then DISCARDS for any test that passes. A skip
/// passes by construction, so a skip notice printed with `eprintln!` is never
/// seen: the operator gets `test x ... ok` and nothing else, which is exactly
/// the "skip masquerading as a pass" this crate exists to prevent. Writing to
/// fd 2 is not captured, so the notice survives the default `cargo test`.
fn emit_uncaptured(msg: &str) {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::fd::FromRawFd;
        // ManuallyDrop: this borrows fd 2, it must never close it.
        let mut fd2 = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(2) });
        if fd2.write_all(msg.as_bytes()).is_ok() {
            return;
        }
    }
    eprint!("{msg}");
}

fn record_skip(requirement: &str, detail: &str) {
    let test = current_test();
    emit_uncaptured(&skip_notice(&test, requirement, detail));
    let Ok(path) = std::env::var(SKIP_LOG) else {
        return;
    };
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        // One `write_all` of one buffer, not `writeln!`. `Write for File` turns a
        // format string into one syscall per fragment, so two tests skipping at the
        // same time interleave mid-record and both records are lost. libtest runs
        // tests in parallel by default, so that is the normal case, not the rare
        // one -- and the skiplog is the artifact that is supposed to be read
        // *instead of* the pass tally. A record that corrupts under load is worse
        // than no record, because the tally still says "ok".
        let line = format!("{test}\t{requirement}\t{detail}\n");
        let _ = f.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_the_missing_variable_and_the_opt_out() {
        let m = refusal("CATALYRST_EXAMPLE_TEST_PG", "the variable is unset");
        assert!(m.contains("CATALYRST_EXAMPLE_TEST_PG"), "{m}");
        assert!(m.contains(OPT_OUT), "{m}");
        assert!(m.contains("fails instead of passing"), "{m}");
    }

    #[test]
    fn breakage_is_explicitly_not_covered_by_the_opt_out() {
        let m = breakage("CATALYRST_EXAMPLE_TEST_PG", "connection refused");
        assert!(m.contains("connection refused"), "{m}");
        assert!(m.contains("does not cover this"), "{m}");
    }

    #[test]
    fn only_a_deliberate_value_opens_the_escape_hatch() {
        assert!(!opt_out_is_set(None));
        assert!(!opt_out_is_set(Some("")));
        assert!(!opt_out_is_set(Some("0")));
        assert!(!opt_out_is_set(Some("false")));
        assert!(!opt_out_is_set(Some("False")));
        assert!(opt_out_is_set(Some("1")));
        assert!(opt_out_is_set(Some("yes")));
    }

    #[test]
    fn a_pg_requirement_names_both_the_crate_var_and_the_shared_one() {
        let r = pg_requirement("CATALYRST_EXAMPLE_TEST_PG");
        assert!(r.contains("CATALYRST_EXAMPLE_TEST_PG"), "{r}");
        assert!(r.contains(SHARED_PG), "{r}");
    }

    #[test]
    fn a_skip_notice_names_the_test_and_says_it_asserted_nothing() {
        let n = skip_notice(
            "drive_a_live_tunnel_origin",
            "DCL1_TUNNEL_PUBLIC_URL",
            "unset",
        );
        assert!(n.contains("drive_a_live_tunnel_origin"), "{n}");
        assert!(n.contains("DCL1_TUNNEL_PUBLIC_URL"), "{n}");
        assert!(n.contains("asserted NOTHING"), "{n}");
        assert!(n.ends_with('\n'), "{n:?}");
    }

    #[test]
    fn a_missing_dependency_panics_unless_the_hatch_is_open() {
        let outcome = std::panic::catch_unwind(|| {
            unavailable::<()>("CATALYRST_TESTGATE_SELFTEST", "the variable is unset")
        });
        if skips_allowed() {
            assert_eq!(outcome.expect("must not panic under the opt-out"), None);
        } else {
            assert!(
                outcome.is_err(),
                "a missing dependency returned instead of failing the test"
            );
        }
    }
}
