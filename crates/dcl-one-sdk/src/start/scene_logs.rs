//! Scene JavaScript errors, read from the running client and printed here.
//!
//! Read from the client's own log buffer over the MCP server it runs under
//! `--mcp` (`GetSceneLogsTool`) rather than by injecting a reporter into the
//! scene: nothing is added to the user's bundle, and we see what the CLIENT
//! saw — including engine-side failures a scene-side hook cannot observe.

use super::SourceContext;
use crate::scene::Project;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(700);

/// A refused connection is silence, not an error: `start` routinely runs for a
/// long time before anyone opens the deep link.
const RETRY: Duration = Duration::from_secs(2);

const LIMIT: u32 = 100;

/// Below any real sequence number, so the next poll asks for the whole buffer.
const REPLAY_FROM_START: i64 = -1;

/// What one poll's outcome means for the cursor.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// First contact. Note where the buffer is and report nothing: a session
    /// starting mid-run must not replay the client's whole history.
    Anchor(i64),
    /// The buffer went backwards, so this is a new client. Replay it whole,
    /// and forget what was printed for the old one.
    Restart,
    /// Report what arrived and move on.
    Advance(i64),
    /// The poll failed. Change nothing: a client too busy to answer is exactly
    /// the one about to restart, and only a kept cursor can then see `latest`
    /// go backwards. Forgetting it would silently anchor past the errors.
    Hold,
}

fn step(cursor: Option<i64>, latest: Result<i64, ()>) -> Step {
    let Ok(latest) = latest else {
        return Step::Hold;
    };
    match cursor {
        Some(seq) if latest < seq => Step::Restart,
        Some(_) => Step::Advance(latest),
        None => Step::Anchor(latest),
    }
}

pub fn spawn(mcp_port: u16, projects: Vec<Project>, context: SourceContext) {
    tokio::spawn(async move {
        let mut reader = Reader::new(mcp_port, projects, context);
        reader.run().await;
    });
}

struct Reader {
    url: String,
    client: reqwest::Client,
    projects: Vec<Project>,
    context: SourceContext,
    /// `None` only until the first successful poll; see [`Step`].
    cursor: Option<i64>,
    /// Message text -> times printed, so a throw on every frame says so once.
    seen: HashMap<String, u32>,
}

impl Reader {
    fn new(port: u16, projects: Vec<Project>, context: SourceContext) -> Self {
        Reader {
            url: format!("http://127.0.0.1:{port}/unity-explorer-mcp"),
            client: reqwest::Client::new(),
            projects,
            context,
            cursor: None,
            seen: HashMap::new(),
        }
    }

    async fn run(&mut self) {
        loop {
            let (latest, entries) = match self.poll().await {
                Ok((latest, entries)) => (Ok(latest), entries),
                Err(()) => (Err(()), Vec::new()),
            };
            let delay = match latest.is_ok() {
                true => POLL,
                false => RETRY,
            };
            self.apply(step(self.cursor, latest), entries);
            tokio::time::sleep(delay).await;
        }
    }

    async fn poll(&self) -> Result<(i64, Vec<Entry>), ()> {
        let args = match self.cursor {
            None => serde_json::json!({ "limit": 1 }),
            Some(seq) => {
                serde_json::json!({ "severity": "error", "sinceSeq": seq, "limit": LIMIT })
            }
        };
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": "get_scene_logs", "arguments": args }
        });
        let resp = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .timeout(Duration::from_secs(5))
            .json(&body)
            .send()
            .await
            .map_err(|_| ())?;
        let value: serde_json::Value = resp.json().await.map_err(|_| ())?;
        let text = value
            .pointer("/result/content/0/text")
            .and_then(|t| t.as_str())
            .ok_or(())?;

        Ok(parse(text))
    }

    fn apply(&mut self, step: Step, entries: Vec<Entry>) {
        match step {
            Step::Hold => {}
            Step::Restart => {
                tracing::info!("client restarted; re-reading its scene log from the start");
                self.seen.clear();
                self.cursor = Some(REPLAY_FROM_START);
            }
            Step::Anchor(latest) => {
                self.cursor = Some(latest);
                if latest > 0 {
                    tracing::info!("reading scene errors from the client (seq {latest})");
                }
            }
            Step::Advance(latest) => {
                for entry in entries {
                    self.report(&entry);
                }
                self.cursor = Some(latest);
            }
        }
    }

    fn report(&mut self, entry: &Entry) {
        let Some(message) = entry.scene_js_message() else {
            return;
        };
        if !self.first_sighting(message) {
            return;
        }
        let frames: Vec<Frame> = entry
            .frames()
            .filter_map(|raw| self.resolve(raw))
            .take(6)
            .collect();
        let kind = match (entry.origin(), entry.is_warning()) {
            (Origin::Thrown, _) => crate::ux::SceneNote::Thrown,
            (Origin::Logged, false) => crate::ux::SceneNote::LoggedError,
            (Origin::Logged, true) => crate::ux::SceneNote::LoggedWarning,
        };
        crate::ux::scene_note(kind, message, &entry.at, &frames);
    }

    fn first_sighting(&mut self, message: &str) -> bool {
        let count = self.seen.entry(message.to_string()).or_insert(0);
        *count += 1;
        *count == 1
    }

    /// Map `dcl-one:///bin/scene.js:3685:12` back to the developer's own file.
    fn resolve(&self, raw: &str) -> Option<Frame> {
        let (chunk, line, col) = parse_frame(raw)?;
        for project in &self.projects {
            let bundle = project.root.join("bin").join(&chunk);
            if !known_chunk(project, &bundle) {
                continue;
            }
            if let Some(f) = map_frame(&bundle, line, col, self.context) {
                return Some(f);
            }
        }
        None
    }
}

