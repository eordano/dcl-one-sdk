//! The join card: launch targets, the deep-link knobs, what each target's
//! client keeps of them ([`Carry`]), and the card markup.

use super::super::chrome::esc;
use serde_json::Value;

/// Terrain is inverted in the client (`!HasFlagWithValueFalse` in
/// `DynamicWorldContainer`): drawn unless the param arrives `=false`, so the
/// box ships checked and only the unchecked state emits a token.
pub(super) const TERRAIN: &str = "landscape-terrain-enabled";

/// The client's own allowlist (`DeepLinkAllowlist.cs`): the first four
/// survive only on a whitelisted realm (loopback, or a world the
/// `deeplink-whitelisted-worlds` flag names), `force-open-backpack` on any.
/// Its other ~90 flags are dropped for every realm, so offering them would
/// promise changes it silently discards.
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

/// How much of the deep link a target's client keeps, decided by whether its
/// realm is loopback ([`TOGGLES`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Carry {
    /// Loopback realm: every knob this page offers.
    Loopback,
    /// Routable (LAN) realm: the loopback-only tier is dropped.
    Routable,
    /// No deep-link params at all: the web build reads none, the mobile link
    /// is a realm and a position.
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
    /// `spawnpoint` is not in the loopback-only tier.
    pub(super) fn keeps_spawn(self) -> bool {
        self != Carry::Nothing
    }
}

pub(super) const WHERE_DESKTOP: &str = "desktop";
pub(super) const WHERE_LAN: &str = "lan";
pub(super) const WHERE_WEB: &str = "web";
pub(super) const WHERE_PHONE: &str = "phone";
pub(super) const WHERE_KEYS: [&str; 4] = [WHERE_DESKTOP, WHERE_LAN, WHERE_WEB, WHERE_PHONE];

/// The page's state, carried in the query string: the one form GETs back here
/// and the server rebuilds the launch link with the choices folded in.
#[derive(Default)]
pub(super) struct Knobs {
    /// Checked `opt=` boxes, in [`TOGGLES`] order.
    pub(super) opts: Vec<String>,
    /// A `spawnPoints[].name` from this scene, or empty for the default one.
    pub(super) spawn: String,
    /// A [`WHERE_KEYS`] entry, or empty for the first target on offer.
    pub(super) where_key: String,
    /// `None` until touched: the server's own `--mcp` decides the default.
    pub(super) mcp: Option<bool>,
}

