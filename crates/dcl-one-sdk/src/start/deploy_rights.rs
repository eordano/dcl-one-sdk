//! Who may publish where: the verdict for the scene's declared target, and
//! the worlds and land the remembered wallet could target instead. All of it
//! is public chain/catalyst state keyed by an address — no signature — so the
//! page asks for an address, never a "connection", and a fetch failure is a
//! sentence, never a guess: a verdict is ✓, ✗, or "could not check".

use super::deploy_status::{
    cache_get, cache_put, fetch_json, host_of, lock, plural, status_client, Dest, STATUS_TTL,
};
use crate::deploy::{self, DocAnswer, WORLDS_CONTENT_SERVER};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Whether the deploy's own permission check would pass, said before the
/// wallet prompt instead of after it.
pub(super) enum Verdict {
    May(String),
    /// The remedy is the exact step that fixes it.
    MayNot {
        why: String,
        remedy: String,
    },
    /// Unanswered; the sentence says by whom.
    Unchecked(String),
}

/// One world the address could deploy to.
pub(super) struct WorldRow {
    pub(super) name: String,
    /// `None` when the worlds list never answered for this name.
    pub(super) scenes: Option<i64>,
    pub(super) last_deployed: Option<i64>,
    /// In practice the deployed scene's title.
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
    /// Owned or operated (one page of each answer) — what the land map lights up.
    pub(super) coords: Vec<(i64, i64)>,
    /// `coords` split by the right held; an owned parcel is not repeated.
    pub(super) owned: Vec<(i64, i64)>,
    pub(super) operated_coords: Vec<(i64, i64)>,
}

/// Everything the rights fetch learned about one address at one destination.
pub(super) struct Rights {
    pub(super) address: String,
    pub(super) verdict: Verdict,
    pub(super) worlds: Vec<WorldRow>,
    pub(super) worlds_note: Option<String>,
    pub(super) holdings: Option<Holdings>,
    pub(super) parcel_rights: Vec<ParcelRight>,
    /// Declared parcels beyond the per-parcel probe cap, so a capped check
    /// never reads as a complete one.
    pub(super) unchecked_parcels: usize,
    /// Set when the parcel rows came from the chain lambdas, not the target's.
    pub(super) parcels_note: Option<String>,
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
            parcels_note: None,
        }
    }
}

/// A page form feeds this; a stranger's garbage becomes a refusal, not a URL.
pub(super) fn valid_address(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Where "connect a Decentraland account" happens: the authorize page the
/// signed-in browser opens, and the relay this process polls for the signed
/// answer. The relay is only a mailbox — the signature is verified here.
pub(super) struct AuthBases {
    pub(super) page: String,
    pub(super) relay: String,
}

/// The configured target's own pair when its sites tier serves `/auth/native`
/// (a stale self-hosted realm 404s it, and a sign-in on a 404 helps nobody),
/// else the catalyst.example.com pair, which grants catalyst.example.com no authority.
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

/// The sites tier's `buildEphemeralMessage`, byte for byte: recovery only
/// proves an address against the exact text.
pub(super) fn ephemeral_message(ephemeral: &str, expiration: &str) -> String {
    format!("Decentraland Login\nEphemeral address: {ephemeral}\nExpiration: {expiration}")
}

/// The address a relayed approval proves, or why it proves nothing. Nothing
/// in the entry is trusted: the ephemeral and expiration must be the ones
/// this process minted, and the signature must recover to the named signer.
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
    fetch_json(status_client().get(url))
        .await
        .map_err(|e| e.sentence(url))
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

/// Epoch milliseconds, or the ISO-8601 string the public worlds server sends.
fn when_ms(v: &Value) -> Option<i64> {
    if let Some(ms) = v.as_i64() {
        return Some(ms);
    }
    chrono::DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .map(|t| t.timestamp_millis())
}

/// The `/worlds` rows; a server that lists worlds without counts still lists.
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

/// Owned names and the deployer-authorized list as one list: the worlds DB
/// only knows names that have touched it, so an owned name with no row still
/// lists, as an empty world.
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

/// The lands and lands-permissions pages, folded to the inventory numbers and
/// the map's coordinates. Both routes speak stringified coordinates; a number
/// is taken too.
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
    let listed = |v: &Value| -> Vec<(i64, i64)> {
        let mut out: Vec<(i64, i64)> = Vec::new();
        for e in v
            .get("elements")
            .and_then(|e| e.as_array())
            .into_iter()
            .flatten()
        {
            if let (Some(x), Some(y)) = (axis(e, "x"), axis(e, "y")) {
                if !out.contains(&(x, y)) {
                    out.push((x, y));
                }
            }
        }
        out
    };
    let owned = listed(lands);
    let operated_coords: Vec<(i64, i64)> = listed(operated)
        .into_iter()
        .filter(|p| !owned.contains(p))
        .collect();
    let coords = owned
        .iter()
        .chain(operated_coords.iter())
        .copied()
        .collect();
    Holdings {
        parcels: count(lands, "parcel"),
        estates: count(lands, "estate"),
        operated: operated
            .get("totalAmount")
            .and_then(|t| t.as_i64())
            .unwrap_or(0),
        coords,
        owned,
        operated_coords,
    }
}

