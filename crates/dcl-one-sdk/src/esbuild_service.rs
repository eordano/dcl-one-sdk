use crate::esbuild::{self, Backend, EsbuildOptions};
use crate::esbuild_proto::{decode_payload, encode_frame, take_frame, Value};
use crate::scene::Project;
use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};

pub enum BuildStatus {
    Success,
    Failed(String),
}

pub struct BuildContext {
    key: i32,
    on_end: mpsc::UnboundedReceiver<Value>,
}

type SharedStdin = Arc<AsyncMutex<Option<ChildStdin>>>;

struct State {
    next_id: AtomicU32,
    pending: Mutex<HashMap<u32, oneshot::Sender<Result<Value, String>>>>,
    on_end: Mutex<HashMap<i32, mpsc::UnboundedSender<Value>>>,
}

pub struct EsbuildService {
    child: Child,
    stdin: SharedStdin,
    state: Arc<State>,
    reader: tokio::task::JoinHandle<()>,
    next_key: i32,
}

impl EsbuildService {
    pub async fn spawn(bin: &Path, cwd: &Path) -> Result<Self> {
        let version = query_version(bin).await?;
        let mut child = Command::new(bin)
            .arg(format!("--service={version}"))
            .arg("--ping")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawning esbuild service {}", bin.display()))?;
        let stdin_pipe = child
            .stdin
            .take()
            .context("esbuild service stdin missing")?;
        let stdout_pipe = child
            .stdout
            .take()
            .context("esbuild service stdout missing")?;
        let stdin: SharedStdin = Arc::new(AsyncMutex::new(Some(stdin_pipe)));
        let state = Arc::new(State {
            next_id: AtomicU32::new(0),
            pending: Mutex::new(HashMap::new()),
            on_end: Mutex::new(HashMap::new()),
        });
        let (hs_tx, hs_rx) = oneshot::channel();
        let reader = tokio::spawn(reader_loop(
            stdout_pipe,
            stdin.clone(),
            state.clone(),
            version,
            hs_tx,
        ));
        match tokio::time::timeout(Duration::from_secs(10), hs_rx).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(msg))) => {
                let _ = child.start_kill();
                bail!(msg);
            }
            Ok(Err(_)) => {
                let _ = child.start_kill();
                bail!("esbuild service exited before handshake");
            }
            Err(_) => {
                let _ = child.start_kill();
                bail!("esbuild service handshake timed out");
            }
        }
        Ok(Self {
            child,
            stdin,
            state,
            reader,
            next_key: 0,
        })
    }

    pub async fn build(
        &mut self,
        opts: &EsbuildOptions,
        abs_working_dir: &Path,
    ) -> Result<BuildStatus> {
        let key = self.take_key();
        let response = self
            .request(build_value(opts, abs_working_dir, key, false))
            .await?;
        Ok(build_status(&response))
    }

    pub async fn create_context(
        &mut self,
        opts: &EsbuildOptions,
        abs_working_dir: &Path,
    ) -> Result<(BuildContext, BuildStatus)> {
        let key = self.take_key();
        let (tx, rx) = mpsc::unbounded_channel();
        self.state.on_end.lock().unwrap().insert(key, tx);
        match self
            .request(build_value(opts, abs_working_dir, key, true))
            .await
        {
            Ok(response) => Ok((BuildContext { key, on_end: rx }, build_status(&response))),
            Err(e) => {
                self.state.on_end.lock().unwrap().remove(&key);
                Err(e)
            }
        }
    }

    pub async fn rebuild(&self, ctx: &mut BuildContext) -> Result<BuildStatus> {
        self.request(Value::Object(vec![
            ("command".into(), Value::Str("rebuild".into())),
            ("key".into(), Value::Int(ctx.key)),
        ]))
        .await?;
        let end = ctx
            .on_end
            .recv()
            .await
            .context("esbuild service stopped before on-end")?;
        Ok(build_status(&end))
    }

    pub async fn dispose(&self, ctx: BuildContext) -> Result<()> {
        let result = self
            .request(Value::Object(vec![
                ("command".into(), Value::Str("dispose".into())),
                ("key".into(), Value::Int(ctx.key)),
            ]))
            .await;
        self.state.on_end.lock().unwrap().remove(&ctx.key);
        result.map(|_| ())
    }

    pub async fn shutdown(mut self) {
        self.stdin.lock().await.take();
        if tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
        }
        self.reader.abort();
    }

    fn take_key(&mut self) -> i32 {
        let key = self.next_key;
        self.next_key += 1;
        key
    }

    async fn request(&self, value: Value) -> Result<Value> {
        let id = self.state.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.state.pending.lock().unwrap().insert(id, tx);
        let frame = encode_frame(id, true, &value);
        if let Err(e) = write_frame(&self.stdin, &frame).await {
            self.state.pending.lock().unwrap().remove(&id);
            return Err(e);
        }
        let response = rx
            .await
            .map_err(|_| anyhow!("esbuild service stopped"))?
            .map_err(|e| anyhow!(e))?;
        if let Some(err) = response.get("error").and_then(Value::as_str) {
            bail!("esbuild service error: {err}");
        }
        Ok(response)
    }
}