impl Knobs {
    /// The `--key=value` tokens these knobs add, in the form
    /// [`joinblock::parse_passthrough_params`] understands, so a core key or
    /// one a flag already set is dropped rather than allowed to repoint the
    /// link. A knob the target's client would discard is left out.
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

/// Checked on a query-less first visit: what every local iteration loop
/// wants. A submitted form always carries `where`, so it names its own set
/// and unchecking sticks.
pub(in crate::start) const DEFAULT_ON: [&str; 2] = ["multi-instance", "skip-auth-screen"];

/// Every key is an allowlist over what the page drew, so a link can only be
/// built out of what it offered — there is no free-text field to smuggle a
/// flag through.
pub(super) fn knobs(query: Option<&str>, spawn_names: &[String]) -> Knobs {
    let Some(query) = query else {
        // The fresh page also draws the terrain, but only as a page default:
        // the terminal deep link carries just DEFAULT_ON.
        return Knobs {
            opts: DEFAULT_ON
                .iter()
                .copied()
                .chain([TERRAIN])
                .map(str::to_string)
                .collect(),
            ..Knobs::default()
        };
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

impl Target {
    pub(super) fn new(
        key: &'static str,
        label: &'static str,
        hint: &str,
        url: String,
        carry: Carry,
    ) -> Self {
        Target {
            key,
            label,
            hint: hint.to_string(),
            url,
            qr: String::new(),
            carry,
        }
    }
}

/// Which tier of the client's deep-link allowlist a realm qualifies for.
/// `ApplicationParametersParser` asks `Uri.IsLoopback`, so this asks the same
/// of the host — a LAN address is not loopback however local it feels.
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

/// A knob the client would throw away rides along hidden, so switching back
/// to a target that can use it finds it still set.
pub(super) fn kept(name: &str, value: &str) -> String {
    format!(
        r#"<input type="hidden" name="{}" value="{}">"#,
        esc(name),
        esc(value)
    )
}

/// The join card IS the GET form: header, target tabs, knobs, footer with the
/// launch button and deep link. Tab/segment radios are `appearance: none`
/// stretched over their labels — still real, focusable inputs.
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
    let on_off = |on: bool| if on { "On" } else { "Off" };

    let tabs: String = targets
        .iter()
        .enumerate()
        .map(|(i, t)| crate::start::chrome::radio_tab("where", t.key, &esc(t.label), i == selected))
        .collect();

    let mcp_control = match carry.keeps_mcp() {
        true => {
            let opts: String = [("off", false), ("on", true)]
                .iter()
                .map(|(name, on)| {
                    format!(
                        r#"<label class="seg"><input class="seg__r" type="radio" name="mcp" value="{name}"{c}>{label}</label>"#,
                        label = on_off(*on),
                        c = if *on == mcp_on { " checked" } else { "" },
                    )
                })
                .collect();
            format!(r#"<div class="seg-group">{opts}</div>"#)
        }
        false => format!(
            r#"<span class="note">{} — this target's link never carries the MCP flag</span>{}"#,
            on_off(mcp_on),
            kept("mcp", if mcp_on { "on" } else { "off" }),
        ),
    };
    let mcp_note = match (carry.keeps_mcp(), mcp_server) {
        (false, _) => None,
        (true, true) => Some("The client opens its MCP port and this preview reads the running scene's errors out of it"),
        (true, false) => Some("This preview was started with --no-mcp, so nothing here reads the port even when the link opens it"),
    }
    .map_or_else(String::new, |note| {
        format!(r#"<span class="knob__note">{note}</span>"#)
    });

    let checked = |key: &str| knobs.opts.iter().any(|o| o == key);
    let flags: String = TOGGLES
        .iter()
        .map(|(key, what)| {
            let on = checked(key);
            let live = carry.keeps_toggle(key);
            let k = esc(key);
            let (class, control) = match live {
                true => (
                    "flag",
                    format!(
                        r#"<label class="flag__l"><input class="sw" type="checkbox" name="opt" value="{k}"{c}><span class="chk__k">{k}</span></label>"#,
                        c = if on { " checked" } else { "" },
                    ),
                ),
                false => (
                    "flag flag--off",
                    format!(
                        r#"<span class="flag__l"><span class="chk__k">{k}</span>{state}</span>"#,
                        state = if on { r#"<b class="knob__on">on</b>"# } else { "" },
                    ),
                ),
            };
            format!(
                r#"<div class="{class}">{control}<span class="chk__w">{w}</span>{keep}</div>"#,
                w = esc(what),
                keep = if on && !live { kept("opt", key) } else { String::new() },
            )
        })
        .collect();
    let flag_count = match TOGGLES
        .iter()
        .filter(|(k, _)| carry.keeps_toggle(k) && checked(k))
        .count()
    {
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

/// The host the visitor reached this page on, scheme and prefix stripped.
pub(super) fn host_label(realm: &str) -> &str {
    let rest = realm.split_once("://").map_or(realm, |(_, r)| r);
    rest.split('/').next().unwrap_or(rest)
}

/// The spawn-point knob; a scene that names no spawn points gets none.
pub(super) fn spawn_select(knobs: &Knobs, spawns: &[Value], carry: Carry) -> String {
    let options: String = spawns
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
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
    let default = knobs.spawn.is_empty();
    if !carry.keeps_spawn() {
        let (chosen, keep) = match default {
            true => ("The scene's default".to_string(), String::new()),
            false => (esc(&knobs.spawn), kept("spawn", &knobs.spawn)),
        };
        return format!(
            r#"<span class="knob__k">Spawn point</span><span class="note">{chosen} — this link carries only the realm and a position</span>{keep}"#
        );
    }
    format!(
        r#"<label class="knob__k" for="spawn">Spawn point</label><select class="knob__sel" id="spawn" name="spawn"><option value=""{def}>The scene's default</option>{options}</select>"#,
        def = if default { " selected" } else { "" },
    )
}