pub struct Frame {
    pub file: String,
    pub line: u32,
    pub col: u32,
    /// The quoted source around the error, as (line number, text).
    pub window: Vec<(u32, String)>,
    /// False under node_modules: a true frame, but not one anyone can act on.
    pub is_user_code: bool,
}

/// One buffer entry: `#12 [03:29:37] [Error] SceneError: … stackTrace: …`.
struct Entry {
    at: String,
    body: String,
}

/// What the client actually recorded. The scene log carries both, under the
/// same `SceneError:` prefix, and they are not the same event: a throw nobody
/// caught is a broken scene, while a `console.error` is the scene talking.
///
/// The client's own WebSocket shim is the case that forced this apart — it
/// `console.error`s and THEN throws (`WebSocketApi.js:152-154`), so a scene
/// that correctly wraps `close()` in try/catch still gets the text logged.
/// Printing that as an uncaught error blames the line that handled it, which
/// is worse than saying nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// The message arrived with the error's own stack, so an `Error` unwound.
    Thrown,
    /// Text with only the host's call-site trace: `console.error`/`warn`.
    Logged,
}

impl Entry {
    /// The message, if this entry came from the scene's JavaScript. The
    /// `SceneError:` prefix the runtime adds is the only `ReportCategory
    /// .JAVASCRIPT` marker that survives into the tool's text; severity alone
    /// would also match a GLTF load failure, which is the client's business.
    fn scene_js_message(&self) -> Option<&str> {
        let rest = self
            .body
            .strip_prefix("SceneError:")
            .or_else(|| self.body.strip_prefix("SceneWarning:"))?;
        let message = match rest.split_once(" stackTrace:") {
            Some((head, _)) => head,
            None => rest,
        };
        let headline = message.split("\n    at ").next().unwrap_or(message);
        Some(headline.trim())
    }

    /// `SceneWarning:` is the scene's own `console.warn`; there is no thrown
    /// form of it, and it must never wear the shape of a crash.
    fn is_warning(&self) -> bool {
        self.body.starts_with("SceneWarning:")
    }

    /// An `Error` that unwound carries its own `at` frames ahead of the host's
    /// `stackTrace:`; a logged string has nothing before it. This is the same
    /// split [`Entry::frames`] already relies on to decide which trace to read,
    /// named so the printer can use it too.
    fn origin(&self) -> Origin {
        let head = match self.body.split_once(" stackTrace:") {
            Some((head, _)) => head,
            None => &self.body,
        };
        match head.contains("\n    at ") {
            true => Origin::Thrown,
            false => Origin::Logged,
        }
    }

    /// Frames from the error's own stack if it carried one, else the host's.
    /// `e.stack` is where the throw happened; the host's `stackTrace:` is where
    /// `console.error` was called — our catch block, whatever the scene did.
    fn frames(&self) -> impl Iterator<Item = &str> {
        let source = match self.body.split_once(" stackTrace:") {
            Some((head, tail)) => match head.contains("\n    at ") {
                true => head,
                false => tail,
            },
            None => self.body.as_str(),
        };
        source
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("at "))
    }
}

/// Split the tool's text into its header and entries. An entry is not one line:
/// a stack arrives inline after `stackTrace:`, so it runs to the next `#<seq>`.
fn parse(text: &str) -> (i64, Vec<Entry>) {
    let mut latest = 0i64;
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.lines() {
        if let Some(seq) = line.strip_prefix("latestSeq=") {
            latest = seq
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            continue;
        }
        match start_of_entry(line) {
            Some(rest) => {
                let (at, body) = split_timestamp(rest);
                entries.push(Entry { at, body });
            }
            None => {
                if let Some(last) = entries.last_mut() {
                    last.body.push('\n');
                    last.body.push_str(line);
                }
            }
        }
    }
    (latest, entries)
}

