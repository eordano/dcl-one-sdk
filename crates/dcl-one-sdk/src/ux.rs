use std::fmt;
use std::io::IsTerminal;
use std::path::Path;
use std::sync::{Mutex, PoisonError};
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

pub(crate) fn stdout_color() -> bool {
    color_allowed(std::io::stdout().is_terminal())
}

pub(crate) fn tint(color: bool, sgr: &str, body: &str) -> String {
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
    silent: bool,
}

impl Steps {
    pub fn new(total: usize) -> Self {
        Steps {
            total,
            next: 1,
            silent: false,
        }
    }

    /// The same accounting, none of the lines: for a run whose story is told
    /// somewhere else (a page-driven publish narrates on the page, not into
    /// the preview terminal).
    pub fn silent() -> Self {
        Steps {
            total: 0,
            next: 1,
            silent: true,
        }
    }

    pub fn done(&mut self, message: impl AsRef<str>) {
        if self.silent {
            return;
        }
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
    emit(tint(stdout_color(), "2", message.as_ref()));
}

fn note_indented(sgr: &str, message: &str) {
    marked("\u{2192}", sgr, message);
}

fn marked(mark: &str, sgr: &str, message: &str) {
    let body = tint(stdout_color(), sgr, &format!("{mark} {message}"));
    match session_gutter() {
        // Inside a watch session the two-space indent becomes the same
        // ten-column gutter the timestamped lines use, so a `!` or a `→`
        // hangs under the event that produced it rather than beside it.
        Some(g) => emit(format!("{g}{body}")),
        None => emit(format!("  {body}")),
    }
}

/// An indented `!` in the warning colour: for something that did not happen,
/// where the arrow would claim it did.
pub fn note_absent(message: impl AsRef<str>) {
    marked("!", "33", message.as_ref());
}

/// Wall-clock `HH:MM:SS`, local. A watch session scrolls for hours, and "did
/// that rebuild fire when I hit save, or is it from ten minutes ago" is only
/// answerable with a clock on the line.
pub fn clock_now() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

/// An event worth putting a clock on, printed with the time in a left gutter.
///
/// The stamp is printed only when the second CHANGES; a repeat becomes a `·`
/// continuation. A save fires a rebuild, a type check and a reload push within
/// the same second, and printing `15:11:22` four times reads as four moments
/// instead of one — the elision is what makes the burst legible as a burst.
pub fn note_clocked(message: impl AsRef<str>) {
    let body = tint(stdout_color(), "2", message.as_ref());
    match session_gutter_stamped() {
        Some(g) => emit(format!("{g}{body}")),
        None => emit(format!("{body} at {}", clock_now())),
    }
}

/// Width of `HH:MM:SS` plus its two trailing spaces. Every gutter — stamp,
/// continuation, or the blank one — is exactly this wide, or the column bends.
const GUTTER: usize = 10;

/// A re-float needs BOTH: a screenful of lines since the last one, AND this
/// long since the last one. Either alone misfires — a screenful can scroll
/// past in ten seconds during a burst of saves, and five quiet minutes can
/// pass with three lines on screen, where the address is still perfectly
/// visible. The line count when the terminal will not say its height —
/// stdout is a pipe, or the ioctl fails.
const FLOAT_EVERY_FALLBACK: usize = 100;
const FLOAT_AFTER: Duration = Duration::from_secs(5 * 60);

/// The line half of the threshold: one terminal height, because that is
/// exactly when the address crosses the top of the screen. Asked fresh each
/// time so a resized window changes the answer.
fn float_every() -> usize {
    let rows = terminal_size::terminal_size()
        .map(|(_, h)| h.0 as usize)
        .unwrap_or(0);
    every_from_rows(rows)
}

/// A height of zero is "unknown", not "tiny": that is what the ioctl reports
/// on a pipe. The floor keeps a genuinely tiny window from floating on every
/// few lines — the quiet period still gates it, but ten lines is already a
/// screenful nobody is reading addresses off of.
fn every_from_rows(rows: usize) -> usize {
    match rows {
        0 => FLOAT_EVERY_FALLBACK,
        r => r.max(10),
    }
}

/// Both conditions, in one place a test can reach without a clock or a
/// terminal.
fn should_float(lines: usize, every: usize, since: Duration) -> bool {
    lines >= every && since >= FLOAT_AFTER
}

static SESSION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SINCE_FLOAT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static LAST_STAMP: Mutex<String> = Mutex::new(String::new());
static SESSION_NOTE: Mutex<String> = Mutex::new(String::new());
static LAST_FLOAT: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Turn on the watch session's left gutter, and register the line to re-float
/// once a screenful of lines has scrolled it away ([`float_every`]).
///
/// A preview runs for hours and its address scrolls away in the first minute;
/// by the time someone wants to open it on a phone the banner is a thousand
/// lines up. Off for one-shot commands, whose whole output is one event.
pub fn set_session_note(note: impl Into<String>) {
    *SESSION_NOTE.lock().unwrap_or_else(PoisonError::into_inner) = note.into();
    // The banner just printed the address, so the clock starts now: the first
    // re-float is five minutes away, not one burst away.
    *LAST_FLOAT.lock().unwrap_or_else(PoisonError::into_inner) = Some(std::time::Instant::now());
    SESSION.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn in_session() -> bool {
    SESSION.load(std::sync::atomic::Ordering::Relaxed)
}

/// The gutter for a continuation line: blank, with a `·` holding the column.
fn session_gutter() -> Option<String> {
    in_session().then(|| format!("{:>width$}\u{b7} ", "", width = GUTTER - 2))
}

/// The gutter for an event: the clock if it has moved since the last one,
/// otherwise the same continuation marker.
fn session_gutter_stamped() -> Option<String> {
    if !in_session() {
        return None;
    }
    let now = clock_now();
    let mut last = LAST_STAMP.lock().unwrap_or_else(PoisonError::into_inner);
    if *last == now {
        return session_gutter();
    }
    *last = now.clone();
    Some(format!("{now}  "))
}

/// Every printed line goes through here, so the re-float can count them.
fn emit(line: String) {
    println!("{line}");
    if !in_session() {
        return;
    }
    let n = SINCE_FLOAT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let mut last = LAST_FLOAT.lock().unwrap_or_else(PoisonError::into_inner);
    let since = last.map(|t| t.elapsed()).unwrap_or(FLOAT_AFTER);
    if !should_float(n, float_every(), since) {
        // The line counter is NOT reset here. Once it is past the threshold it
        // stays past, so the float happens the moment the quiet period is also
        // satisfied rather than waiting for another hundred lines after it.
        return;
    }
    SINCE_FLOAT.store(0, std::sync::atomic::Ordering::Relaxed);
    *last = Some(std::time::Instant::now());
    drop(last);
    let note = SESSION_NOTE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if !note.is_empty() {
        // At column 0, outside the gutter: this is not an event in the stream,
        // it is the stream pausing to say where it lives. `\u{2302}` is a
        // house — an address, the one thing this line is for — and it is not
        // used anywhere else in the output, so it cannot be mistaken for a
        // rebuild, a warning or a continuation.
        //
        // Straight to println: routing it through `emit` would count the
        // re-float itself and eventually float on top of a float.
        println!("{}", tint(stdout_color(), "2", &format!("\u{2302} {note}")));
    }
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
/// The three shapes a scene log entry can take on screen. They are three and
/// not one because the client records a thrown error and a `console.error`
/// under the same prefix: printing a logged line as a crash blames a line that
/// may have handled it perfectly (see `scene_logs::Origin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneNote {
    /// Nobody caught it. The loud one.
    Thrown,
    /// The scene called `console.error`. Its own words, not a diagnosis.
    LoggedError,
    /// The scene called `console.warn`.
    LoggedWarning,
}

impl SceneNote {
    /// Marker, label, preposition and SGR colour. The preposition carries the
    /// distinction as much as the marker: a throw happened *in* a line, a log
    /// was emitted *from* one.
    fn shape(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            SceneNote::Thrown => ("\u{2718}", "scene error", "in", "31"),
            SceneNote::LoggedError => ("\u{25cf}", "console.error", "from", "33"),
            SceneNote::LoggedWarning => ("\u{25cf}", "console.warn", "from", "33"),
        }
    }

    fn plain(self) -> &'static str {
        match self {
            SceneNote::Thrown => "x",
            _ => "-",
        }
    }
}

