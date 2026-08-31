//! The chrome every page on this server shares — one stylesheet, one escape,
//! one document shell — so `/` and `/deploy` cannot drift into looking like
//! different servers.

use axum::http::header;
use axum::response::{IntoResponse, Response};

pub(crate) const STYLE: &str = include_str!("landing.css");

pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// One radio sub-tab, the strip idiom the join and target cards share.
pub(crate) fn radio_tab(name: &str, value: &str, label: &str, checked: bool) -> String {
    format!(
        r#"<label class="jn2__tab"><input class="jn2__tab-r" type="radio" name="{}" value="{}"{}>{label}</label>"#,
        esc(name),
        esc(value),
        if checked { " checked" } else { "" },
    )
}

/// One label/value row, the shape every panel on every page uses.
pub(crate) fn kv(k: &str, v: String) -> String {
    format!(
        r#"<div class="kv"><span class="k">{}</span>{v}</div>"#,
        esc(k)
    )
}

/// A rendered page, with the headers that keep a preview page from being
/// cached while the scene under it changes.
pub(crate) fn html(body: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        body,
    )
        .into_response()
}

/// The four sections every page links between, plus what the bar shows for
/// this render: which link is current, the deploy run badge, the account
/// pill or its connect button, and the server host pill. `None` renders the
/// bare mark — the CLI's standalone signing server has no sections to offer.
pub(crate) struct Nav<'a> {
    pub active: &'a str,
    pub badge: &'a str,
    pub host: &'a str,
    /// The remembered account, drawn as a pill linking to /target. `None`
    /// draws the connect button instead — the account is a server-wide fact,
    /// so its handle lives in the bar, not inside one page's card.
    pub account: Option<String>,
    /// The page token the connect button's POST carries; the POST is gated
    /// exactly like the publish button, so rendering this to every reader
    /// gives a stranger nothing a loopback check does not take back.
    pub token: &'a str,
}

/// `0x` plus enough hex to recognize a wallet without a full line of it.
pub(crate) fn short_account(addr: &str) -> String {
    match addr.len() > 12 {
        true => format!("{}\u{2026}{}", &addr[..6], &addr[addr.len() - 4..]),
        false => addr.to_string(),
    }
}

const SECTIONS: [(&str, &str, &str); 4] = [
    ("preview", "/", "Preview"),
    ("scene", "/scene", "Scene"),
    ("target", "/target", "Target"),
    ("deploy", "/deploy", "Deploy"),
];

fn pgnav(prefix: &str, nav: &Nav) -> String {
    let mut out = String::from(r#"<nav class="pgnav" aria-label="Sections">"#);
    for (key, path, label) in SECTIONS {
        let href = match (prefix.is_empty(), path) {
            (true, "/") => "/".to_string(),
            (false, "/") => prefix.to_string(),
            _ => format!("{prefix}{path}"),
        };
        let current = if *key == *nav.active {
            r#" aria-current="page""#
        } else {
            ""
        };
        let badge = match key {
            "deploy" => format!(
                r#"<span class="pgnav__badge" id="deploy-badge">{}</span>"#,
                esc(nav.badge)
            ),
            _ => String::new(),
        };
        out.push_str(&format!(
            r#"<a class="pgnav__lnk" href="{}"{current}>{label}{badge}</a>"#,
            esc(&href)
        ));
    }
    out.push_str("</nav>");
    match &nav.account {
        // The pill names the account and links to /target; the sliver beside
        // it disconnects. No caption anywhere — the connection worked, and
        // the pages below are what it bought.
        Some(addr) => out.push_str(&format!(
            r#"<span class="bar__acct bar__acct--split"><a class="bar__cta" href="{href}" title="{full}">{short}</a><form method="post" action="{action}"><input type="hidden" name="token" value="{tok}"><input type="hidden" name="address" value=""><button class="bar__cta" type="submit" title="Disconnect">×</button></form></span>"#,
            href = esc(&format!("{prefix}/target")),
            full = esc(addr),
            short = esc(&short_account(addr)),
            action = esc(&format!("{prefix}/target/address")),
            tok = esc(nav.token),
        )),
        // One pill, two doors: the browser wallet directly (script-armed;
        // inert without one) and the Decentraland sign-in, whose form POST
        // works with no script at all.
        None => out.push_str(&format!(
            r#"<span class="bar__acct bar__acct--split"><button class="bar__cta" id="bar-wallet" type="button">Connect Wallet</button><form method="post" action="{action}"><input type="hidden" name="token" value="{tok}"><button class="bar__cta" type="submit">Connect with DCL</button></form></span>"#,
            action = esc(&format!("{prefix}/target/connect")),
            tok = esc(nav.token),
        )),
    }
    if !nav.host.is_empty() {
        out.push_str(&format!(
            r#"<span class="jn2__host"><span class="bar__dot"></span>{}</span>"#,
            esc(nav.host)
        ));
    }
    out
}

/// The chrome every page on this server shares: one stylesheet, one header
/// with the section nav, one skip link — so the four sections read as one
/// server, not four pages.
pub(crate) fn document(
    title: &str,
    prefix: &str,
    extra_css: &str,
    skip_to: &str,
    skip_label: &str,
    nav: Option<&Nav>,
    body: &str,
) -> String {
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{t} — dcl-one-sdk preview</title>
<style>{STYLE}{extra_css}</style>
</head>
<body>
<a class="skip" href="{skip_to}">{skip_label}</a>
<header class="bar">
  <div class="bar__mark"><span class="bar__dot"></span>DCL One SDK</div>
  {nav}
</header>
{body}
</body>
</html>
"##,
        t = esc(title),
        nav = nav.map(|n| pgnav(prefix, n)).unwrap_or_default(),
    )
}
