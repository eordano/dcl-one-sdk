use std::fmt;
use std::io::IsTerminal;
use std::path::Path;
use std::time::Duration;

pub struct TrySteps(Vec<String>);

impl TrySteps {
    pub fn one(step: impl Into<String>) -> Self {
        TrySteps(vec![step.into()])
    }

    pub fn and(mut self, step: impl Into<String>) -> Self {
        self.0.push(step.into());
        self
    }
}

#[derive(Debug)]
pub struct UserError {
    what: String,
    why: Option<String>,
    try_next: Vec<String>,
    docs: Option<String>,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl UserError {
    pub fn new(what: impl Into<String>, try_next: TrySteps) -> Self {
        UserError {
            what: what.into(),
            why: None,
            try_next: try_next.0,
            docs: None,
            source: None,
        }
    }

    pub fn why(mut self, why: impl Into<String>) -> Self {
        self.why = Some(why.into());
        self
    }

    pub fn docs(mut self, url: impl Into<String>) -> Self {
        self.docs = Some(url.into());
        self
    }

    pub fn caused_by(
        mut self,
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        self.source = Some(source.into());
        self
    }
}

impl fmt::Display for UserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.what)
    }
}

impl std::error::Error for UserError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

fn color_allowed(is_tty: bool) -> bool {
    is_tty && std::env::var_os("NO_COLOR").is_none()
}

fn stderr_color() -> bool {
    color_allowed(std::io::stderr().is_terminal())
}

fn stdout_color() -> bool {
    color_allowed(std::io::stdout().is_terminal())
}

fn find_user(err: &anyhow::Error) -> Option<&UserError> {
    err.chain().find_map(|c| c.downcast_ref::<UserError>())
}

pub fn concise_cause(err: &anyhow::Error) -> String {
    let root = err.root_cause().to_string();
    let cleaned = match root.find(" (os error") {
        Some(ix) => root[..ix].to_string(),
        None => root,
    };
    if cleaned.to_lowercase().contains("connection refused") {
        return "connection refused".to_string();
    }
    cleaned
}

fn fallback(err: &anyhow::Error) -> UserError {
    UserError::new(
        err.to_string(),
        TrySteps::one("re-run with --verbose for the full error chain"),
    )
}

fn write_block(out: &mut String, prefix: &str, sgr: &str, u: &UserError, color: bool) {
    if color {
        out.push_str(&format!("\x1b[{sgr}m{prefix}\x1b[0m {}\n", u.what));
    } else {
        out.push_str(&format!("{prefix} {}\n", u.what));
    }
    if let Some(why) = &u.why {
        for line in why.lines() {
            if color {
                out.push_str(&format!("  \x1b[2m{line}\x1b[0m\n"));
            } else {
                out.push_str(&format!("  {line}\n"));
            }
        }
    }
    for step in &u.try_next {
        if color {
            out.push_str(&format!("  \x1b[36m\u{2192} try:\x1b[0m {step}\n"));
        } else {
            out.push_str(&format!("  \u{2192} try: {step}\n"));
        }
    }
    if let Some(docs) = &u.docs {
        out.push_str(&format!("  docs: {docs}\n"));
    }
}

pub fn render(err: &anyhow::Error, verbose: bool, color: bool) -> String {
    let mut out = String::new();
    match find_user(err) {
        Some(u) => write_block(&mut out, "Error:", "1;31", u, color),
        None => write_block(&mut out, "Error:", "1;31", &fallback(err), color),
    }
    if verbose {
        out.push_str("  caused by:\n");
        for (i, cause) in err.chain().enumerate() {
            out.push_str(&format!("    {i}: {cause}\n"));
        }
    } else if err.chain().count() > 1 && !out.contains("--verbose") {
        if color {
            out.push_str(
                "  \x1b[36m\u{2192} more:\x1b[0m re-run with --verbose for the full error chain\n",
            );
        } else {
            out.push_str("  \u{2192} more: re-run with --verbose for the full error chain\n");
        }
    }
    out
}

pub fn report(err: &anyhow::Error, verbose: bool) {
    eprint!("{}", render(err, verbose, stderr_color()));
}

pub fn report_watch(err: &anyhow::Error) {
    let color = stderr_color();
    let mut out = String::new();
    match find_user(err) {
        Some(u) => write_block(&mut out, "warning:", "1;33", u, color),
        None => write_block(&mut out, "warning:", "1;33", &fallback(err), color),
    }
    eprint!("{out}");
}

pub struct Steps {
    total: usize,
    next: usize,
}

impl Steps {
    pub fn new(total: usize) -> Self {
        Steps { total, next: 1 }
    }

    pub fn done(&mut self, message: impl AsRef<str>) {
        if stdout_color() {
            println!(
                "\x1b[1m[{}/{}]\x1b[0m {}",
                self.next,
                self.total,
                message.as_ref()
            );
        } else {
            println!("[{}/{}] {}", self.next, self.total, message.as_ref());
        }
        self.next += 1;
    }
}

pub fn note(message: impl AsRef<str>) {
    if stdout_color() {
        println!("\x1b[2m{}\x1b[0m", message.as_ref());
    } else {
        println!("{}", message.as_ref());
    }
}

