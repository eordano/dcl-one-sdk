use std::fmt;
use std::io::IsTerminal;
use std::path::Path;
use std::time::Duration;

static VERBOSE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_verbose(on: bool) {
    VERBOSE.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn verbose() -> bool {
    VERBOSE.load(std::sync::atomic::Ordering::Relaxed)
}

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
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl UserError {
    pub fn new(what: impl Into<String>, try_next: TrySteps) -> Self {
        UserError {
            what: what.into(),
            why: None,
            try_next: try_next.0,
            source: None,
        }
    }

    pub fn why(mut self, why: impl Into<String>) -> Self {
        self.why = Some(why.into());
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

fn tint(color: bool, sgr: &str, body: &str) -> String {
    match color {
        true => format!("\x1b[{sgr}m{body}\x1b[0m"),
        false => body.to_string(),
    }
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

fn arrow_line(color: bool, label: &str, text: &str) -> String {
    format!(
        "  {} {text}\n",
        tint(color, "36", &format!("\u{2192} {label}:"))
    )
}

fn write_block(out: &mut String, prefix: &str, sgr: &str, u: &UserError, color: bool) {
    out.push_str(&format!("{} {}\n", tint(color, sgr, prefix), u.what));
    for line in u.why.iter().flat_map(|why| why.lines()) {
        out.push_str(&format!("  {}\n", tint(color, "2", line)));
    }
    for step in &u.try_next {
        out.push_str(&arrow_line(color, "try", step));
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
        out.push_str(&arrow_line(
            color,
            "more",
            "re-run with --verbose for the full error chain",
        ));
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
        let counter = format!("[{}/{}]", self.next, self.total);
        println!(
            "{} {}",
            tint(stdout_color(), "1", &counter),
            message.as_ref()
        );
        self.next += 1;
    }
}

/// A step that redraws its elapsed time in place once it passes [`SLOW_AFTER`].
/// Terminal only: a carriage-return spinner in a piped log is noise, not
/// progress.
pub struct Slow {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

const SLOW_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

impl Slow {
    pub fn start(label: impl Into<String>) -> Self {
        let label = label.into();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        if !stdout_color() {
            return Slow { stop, handle: None };
        }
        let s = stop.clone();
        let handle = std::thread::spawn(move || {
            let began = std::time::Instant::now();
            let mut drawn = false;
            while !s.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let waited = began.elapsed();
                if waited < SLOW_AFTER {
                    continue;
                }
                use std::io::Write;
                print!("\r\x1b[2K  {label} {}s", waited.as_secs());
                let _ = std::io::stdout().flush();
                drawn = true;
            }
            if drawn {
                use std::io::Write;
                print!("\r\x1b[2K");
                let _ = std::io::stdout().flush();
            }
        });
        Slow {
            stop,
            handle: Some(handle),
        }
    }

    /// Stops the redraw and clears the line. Idempotent via Drop.
    pub fn finish(self) {}
}

impl Drop for Slow {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

pub fn note(message: impl AsRef<str>) {
    println!("{}", tint(stdout_color(), "2", message.as_ref()));
}

fn note_indented(sgr: &str, message: &str) {
    println!(
        "  {}",
        tint(stdout_color(), sgr, &format!("\u{2192} {message}"))
    );
}

/// An indented arrow in green, shaped like the `\u{2192} try:` lines of the failure
/// block still on screen above it: for something broken working again.
pub fn note_good(message: impl AsRef<str>) {
    note_indented("32", message.as_ref());
}

/// [`note_good`]'s arrow in [`note`]'s dim register: something that came up,
/// rather than something that recovered.
pub fn note_arrow(message: impl AsRef<str>) {
    note_indented("2", message.as_ref());
}

/// A scene's own error, as the running client saw it. Deliberately not a
/// `UserError`: those are OUR failures, in our voice. This is the developer's
/// TypeScript, so it leads with their source line.
pub fn scene_error(message: &str, at: &str, frames: &[crate::start::scene_logs::Frame]) {
    let color = stderr_color();
    let mut out = String::new();
    let dim = |s: &str| tint(color, "2", s);

    let blamed_ix = frames.iter().position(|f| f.is_user_code);
    let blamed = blamed_ix.map(|ix| &frames[ix]);
    let where_ = match blamed {
        Some(f) => format!("{}:{}", f.file, f.line),
        None => "your scene".to_string(),
    };
    match color {
        true => out.push_str(&format!(
            "\n  \x1b[1;31m\u{2718} scene error\x1b[0m \x1b[2min\x1b[0m \x1b[1m{where_}\x1b[0m"
        )),
        false => out.push_str(&format!("\n  x scene error in {where_}")),
    }
    if !at.is_empty() {
        out.push_str(&format!("  {}", dim(at)));
    }
    out.push('\n');
    out.push_str(&format!("    {}\n", sanitize(message)));

    if let Some(f) = blamed {
        for (n, text) in &f.window {
            let gutter = format!("{n:>5} \u{2502} ");
            let hot = *n == f.line;
            let painted = match hot {
                true => tint(color, "31", &gutter),
                false => dim(&gutter),
            };
            out.push_str(&format!("{painted}{}\n", sanitize(text)));
            if !hot {
                continue;
            }
            let lead = text.len() - text.trim_start().len();
            let pad = gutter.chars().count() + (f.col.saturating_sub(1) as usize).max(lead);
            out.push_str(&format!("{}{}\n", " ".repeat(pad), tint(color, "31", "^")));
        }
    }

    for (ix, f) in frames.iter().enumerate() {
        if Some(ix) == blamed_ix {
            continue;
        }
        let at = format!("    at {}:{}:{}", f.file, f.line, f.col);
        match f.is_user_code {
            true => {
                out.push_str(&format!("{at}\n"));
                if let Some((n, text)) = f.window.iter().find(|(n, _)| *n == f.line) {
                    out.push_str(&format!(
                        "{}{}\n",
                        dim(&format!("{n:>5} \u{2502} ")),
                        sanitize(text)
                    ));
                }
            }
            false => out.push_str(&format!("{}\n", dim(&at))),
        }
    }
    eprint!("{out}");
}

/// Client text comes off the wire, so it must not be able to move the cursor,
/// clear the screen or forge a line of our output.
fn sanitize(s: &str) -> String {
    const MAX: usize = 300;
    let mut out: String = s
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .take(MAX)
        .collect();
    if s.chars().count() > MAX {
        out.push('\u{2026}');
    }
    out
}

pub fn note_stderr(message: impl AsRef<str>) {
    eprintln!("{}", tint(stderr_color(), "2", message.as_ref()));
}

/// Re-open dim after a nested colour has reset it. Pass as `restore` to
/// [`fmt_elapsed_tinted`] from anything printed through [`note`].
pub const RESTORE_DIM: &str = "\x1b[2m";

/// Under 50ms is the cost of doing the work at all and gets no colour;
/// colouring every number trains the eye to ignore all of them.
fn elapsed_sgr(d: Duration) -> Option<&'static str> {
    match d {
        d if d > Duration::from_millis(200) => Some("31"),
        d if d > Duration::from_millis(50) => Some("33"),
        _ => None,
    }
}

pub fn elapsed_is_notable(d: Duration) -> bool {
    elapsed_sgr(d).is_some()
}

/// `restore` is re-emitted after the colour resets, because a nested `\x1b[0m`
/// clears the surrounding style too. Pass `""` from a default-styled line,
/// [`RESTORE_DIM`] from a dim one.
pub fn fmt_elapsed_tinted(d: Duration, restore: &str) -> String {
    tinted(d, restore, stdout_color())
}

/// Colour is a parameter, not ambient tty state: reading the tty here left the
/// tinted branch untestable, so the assertion passed under redirected output
/// while proving nothing, and failed in a terminal.
fn tinted(d: Duration, restore: &str, color: bool) -> String {
    let text = fmt_elapsed(d);
    match (color, elapsed_sgr(d)) {
        (true, Some(sgr)) => format!("\x1b[{sgr}m{text}\x1b[0m{restore}"),
        _ => text,
    }
}

/// A duration at three significant figures, in the largest unit that keeps it
/// above 1. The fourth digit of a wall-clock measurement is scheduler noise.
pub fn fmt_elapsed(d: Duration) -> String {
    fn sig3(v: f64) -> String {
        match v {
            v if v < 10.0 => format!("{v:.2}"),
            v if v < 100.0 => format!("{v:.1}"),
            _ => format!("{v:.0}"),
        }
    }
    fn prints_below(v: f64, limit: f64) -> bool {
        sig3(v).parse::<f64>().unwrap_or(v) < limit
    }

    let secs = d.as_secs_f64();
    let ms = secs * 1_000.0;
    if ms < 1.0 {
        return format!("{}\u{00b5}s", d.as_micros());
    }
    if ms < 1_000.0 && prints_below(ms, 1_000.0) {
        return format!("{} ms", sig3(ms));
    }
    if secs < 60.0 && prints_below(secs, 60.0) {
        return format!("{} sec", sig3(secs));
    }
    let whole_secs = secs.round() as u64;
    if whole_secs < 3_600 {
        return format!("{}min {}sec", whole_secs / 60, whole_secs % 60);
    }
    let whole_mins = (secs / 60.0).round() as u64;
    format!("{}hr {}min", whole_mins / 60, whole_mins % 60)
}

pub fn fmt_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    let v = n as f64;
    if v < KB {
        format!("{n}b")
    } else if v < KB * KB {
        format!("{:.1}kb", v / KB)
    } else if v < KB * KB * KB {
        format!("{:.1}mb", v / (KB * KB))
    } else {
        format!("{:.1}gb", v / (KB * KB * KB))
    }
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
    fn fmt_elapsed_tiers() {
        assert_eq!(fmt_elapsed(Duration::from_micros(320)), "320\u{00b5}s");
        assert_eq!(fmt_elapsed(Duration::from_micros(999)), "999\u{00b5}s");
        assert_eq!(fmt_elapsed(Duration::from_micros(1_000)), "1.00 ms");

        assert_eq!(fmt_elapsed(Duration::from_micros(1_320)), "1.32 ms");
        assert_eq!(fmt_elapsed(Duration::from_micros(12_400)), "12.4 ms");
        assert_eq!(fmt_elapsed(Duration::from_millis(143)), "143 ms");
        assert_eq!(fmt_elapsed(Duration::from_millis(1_230)), "1.23 sec");
        assert_eq!(fmt_elapsed(Duration::from_millis(12_500)), "12.5 sec");
        assert_eq!(fmt_elapsed(Duration::from_millis(59_900)), "59.9 sec");
        assert_eq!(fmt_elapsed(Duration::from_secs(84)), "1min 24sec");
        assert_eq!(fmt_elapsed(Duration::from_secs(10_920)), "3hr 2min");

        assert_eq!(fmt_elapsed(Duration::from_micros(999_700)), "1.00 sec");
        assert_eq!(fmt_elapsed(Duration::from_millis(59_970)), "1min 0sec");
        assert_eq!(fmt_elapsed(Duration::from_millis(3_599_700)), "1hr 0min");

        assert_eq!(fmt_elapsed(Duration::from_millis(999)), "999 ms");
        assert_eq!(fmt_elapsed(Duration::from_secs(1)), "1.00 sec");
        assert_eq!(fmt_elapsed(Duration::from_secs(60)), "1min 0sec");
        assert_eq!(fmt_elapsed(Duration::from_secs(3_600)), "1hr 0min");
        assert_eq!(fmt_elapsed(Duration::from_secs(3_599)), "59min 59sec");
    }

    #[test]
    fn only_a_duration_worth_worrying_about_gets_a_colour() {
        assert_eq!(elapsed_sgr(Duration::from_millis(50)), None);
        assert_eq!(elapsed_sgr(Duration::from_micros(999)), None);
        assert_eq!(elapsed_sgr(Duration::from_millis(51)), Some("33"));
        assert_eq!(elapsed_sgr(Duration::from_millis(200)), Some("33"));
        assert_eq!(elapsed_sgr(Duration::from_millis(201)), Some("31"));
        assert_eq!(elapsed_sgr(Duration::from_secs(3)), Some("31"));

        for d in [Duration::from_millis(5), Duration::from_secs(3)] {
            assert_eq!(tinted(d, RESTORE_DIM, false), fmt_elapsed(d));
        }
        assert_eq!(
            tinted(Duration::from_secs(3), RESTORE_DIM, true),
            "\x1b[31m3.00 sec\x1b[0m\x1b[2m"
        );
        assert_eq!(
            tinted(Duration::from_millis(120), "", true),
            "\x1b[33m120 ms\x1b[0m"
        );
        assert_eq!(
            tinted(Duration::from_millis(5), RESTORE_DIM, true),
            "5.00 ms"
        );
    }

    #[test]
    fn fmt_bytes_tiers() {
        assert_eq!(fmt_bytes(0), "0b");
        assert_eq!(fmt_bytes(512), "512b");
        assert_eq!(fmt_bytes(33866), "33.1kb");
        assert_eq!(fmt_bytes(1_258_291), "1.2mb");
        assert_eq!(fmt_bytes(2_684_354_560), "2.5gb");
    }

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
