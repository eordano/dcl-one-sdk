//! `dcl-one-sdk storage`: upstream's `sdk-commands storage` — `env`, `scene`
//! and `player` scopes with `get`, `set`, `delete` and `clear` — plus what
//! a local-first tool can add: `list`, `target`, `export` and `import`.
//!
//! Where a command acts follows the project's remembered target (the
//! switch on the preview's /storage page) unless `--target` says otherwise:
//! `local` edits the SQLite file directly, `preview` asks a running preview
//! (which applies its own target), and `org`, `zone` or a URL reach that
//! service with requests signed by `--sign-key` or `DCL_PRIVATE_KEY`.

use crate::scene::Project;
use crate::storage::{self, Scope, Store, Target};
use crate::storage_remote::{self as remote, Outgoing, SceneMetadata, Signer};
use crate::ux::{self, TrySteps, UserError};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Args, Debug, Clone)]
pub struct StorageOptions {
    #[arg(long, global = true, default_value = ".", help = "Project folder")]
    pub dir: PathBuf,
    #[arg(
        short = 't',
        long,
        global = true,
        help = "Where to act: local (this project's database), preview (a running preview, which applies its own target), org, zone, or a storage service URL. Default: the target the project remembers"
    )]
    pub target: Option<String>,
    #[arg(
        long,
        global = true,
        default_value = "http://127.0.0.1:8000",
        help = "The preview server `--target preview` talks to"
    )]
    pub preview: String,
    #[arg(
        long,
        global = true,
        help = "Private key file that signs requests to org, zone or a URL; DCL_PRIVATE_KEY otherwise"
    )]
    pub sign_key: Option<PathBuf>,
    #[arg(long, global = true, help = "Print results as JSON")]
    pub json: bool,
    // upstream's `sdk-commands storage` takes these for its linker dApp;
    // accepted so scripts written for it run unchanged
    #[arg(short = 'p', long, global = true, hide = true)]
    pub port: Option<u16>,
    #[arg(short = 'b', long, global = true, hide = true)]
    pub no_browser: bool,
    #[arg(long, global = true, hide = true)]
    pub https: bool,
}