/// `#12 rest` -> `rest`, but only when the digits really are a sequence number.
fn start_of_entry(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('#')?;
    let digits = rest.find(' ')?;
    rest[..digits].parse::<i64>().ok()?;
    Some(rest[digits + 1..].trim_start())
}

/// `[03:29:37] [Error] SceneError: x` -> (`03:29:37`, `SceneError: x`).
fn split_timestamp(rest: &str) -> (String, String) {
    let mut at = String::new();
    let mut body = rest;
    for _ in 0..2 {
        let Some(inner) = body.strip_prefix('[') else {
            break;
        };
        let Some(end) = inner.find(']') else { break };
        let tag = &inner[..end];
        if tag.contains(':') && at.is_empty() {
            at = tag.to_string();
        }
        body = inner[end + 1..].trim_start();
    }
    (at, body.to_string())
}

/// `at f (dcl-one:///bin/scene.js:3685:12)` -> (`scene.js`, 3685, 12).
fn parse_frame(raw: &str) -> Option<(String, u32, u32)> {
    let start = raw.find("dcl-one:///bin/")? + "dcl-one:///bin/".len();
    let rest = &raw[start..];
    let end = rest.find(')').unwrap_or(rest.len());
    let rest = &rest[..end];
    let mut parts = rest.split(':');
    let file = parts
        .next()?
        .split([',', ' '])
        .next()
        .unwrap_or_default()
        .to_string();
    if file.is_empty() {
        return None;
    }
    let line = parts.next()?.trim().parse().ok()?;
    let col = parts
        .next()
        .and_then(|c| c.split(|ch: char| !ch.is_ascii_digit()).next())
        .and_then(|c| c.parse().ok())
        .unwrap_or(1);
    Some((file, line, col))
}

/// Is this one of the chunks this project emits? Keeps a wire-supplied name
/// from selecting anything else on disk.
fn known_chunk(project: &Project, bundle: &PathBuf) -> bool {
    let Ok(main) = project.main_output() else {
        return false;
    };
    let (sdk, scene) = crate::split::chunk_rel_paths(&main);
    let smart = crate::split::smart_chunk_rel_path(&main);
    [sdk, scene, smart, main]
        .iter()
        .any(|rel| project.root.join(rel) == *bundle)
}