/// The strongest deploy-granting leg in a flags document, in the validator's
/// precedence order; `null` flags (an unindexed parcel) grant nothing.
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
            plural(deploying.len()),
        ));
    }
    let owner = doc
        .get("owner")
        .and_then(|o| o.as_str())
        .unwrap_or("the owner");
    Verdict::MayNot {
        why: format!(
            "no deploy permission on {world} for parcel{} {}",
            plural(denied.len()),
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
                plural(denied.len()),
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
            plural(rows.len()),
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
    let user = |page: &str| format!("{lambdas}/users/{addr}/{page}?pageSize=100&pageNum=1");
    let list = |base: &str| format!("{base}/worlds?authorized_deployer={addr}&limit=100");

    let (names_url, list_url, lands_url, operated_url) = (
        user("names"),
        list(worlds),
        user("lands"),
        user("lands-permissions"),
    );
    let (names, listed, lands, operated, target) = tokio::join!(
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
    let listed = match listed {
        // A self-hosted realm often runs no worlds service at all — the
        // route 404s by design, not by failure — so the list falls back to
        // the public worlds server, where the wallet's worlds actually
        // live, and the note says whose answer this is.
        Err(_) if worlds != WORLDS_CONTENT_SERVER => {
            get_json(&list(WORLDS_CONTENT_SERVER)).await.inspect(|_| {
                worlds_note = Some(format!(
                    "the target runs no worlds service \u{2014} listing {}",
                    host_of(WORLDS_CONTENT_SERVER)
                ));
            })
        }
        other => other,
    };
    let rows = match listed {
        Ok(v) => parse_world_rows(&v),
        Err(why) => {
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

    Rights {
        address: address.to_string(),
        verdict: target.verdict,
        worlds: merge_worlds(owned, rows),
        worlds_note,
        holdings,
        parcel_rights: target.rows,
        unchecked_parcels: target.unchecked,
        parcels_note: target.note,
    }
}

/// The declared target's verdict, with the per-parcel rows behind a land one
/// and their note when the answer is not the target's own.
struct TargetVerdict {
    verdict: Verdict,
    rows: Vec<ParcelRight>,
    unchecked: usize,
    note: Option<String>,
}

impl TargetVerdict {
    fn bare(verdict: Verdict) -> Self {
        TargetVerdict {
            verdict,
            rows: Vec::new(),
            unchecked: 0,
            note: None,
        }
    }
}

async fn verdict_fetch(dest: &Dest, addr: &str) -> TargetVerdict {
    if let Some(w) = &dest.world {
        let worlds = dest.worlds_base.trim_end_matches('/');
        let doc_url = format!("{worlds}/world/{}/permissions", deploy::encode_segment(w));
        let doc = match get_json(&doc_url).await {
            Ok(doc) => doc,
            Err(why) => {
                return TargetVerdict::bare(Verdict::Unchecked(format!(
                    "could not check permissions: {why}"
                )))
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
        return TargetVerdict::bare(world_verdict(
            &doc,
            scoped.as_ref(),
            w,
            addr,
            &dest.pointers,
        ));
    }
    if dest.pointers.is_empty() {
        return TargetVerdict::bare(Verdict::Unchecked(
            "scene.json declares no parcels".to_string(),
        ));
    }
    // A worlds server (or a self-hosted realm without a squid) has no parcel
    // routes at all and 404s them by design; parcel rights are chain state,
    // network-wide consistent, so the public chain lambdas answer instead
    // and the note says whose answer the rows are. Only when neither
    // answers is the check "unchecked".
    let target = dest.lambdas_base.trim_end_matches('/');
    let chain = dest.chain_lambdas.trim_end_matches('/');
    let (rows, note) = match probe_parcels(target, addr, &dest.pointers).await {
        Ok(rows) => (rows, None),
        Err((why, partial)) => {
            let fallback = match target != chain {
                true => probe_parcels(chain, addr, &dest.pointers).await.ok(),
                false => None,
            };
            match fallback {
                Some(rows) => (
                    rows,
                    Some(format!(
                        "{why} \u{2014} parcel rights read from {}",
                        host_of(chain)
                    )),
                ),
                None => {
                    return TargetVerdict {
                        verdict: Verdict::Unchecked(format!(
                            "could not check parcel rights: {why}"
                        )),
                        rows: partial,
                        unchecked: 0,
                        note: None,
                    }
                }
            }
        }
    };
    let unchecked = dest.pointers.len().saturating_sub(rows.len());
    TargetVerdict {
        verdict: land_verdict(&rows, unchecked),
        rows,
        unchecked,
        note,
    }
}

/// Batch route first, else one probe per parcel; an error carries the rows
/// gathered before it.
async fn probe_parcels(
    lambdas: &str,
    addr: &str,
    pointers: &[String],
) -> Result<Vec<ParcelRight>, (String, Vec<ParcelRight>)> {
    let batch: Vec<String> = pointers.iter().take(100).cloned().collect();
    let req = status_client()
        .post(format!("{lambdas}/users/{addr}/parcels/permissions"))
        .json(&serde_json::json!({ "parcels": batch }));
    if let Ok(v) = fetch_json::<Value>(req).await {
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
            return Ok(rows);
        }
    }
    // One probe per parcel, a handful in flight at once: serially this was
    // the slowest thing a target flip could trigger (every probe a public
    // round-trip), and the answers are independent. `buffered` keeps the
    // rows in declared-parcel order, and the first error still ends the
    // check exactly where the serial loop did — in-flight probes drop.
    use futures::StreamExt;
    let mut rows = Vec::new();
    let probes: Vec<_> = pointers
        .iter()
        .take(PARCEL_PROBE_CAP)
        .filter_map(|pointer| {
            let (x, y) = catalyrst_auth_chain::pointer::parse_pointer(pointer)?;
            let url = format!("{lambdas}/users/{addr}/parcels/{x}/{y}/permissions");
            Some(async move {
                get_json(&url).await.map(|flags| ParcelRight {
                    pointer: pointer.clone(),
                    leg: flags_leg(&flags),
                })
            })
        })
        .collect();
    let mut probes = futures::stream::iter(probes).buffered(6);
    while let Some(result) = probes.next().await {
        match result {
            Ok(row) => rows.push(row),
            Err(why) => return Err((why, rows)),
        }
    }
    Ok(rows)
}

/// Same TTL and eviction as the live-status cache, keyed by everything that
/// can change the answer.
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

/// The cached answer if still warm, never fetching: the no-wait read the
/// instant page render uses while a background task warms the cache.
pub(super) fn rights_peek(cache: &RightsCache, dest: &Dest, address: &str) -> Option<Arc<Rights>> {
    cache_get(&lock(cache), &rights_key(dest, address), STATUS_TTL)
}

pub(super) async fn cached_rights(cache: &RightsCache, dest: &Dest, address: &str) -> Arc<Rights> {
    if let Some(hit) = rights_peek(cache, dest, address) {
        return hit;
    }
    let entry = Arc::new(fetch_rights(dest, address).await);
    cache_put(
        &mut lock(cache),
        rights_key(dest, address),
        entry.clone(),
        STATUS_TTL,
    );
    entry
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::Router;
    use serde_json::json;

    async fn serve(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        base
    }

    /// The names page answers bare labels; the worlds tier speaks
    /// `name.dcl.eth`, so the parser does too and the merge has one spelling.
    #[test]
    fn names_become_world_names() {
        let v = json!({ "elements": [
            { "name": "Gather" }, { "name": "plaza.dcl.eth" }
        ], "totalAmount": 2 });
        assert_eq!(parse_names(&v), ["gather.dcl.eth", "plaza.dcl.eth"]);
        assert!(parse_names(&json!({})).is_empty());
    }

    /// An owned name with no worlds row still lists, as an empty world; a
    /// listed world the wallet owns is marked owned, not listed twice; ISO
    /// and millisecond timestamps become the same clock.
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

    /// The five legs in the validator's order; `null` flags grant nothing.
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

    /// Owner and world-wide grants pass on the first document, a
    /// parcel-scoped grant only when it covers every declared parcel, and
    /// the refusal carries the exact grant command.
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

    /// One right-less parcel refuses, a capped check says how much it did
    /// not see, and an empty check is unchecked rather than a quiet pass.
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
        assert_eq!(h.owned, [(5, -3), (6, -3)]);
        assert_eq!(
            h.operated_coords,
            [(9, 9)],
            "an owned parcel that is also operated lists once, as owned"
        );
    }

    #[test]
    fn only_a_plausible_address_is_worth_asking_about() {
        assert!(valid_address("0x1234567890abcdef1234567890abcdef12345678"));
        assert!(!valid_address("0x1234"));
        assert!(!valid_address("1234567890abcdef1234567890abcdef1234567890"));
        assert!(!valid_address("0x1234567890abcdef1234567890abcdef1234567g"));
    }

    /// A configured target keeps the sign-in on its own domain only while it
    /// serves the authorize page: a 404 and an unreachable host both fall
    /// back to the catalyst.example.com pair.
    #[tokio::test]
    async fn the_connect_bases_fall_back_when_the_target_page_is_missing() {
        let serve_page = |ok: bool| {
            serve(Router::new().route(
                "/auth/native",
                get(move || async move {
                    match ok {
                        true => axum::http::StatusCode::OK,
                        false => axum::http::StatusCode::NOT_FOUND,
                    }
                }),
            ))
        };
        let fresh = serve_page(true).await;
        let bases = working_auth_bases(Some(&fresh)).await;
        assert_eq!(
            bases.page,
            format!("{fresh}/auth/native"),
            "a target that serves the page keeps the sign-in"
        );

        let stale = serve_page(false).await;
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

    #[tokio::test]
    async fn parcel_rights_fall_back_to_the_chain_lambdas_when_the_target_has_none() {
        let serve_lambdas = |answers: bool| {
            serve(match answers {
                true => Router::new().route(
                    "/lambdas/users/{addr}/parcels/{x}/{y}/permissions",
                    get(|| async { axum::Json(json!({ "owner": false, "operator": true })) }),
                ),
                false => Router::new(),
            })
        };
        let worlds_only = serve_lambdas(false).await;
        let chain = serve_lambdas(true).await;
        let dest = |lambdas: &str, chain: &str| Dest {
            world: None,
            pointers: vec!["2,12".into(), "3,12".into()],
            base_pointer: "2,12".into(),
            read_bases: Vec::new(),
            lambdas_base: format!("{lambdas}/lambdas"),
            chain_lambdas: format!("{chain}/lambdas"),
            worlds_base: worlds_only.clone(),
            headline: String::new(),
            server_line: String::new(),
        };
        let addr = "0x1234567890abcdef1234567890abcdef12345678";

        let t = verdict_fetch(&dest(&worlds_only, &chain), addr).await;
        assert!(
            matches!(&t.verdict, Verdict::May(why) if why.contains("all 2 declared parcels")),
            "the chain answer rules"
        );
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].leg, Some("operator"));
        assert_eq!(t.unchecked, 0);
        let note = t.note.expect("the rows say whose answer they are");
        assert!(
            note.contains("answered HTTP 404") && note.contains("parcel rights read from"),
            "{note}"
        );

        let t = verdict_fetch(&dest(&chain, &chain), addr).await;
        assert!(matches!(t.verdict, Verdict::May(_)));
        assert_eq!(t.rows.len(), 2);
        assert!(t.note.is_none(), "the target's own answer needs no note");

        let t = verdict_fetch(&dest(&worlds_only, &worlds_only), addr).await;
        assert!(
            matches!(&t.verdict, Verdict::Unchecked(why) if why.contains("could not check parcel rights")),
            "no fallback to itself"
        );
        assert!(t.rows.is_empty() && t.note.is_none());
    }

    #[test]
    fn the_auth_bases_follow_the_target() {
        let public = auth_bases(None);
        let public_relay = "https://catalyst.example.com/internal/native-auth-relay";
        assert_eq!(public.page, "https://catalyst.example.com/auth/native");
        assert_eq!(public.relay, public_relay);
        let own = auth_bases(Some("peer.example.net/content"));
        let own_relay = "https://peer.example.net/internal/native-auth-relay";
        assert_eq!(own.page, "https://peer.example.net/auth/native");
        assert_eq!(own.relay, own_relay);
        assert_eq!(auth_bases(Some("  ")).page, public.page, "blank is unset");
    }

    /// A forged entry, a swapped session key, a shifted expiration or
    /// somebody else's signature all become refusals.
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
