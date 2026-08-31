//! Who may publish where: the verdict for the scene's declared target, and
//! the worlds and land the remembered wallet could target instead. Everything
//! here is public chain/catalyst state keyed by an address — no signature is
//! involved — so the page asks for an address, never a "connection", and
//! every fetch failure collapses into a sentence rather than a guess: a
//! verdict is ✓, ✗, or "could not check", and the third is never dressed up
//! as either of the others.

use super::deploy_status::{host_of, status_client, Dest, STATUS_TTL};
use crate::deploy::{self, DocAnswer};
use serde_json::Value;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

/// Whether the deploy's own permission check would pass, said before the
/// wallet prompt instead of after it.
pub(super) enum Verdict {
    /// The address may publish to the declared target, and why.
    May(String),
    /// It may not; the why names what is missing and the remedy is the exact
    /// step that fixes it.
    MayNot { why: String, remedy: String },
    /// The question went unanswered. The sentence says by whom.
    Unchecked(String),
}

/// One world the address could deploy to.
pub(super) struct WorldRow {
    pub(super) name: String,
    /// `None` when the worlds list never answered for this name — an owned
    /// name with no row is simply empty, and says so.
    pub(super) scenes: Option<i64>,
    pub(super) last_deployed: Option<i64>,
    /// What the world calls itself — in practice the deployed scene's title,
    /// which is exactly the "what is there" a row wants to say.
    pub(super) title: Option<String>,
    /// Owned on-chain, as opposed to reachable through a grant.
    pub(super) owned: bool,
}

/// One declared parcel and the strongest right the address holds on it.
pub(super) struct ParcelRight {
    pub(super) pointer: String,
    pub(super) leg: Option<&'static str>,
}

pub(super) struct Holdings {
    pub(super) parcels: i64,
    pub(super) estates: i64,
    pub(super) operated: i64,
    /// The coordinates the wallet owns or operates (as far as one page of
    /// each answer goes) — what the land map lights up.
    pub(super) coords: Vec<(i64, i64)>,
}

/// Everything the rights fetch learned about one address at one destination.
pub(super) struct Rights {
    pub(super) address: String,
    pub(super) verdict: Verdict,
    pub(super) worlds: Vec<WorldRow>,
    pub(super) worlds_note: Option<String>,
    pub(super) holdings: Option<Holdings>,
    pub(super) parcel_rights: Vec<ParcelRight>,
    /// Declared parcels beyond the per-parcel probe cap, named so a capped
    /// check never reads as a complete one.
    pub(super) unchecked_parcels: usize,
}

impl Rights {
    pub(super) fn unchecked(address: &str, why: &str) -> Self {
        Rights {
            address: address.to_string(),
            verdict: Verdict::Unchecked(why.to_string()),
            worlds: Vec::new(),
            worlds_note: None,
            holdings: None,
            parcel_rights: Vec::new(),
            unchecked_parcels: 0,
        }
    }
}

/// An address is worth asking servers about only when it is one: a page form
/// feeds this, and a stranger's garbage becomes a refusal, not a URL.
pub(super) fn valid_address(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Where "connect a Decentraland account" happens: the authorize page the
/// person's signed-in browser opens, and the relay this process polls for
/// the signed answer. A configured target hosts both on its own domain (the
/// sites tier serves `/auth/native` and its single-read relay); without one,
/// catalyst.example.com — the flow only needs a mailbox and a page that can sign, not the
/// destination's blessing, and the signature is verified here either way.
pub(super) struct AuthBases {
    pub(super) page: String,
    pub(super) relay: String,
}

/// The bases whose authorize page actually answers: the configured target's
/// own pair when its sites tier serves `/auth/native`, else the catalyst.example.com
/// pair — probed at connect time, because a stale self-hosted realm 404s
/// the page and a sign-in that lands on a 404 helps nobody. The relay is
/// only a mailbox and the signature verifies here either way, so the
/// fallback grants catalyst.example.com no authority.
pub(super) async fn working_auth_bases(default_target: Option<&str>) -> AuthBases {
    let own = auth_bases(default_target);
    let public = auth_bases(None);
    if own.page == public.page {
        return own;
    }
    let answers = status_client()
        .get(&own.page)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    match answers {
        true => own,
        false => public,
    }
}

pub(super) fn auth_bases(default_target: Option<&str>) -> AuthBases {
    let root = match default_target.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => {
            let base = deploy::sanitize_catalyst_url(t);
            base.trim_end_matches("/content").to_string()
        }
        None => "https://catalyst.example.com".to_string(),
    };
    AuthBases {
        page: format!("{root}/auth/native"),
        relay: format!("{root}/internal/native-auth-relay"),
    }
}