async fn query_version(bin: &Path) -> Result<String> {
    let out = Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("running {} --version", bin.display()))?;
    if !out.status.success() {
        bail!("esbuild --version failed ({})", out.status);
    }
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if version.is_empty() {
        bail!("esbuild --version printed nothing");
    }
    Ok(version)
}

async fn write_frame(stdin: &SharedStdin, frame: &[u8]) -> Result<()> {
    let mut guard = stdin.lock().await;
    let pipe = guard.as_mut().context("esbuild service stdin closed")?;
    pipe.write_all(frame)
        .await
        .context("writing to esbuild service")?;
    pipe.flush()
        .await
        .context("flushing esbuild service stdin")?;
    Ok(())
}

async fn reader_loop(
    mut stdout: ChildStdout,
    stdin: SharedStdin,
    state: Arc<State>,
    version: String,
    hs_tx: oneshot::Sender<Result<(), String>>,
) {
    let mut hs_tx = Some(hs_tx);
    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut chunk = vec![0u8; 64 * 1024];
    let mut saw_handshake = false;
    'read: loop {
        match stdout.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        while let Some(payload) = take_frame(&mut buf) {
            if !saw_handshake {
                saw_handshake = true;
                let got = String::from_utf8_lossy(&payload).to_string();
                if got == version {
                    if let Some(tx) = hs_tx.take() {
                        let _ = tx.send(Ok(()));
                    }
                    continue;
                }
                if let Some(tx) = hs_tx.take() {
                    let _ = tx.send(Err(format!(
                        "esbuild service handshake version {got:?} != argv version {version:?}"
                    )));
                }
                break 'read;
            }
            let Ok(pkt) = decode_payload(&payload) else {
                tracing::warn!("esbuild service sent an undecodable packet; stopping reader");
                break 'read;
            };
            if pkt.is_request {
                let command = pkt
                    .value
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if command == "on-end" {
                    if let Some(key) = pkt.value.get("key").and_then(Value::as_int) {
                        let tx = state.on_end.lock().unwrap().get(&key).cloned();
                        if let Some(tx) = tx {
                            let _ = tx.send(pkt.value.clone());
                        }
                    }
                    let reply = Value::Object(vec![
                        ("errors".into(), Value::Array(vec![])),
                        ("warnings".into(), Value::Array(vec![])),
                    ]);
                    if write_frame(&stdin, &encode_frame(pkt.id, false, &reply))
                        .await
                        .is_err()
                    {
                        break 'read;
                    }
                } else if write_frame(&stdin, &encode_frame(pkt.id, false, &Value::Object(vec![])))
                    .await
                    .is_err()
                {
                    break 'read;
                }
            } else {
                let tx = state.pending.lock().unwrap().remove(&pkt.id);
                if let Some(tx) = tx {
                    let _ = tx.send(Ok(pkt.value));
                }
            }
        }
    }
    let mut pending = state.pending.lock().unwrap();
    for (_, tx) in pending.drain() {
        let _ = tx.send(Err("esbuild service stopped".into()));
    }
}

fn build_value(opts: &EsbuildOptions, abs_working_dir: &Path, key: i32, context: bool) -> Value {
    let flags = esbuild::flags(opts).into_iter().map(Value::Str).collect();
    Value::Object(vec![
        ("command".into(), Value::Str("build".into())),
        ("key".into(), Value::Int(key)),
        (
            "entries".into(),
            Value::Array(vec![Value::Array(vec![
                Value::Str(String::new()),
                Value::Str(opts.entrypoint.display().to_string()),
            ])]),
        ),
        ("flags".into(), Value::Array(flags)),
        ("write".into(), Value::Bool(true)),
        ("stdinContents".into(), Value::Null),
        ("stdinResolveDir".into(), Value::Null),
        (
            "absWorkingDir".into(),
            Value::Str(abs_working_dir.display().to_string()),
        ),
        ("nodePaths".into(), Value::Array(vec![])),
        ("context".into(), Value::Bool(context)),
    ])
}