/// Resolve a position through the bundle's inline source map. Preview bundles
/// carry it with `sourcesContent`, so nothing is read from the source tree and
/// a stale map can only produce a wrong line, never a wrong file.
fn map_frame(bundle: &PathBuf, line: u32, col: u32, context: SourceContext) -> Option<Frame> {
    let code = std::fs::read_to_string(bundle).ok()?;
    let json = inline_map(&code)?;
    let map = oxc_sourcemap::SourceMap::from_json_string(&json).ok()?;
    let table = map.generate_lookup_table();
    let token = map.lookup_token(&table, line.saturating_sub(1), col.saturating_sub(1))?;
    let source_id = token.get_source_id()?;
    let file = map.get_source(source_id)?.to_string();
    let src_line = token.get_src_line();
    let window = map
        .get_source_content(source_id)
        .map(|content| {
            let lines: Vec<&str> = content.lines().collect();
            let first = src_line.saturating_sub(context.before) as usize;
            let last = (src_line + context.after) as usize;
            (first..=last.min(lines.len().saturating_sub(1)))
                .map(|i| (i as u32 + 1, lines[i].trim_end().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let tidy = file.trim_start_matches("../").to_string();
    if tidy.starts_with(".dcl-one/") {
        return None;
    }
    Some(Frame {
        is_user_code: !tidy.starts_with("node_modules/"),
        file: tidy,
        line: src_line + 1,
        col: token.get_src_col() + 1,
        window,
    })
}

fn inline_map(code: &str) -> Option<String> {
    use base64::Engine;
    let at = code.rfind("sourceMappingURL=data:application/json")?;
    let b64 = code[at..].split("base64,").nth(1)?;
    let b64: String = b64
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
        .collect();
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "latestSeq=11 returned=2\n\
#10 [03:29:05] [Error] SceneError: [asset-packs] No SDK composite provider registered stackTrace:     at initAssetPacks (dcl-one:///bin/sdk-runtime.js:51002:16)\n\
    at eval (dcl-one:///bin/scene.js:3676:59)\n\
#11 [03:29:37] [Log] Starting loading http://127.0.0.1:8000/content/contents/b64-xyz\n";

    #[test]
    fn an_entry_runs_until_the_next_sequence_number() {
        let (latest, entries) = parse(SAMPLE);
        assert_eq!(latest, 11);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].at, "03:29:05");
        assert_eq!(entries[0].frames().count(), 2);
    }

    /// The line that forced this apart, verbatim from a real preview: the
    /// client's WebSocket shim console.errors and then throws, so a scene that
    /// wraps close() in try/catch still gets the text logged. It must not
    /// print as a crash in the line that handled it.
    #[test]
    fn a_console_error_is_not_a_thrown_error() {
        let logged = "latestSeq=2 returned=1\n\
#1 [02:58:44] [Error] SceneError: WebSocket state is 3, cannot close stackTrace:     at close (dcl-one:///bin/sdk-runtime.js:900:9)\n\
    at eval (dcl-one:///bin/scene.js:1200:7)\n";
        let (_, entries) = parse(logged);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].origin(), Origin::Logged);
        assert!(!entries[0].is_warning());
        assert_eq!(
            entries[0].scene_js_message(),
            Some("WebSocket state is 3, cannot close")
        );
        assert_eq!(entries[0].frames().count(), 2);
    }

    /// An Error that unwound carries its own stack ahead of the host's
    /// stackTrace:, which is the whole discriminator.
    #[test]
    fn an_unwound_error_keeps_its_own_stack_and_reads_as_thrown() {
        let thrown = "latestSeq=1 returned=1\n\
#1 [03:00:00] [Error] SceneError: TypeError: x is not a function\n    at main (dcl-one:///bin/scene.js:10:3) stackTrace:     at report (dcl-one:///bin/sdk-runtime.js:1:1)\n";
        let (_, entries) = parse(thrown);
        assert_eq!(entries[0].origin(), Origin::Thrown);
        assert_eq!(
            entries[0].scene_js_message(),
            Some("TypeError: x is not a function")
        );
    }

    #[test]
    fn a_scene_warning_is_never_a_crash() {
        let warned = "latestSeq=1 returned=1\n\
#1 [03:00:00] [Warning] SceneWarning: deprecated thing stackTrace:     at eval (dcl-one:///bin/scene.js:5:1)\n";
        let (_, entries) = parse(warned);
        assert!(entries[0].is_warning());
        assert_eq!(entries[0].origin(), Origin::Logged);
    }

    #[test]
    fn only_scene_javascript_is_reported() {
        let (_, entries) = parse(SAMPLE);
        assert_eq!(
            entries[0].scene_js_message(),
            Some("[asset-packs] No SDK composite provider registered")
        );
        assert_eq!(entries[1].scene_js_message(), None);
    }

    #[test]
    fn frames_parse_across_runtime_spellings() {
        assert_eq!(
            parse_frame("at eval (dcl-one:///bin/scene.js:3676:59)"),
            Some(("scene.js".to_string(), 3676, 59))
        );
        assert_eq!(
            parse_frame("at main (dcl-one:///bin/scene.js, <anonymous>:13:87)"),
            Some(("scene.js".to_string(), 13, 87))
        );
        assert_eq!(
            parse_frame("at HostDelegate.<anonymous> (<anonymous>)"),
            None
        );
    }

    const BOOM: &str = "TypeError: cannot read x of undefined";

    fn reader() -> Reader {
        Reader::new(0, Vec::new(), SourceContext::default())
    }

    #[test]
    fn a_fresh_session_skips_the_history_it_arrived_to() {
        assert_eq!(step(None, Ok(40)), Step::Anchor(40));
        let mut reader = reader();
        reader.apply(step(reader.cursor, Ok(40)), Vec::new());
        assert_eq!(reader.cursor, Some(40));
    }

    #[test]
    fn a_failed_poll_keeps_the_cursor() {
        assert_eq!(step(Some(41), Err(())), Step::Hold);
        let mut reader = reader();
        reader.cursor = Some(41);
        reader.apply(step(reader.cursor, Err(())), Vec::new());
        assert_eq!(reader.cursor, Some(41));
    }

    #[test]
    fn a_client_that_died_mid_poll_still_replays_when_it_comes_back() {
        let mut reader = reader();
        reader.apply(step(reader.cursor, Ok(40)), Vec::new());
        assert!(reader.first_sighting(BOOM));
        reader.apply(step(reader.cursor, Ok(41)), Vec::new());
        assert!(!reader.first_sighting(BOOM));

        for _ in 0..5 {
            reader.apply(step(reader.cursor, Err(())), Vec::new());
        }

        reader.apply(step(reader.cursor, Ok(3)), Vec::new());
        assert_eq!(reader.cursor, Some(REPLAY_FROM_START));
        assert!(
            reader.first_sighting(BOOM),
            "the same error on the relaunched client must print again"
        );
    }

    #[test]
    fn a_timestamp_and_level_are_stripped_but_the_clock_is_kept() {
        let (at, body) = split_timestamp("[03:29:37] [Error] SceneError: boom");
        assert_eq!(at, "03:29:37");
        assert_eq!(body, "SceneError: boom");
    }
}