/// The delegation message the authorize page has the wallet sign — the sites
/// tier's `buildEphemeralMessage`, byte for byte, because recovery only
/// proves an address against the exact text.
pub(super) fn ephemeral_message(ephemeral: &str, expiration: &str) -> String {
    format!("Decentraland Login\nEphemeral address: {ephemeral}\nExpiration: {expiration}")
}

/// The address a relayed approval proves, or why it proves nothing. The
/// relay stores whatever was posted at it, so nothing in the entry is
/// trusted: the ephemeral and expiration must be the ones this process
/// minted, and the signature must recover to the signer it names — a
/// forgery is a refusal, never a shrug.
pub(super) fn relayed_address(
    ephemeral: &str,
    expiration: &str,
    entry: &Value,
) -> Result<String, String> {
    let field = |k: &str| entry.get(k).and_then(|v| v.as_str());
    let signer = field("signer").ok_or_else(|| "the approval named no signer".to_string())?;
    let signature =
        field("signature").ok_or_else(|| "the approval carried no signature".to_string())?;
    if field("ephemeral") != Some(ephemeral) {
        return Err("the approval was for a different session key".to_string());
    }
    if field("expiration") != Some(expiration) {
        return Err("the approval was for a different expiration".to_string());
    }
    let message = ephemeral_message(ephemeral, expiration);
    let recovered = catalyrst_crypto::recover::recover_address(message.as_bytes(), signature)
        .map_err(|e| format!("the signature did not verify ({e})"))?;
    if !recovered.eq_ignore_ascii_case(signer) {
        return Err("the signature was not the signer's".to_string());
    }
    Ok(recovered.to_lowercase())
}

/// Per-parcel probes are one request each; past this many the page says how
/// many went unchecked instead of stalling the render on a big estate.
pub(super) const PARCEL_PROBE_CAP: usize = 12;

async fn get_json(url: &str) -> Result<Value, String> {
    let resp = status_client()
        .get(url)
        .send()
        .await
        .map_err(|_| format!("could not reach {}", host_of(url)))?;
    if !resp.status().is_success() {
        return Err(format!(
            "{} answered HTTP {}",
            host_of(url),
            resp.status().as_u16()
        ));
    }
    resp.json()
        .await
        .map_err(|_| format!("{} sent an unreadable answer", host_of(url)))
}

async fn post_json(url: &str, body: &Value) -> Result<Value, String> {
    let resp = status_client()
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|_| format!("could not reach {}", host_of(url)))?;
    if !resp.status().is_success() {
        return Err(format!(
            "{} answered HTTP {}",
            host_of(url),
            resp.status().as_u16()
        ));
    }
    resp.json()
        .await
        .map_err(|_| format!("{} sent an unreadable answer", host_of(url)))
}