fn build_status(result: &Value) -> BuildStatus {
    let errors = result
        .get("errors")
        .and_then(Value::as_array)
        .unwrap_or_default();
    if errors.is_empty() {
        BuildStatus::Success
    } else {
        BuildStatus::Failed(format_messages(errors))
    }
}

fn format_messages(messages: &[Value]) -> String {
    messages
        .iter()
        .map(|m| {
            let text = m
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            match m.get("location") {
                Some(loc) if *loc != Value::Null => {
                    let file = loc.get("file").and_then(Value::as_str).unwrap_or("?");
                    let line = loc.get("line").and_then(Value::as_int).unwrap_or(0);
                    let column = loc.get("column").and_then(Value::as_int).unwrap_or(0);
                    let mut s = format!("{file}:{line}:{column}: {text}");
                    if let Some(line_text) = loc.get("lineText").and_then(Value::as_str) {
                        let ln = line.to_string();
                        let pad = " ".repeat(ln.len());
                        let caret = " ".repeat(column.max(0) as usize);
                        s.push_str(&format!(
                            "\n  {ln} \u{2502} {line_text}\n  {pad} \u{2575} {caret}^"
                        ));
                    }
                    s
                }
                _ => text.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn run_with_fallback(project: &Project, opts: &EsbuildOptions) -> Result<()> {
    if opts.backend == Backend::Rolldown {
        return esbuild::bundle(project, opts).await;
    }
    if std::env::var("DCL_ONE_SDK_NO_SERVICE").is_ok_and(|v| v == "1") {
        return esbuild::run(project, opts).await;
    }
    match run_service(project, opts).await {
        Ok(BuildStatus::Success) => Ok(()),
        Ok(BuildStatus::Failed(msg)) => Err(crate::ux::bundle_failed(&msg)),
        Err(e) => {
            tracing::warn!("esbuild service unavailable ({e:#}); falling back to CLI");
            esbuild::run(project, opts).await
        }
    }
}

async fn run_service(project: &Project, opts: &EsbuildOptions) -> Result<BuildStatus> {
    let bin = esbuild::locate(project)?;
    let mut service = EsbuildService::spawn(&bin, &project.root).await?;
    let status = service.build(opts, &project.root).await;
    service.shutdown().await;
    if matches!(status, Ok(BuildStatus::Success)) {
        tracing::debug!("bundled via esbuild service");
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn context_rebuild_dispose_roundtrip() {
        let bin = std::env::var("DCL_ONE_SDK_ESBUILD").expect("set DCL_ONE_SDK_ESBUILD");
        let dir = std::env::temp_dir().join("dcl-one-sdk-esvc-test");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("entry.ts"),
            "import { msg } from './lib'\nconsole.log(msg)\n",
        )
        .unwrap();
        std::fs::write(dir.join("lib.ts"), "export const msg: string = 'hi'\n").unwrap();
        std::fs::write(dir.join("tsconfig.json"), "{\"compilerOptions\":{}}").unwrap();
        let outfile = dir.join("out.js");
        let opts = EsbuildOptions {
            backend: Backend::Esbuild,
            production: false,
            entrypoint: dir.join("entry.ts"),
            outfile: outfile.clone(),
            tsconfig: dir.join("tsconfig.json"),
            aliases: vec![],
            externals: vec![],
        };
        let mut service = EsbuildService::spawn(Path::new(&bin), &dir).await.unwrap();
        let (mut ctx, status) = service.create_context(&opts, &dir).await.unwrap();
        assert!(matches!(status, BuildStatus::Success));
        assert!(!outfile.exists());
        let status = service.rebuild(&mut ctx).await.unwrap();
        assert!(matches!(status, BuildStatus::Success));
        let first = std::fs::read_to_string(&outfile).unwrap();
        assert!(first.contains("hi"));
        std::fs::write(dir.join("lib.ts"), "export const msg: string = 'rebuilt'\n").unwrap();
        let status = service.rebuild(&mut ctx).await.unwrap();
        assert!(matches!(status, BuildStatus::Success));
        assert!(std::fs::read_to_string(&outfile)
            .unwrap()
            .contains("rebuilt"));
        let key = ctx.key;
        service.dispose(ctx).await.unwrap();
        let err = service
            .request(Value::Object(vec![
                ("command".into(), Value::Str("rebuild".into())),
                ("key".into(), Value::Int(key)),
            ]))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Cannot rebuild"));
        service.shutdown().await;
        std::fs::remove_dir_all(&dir).ok();
    }
}
