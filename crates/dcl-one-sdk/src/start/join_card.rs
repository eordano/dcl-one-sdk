//! The join card: launch targets, the deep-link knobs, what each target's
//! client keeps of them ([`Carry`]), and the card markup.

use super::super::chrome::esc;
use serde_json::Value;

/// The deep-link flags offered, from the client's own allowlist
/// (`DeepLinkAllowlist.cs`): the first four survive only on a whitelisted
/// realm (loopback, or a world the `deeplink-whitelisted-worlds` flag names),
/// `force-open-backpack` on any. The client's other ~90 flags are dropped for
/// every realm, so offering them would promise changes it silently discards;
/// [`Carry`] renders what the target would drop as fixed text.
///
/// Terrain is the inverted one: the client draws it unless the param arrives
/// `=false` (`!HasFlagWithValueFalse` in `DynamicWorldContainer`), so the box
/// ships checked and only the unchecked state emits a token — `=true` would
/// be a no-op dressed as a control.
pub(super) const TERRAIN: &str = "landscape-terrain-enabled";

pub(super) const TOGGLES: [(&str, &str); 5] = [
    (
        "multi-instance",
        "A second client beside one already running",
    ),
    ("skip-auth-screen", "Straight in on the cached identity"),
    (TERRAIN, "Draw the terrain around the scene"),
    ("hub", "Mark the session as launched from the Creator Hub"),
    ("force-open-backpack", "Land with the backpack open"),
];

/// How much of the deep link a target's client will actually keep, which is
/// decided by whether its realm is loopback ([`TOGGLES`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Carry {
    /// A loopback realm: the client keeps every knob this page offers.
    Loopback,
    /// A routable realm — the LAN address another device has to dial. The
    /// client drops the loopback-only tier of its allowlist.
    Routable,
    /// A link with no deep-link params at all: the web build reads none of
    /// them, and the mobile link is a realm and a position.
    Nothing,
}

impl Carry {
    pub(super) fn keeps_toggle(self, key: &str) -> bool {
        match self {
            Carry::Loopback => true,
            Carry::Routable => key == "force-open-backpack",
            Carry::Nothing => false,
        }
    }
    pub(super) fn keeps_mcp(self) -> bool {
        self == Carry::Loopback
    }
    /// `spawnpoint` is not in the loopback-only tier, so it survives anywhere
    /// the client reads deep-link params at all.
    pub(super) fn keeps_spawn(self) -> bool {
        self != Carry::Nothing
    }
}

pub(super) const WHERE_DESKTOP: &str = "desktop";
pub(super) const WHERE_LAN: &str = "lan";
pub(super) const WHERE_WEB: &str = "web";
pub(super) const WHERE_PHONE: &str = "phone";
pub(super) const WHERE_KEYS: [&str; 4] = [WHERE_DESKTOP, WHERE_LAN, WHERE_WEB, WHERE_PHONE];

/// The page's own state, carried in the query string because the knobs have no
/// JavaScript to hold it: the one form GETs back here and the server rebuilds
/// the launch link with the choices folded in.
#[derive(Default)]
pub(super) struct Knobs {
    /// Checked `opt=` boxes, in [`TOGGLES`] order.
    pub(super) opts: Vec<String>,
    /// A `spawnPoints[].name` from this scene, or empty for the default one.
    pub(super) spawn: String,
    /// A [`WHERE_KEYS`] entry, or empty for the first target on offer.
    pub(super) where_key: String,
    /// The mcp radio, or `None` before anyone has touched it — the server's
    /// own `--mcp` decides the default.
    pub(super) mcp: Option<bool>,
}

impl Knobs {
    /// The `--key=value` tokens these knobs add, in the form
    /// [`joinblock::parse_passthrough_params`] already understands — so the
    /// same rules apply as to `--` params on the command line: a core key
    /// (realm, position, …) or one a flag already set is dropped rather than
    /// allowed to repoint the link.
    ///
    /// A knob the target's client would throw away is left out of the link
    /// rather than sent and silently dropped.
    pub(super) fn tokens(&self, carry: Carry) -> Vec<String> {
        let mut out: Vec<String> = self
            .opts
            .iter()
            .filter(|o| carry.keeps_toggle(o) && *o != TERRAIN)
            .map(|o| format!("--{o}=true"))
            .collect();
        if carry.keeps_toggle(TERRAIN) && !self.opts.iter().any(|o| o == TERRAIN) {
            out.push(format!("--{TERRAIN}=false"));
        }
        if !self.spawn.is_empty() && carry.keeps_spawn() {
            out.push(format!("--spawnpoint={}", self.spawn));
        }
        out
    }
}

