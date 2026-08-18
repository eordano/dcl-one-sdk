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
    /// `None` until the first poll, which only learns where the buffer is: a
    /// session starting mid-run must not replay the client's whole history.
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
            match self.poll().await {
                Ok(()) => tokio::time::sleep(POLL).await,
                Err(()) => {
                    self.cursor = None;
                    tokio::time::sleep(RETRY).await;
                }
            }
        }
    }

    async fn poll(&mut self) -> Result<(), ()> {
        let since = self.cursor;
        let args = match since {
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

        let (latest, entries) = parse(text);
        if since.is_some_and(|seq| latest < seq) {
            tracing::info!("client restarted; re-reading its scene log from the start");
            self.seen.clear();
            self.cursor = Some(-1);
            return Ok(());
        }
        match since {
            None => {
                self.cursor = Some(latest);
                if latest > 0 {
                    tracing::info!("reading scene errors from the client (seq {latest})");
                }
            }
            Some(_) => {
                for entry in entries {
                    self.report(&entry);
                }
                self.cursor = Some(latest);
            }
        }
        Ok(())
    }

    fn report(&mut self, entry: &Entry) {
        let Some(message) = entry.scene_js_message() else {
            return;
        };
        let count = self.seen.entry(message.to_string()).or_insert(0);
        *count += 1;
        if *count > 1 {
            return;
        }
        let frames: Vec<Frame> = entry
            .frames()
            .filter_map(|raw| self.resolve(raw))
            .take(6)
            .collect();
        crate::ux::scene_error(message, &entry.at, &frames);
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

    #[test]
    fn a_timestamp_and_level_are_stripped_but_the_clock_is_kept() {
        let (at, body) = split_timestamp("[03:29:37] [Error] SceneError: boom");
        assert_eq!(at, "03:29:37");
        assert_eq!(body, "SceneError: boom");
    }
}