pub fn note_stderr(message: impl AsRef<str>) {
    if stderr_color() {
        eprintln!("\x1b[2m{}\x1b[0m", message.as_ref());
    } else {
        eprintln!("{}", message.as_ref());
    }
}

pub fn fmt_elapsed(d: Duration) -> String {
    format!("{:.1}s", d.as_secs_f64())
}

pub fn rel_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

pub fn bundle_failed(body: &str) -> anyhow::Error {
    let body = body.trim_end();
    let cli_count = body.matches("[ERROR]").count();
    let loc_count = body.lines().filter(|l| loc_file(l).is_some()).count();
    let count = if cli_count > 0 {
        cli_count
    } else if loc_count > 0 {
        loc_count
    } else {
        1
    };
    let file = body.lines().find_map(loc_file);
    let what = match (&file, count) {
        (Some(f), 1) => format!("build failed \u{2014} 1 error in {f}"),
        (Some(f), n) => format!("build failed \u{2014} {n} errors (first: {f})"),
        (None, 1) => "build failed \u{2014} 1 error".to_string(),
        (None, n) => format!("build failed \u{2014} {n} errors"),
    };
    UserError::new(
        what,
        TrySteps::one("fix the error above, then save (watch mode) or re-run dcl-one-sdk build"),
    )
    .why(body)
    .into()
}

fn loc_file(line: &str) -> Option<String> {
    let mut parts = line.trim().split(':');
    let file = parts.next()?;
    let line_no = parts.next()?;
    let col = parts.next()?;
    if file.is_empty() || !file.contains('.') || file.contains(' ') {
        return None;
    }
    if line_no.is_empty() || !line_no.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if col.is_empty() || !col.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(file.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_error_renders_try_line() {
        let e: anyhow::Error = UserError::new("x", TrySteps::one("do y")).into();
        let out = render(&e, false, false);
        assert!(out.starts_with("Error: x"));
        assert!(out.contains("\n  \u{2192} try: do y"));
        assert!(!out.contains('\u{1b}'));
        assert!(!out.contains("caused by:"));
    }

    #[test]
    fn fallback_always_names_a_next_step() {
        let e = anyhow::anyhow!("mystery");
        let out = render(&e, false, false);
        assert!(out.starts_with("Error: mystery"));
        assert!(out.contains("\u{2192} try: re-run with --verbose"));
    }

    #[test]
    fn hidden_chain_advertises_verbose() {
        let e = anyhow::Error::from(UserError::new("x", TrySteps::one("do y")))
            .context("outer context");
        let out = render(&e, false, false);
        assert!(out.contains("\u{2192} more: re-run with --verbose for the full error chain"));
        let v = render(&e, true, false);
        assert!(v.contains("caused by:"));
        assert!(!v.contains("\u{2192} more:"));
        let flat: anyhow::Error = UserError::new("x", TrySteps::one("do y")).into();
        assert!(!render(&flat, false, false).contains("\u{2192} more:"));
    }

    #[test]
    fn why_lines_are_indented_between_what_and_try() {
        let e: anyhow::Error = UserError::new("w", TrySteps::one("s"))
            .why("line one\nline two")
            .into();
        let out = render(&e, false, false);
        assert_eq!(out, "Error: w\n  line one\n  line two\n  \u{2192} try: s\n");
    }

    #[test]
    fn verbose_appends_the_chain() {
        let e: anyhow::Error = UserError::new("x", TrySteps::one("y"))
            .caused_by(std::io::Error::other("boom"))
            .into();
        let out = render(&e, true, false);
        assert!(out.contains("  caused by:"));
        assert!(out.contains("boom"));
    }

    #[test]
    fn color_mode_styles_the_prefix() {
        let e: anyhow::Error = UserError::new("x", TrySteps::one("y")).into();
        let out = render(&e, false, true);
        assert!(out.starts_with("\x1b[1;31mError:\x1b[0m x"));
    }

    #[test]
    fn bundle_failed_summarizes_cli_stderr() {
        let body = "\u{2718} [ERROR] Expected \";\" but found \"=\"\n\n    src/index.ts:4:11:\n      4 \u{2502} const x = = 1\n        \u{2575}           ^\n\n1 error\n";
        let e = bundle_failed(body);
        assert_eq!(
            e.to_string(),
            "build failed \u{2014} 1 error in src/index.ts"
        );
        let rendered = render(&e, false, false);
        assert!(rendered.contains("const x = = 1"));
        assert!(rendered.contains("\u{2192} try: fix the error above"));
    }

    #[test]
    fn bundle_failed_summarizes_service_messages() {
        let body = "src/a.ts:1:2: boom\nsrc/b.ts:3:4: bam";
        assert_eq!(
            bundle_failed(body).to_string(),
            "build failed \u{2014} 2 errors (first: src/a.ts)"
        );
    }

    #[test]
    fn report_watch_uses_warning_prefix() {
        let e: anyhow::Error = UserError::new("x", TrySteps::one("y")).into();
        let mut out = String::new();
        write_block(&mut out, "warning:", "1;33", find_user(&e).unwrap(), false);
        assert!(out.starts_with("warning: x"));
        assert!(out.contains("\u{2192} try: y"));
    }
}