/// The flags a fresh page starts with checked: a second client beside the
/// one already running, and no auth screen in the way — what every local
/// iteration loop wants. They apply only to the query-less first visit; a
/// submitted form names its own set, so unchecking sticks (the form always
/// carries `where`, so a submission is never mistaken for a fresh visit).
pub(in crate::start) const DEFAULT_ON: [&str; 2] = ["multi-instance", "skip-auth-screen"];

/// Every knob on the page comes back through the query string, and nothing
/// else does: a key this page does not own is ignored, and so is a value the
/// page never offered. The page can therefore only ever build a link out of
/// what it drew — there is no free-text field to smuggle a flag through.
pub(super) fn knobs(query: Option<&str>, spawn_names: &[String]) -> Knobs {
    let Some(query) = query else {
        // The page's fresh visit also draws the terrain by default — a
        // preview wants the scene in its surroundings — but that one stays a
        // page default only: the terminal deep link carries just DEFAULT_ON.
        let mut knobs = Knobs {
            opts: DEFAULT_ON.iter().map(|s| s.to_string()).collect(),
            ..Knobs::default()
        };
        knobs.opts.push(TERRAIN.to_string());
        return knobs;
    };
    let mut knobs = Knobs::default();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "opt" if TOGGLES.iter().any(|(k, _)| *k == value) => {
                knobs.opts.push(value.into_owned())
            }
            "spawn" if spawn_names.iter().any(|n| *n == value) => knobs.spawn = value.into_owned(),
            "where" if WHERE_KEYS.contains(&value.as_ref()) => knobs.where_key = value.into_owned(),
            "mcp" if value == "on" || value == "off" => knobs.mcp = Some(value == "on"),
            _ => {}
        }
    }
    knobs
}

/// One choice of the "where" knob: the link it launches and how much of the
/// rest of the panel that link can carry.
pub(super) struct Target {
    /// The value this choice rides under in the query string ([`WHERE_KEYS`]).
    pub(super) key: &'static str,
    pub(super) label: &'static str,
    /// The one-line "what this launches" the card's title row shows.
    pub(super) hint: String,
    /// Built with the knobs this target can carry already folded in.
    pub(super) url: String,
    pub(super) qr: String,
    pub(super) carry: Carry,
}

/// Which tier of the client's deep-link allowlist a realm qualifies for.
/// `ApplicationParametersParser` asks `Uri.IsLoopback`, so this asks the same
/// question of the host — a LAN address is not loopback however local it feels.
pub(super) fn realm_carry(realm: &str) -> Carry {
    let host = realm
        .split_once("://")
        .map_or(realm, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    let host = match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(host, |(h, _)| h),
    };
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    match loopback {
        true => Carry::Loopback,
        false => Carry::Routable,
    }
}

/// A knob the client would throw away is disabled, but the choice is not
/// forgotten: it rides along hidden so switching back to a target that can use
/// it finds it still set.
pub(super) fn kept(name: &str, value: &str) -> String {
    format!(
        r#"<input type="hidden" name="{}" value="{}">"#,
        esc(name),
        esc(value)
    )
}

