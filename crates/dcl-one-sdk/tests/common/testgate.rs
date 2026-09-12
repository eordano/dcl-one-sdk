//! The four `catalyrst-testgate` primitives, copied verbatim so the standalone
//! workspace does not carry that crate; keep behaviour and messages in step.

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

/// Has to name the test: the harness line right after it says `ok`, and that
/// is the only other thing on screen.
fn skip_notice(test: &str, requirement: &str, detail: &str) -> String {
    format!(
        "SKIPPED {test}: {requirement} unavailable ({detail}); \
         {OPT_OUT} is set, so this test asserted NOTHING and still reports ok\n"
    )
}

/// Writes straight to the stderr *file descriptor*: `eprintln!` goes through
/// libtest's per-test capture buffer, which is DISCARDED for every passing
/// test — and a skip passes by construction, so the notice would reach nobody.
/// fd 2 is not captured.
fn emit_uncaptured(msg: &str) {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::fd::FromRawFd;
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
        let line = format!("{test}\t{requirement}\t{detail}\n");
        let _ = f.write_all(line.as_bytes());
    }
}