/// `elements[].name` of the lambdas names page, as world names.
pub(super) fn parse_names(v: &Value) -> Vec<String> {
    v.get("elements")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("name").and_then(|n| n.as_str()))
                .map(|n| match n.ends_with(".dcl.eth") {
                    true => n.to_lowercase(),
                    false => format!("{}.dcl.eth", n.to_lowercase()),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A timestamp the way either worlds tier says it: epoch milliseconds, or
/// the ISO-8601 string the public server sends.
fn when_ms(v: &Value) -> Option<i64> {
    if let Some(ms) = v.as_i64() {
        return Some(ms);
    }
    chrono::DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.timestamp_millis())
}

/// The `/worlds` rows: name, scene count, title, last deploy. Field absence
/// is data too — a server that lists worlds without counts still lists.
pub(super) fn parse_world_rows(v: &Value) -> Vec<WorldRow> {
    v.get("worlds")
        .and_then(|w| w.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|w| {
                    let name = w.get("name").and_then(|n| n.as_str())?;
                    Some(WorldRow {
                        name: name.to_lowercase(),
                        scenes: w.get("deployed_scenes").and_then(|s| s.as_i64()),
                        last_deployed: w.get("last_deployed_at").and_then(when_ms),
                        title: w
                            .get("title")
                            .and_then(|t| t.as_str())
                            .filter(|t| !t.trim().is_empty())
                            .map(str::to_string),
                        owned: false,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Owned names and the deployer-authorized list, as one list: the worlds DB
/// only knows names that have touched it, so a name owned on-chain with no
/// row still belongs on the page — as an empty world.
pub(super) fn merge_worlds(names: Vec<String>, mut listed: Vec<WorldRow>) -> Vec<WorldRow> {
    for name in names {
        match listed.iter_mut().find(|w| w.name == name) {
            Some(row) => row.owned = true,
            None => listed.push(WorldRow {
                name,
                scenes: None,
                last_deployed: None,
                title: None,
                owned: true,
            }),
        }
    }
    listed.sort_by(|a, b| {
        b.last_deployed
            .unwrap_or(0)
            .cmp(&a.last_deployed.unwrap_or(0))
            .then_with(|| a.name.cmp(&b.name))
    });
    listed
}

/// The lands and lands-permissions pages, folded to the numbers the
/// inventory line says and the coordinates the map lights up. Both routes
/// speak stringified coordinates; a number is taken too rather than argued
/// with.
pub(super) fn parse_holdings(lands: &Value, operated: &Value) -> Holdings {
    let count = |v: &Value, cat: &str| {
        v.get("elements")
            .and_then(|e| e.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|e| e.get("category").and_then(|c| c.as_str()) == Some(cat))
                    .count() as i64
            })
            .unwrap_or(0)
    };
    let axis = |e: &Value, k: &str| -> Option<i64> {
        match e.get(k)? {
            Value::String(s) => s.parse().ok(),
            other => other.as_i64(),
        }
    };
    let mut coords: Vec<(i64, i64)> = Vec::new();
    for v in [lands, operated] {
        if let Some(arr) = v.get("elements").and_then(|e| e.as_array()) {
            for e in arr {
                if let (Some(x), Some(y)) = (axis(e, "x"), axis(e, "y")) {
                    if !coords.contains(&(x, y)) {
                        coords.push((x, y));
                    }
                }
            }
        }
    }
    Holdings {
        parcels: count(lands, "parcel"),
        estates: count(lands, "estate"),
        operated: operated
            .get("totalAmount")
            .and_then(|t| t.as_i64())
            .unwrap_or(0),
        coords,
    }
}

/// The strongest deploy-granting leg in a flags document, in the validator's
/// own precedence order — `null` flags (an unindexed parcel) grant nothing,
/// which is exactly what the deploy would decide.
pub(super) fn flags_leg(flags: &Value) -> Option<&'static str> {
    const LEGS: [(&str, &str); 5] = [
        ("owner", "owner"),
        ("operator", "operator"),
        ("updateOperator", "update operator"),
        ("updateManager", "update manager"),
        ("approvedForAll", "approved for all"),
    ];
    LEGS.iter()
        .find(|(key, _)| flags.get(*key).and_then(|v| v.as_bool()) == Some(true))
        .map(|(_, label)| *label)
}

/// Why a granted world verdict is granted, from the same document the
/// decision read.
pub(super) fn world_grant_reason(doc: &Value, address: &str) -> &'static str {
    if doc
        .get("owner")
        .and_then(|o| o.as_str())
        .is_some_and(|o| o.eq_ignore_ascii_case(address))
    {
        return "you own this name";
    }
    if doc
        .get("permissions")
        .and_then(|p| p.get("deployment"))
        .and_then(|d| d.get("type"))
        .and_then(|t| t.as_str())
        == Some("unrestricted")
    {
        return "deployment is open on this world";
    }
    "deployment granted to this wallet"
}

/// The verdict for a world destination, over documents already fetched.
pub(super) fn world_verdict(
    doc: &Value,
    scoped: Option<&Value>,
    world: &str,
    address: &str,
    deploying: &[String],
) -> Verdict {
    if matches!(
        deploy::deployment_permission_in_doc(doc, address),
        DocAnswer::Granted
    ) {
        return Verdict::May(world_grant_reason(doc, address).to_string());
    }
    let Some(scoped) = scoped else {
        return Verdict::Unchecked("the scoped parcel grants went unfetched".to_string());
    };
    let denied = deploy::denied_parcels_in(scoped, deploying);
    if denied.is_empty() {
        return Verdict::May(format!(
            "granted on all {} declared parcel{}",
            deploying.len().max(1),
            if deploying.len() == 1 { "" } else { "s" },
        ));
    }
    let owner = doc
        .get("owner")
        .and_then(|o| o.as_str())
        .unwrap_or("the owner");
    Verdict::MayNot {
        why: format!(
            "no deploy permission on {world} for parcel{} {}",
            if denied.len() == 1 { "" } else { "s" },
            denied.join(", ")
        ),
        remedy: format!(
            "ask {owner} to grant it: dcl-one-sdk world permissions grant {world} deployment {address}"
        ),
    }
}

/// The verdict for a land destination, over the per-parcel rights rows.
pub(super) fn land_verdict(rows: &[ParcelRight], unchecked: usize) -> Verdict {
    if rows.is_empty() {
        return Verdict::Unchecked("no declared parcel could be checked".to_string());
    }
    let denied: Vec<&str> = rows
        .iter()
        .filter(|r| r.leg.is_none())
        .map(|r| r.pointer.as_str())
        .collect();
    if !denied.is_empty() {
        return Verdict::MayNot {
            why: format!(
                "this wallet holds no right on parcel{} {}",
                if denied.len() == 1 { "" } else { "s" },
                denied.join(", ")
            ),
            remedy: "sign with a wallet that owns or operates every declared parcel, \
                     or reshape the scene onto parcels it does"
                .to_string(),
        };
    }
    match unchecked {
        0 => Verdict::May(format!(
            "rights held on all {} declared parcel{}",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" },
        )),
        n => Verdict::May(format!(
            "rights held on the {} parcels checked ({n} more unchecked)",
            rows.len()
        )),
    }
}

/// One look at everything the address can reach, fetched concurrently — the
/// slowest probe bounds the wait, not the sum.
pub(super) async fn fetch_rights(dest: &Dest, address: &str) -> Rights {
    let addr = address.to_lowercase();
    let lambdas = dest.chain_lambdas.trim_end_matches('/');
    let worlds = dest.worlds_base.trim_end_matches('/');

    let names_url = format!("{lambdas}/users/{addr}/names?pageSize=100&pageNum=1");
    let list_url = format!("{worlds}/worlds?authorized_deployer={addr}&limit=100");
    let lands_url = format!("{lambdas}/users/{addr}/lands?pageSize=100&pageNum=1");
    let operated_url = format!("{lambdas}/users/{addr}/lands-permissions?pageSize=100&pageNum=1");

    let (names, listed, lands, operated, verdict_parts) = tokio::join!(
        get_json(&names_url),
        get_json(&list_url),
        get_json(&lands_url),
        get_json(&operated_url),
        verdict_fetch(dest, &addr),
    );

    let mut worlds_note = None;
    let owned = match names {
        Ok(v) => parse_names(&v),
        Err(why) => {
            worlds_note = Some(format!("owned names unchecked: {why}"));
            Vec::new()
        }
    };
    let rows = match listed {
        Ok(v) => parse_world_rows(&v),
        // A self-hosted realm often runs no worlds service at all — the
        // route 404s by design, not by failure — so the list falls back to
        // the public worlds server, where the wallet's worlds actually
        // live, and the note says whose answer this is.
        Err(_) if worlds != deploy::WORLDS_CONTENT_SERVER => {
            let upstream = format!(
                "{}/worlds?authorized_deployer={addr}&limit=100",
                deploy::WORLDS_CONTENT_SERVER
            );
            match get_json(&upstream).await {
                Ok(v) => {
                    worlds_note = Some(format!(
                        "the target runs no worlds service \u{2014} listing {}",
                        host_of(deploy::WORLDS_CONTENT_SERVER)
                    ));
                    parse_world_rows(&v)
                }
                Err(why) => {
                    if worlds_note.is_none() {
                        worlds_note = Some(format!("granted worlds unchecked: {why}"));
                    }
                    Vec::new()
                }
            }
        }
        Err(why) => {
            // The public worlds server answered no list; owned names still
            // render, so this is a footnote, not a failure.
            if worlds_note.is_none() {
                worlds_note = Some(format!("granted worlds unchecked: {why}"));
            }
            Vec::new()
        }
    };
    let holdings = match (&lands, &operated) {
        (Ok(l), Ok(o)) => Some(parse_holdings(l, o)),
        (Ok(l), Err(_)) => Some(parse_holdings(l, &Value::Null)),
        _ => None,
    };
    let (verdict, parcel_rights, unchecked_parcels) = verdict_parts;

    Rights {
        address: address.to_string(),
        verdict,
        worlds: merge_worlds(owned, rows),
        worlds_note,
        holdings,
        parcel_rights,
        unchecked_parcels,
    }
}

/// The declared target's verdict: the world permission documents, or the
/// per-parcel flags — batch route first (one request), the per-parcel route
/// as the fallback that also works against a public catalyst.
async fn verdict_fetch(dest: &Dest, addr: &str) -> (Verdict, Vec<ParcelRight>, usize) {
    if let Some(w) = &dest.world {
        let worlds = dest.worlds_base.trim_end_matches('/');
        let doc_url = format!("{worlds}/world/{}/permissions", deploy::encode_segment(w));
        let doc = match get_json(&doc_url).await {
            Ok(doc) => doc,
            Err(why) => {
                return (
                    Verdict::Unchecked(format!("could not check permissions: {why}")),
                    Vec::new(),
                    0,
                )
            }
        };
        let scoped = match deploy::deployment_permission_in_doc(&doc, addr) {
            DocAnswer::Granted => None,
            DocAnswer::NeedsParcels => {
                let url = format!(
                    "{worlds}/world/{}/permissions/deployment/address/{addr}/parcels",
                    deploy::encode_segment(w)
                );
                Some(get_json(&url).await.unwrap_or(Value::Null))
            }
        };
        return (
            world_verdict(&doc, scoped.as_ref(), w, addr, &dest.pointers),
            Vec::new(),
            0,
        );
    }
    if dest.pointers.is_empty() {
        return (
            Verdict::Unchecked("scene.json declares no parcels".to_string()),
            Vec::new(),
            0,
        );
    }
    let lambdas = dest.lambdas_base.trim_end_matches('/');
    let batch_url = format!("{lambdas}/users/{addr}/parcels/permissions");
    let batch: Vec<String> = dest.pointers.iter().take(100).cloned().collect();
    if let Ok(v) = post_json(&batch_url, &serde_json::json!({ "parcels": batch })).await {
        let rows: Vec<ParcelRight> = v
            .get("elements")
            .and_then(|e| e.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|e| ParcelRight {
                        pointer: format!(
                            "{},{}",
                            e.get("x").and_then(|x| x.as_i64()).unwrap_or(0),
                            e.get("y").and_then(|y| y.as_i64()).unwrap_or(0)
                        ),
                        leg: e.get("permissions").and_then(flags_leg),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !rows.is_empty() {
            let unchecked = dest.pointers.len().saturating_sub(rows.len());
            return (land_verdict(&rows, unchecked), rows, unchecked);
        }
    }
    // One probe per parcel, a handful in flight at once: serially this was
    // the slowest thing a target flip could trigger (every probe a public
    // round-trip), and the answers are independent. `buffered` keeps the
    // rows in declared-parcel order, and the first error still ends the
    // check exactly where the serial loop did — in-flight probes drop.
    use futures::StreamExt;
    let mut rows = Vec::new();
    let probe_stream = dest
        .pointers
        .iter()
        .take(PARCEL_PROBE_CAP)
        .filter_map(|pointer| {
            let (x, y) = catalyrst_types::pointer::parse_pointer(pointer)?;
            let url = format!("{lambdas}/users/{addr}/parcels/{x}/{y}/permissions");
            Some(async move {
                get_json(&url).await.map(|flags| ParcelRight {
                    pointer: pointer.clone(),
                    leg: flags_leg(&flags),
                })
            })
        })
        .collect::<Vec<_>>();
    let mut probe_stream = futures::stream::iter(probe_stream).buffered(6);
    while let Some(result) = probe_stream.next().await {
        match result {
            Ok(row) => rows.push(row),
            Err(why) => {
                return (
                    Verdict::Unchecked(format!("could not check parcel rights: {why}")),
                    rows,
                    0,
                )
            }
        }
    }
    let unchecked = dest.pointers.len().saturating_sub(rows.len());
    (land_verdict(&rows, unchecked), rows, unchecked)
}

/// The rights cache: same TTL and eviction as the live-status cache, keyed
/// by everything that can change the answer.
pub(super) type RightsCache = Mutex<Vec<(String, Instant, Arc<Rights>)>>;

fn rights_key(dest: &Dest, address: &str) -> String {
    format!(
        "{address}|{}|{}|{}",
        dest.lambdas_base,
        dest.worlds_base,
        dest.world
            .clone()
            .unwrap_or_else(|| dest.pointers.join(";"))
    )
}

/// The cached answer if it is still warm, without ever fetching: the no-wait
/// read the instant page render uses while a background task warms the cache.
pub(super) fn rights_peek(cache: &RightsCache, dest: &Dest, address: &str) -> Option<Arc<Rights>> {
    let key = rights_key(dest, address);
    cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(k, at, _)| *k == key && at.elapsed() < STATUS_TTL)
        .map(|(_, _, v)| v.clone())
}

pub(super) async fn cached_rights(cache: &RightsCache, dest: &Dest, address: &str) -> Arc<Rights> {
    let key = rights_key(dest, address);
    let hit = cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(k, at, _)| *k == key && at.elapsed() < STATUS_TTL)
        .map(|(_, _, v)| v.clone());
    if let Some(hit) = hit {
        return hit;
    }
    let entry = Arc::new(fetch_rights(dest, address).await);
    let mut c = cache.lock().unwrap_or_else(PoisonError::into_inner);
    c.retain(|(k, at, _)| *k != key && at.elapsed() < STATUS_TTL);
    c.push((key, Instant::now(), entry.clone()));
    entry
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The names page answers bare labels; the worlds tier speaks
    /// `name.dcl.eth`. The parser speaks the worlds dialect so the merge has
    /// one spelling to match on.
    #[test]
    fn names_become_world_names() {
        let v = json!({ "elements": [
            { "name": "Gather" }, { "name": "plaza.dcl.eth" }
        ], "totalAmount": 2 });
        assert_eq!(parse_names(&v), ["gather.dcl.eth", "plaza.dcl.eth"]);
        assert!(parse_names(&json!({})).is_empty());
    }

    /// An owned name with no worlds row still lists — as an empty world —
    /// and a listed world the wallet also owns is marked owned rather than
    /// listed twice. The public server speaks ISO timestamps and the
    /// self-hosted one milliseconds; both become the same clock.
    #[test]
    fn owned_names_merge_into_the_listed_worlds() {
        let listed = parse_world_rows(&json!({ "worlds": [
            { "name": "Gather.dcl.eth", "title": "Gather2", "deployed_scenes": 2,
              "last_deployed_at": 100 },
            { "name": "granted.dcl.eth", "deployed_scenes": 1,
              "last_deployed_at": "2026-07-21T15:09:46.910Z" }
        ], "total": 2 }));
        let merged = merge_worlds(
            vec!["gather.dcl.eth".into(), "fresh.dcl.eth".into()],
            listed,
        );
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].name, "granted.dcl.eth", "latest deploy first");
        assert!(!merged[0].owned, "reachable through a grant only");
        assert!(
            merged[0]
                .last_deployed
                .is_some_and(|ms| ms > 1_700_000_000_000),
            "the ISO timestamp became milliseconds: {:?}",
            merged[0].last_deployed
        );
        assert!(merged[1].owned && merged[1].scenes == Some(2));
        assert_eq!(merged[1].title.as_deref(), Some("Gather2"));
        let fresh = &merged[2];
        assert!(fresh.owned && fresh.scenes.is_none() && fresh.name == "fresh.dcl.eth");
    }

    /// The five legs in the validator's order, and a `null` flags document
    /// (an unindexed parcel) grants nothing — the same answer the deploy
    /// would give.
    #[test]
    fn the_strongest_leg_names_the_right() {
        let flags = |k: &str| {
            let mut f = json!({ "owner": false, "operator": false,
                "updateOperator": false, "updateManager": false, "approvedForAll": false });
            f[k] = json!(true);
            f
        };
        assert_eq!(flags_leg(&flags("owner")), Some("owner"));
        assert_eq!(flags_leg(&flags("updateOperator")), Some("update operator"));
        assert_eq!(
            flags_leg(&flags("approvedForAll")),
            Some("approved for all")
        );
        assert_eq!(flags_leg(&json!(null)), None);
        assert_eq!(
            flags_leg(&flags("owner").as_object().map(|_| json!({})).unwrap()),
            None
        );
    }

    /// The world verdict over the same documents the deploy reads: owner and
    /// world-wide grants pass on the first document, a parcel-scoped grant
    /// passes only when it covers every declared parcel, and the refusal
    /// carries the exact grant command.
    #[test]
    fn the_world_verdict_matches_the_deploy_check() {
        let deploying = vec!["0,0".to_string(), "0,1".to_string()];
        let owner_doc = json!({ "owner": "0xAB", "permissions": {} });
        assert!(matches!(
            world_verdict(&owner_doc, None, "w.dcl.eth", "0xab", &deploying),
            Verdict::May(reason) if reason.contains("own")
        ));

        let scoped_doc = json!({ "owner": "0xowner", "permissions": { "deployment": {
            "type": "allow-list", "wallets": ["0xcd"] } },
            "summary": { "0xcd": [ { "permission": "deployment", "world_wide": false } ] } });
        let all = json!({ "parcels": ["0,0", "0,1"] });
        assert!(matches!(
            world_verdict(&scoped_doc, Some(&all), "w.dcl.eth", "0xcd", &deploying),
            Verdict::May(_)
        ));
        let partial = json!({ "parcels": ["0,0"] });
        let Verdict::MayNot { why, remedy } =
            world_verdict(&scoped_doc, Some(&partial), "w.dcl.eth", "0xcd", &deploying)
        else {
            panic!("a half-covered footprint is a refusal");
        };
        assert!(why.contains("0,1") && !why.contains("0,0"), "{why}");
        assert!(
            remedy.contains("dcl-one-sdk world permissions grant w.dcl.eth deployment 0xcd"),
            "{remedy}"
        );
        assert!(remedy.contains("0xowner"), "{remedy}");

        assert!(matches!(
            world_verdict(&scoped_doc, None, "w.dcl.eth", "0xcd", &deploying),
            Verdict::Unchecked(_)
        ));
    }

    /// The land verdict never averages: one right-less parcel refuses, a
    /// capped check says how much it did not see, and an empty check is
    /// unchecked rather than a quiet pass.
    #[test]
    fn the_land_verdict_refuses_on_one_bad_parcel() {
        let row = |p: &str, leg: Option<&'static str>| ParcelRight {
            pointer: p.to_string(),
            leg,
        };
        let good = vec![
            row("1,1", Some("owner")),
            row("1,2", Some("update operator")),
        ];
        assert!(matches!(land_verdict(&good, 0), Verdict::May(_)));
        assert!(matches!(
            land_verdict(&good, 3),
            Verdict::May(reason) if reason.contains("3 more unchecked")
        ));
        let mixed = vec![row("1,1", Some("owner")), row("1,2", None)];
        let Verdict::MayNot { why, .. } = land_verdict(&mixed, 0) else {
            panic!("one right-less parcel refuses the deploy");
        };
        assert!(why.contains("1,2") && !why.contains("1,1"), "{why}");
        assert!(matches!(land_verdict(&[], 0), Verdict::Unchecked(_)));
    }

    /// The holdings line counts what the two pages actually say, and the
    /// map's coordinates come from both — deduped, stringified or not.
    #[test]
    fn holdings_count_by_category_and_keep_their_coordinates() {
        let lands = json!({ "elements": [
            { "category": "parcel", "x": "5", "y": "-3" },
            { "category": "parcel", "x": "6", "y": "-3" },
            { "category": "estate" }
        ], "totalAmount": 3 });
        let operated = json!({ "elements": [
            { "x": 5, "y": -3 }, { "x": "9", "y": "9" }
        ], "totalAmount": 4 });
        let h = parse_holdings(&lands, &operated);
        assert_eq!((h.parcels, h.estates, h.operated), (2, 1, 4));
        assert_eq!(
            h.coords,
            [(5, -3), (6, -3), (9, 9)],
            "deduped, both sources"
        );
    }

    /// The address form feeds URLs; only a plausible address may become one.
    #[test]
    fn only_a_plausible_address_is_worth_asking_about() {
        assert!(valid_address("0x1234567890abcdef1234567890abcdef12345678"));
        assert!(!valid_address("0x1234"));
        assert!(!valid_address("1234567890abcdef1234567890abcdef1234567890"));
        assert!(!valid_address("0x1234567890abcdef1234567890abcdef1234567g"));
    }

    /// A configured target keeps the sign-in on its own domain only while
    /// its sites tier actually serves the authorize page: a stale realm
    /// 404s it, an unreachable one answers nothing, and both fall back to
    /// the catalyst.example.com pair instead of bouncing the browser onto a dead page.
    #[tokio::test]
    async fn the_connect_bases_fall_back_when_the_target_page_is_missing() {
        let serve = |ok: bool| async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let app = axum::Router::new().route(
                "/auth/native",
                axum::routing::get(move || async move {
                    match ok {
                        true => axum::http::StatusCode::OK,
                        false => axum::http::StatusCode::NOT_FOUND,
                    }
                }),
            );
            tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            base
        };
        let fresh = serve(true).await;
        let bases = working_auth_bases(Some(&fresh)).await;
        assert_eq!(
            bases.page,
            format!("{fresh}/auth/native"),
            "a target that serves the page keeps the sign-in"
        );

        let stale = serve(false).await;
        let bases = working_auth_bases(Some(&stale)).await;
        assert_eq!(
            bases.page, "https://catalyst.example.com/auth/native",
            "a 404 falls back"
        );

        let bases = working_auth_bases(Some("http://127.0.0.1:9")).await;
        assert_eq!(
            bases.page, "https://catalyst.example.com/auth/native",
            "unreachable falls back"
        );
    }

    /// The sign-in happens on the configured target's own domain — where
    /// the sites tier serves the authorize page and its relay — or on
    /// catalyst.example.com when nothing is configured.
    #[test]
    fn the_auth_bases_follow_the_target() {
        let public = auth_bases(None);
        assert_eq!(public.page, "https://catalyst.example.com/auth/native");
        assert_eq!(public.relay, "https://catalyst.example.com/internal/native-auth-relay");
        let own = auth_bases(Some("peer.example.net/content"));
        assert_eq!(own.page, "https://peer.example.net/auth/native");
        assert_eq!(
            own.relay,
            "https://peer.example.net/internal/native-auth-relay"
        );
        assert_eq!(auth_bases(Some("  ")).page, public.page, "blank is unset");
    }

    /// A relayed approval proves an address only through its signature over
    /// the exact delegation this process minted — a forged entry, a swapped
    /// session key, a shifted expiration or somebody else's signature all
    /// become refusals.
    #[test]
    fn a_relayed_approval_is_believed_only_with_a_verifying_signature() {
        let wallet = catalyrst_crypto::Wallet::from_hex(
            "0x0000000000000000000000000000000000000000000000000000000000000001",
        )
        .unwrap();
        let ephemeral = "0x00000000000000000000000000000000000000ab";
        let expiration = "2027-01-01T00:00:00.000Z";
        let message = ephemeral_message(ephemeral, expiration);
        let signature = wallet.sign_message(message.as_bytes()).unwrap();
        let good = json!({
            "signer": wallet.address(),
            "signature": signature,
            "ephemeral": ephemeral,
            "expiration": expiration,
        });
        assert_eq!(
            relayed_address(ephemeral, expiration, &good).unwrap(),
            wallet.address().to_lowercase()
        );
        assert!(
            relayed_address(
                "0x00000000000000000000000000000000000000cd",
                expiration,
                &good
            )
            .is_err(),
            "an approval for another session key proves nothing here"
        );
        assert!(
            relayed_address(ephemeral, "2027-06-01T00:00:00.000Z", &good).is_err(),
            "a shifted expiration changes the signed text"
        );
        let stolen = json!({
            "signer": "0x1234567890abcdef1234567890abcdef12345678",
            "signature": good["signature"],
            "ephemeral": ephemeral,
            "expiration": expiration,
        });
        assert!(
            relayed_address(ephemeral, expiration, &stolen).is_err(),
            "someone else's signature does not become their address"
        );
        assert!(relayed_address(ephemeral, expiration, &json!({})).is_err());
    }
}