/// The headline, split out so a test can read it without a terminal: the
/// colour must be a parameter, not `stderr_color()`, because a test whose
/// stdout is a pipe would otherwise assert on an empty escape sequence and
/// pass for the wrong reason.
fn scene_headline(kind: SceneNote, where_: &str, color: bool) -> String {
    let (mark, label, prep, sgr) = kind.shape();
    match color {
        true => format!(
            "\n  \x1b[1;{sgr}m{mark} {label}\x1b[0m \x1b[2m{prep}\x1b[0m \x1b[1m{where_}\x1b[0m"
        ),
        false => format!("\n  {} {label} {prep} {where_}", kind.plain()),
    }
}

pub fn scene_note(
    kind: SceneNote,
    message: &str,
    at: &str,
    frames: &[crate::start::scene_logs::Frame],
) {
    let color = stderr_color();
    let mut out = String::new();
    let dim = |s: &str| tint(color, "2", s);

    let blamed_ix = frames.iter().position(|f| f.is_user_code);
    let blamed = blamed_ix.map(|ix| &frames[ix]);
    let where_ = match blamed {
        Some(f) => format!("{}:{}", f.file, f.line),
        None => "your scene".to_string(),
    };
    out.push_str(&scene_headline(kind, &where_, color));
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

    /// Both conditions, and neither alone. The AND is the whole design: a
    /// screenful can scroll in ten seconds during a burst of saves, and
    /// five quiet minutes can pass with three lines on screen.
    #[test]
    fn a_refloat_needs_both_the_lines_and_the_quiet() {
        let quiet = FLOAT_AFTER;
        let recent = FLOAT_AFTER - Duration::from_secs(1);
        assert!(should_float(50, 50, quiet), "both satisfied");
        assert!(should_float(250, 50, quiet), "well past both");
        assert!(
            !should_float(50, 50, recent),
            "a burst of lines inside the quiet period must not float"
        );
        assert!(
            !should_float(49, 50, quiet),
            "a long quiet stretch with little output must not float"
        );
        assert!(!should_float(0, 50, Duration::ZERO));
    }

    /// The line threshold is the screen: a taller window scrolls the address
    /// away later. Zero is a pipe saying nothing, not a zero-row terminal,
    /// and a toy-sized window still gets a floor the quiet period paces.
    #[test]
    fn the_line_threshold_is_one_screen_height() {
        assert_eq!(every_from_rows(50), 50, "a screenful of a 50-row window");
        assert_eq!(every_from_rows(0), FLOAT_EVERY_FALLBACK, "unknown height");
        assert_eq!(every_from_rows(3), 10, "a tiny window keeps the floor");
    }

    /// The gutter is a column, so every variant of it must be the same width
    /// or the log bends. Widths, not contents, are the invariant here.
    #[test]
    fn every_gutter_is_the_same_width() {
        SESSION.store(true, std::sync::atomic::Ordering::Relaxed);
        let cont = session_gutter().expect("in session");
        assert_eq!(cont.chars().count(), GUTTER, "{cont:?}");
        let stamped = session_gutter_stamped().expect("in session");
        assert_eq!(stamped.chars().count(), GUTTER, "{stamped:?}");
        assert!(stamped.trim_end().len() == 8, "HH:MM:SS, got {stamped:?}");
        // The second call inside the same second elides to the continuation.
        assert_eq!(session_gutter_stamped().as_deref(), Some(cont.as_str()));
        SESSION.store(false, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(session_gutter(), None, "off outside a session");
        assert_eq!(session_gutter_stamped(), None);
    }

    /// Three shapes, and the difference has to survive both a terminal and a
    /// pipe: colour is a parameter here precisely because a test that read
    /// stderr_color() would assert on nothing when run under cargo's capture.
    #[test]
    fn a_logged_line_never_wears_the_shape_of_a_crash() {
        for color in [true, false] {
            let thrown = scene_headline(SceneNote::Thrown, "src/index.ts:41", color);
            let logged = scene_headline(SceneNote::LoggedError, "src/index.ts:41", color);
            let warned = scene_headline(SceneNote::LoggedWarning, "src/index.ts:41", color);
            assert!(thrown.contains("scene error"), "{thrown}");
            assert!(logged.contains("console.error"), "{logged}");
            assert!(warned.contains("console.warn"), "{warned}");
            for other in [&logged, &warned] {
                assert!(
                    !other.contains("scene error"),
                    "a log must not be called an error: {other}"
                );
            }
            let bare = |s: &str| {
                let mut out = String::new();
                let mut chars = s.chars();
                while let Some(c) = chars.next() {
                    if c != '\u{1b}' {
                        out.push(c);
                        continue;
                    }
                    for c in chars.by_ref() {
                        if c == 'm' {
                            break;
                        }
                    }
                }
                out
            };
            assert!(bare(&thrown).contains(" in "), "{thrown:?}");
            assert!(bare(&logged).contains(" from "), "{logged:?}");
            assert!(bare(&warned).contains(" from "), "{warned:?}");
            if color {
                assert!(thrown.contains("\x1b[1;31m"), "a crash is red");
                assert!(logged.contains("\x1b[1;33m"), "a log is not red");
            }
        }
    }

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