/// The join surface: one card that IS the GET form — header, target tabs,
/// knobs, footer with the launch button and deep link. Knobs the target's
/// client would discard render disabled with a note rather than live and
/// ignored. Tab/segment radios are `appearance: none` stretched over their
/// labels: still real, focusable inputs.
pub(super) fn join_control(
    targets: &[Target],
    selected: usize,
    mcp_server: bool,
    mcp_on: bool,
    knobs: &Knobs,
    spawns: &[Value],
    prefix: &str,
) -> String {
    let target = &targets[selected];
    let carry = target.carry;

    let tabs: String = targets
        .iter()
        .enumerate()
        .map(|(i, t)| crate::start::chrome::radio_tab("where", t.key, &esc(t.label), i == selected))
        .collect();

    // No disabled controls anywhere on the card: a knob the target's client
    // would throw away reads as its value in plain text, and a hidden input
    // keeps that value riding an Apply round-trip.
    let mcp_control = match carry.keeps_mcp() {
        true => {
            let opts: String = [("off", false), ("on", true)]
                .iter()
                .map(|(name, on)| {
                    format!(
                        r#"<label class="seg"><input class="seg__r" type="radio" name="mcp" value="{name}"{c}>{label}</label>"#,
                        label = if *on { "On" } else { "Off" },
                        c = if *on == mcp_on { " checked" } else { "" },
                    )
                })
                .collect();
            format!(r#"<div class="seg-group">{opts}</div>"#)
        }
        false => format!(
            r#"<span class="note">{} — this target's link never carries the MCP flag</span>{}"#,
            if mcp_on { "On" } else { "Off" },
            kept("mcp", if mcp_on { "on" } else { "off" }),
        ),
    };
    let mcp_note = match (carry.keeps_mcp(), mcp_server) {
        (false, _) => "",
        (true, true) => "The client opens its MCP port and this preview reads the running scene's errors out of it",
        (true, false) => "This preview was started with --no-mcp, so nothing here reads the port even when the link opens it",
    };
    let mcp_note = match mcp_note {
        "" => String::new(),
        note => format!(r#"<span class="knob__note">{}</span>"#, esc(note)),
    };

    let flags: String = TOGGLES
        .iter()
        .map(|(key, what)| {
            let on = knobs.opts.iter().any(|o| o == key);
            let live = carry.keeps_toggle(key);
            let mut row = match live {
                true => format!(
                    r#"<div class="flag"><label class="flag__l"><input class="sw" type="checkbox" name="opt" value="{k}"{c}><span class="chk__k">{k}</span></label><span class="chk__w">{w}</span>"#,
                    k = esc(key),
                    w = esc(what),
                    c = if on { " checked" } else { "" },
                ),
                false => format!(
                    r#"<div class="flag flag--off"><span class="flag__l"><span class="chk__k">{k}</span>{state}</span><span class="chk__w">{w}</span>"#,
                    k = esc(key),
                    w = esc(what),
                    state = if on { r#"<b class="knob__on">on</b>"# } else { "" },
                ),
            };
            if on && !live {
                row.push_str(&kept("opt", key));
            }
            row.push_str("</div>");
            row
        })
        .collect();
    let flags_on = TOGGLES
        .iter()
        .filter(|(k, _)| carry.keeps_toggle(k) && knobs.opts.iter().any(|o| o == *k))
        .count();
    let flag_count = match flags_on {
        0 => String::new(),
        n => format!(r#"<b class="knob__on">{n} on</b>"#),
    };

    format!(
        r#"<div class="jn"><form class="side" method="get" action="{action}"><div class="jn2__head"><div class="jn2__title"><h2>Join this preview</h2><span class="jn__hint">{hint}</span></div></div><fieldset class="knob knob--tabs"><legend class="knob__k u-sr-only">where</legend><div class="jn2__tabs">{tabs}</div></fieldset><div class="jn2__body"><div class="jn2__col">{spawn_knob}<fieldset class="knob"><legend class="knob__k">Scene errors in the terminal</legend>{mcp_control}{mcp_note}</fieldset><button class="knob__go" type="submit">Apply</button></div><div class="jn2__col"><fieldset class="knob"><legend class="knob__k">Deep-link flags{flag_count}</legend><div class="flags">{flags}</div></fieldset></div></div><div class="jn2__foot"><a class="jn__cta" id="launch" href="{u}">Launch</a><span class="jn2__link"><span class="jn__url" id="deep-link">{u}</span><button class="deep__copy" id="copy-link" type="button" hidden>Copy</button></span>{qr}</div></form></div>"#,
        action = esc(&format!("{prefix}/")),
        hint = esc(&target.hint),
        u = esc(&target.url),
        qr = target.qr,
        spawn_knob = match spawn_select(knobs, spawns, carry) {
            knob if knob.is_empty() => String::new(),
            knob => format!(r#"<div class="knob">{knob}</div>"#),
        },
    )
}

/// What the header pill names: the host the visitor reached this page on,
/// scheme and prefix stripped.
pub(super) fn host_label(realm: &str) -> &str {
    let rest = realm.split_once("://").map_or(realm, |(_, r)| r);
    rest.split('/').next().unwrap_or(rest)
}

/// `spawnpoint` picks which of the scene's own named spawn points the player
/// lands on; a scene that names none has nothing to choose, so the knob is
/// left out rather than shown empty.
pub(super) fn spawn_select(knobs: &Knobs, spawns: &[Value], carry: Carry) -> String {
    let options: String = spawns
        .iter()
        .filter_map(|s| s.get("name").and_then(|n| n.as_str()))
        .map(|name| {
            format!(
                r#"<option value="{n}"{sel}>{n}</option>"#,
                n = esc(name),
                sel = if knobs.spawn == name { " selected" } else { "" },
            )
        })
        .collect();
    if options.is_empty() {
        return String::new();
    }
    if !carry.keeps_spawn() {
        return format!(
            r#"<span class="knob__k">Spawn point</span><span class="note">{chosen} — this link carries only the realm and a position</span>{keep}"#,
            chosen = match knobs.spawn.is_empty() {
                true => "The scene's default".to_string(),
                false => esc(&knobs.spawn),
            },
            keep = match knobs.spawn.is_empty() {
                true => String::new(),
                false => kept("spawn", &knobs.spawn),
            },
        );
    }
    format!(
        r#"<label class="knob__k" for="spawn">Spawn point</label><select class="knob__sel" id="spawn" name="spawn"><option value=""{def}>The scene's default</option>{options}</select>"#,
        def = if knobs.spawn.is_empty() {
            " selected"
        } else {
            ""
        },
    )
}
