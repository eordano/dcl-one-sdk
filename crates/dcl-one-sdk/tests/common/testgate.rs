//! The four `catalyrst-testgate` primitives dcl-one-sdk's suites gate on,
//! copied verbatim so the standalone workspace does not carry that crate.
//! Behaviour and messages match the monorepo crate; keep them in step.

pub const OPT_OUT: &str = "ALLOW_SKIPPED_INTEGRATION";
pub const SKIP_LOG: &str = "CATALYRST_TESTGATE_SKIPLOG";

fn opt_out_is_set(raw: Option<&str>) -> bool {
    match raw {
        Some(v) => !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"),
        None => false,
    }
}

fn skips_allowed() -> bool {
    opt_out_is_set(std::env::var(OPT_OUT).ok().as_deref())
}

fn current_test() -> String {
    std::thread::current()
        .name()
        .filter(|n| *n != "main")
        .unwrap_or("<unnamed test>")
        .to_string()
}

fn refusal(requirement: &str, detail: &str) -> String {
    format!(
        "integration dependency unavailable: {requirement}\n  {detail}\n  \
         this test asserts nothing without it, so it fails instead of passing.\n  \
         provide {requirement}, or set {OPT_OUT}=1 to let it skip on a machine that cannot run it."
    )
}

pub fn unavailable<T>(requirement: &str, detail: &str) -> Option<T> {
    if !skips_allowed() {
        panic!("{}", refusal(requirement, detail));
    }
    record_skip(requirement, detail);
    None
}

fn env_value(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

pub fn require_env(var: &str) -> Option<String> {
    match env_value(var) {
        Some(v) => Some(v),
        None => unavailable(var, "the variable is unset"),
    }
}

/// The one line an operator gets in place of an assertion. It has to name the
/// test, because the harness line right after it says `ok` and that is the only
/// other thing on screen.
fn skip_notice(test: &str, requirement: &str, detail: &str) -> String {
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
/// the "skip masquerading as a pass" this module exists to prevent. Writing to
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