#[derive(Subcommand, Debug)]
pub enum StorageCommand {
    #[command(about = "Scene-wide values: what Storage.get and Storage.set share")]
    Scene {
        #[command(subcommand)]
        action: ValueAction,
        #[command(flatten)]
        opts: StorageOptions,
    },
    #[command(
        about = "Per-player values: what Storage.player.get and Storage.player.set keep; `clear` without --address clears every player"
    )]
    Player {
        #[arg(short = 'a', long, global = true, help = "The player's wallet address")]
        address: Option<String>,
        #[command(subcommand)]
        action: ValueAction,
        #[command(flatten)]
        opts: StorageOptions,
    },
    #[command(about = "Environment variables: what EnvVar.get answers")]
    Env {
        #[command(subcommand)]
        action: EnvAction,
        #[command(flatten)]
        opts: StorageOptions,
    },
    #[command(
        about = "Show or set where the preview and this CLI keep storage: local, org, zone or a service URL"
    )]
    Target {
        #[arg(help = "local | org | zone | <url>; omit to show the current target")]
        target: Option<String>,
        #[arg(long, default_value = ".", help = "Project folder")]
        dir: PathBuf,
    },
    #[command(about = "Write the local database as upstream's server-storage.json")]
    Export {
        #[arg(long, default_value = ".", help = "Project folder")]
        dir: PathBuf,
        #[arg(long, help = "File to write; stdout when omitted")]
        out: Option<PathBuf>,
    },
    #[command(about = "Load a server-storage.json snapshot into the local database")]
    Import {
        #[arg(help = "The snapshot file")]
        file: PathBuf,
        #[arg(
            long,
            help = "Lay the snapshot over what is there instead of replacing it"
        )]
        merge: bool,
        #[arg(long, default_value = ".", help = "Project folder")]
        dir: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum ValueAction {
    #[command(about = "Print a value")]
    Get { key: String },
    #[command(about = "Store a value: the text as a string, or JSON with --json")]
    Set {
        key: String,
        #[arg(
            short = 'v',
            long,
            help = "The value, stored as a string unless --json"
        )]
        value: String,
        #[arg(
            long,
            help = "Parse the value as JSON (a number, list or object) instead of storing the text"
        )]
        json: bool,
    },
    #[command(about = "Delete a value")]
    Delete { key: String },
    #[command(about = "List values, a page at a time")]
    List {
        #[arg(long, help = "Only keys starting with this")]
        prefix: Option<String>,
        #[arg(long, help = "Keys per page, 1 to 100 (the default)")]
        limit: Option<usize>,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    #[command(about = "Delete every value in the scope")]
    Clear {
        #[arg(short = 'c', long, help = "Required: this cannot be undone")]
        confirm: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum EnvAction {
    #[command(about = "Print a variable")]
    Get { key: String },
    #[command(about = "Set a variable")]
    Set {
        key: String,
        #[arg(short = 'v', long)]
        value: String,
    },
    #[command(about = "Delete a variable")]
    Delete { key: String },
    #[command(about = "List variables")]
    List,
    #[command(about = "Delete every runtime variable (the scene's .env is untouched)")]
    Clear {
        #[arg(short = 'c', long, help = "Required: this cannot be undone")]
        confirm: bool,
    },
}

/// Where a command acts once `--target` and the project's memory are read.
#[derive(Debug)]
enum Mode {
    File(PathBuf),
    Http {
        base: String,
        signer: Option<Signer>,
        metadata: SceneMetadata,
    },
}

/// `local`, `preview`, a target spelling, or (unset) the project's memory.
fn resolve_mode(opts: &StorageOptions) -> Result<Mode> {
    let project = Project::load(&opts.dir)?;
    let root = project.root.clone();
    let target = match opts.target.as_deref().map(str::trim) {
        Some("preview") => {
            return Ok(Mode::Http {
                base: opts.preview.trim_end_matches('/').to_string(),
                signer: None,
                metadata: SceneMetadata::from_scene_json(&project.scene_json),
            })
        }
        Some(raw) => Target::parse(raw).map_err(|why| {
            UserError::new(
                why,
                TrySteps::one("--target local | preview | org | zone | https://…"),
            )
        })?,
        None => storage::open(&root)?.target()?,
    };
    let Some(base) = target.url() else {
        return Ok(Mode::File(root));
    };
    let metadata = SceneMetadata::from_scene_json(&project.scene_json);
    // load_signer validates the key (and says so when it comes from the
    // environment); the signer keeps the hex itself, as the proxy does
    let signer = crate::deploy::load_signer(opts.sign_key.as_deref())?.map(|_| {
        Signer::Key(match &opts.sign_key {
            Some(path) => std::fs::read_to_string(path)
                .map(|raw| raw.trim().to_string())
                .unwrap_or_default(),
            None => std::env::var("DCL_PRIVATE_KEY").unwrap_or_default(),
        })
    });
    if signer.is_none() && !is_local_development(base) {
        ux::note_stderr(format!(
            "no signing key: requests to {base} go out unsigned and the service will refuse them (set DCL_PRIVATE_KEY or pass --sign-key)"
        ));
    }
    Ok(Mode::Http {
        base: base.to_string(),
        signer,
        metadata,
    })
}

/// Upstream's "local development mode": a target on this machine, which
/// takes unsigned requests.
fn is_local_development(base: &str) -> bool {
    url::Url::parse(base)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .is_some_and(|host| {
            matches!(
                host.trim_matches(|c| c == '[' || c == ']'),
                "localhost" | "127.0.0.1" | "::1"
            )
        })
}

/// The line upstream prints before acting on a service: which scene the
/// signed metadata names.
fn scene_line(metadata: &SceneMetadata) -> String {
    match &metadata.world {
        Some(world) => format!("World: {world}"),
        None => format!("Genesis City scene at parcel {}", metadata.parcel),
    }
}

/// What a command found out, printed as prose or as JSON.
#[derive(Debug)]
enum Outcome {
    Value(Value),
    Absent(String),
    Done(String),
    Page(Value),
}

fn print(outcome: Outcome, as_json: bool) -> Result<()> {
    match outcome {
        Outcome::Value(v) if as_json => println!("{}", serde_json::to_string(&v)?),
        Outcome::Value(Value::String(s)) => println!("{s}"),
        Outcome::Value(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Outcome::Absent(key) => {
            return Err(UserError::new(
                format!("no value for {key}"),
                TrySteps::one("storage … list shows what is stored"),
            )
            .into())
        }
        Outcome::Done(what) if as_json => println!("{}", json!({ "ok": what })),
        Outcome::Done(what) => ux::note_good(what),
        Outcome::Page(page) if as_json => println!("{}", serde_json::to_string(&page)?),
        Outcome::Page(page) => {
            let rows = page["data"].as_array().cloned().unwrap_or_default();
            let total = page["pagination"]["total"]
                .as_u64()
                .unwrap_or(rows.len() as u64);
            if rows.is_empty() {
                println!("(nothing stored)");
            }
            for row in &rows {
                if let Some(name) = row.as_str() {
                    println!("{name}");
                    continue;
                }
                let key = row["key"].as_str().unwrap_or_default();
                let value = match &row["value"] {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                match row.get("source").and_then(Value::as_str) {
                    Some(source) => println!("{key}={value}\t({source})"),
                    None => println!("{key}={value}"),
                }
            }
            if total as usize > rows.len() {
                let offset =
                    page["pagination"]["offset"].as_u64().unwrap_or(0) as usize + rows.len();
                println!(
                    "({} of {total} keys; --offset {offset} continues)",
                    rows.len()
                );
            }
        }
    }
    Ok(())
}

/// `--value` as upstream sends it, the text as a string; `--json` parses it.
pub fn parse_value(raw: &str, as_json: bool) -> Result<Value> {
    if !as_json {
        return Ok(Value::String(raw.to_string()));
    }
    serde_json::from_str(raw).map_err(|e| {
        UserError::new(
            format!("--value is not JSON: {e}"),
            TrySteps::one("quote strings: --json --value '\"text\"'")
                .and("or drop --json to store the text as it is"),
        )
        .into()
    })
}

fn need_confirm(confirm: bool, what: &str) -> Result<()> {
    if confirm {
        return Ok(());
    }
    Err(UserError::new(
        format!("clearing {what} cannot be undone"),
        TrySteps::one("add --confirm to go ahead"),
    )
    .into())
}

fn scope_path(scope: Scope<'_>, key: Option<&str>) -> String {
    let base = match scope {
        Scope::Scene => "/values".to_string(),
        Scope::Player(_) => {
            format!(
                "/players/{}/values",
                crate::deploy::encode_segment(&scope.address())
            )
        }
        Scope::Env => "/env".to_string(),
    };
    match key {
        Some(key) => format!("{base}/{}", crate::deploy::encode_segment(key)),
        None => base,
    }
}

async fn send(
    base: &str,
    signer: Option<&Signer>,
    metadata: &SceneMetadata,
    method: &str,
    path: &str,
    query: Option<&str>,
    body: Option<Value>,
    confirm_all: bool,
) -> Result<remote::Reply> {
    let client = remote::client()?;
    let reply = remote::forward(
        &client,
        base,
        signer,
        metadata,
        Outgoing {
            method,
            path,
            query,
            body: body.map(|v| v.to_string().into_bytes()),
            confirm_all,
            source: "cli",
        },
    )
    .await?;
    if reply.status == 401 || reply.status == 403 {
        return Err(UserError::new(
            format!("{base} refused the request (HTTP {})", reply.status),
            TrySteps::one("check the signing wallet owns the scene's world or parcel")
                .and("DCL_PRIVATE_KEY or --sign-key picks the wallet"),
        )
        .why(reply.message())
        .into());
    }
    if reply.status >= 400 && reply.status != 404 {
        anyhow::bail!("{base} answered HTTP {}: {}", reply.status, reply.message());
    }
    Ok(reply)
}

async fn value_action(mode: &Mode, scope: Scope<'_>, action: ValueAction) -> Result<Outcome> {
    match mode {
        Mode::File(root) => {
            let db = storage::open(root)?;
            Ok(match action {
                ValueAction::Get { key } => {
                    storage::check_key(&key).map_err(|w| anyhow::anyhow!(w))?;
                    match db.get(scope, &key)? {
                        Some(v) => Outcome::Value(v),
                        None => Outcome::Absent(key),
                    }
                }
                ValueAction::Set { key, value, json } => {
                    storage::check_key(&key).map_err(|w| anyhow::anyhow!(w))?;
                    let value = parse_value(&value, json)?;
                    storage::check_value(scope, &value).map_err(|w| anyhow::anyhow!(w))?;
                    db.check_fits(scope, &key, storage::value_size(&value))?
                        .map_err(|w| anyhow::anyhow!(w))?;
                    db.set(scope, &key, &value, "cli")?;
                    Outcome::Done(format!("set {key}"))
                }
                ValueAction::Delete { key } => match db.delete(scope, &key, "cli")? {
                    true => Outcome::Done(format!("deleted {key}")),
                    false => Outcome::Absent(key),
                },
                ValueAction::List {
                    prefix,
                    limit,
                    offset,
                } => Outcome::Page(
                    db.list(scope, prefix.as_deref(), storage::page_limit(limit), offset)?
                        .to_json(),
                ),
                ValueAction::Clear { confirm } => {
                    need_confirm(confirm, &format!("{} storage", scope.name()))?;
                    let n = db.clear(scope, "cli")?;
                    Outcome::Done(format!("cleared {n} {} value(s)", scope.name()))
                }
            })
        }
        Mode::Http {
            base,
            signer,
            metadata,
        } => {
            let signer = signer.as_ref();
            Ok(match action {
                ValueAction::Get { key } => {
                    let reply = send(
                        base,
                        signer,
                        metadata,
                        "GET",
                        &scope_path(scope, Some(&key)),
                        None,
                        None,
                        false,
                    )
                    .await?;
                    match reply.status {
                        404 => Outcome::Absent(key),
                        _ => Outcome::Value(
                            reply
                                .json()
                                .and_then(|v| v.get("value").cloned())
                                .unwrap_or(Value::Null),
                        ),
                    }
                }
                ValueAction::Set { key, value, json } => {
                    let value = parse_value(&value, json)?;
                    send(
                        base,
                        signer,
                        metadata,
                        "PUT",
                        &scope_path(scope, Some(&key)),
                        None,
                        Some(json!({ "value": value })),
                        false,
                    )
                    .await?;
                    Outcome::Done(format!("set {key}"))
                }
                ValueAction::Delete { key } => {
                    let reply = send(
                        base,
                        signer,
                        metadata,
                        "DELETE",
                        &scope_path(scope, Some(&key)),
                        None,
                        None,
                        false,
                    )
                    .await?;
                    match reply.status {
                        404 => Outcome::Absent(key),
                        _ => Outcome::Done(format!("deleted {key}")),
                    }
                }
                ValueAction::List {
                    prefix,
                    limit,
                    offset,
                } => {
                    let mut query = vec![format!("offset={offset}")];
                    if let Some(p) = prefix {
                        query.push(format!("prefix={}", crate::deploy::encode_segment(&p)));
                    }
                    if let Some(l) = limit {
                        query.push(format!("limit={l}"));
                    }
                    let reply = send(
                        base,
                        signer,
                        metadata,
                        "GET",
                        &scope_path(scope, None),
                        Some(&query.join("&")),
                        None,
                        false,
                    )
                    .await?;
                    Outcome::Page(reply.json().unwrap_or_else(
                        || json!({ "data": [], "pagination": { "offset": offset, "total": 0 } }),
                    ))
                }
                ValueAction::Clear { confirm } => {
                    need_confirm(confirm, &format!("{} storage on {base}", scope.name()))?;
                    send(
                        base,
                        signer,
                        metadata,
                        "DELETE",
                        &scope_path(scope, None),
                        None,
                        None,
                        true,
                    )
                    .await?;
                    Outcome::Done(format!("cleared {} storage", scope.name()))
                }
            })
        }
    }
}

async fn env_action(mode: &Mode, root: Option<&Path>, action: EnvAction) -> Result<Outcome> {
    match mode {
        Mode::File(root) => {
            let db = storage::open(root)?;
            Ok(match action {
                EnvAction::Get { key } => match storage::env_value(root, &db, &key)? {
                    Some(v) => Outcome::Value(Value::String(v)),
                    None => Outcome::Absent(key),
                },
                EnvAction::Set { key, value } => {
                    storage::check_key(&key).map_err(|w| anyhow::anyhow!(w))?;
                    db.env_set(&key, &value, "cli")?;
                    Outcome::Done(format!("set {key}"))
                }
                EnvAction::Delete { key } => match db.delete(Scope::Env, &key, "cli")? {
                    true => Outcome::Done(format!("deleted {key}")),
                    false => Outcome::Absent(key),
                },
                EnvAction::List => {
                    let entries = storage::env_entries(root, &db)?;
                    Outcome::Page(json!({
                        "data": entries.iter().map(|e| json!({
                            "key": e.key, "value": e.value,
                            "source": match e.source { storage::EnvSource::Runtime => "runtime", storage::EnvSource::DotEnv => "dotenv" }
                        })).collect::<Vec<_>>(),
                        "pagination": { "offset": 0, "total": entries.len() }
                    }))
                }
                EnvAction::Clear { confirm } => {
                    need_confirm(confirm, "the runtime environment")?;
                    let n = db.clear(Scope::Env, "cli")?;
                    Outcome::Done(format!("cleared {n} variable(s)"))
                }
            })
        }
        Mode::Http { .. } => {
            let _ = root;
            let action = match action {
                EnvAction::Get { key } => ValueAction::Get { key },
                EnvAction::Set { key, value } => ValueAction::Set {
                    key,
                    value,
                    json: false,
                },
                EnvAction::Delete { key } => ValueAction::Delete { key },
                EnvAction::List => ValueAction::List {
                    prefix: None,
                    limit: None,
                    offset: 0,
                },
                EnvAction::Clear { confirm } => ValueAction::Clear { confirm },
            };
            value_action(mode, Scope::Env, action).await
        }
    }
}

/// Upstream says which scene it acts for before touching a service.
fn announce(mode: &Mode, as_json: bool) {
    if let Mode::Http { base, metadata, .. } = mode {
        if !as_json {
            ux::note_stderr(format!("{} on {base}", scene_line(metadata)));
        }
    }
}

/// `DELETE /players`: every player's values at once.
async fn clear_all_players(mode: &Mode) -> Result<Outcome> {
    match mode {
        Mode::File(root) => {
            let n = storage::open(root)?.clear_all_players("cli")?;
            Ok(Outcome::Done(format!("cleared {n} player value(s)")))
        }
        Mode::Http {
            base,
            signer,
            metadata,
        } => {
            send(
                base,
                signer.as_ref(),
                metadata,
                "DELETE",
                "/players",
                None,
                None,
                true,
            )
            .await?;
            Ok(Outcome::Done("cleared every player's storage".to_string()))
        }
    }
}

pub async fn run(command: StorageCommand) -> Result<()> {
    match command {
        StorageCommand::Scene { action, opts } => {
            let mode = resolve_mode(&opts)?;
            announce(&mode, opts.json);
            print(value_action(&mode, Scope::Scene, action).await?, opts.json)
        }
        StorageCommand::Player {
            address,
            action,
            opts,
        } => {
            let mode = resolve_mode(&opts)?;
            announce(&mode, opts.json);
            let address = match (address, &action) {
                // upstream: `player clear` with no address clears every player
                (None, ValueAction::Clear { confirm }) => {
                    need_confirm(*confirm, "every player's storage")?;
                    return print(clear_all_players(&mode).await?, opts.json);
                }
                (None, _) => {
                    return Err(UserError::new(
                        "player storage needs the player's address",
                        TrySteps::one("add --address 0x…"),
                    )
                    .into())
                }
                (Some(address), _) => {
                    storage::normalize_address(&address).map_err(|w| anyhow::anyhow!(w))?
                }
            };
            print(
                value_action(&mode, Scope::Player(&address), action).await?,
                opts.json,
            )
        }
        StorageCommand::Env { action, opts } => {
            let mode = resolve_mode(&opts)?;
            announce(&mode, opts.json);
            let root = match &mode {
                Mode::File(root) => Some(root.clone()),
                Mode::Http { .. } => None,
            };
            print(env_action(&mode, root.as_deref(), action).await?, opts.json)
        }
        StorageCommand::Target { target, dir } => {
            let project = Project::load(&dir)?;
            let db = storage::open(&project.root)?;
            match target {
                Some(raw) => {
                    let target = Target::parse(&raw).map_err(|why| {
                        UserError::new(why, TrySteps::one("local | org | zone | https://…"))
                    })?;
                    db.set_target(&target)?;
                    ux::note_good(format!("storage target: {}", target.label()));
                }
                None => println!("{}", db.target()?.to_arg()),
            }
            Ok(())
        }
        StorageCommand::Export { dir, out } => {
            let project = Project::load(&dir)?;
            let store = storage::open(&project.root)?.export()?;
            let text = serde_json::to_string_pretty(&store)?;
            match out {
                Some(path) => {
                    std::fs::write(&path, format!("{text}\n"))
                        .with_context(|| format!("writing {}", path.display()))?;
                    ux::note_good(format!("wrote {}", path.display()));
                }
                None => println!("{text}"),
            }
            Ok(())
        }
        StorageCommand::Import { file, merge, dir } => {
            let project = Project::load(&dir)?;
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("reading {}", file.display()))?;
            let store = serde_json::from_str::<Value>(&text)
                .map_err(anyhow::Error::new)
                .and_then(Store::from_value)
                .map_err(|e| {
                    UserError::new(
                        format!("{} is not a storage snapshot", file.display()),
                        TrySteps::one("expected { \"env\": {}, \"world\": {}, \"players\": {} }"),
                    )
                    .why(e.to_string())
                })?;
            let db = storage::open(&project.root)?;
            db.import(&store, merge, "cli")?;
            let (scene, player, env) = db.counts()?;
            ux::note_good(format!(
                "{} {}: {scene} scene, {player} player, {env} env value(s)",
                if merge { "merged" } else { "imported" },
                file.display()
            ));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dcl-one-sdk-storage-cli-{tag}-{}-{:x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/index.js"), "").unwrap();
        std::fs::write(
            dir.join("scene.json"),
            json!({ "main": "bin/index.js", "runtimeVersion": "7", "scene": { "base": "0,0", "parcels": ["0,0"] } }).to_string(),
        )
        .unwrap();
        dir
    }

    fn opts(dir: &Path, target: Option<&str>) -> StorageOptions {
        StorageOptions {
            dir: dir.to_path_buf(),
            target: target.map(str::to_string),
            preview: "http://127.0.0.1:1".to_string(),
            sign_key: None,
            json: false,
            port: None,
            no_browser: false,
            https: false,
        }
    }

    #[test]
    fn values_are_text_unless_json_is_asked_for() {
        assert_eq!(parse_value("7", false).unwrap(), json!("7"));
        assert_eq!(parse_value("7", true).unwrap(), json!(7));
        assert_eq!(parse_value("{\"a\":1}", true).unwrap(), json!({ "a": 1 }));
        assert_eq!(
            parse_value("plain words", false).unwrap(),
            json!("plain words")
        );
        assert!(parse_value("plain words", true).is_err());
        assert!(is_local_development("http://localhost:8000"));
        assert!(is_local_development("http://127.0.0.1:5199/storage"));
        assert!(!is_local_development(storage::ORG_URL));
        assert_eq!(
            scene_line(&SceneMetadata {
                world: Some("my.dcl.eth".into()),
                parcel: "0,0".into()
            }),
            "World: my.dcl.eth"
        );
        assert_eq!(
            scene_line(&SceneMetadata {
                world: None,
                parcel: "-3,12".into()
            }),
            "Genesis City scene at parcel -3,12"
        );
    }

    #[tokio::test]
    async fn file_mode_edits_the_projects_database_directly() {
        let dir = project("file");
        let mode = resolve_mode(&opts(&dir, None)).unwrap();
        assert!(matches!(mode, Mode::File(_)));
        let set = value_action(
            &mode,
            Scope::Scene,
            ValueAction::Set {
                key: "k".into(),
                value: "[1,2]".into(),
                json: true,
            },
        )
        .await
        .unwrap();
        assert!(matches!(set, Outcome::Done(ref w) if w == "set k"));
        let got = value_action(&mode, Scope::Scene, ValueAction::Get { key: "k".into() })
            .await
            .unwrap();
        assert!(matches!(got, Outcome::Value(ref v) if *v == json!([1, 2])));
        let db = storage::open(&dir).unwrap();
        assert_eq!(
            db.list(Scope::Scene, None, storage::ALL, 0).unwrap().data[0].source,
            "cli"
        );
        let missing = value_action(&mode, Scope::Scene, ValueAction::Get { key: "zz".into() })
            .await
            .unwrap();
        assert!(matches!(missing, Outcome::Absent(ref k) if k == "zz"));
        let refused = value_action(&mode, Scope::Scene, ValueAction::Clear { confirm: false })
            .await
            .unwrap_err();
        assert!(
            refused.to_string().contains("cannot be undone"),
            "{refused}"
        );
        let cleared = value_action(&mode, Scope::Scene, ValueAction::Clear { confirm: true })
            .await
            .unwrap();
        assert!(matches!(cleared, Outcome::Done(ref w) if w == "cleared 1 scene value(s)"));
        std::fs::write(dir.join(".env"), "FROM_FILE=1\n").unwrap();
        let env = env_action(
            &mode,
            Some(&dir),
            EnvAction::Get {
                key: "FROM_FILE".into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(env, Outcome::Value(Value::String(ref s)) if s == "1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_remembered_target_picks_the_mode_and_flags_override_it() {
        let _guard = crate::deploy::ENV_LOCK.lock().await;
        std::env::remove_var("DCL_PRIVATE_KEY");
        let dir = project("mode");
        storage::open(&dir)
            .unwrap()
            .set_target(&Target::Zone)
            .unwrap();
        match resolve_mode(&opts(&dir, None)).unwrap() {
            Mode::Http {
                base,
                signer,
                metadata,
            } => {
                assert_eq!(base, storage::ZONE_URL);
                assert!(signer.is_none());
                assert_eq!(metadata.parcel, "0,0");
            }
            Mode::File(_) => panic!("the remembered zone target should be honored"),
        }
        assert!(matches!(
            resolve_mode(&opts(&dir, Some("local"))).unwrap(),
            Mode::File(_)
        ));
        match resolve_mode(&opts(&dir, Some("preview"))).unwrap() {
            Mode::Http { base, signer, .. } => {
                assert_eq!(base, "http://127.0.0.1:1");
                assert!(signer.is_none(), "the preview signs on its own");
            }
            Mode::File(_) => panic!("preview is an http mode"),
        }
        std::env::set_var(
            "DCL_PRIVATE_KEY",
            "0x0123456789012345678901234567890123456789012345678901234567890123",
        );
        match resolve_mode(&opts(&dir, Some("http://localhost:5199/storage"))).unwrap() {
            Mode::Http { base, signer, .. } => {
                assert_eq!(base, "http://localhost:5199/storage");
                assert!(matches!(signer, Some(Signer::Key(_))));
            }
            Mode::File(_) => panic!("a URL is an http mode"),
        }
        std::env::remove_var("DCL_PRIVATE_KEY");
        let err = resolve_mode(&opts(&dir, Some("nonsense")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a storage target"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn http_mode_reads_the_services_answers() {
        use axum::http::{header, HeaderMap, StatusCode};
        // (method, uri, confirm header present, body) per request the mock saw
        type Seen = std::sync::Arc<std::sync::Mutex<Vec<(String, String, bool, String)>>>;
        let seen: Seen = Default::default();
        let log = seen.clone();
        let app = axum::Router::new().fallback(
            move |method: axum::http::Method, uri: axum::http::Uri, headers: HeaderMap, body: String| {
                let log = log.clone();
                async move {
                    let path = uri.path().to_string();
                    log.lock().unwrap().push((
                        method.to_string(),
                        uri.to_string(),
                        headers.contains_key("x-confirm-delete-all"),
                        body,
                    ));
                    let (status, body) = if path.ends_with("/missing") {
                        (StatusCode::NOT_FOUND, r#"{"error":"Not Found","message":"Value not found"}"#)
                    } else if path.ends_with("/forbidden") {
                        (StatusCode::FORBIDDEN, r#"{"message":"not yours"}"#)
                    } else if method == axum::http::Method::GET && path.ends_with("/values") {
                        (StatusCode::OK, r#"{"data":[{"key":"a","value":1}],"pagination":{"offset":0,"total":1}}"#)
                    } else {
                        (StatusCode::OK, r#"{"value":"served"}"#)
                    };
                    (status, [(header::CONTENT_TYPE, "application/json")], body)
                }
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mode = Mode::Http {
            base: base.clone(),
            signer: None,
            metadata: SceneMetadata {
                world: None,
                parcel: "0,0".into(),
            },
        };
        let player = format!("0x{}", "AB".repeat(20));
        let got = value_action(
            &mode,
            Scope::Player(&player),
            ValueAction::Get { key: "k".into() },
        )
        .await
        .unwrap();
        assert!(matches!(got, Outcome::Value(Value::String(ref s)) if s == "served"));
        let missing = value_action(
            &mode,
            Scope::Scene,
            ValueAction::Get {
                key: "missing".into(),
            },
        )
        .await
        .unwrap();
        assert!(matches!(missing, Outcome::Absent(_)));
        let refused = value_action(
            &mode,
            Scope::Scene,
            ValueAction::Get {
                key: "forbidden".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("refused the request (HTTP 403)"),
            "{refused}"
        );
        let page = value_action(
            &mode,
            Scope::Scene,
            ValueAction::List {
                prefix: Some("a b".into()),
                limit: Some(5),
                offset: 2,
            },
        )
        .await
        .unwrap();
        assert!(matches!(page, Outcome::Page(ref p) if p["pagination"]["total"] == 1));
        value_action(&mode, Scope::Scene, ValueAction::Clear { confirm: true })
            .await
            .unwrap();
        env_action(
            &mode,
            None,
            EnvAction::Set {
                key: "K".into(),
                value: "7".into(),
            },
        )
        .await
        .unwrap();
        clear_all_players(&mode).await.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(
            seen[0].1,
            format!("/players/0x{}/values/k", "ab".repeat(20))
        );
        assert_eq!(seen[3].1, "/values?offset=2&prefix=a%20b&limit=5");
        assert_eq!(
            (seen[4].0.as_str(), seen[4].1.as_str(), seen[4].2),
            ("DELETE", "/values", true)
        );
        assert_eq!(
            (seen[5].0.as_str(), seen[5].1.as_str(), seen[5].3.as_str()),
            ("PUT", "/env/K", r#"{"value":"7"}"#),
            "env values stay strings"
        );
        assert_eq!(
            (seen[6].0.as_str(), seen[6].1.as_str(), seen[6].2),
            ("DELETE", "/players", true)
        );
    }
}
