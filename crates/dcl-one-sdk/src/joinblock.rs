use crate::netinfo::{nat_vm_guest, share_ip, Iface, IfaceClass};
use serde_json::Value;
use std::net::Ipv4Addr;

pub const DEFAULT_WEB_EXPLORER: &str = "https://decentraland.org/bevy-web";

pub fn web_explorer_base() -> String {
    std::env::var("DCL_ONE_SDK_WEB_EXPLORER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_WEB_EXPLORER.to_string())
        .trim_end_matches('/')
        .to_string()
}

pub fn world_name(scene_json: &Value) -> Option<String> {
    scene_json
        .get("worldConfiguration")
        .and_then(|w| w.get("name"))
        .and_then(|n| n.as_str())
        .map(str::to_string)
}

pub fn base_coords(scene_json: &Value) -> (i64, i64) {
    let scene = scene_json.get("scene");
    let base = scene
        .and_then(|s| s.get("base"))
        .and_then(|b| b.as_str())
        .or_else(|| {
            scene
                .and_then(|s| s.get("parcels"))
                .and_then(|p| p.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
        });
    catalyrst_types::pointer::parse_pointer(base.unwrap_or_default()).unwrap_or((0, 0))
}

pub fn scene_title(scene_json: &Value) -> String {
    scene_json
        .get("display")
        .and_then(|d| d.get("title"))
        .and_then(|t| t.as_str())
        .filter(|t| !t.trim().is_empty())
        .unwrap_or("untitled")
        .to_string()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum QrMode {
    Print,
    Hint,
}

#[derive(Clone)]
pub struct JoinBlock {
    pub title: String,
    pub position: (i64, i64),
    pub port: u16,
    pub ifaces: Vec<Iface>,
    pub web_explorer: String,
    pub qr: QrMode,
    pub unreachable: Vec<Ipv4Addr>,
    pub tunnel_hint: bool,
    pub editor: bool,
    pub optimized_assets_url: Option<String>,
    /// Pre-encoded `&key=value...`, appended verbatim to every desktop link.
    pub deep_link_extra: String,
    pub native_hud: bool,
    /// Native bevy client binary found on this machine (see
    /// [`detect_native_bin`]); None prints the generic `bevy-explorer` name.
    pub native_bin: Option<String>,
}

/// The native bevy client installed on this machine, if any: an explicit
/// `DCL_ONE_NATIVE_BIN` wins, else `decentra-bevy` (the upstream bevy-explorer
/// binary name) is looked up on PATH. Upstream rejects unknown flags, so the
/// printed command must also drop the fork-only `--hud` for it — native_cmd
/// keys that on the binary name.
pub fn detect_native_bin() -> Option<String> {
    if let Ok(explicit) = std::env::var("DCL_ONE_NATIVE_BIN") {
        if !explicit.is_empty() {
            return Some(explicit);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("decentra-bevy"))
        .find(|candidate| candidate.is_file())
        .map(|_| "decentra-bevy".to_string())
}

fn form_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Port of upstream `parsePassthroughParams`: CLI tokens after a standalone
/// `--` become deep-link params; a repeated key overwrites, as URLSearchParams.
pub fn parse_passthrough_params(tokens: &[String]) -> Vec<(String, String)> {
    let mut params: Vec<(String, String)> = Vec::new();
    let mut set = |key: String, value: String| match params.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => params.push((key, value)),
    };
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        i += 1;
        if !token.starts_with('-') {
            continue;
        }
        let stripped = token.trim_start_matches('-');
        if stripped.is_empty() {
            continue;
        }
        match stripped.find('=') {
            Some(0) => {}
            Some(eq) => set(stripped[..eq].to_string(), stripped[eq + 1..].to_string()),
            None => {
                if i < tokens.len() && !tokens[i].starts_with('-') {
                    set(stripped.to_string(), tokens[i].clone());
                    i += 1;
                } else {
                    set(stripped.to_string(), "true".to_string());
                }
            }
        }
    }
    params
}

/// The port the Explorer's own MCP server picks when the deep link names none
/// (`McpServerPlugin.DEFAULT_PORT`). Sending it explicitly costs nothing and
/// means the preview polls the port the client actually opened, rather than
/// both sides guessing the same constant independently.
pub const DEFAULT_EXPLORER_MCP_PORT: u16 = 8123;

/// Declared flags and core keys beat passthrough, as upstream's `params.has`
/// merge; per-row keys (`multi-instance`) dedupe in `desktop_link_with`.
///
/// Both guards compare case-insensitively: the client reads the deep link with
/// `HttpUtility.ParseQueryString`, whose keys are case-insensitive, so a
/// passthrough `--REALM=…` would otherwise collide with our own `realm=` and be
/// handed to the client as the comma-joined value of one key.
pub fn deep_link_extra(
    local_ab: bool,
    mcp: bool,
    mcp_port: Option<u16>,
    passthrough: &[String],
) -> String {
    const CORE_KEYS: &[&str] = &[
        "realm",
        "position",
        "local-scene",
        "dclenv",
        "optimized-assets-url",
    ];
    let mut params: Vec<(String, String)> = Vec::new();
    if local_ab {
        params.push(("local-ab".to_string(), "true".to_string()));
    }
    if mcp {
        params.push(("mcp".to_string(), "true".to_string()));
    }
    if let Some(port) = mcp_port {
        params.push(("mcp-port".to_string(), port.to_string()));
    }
    for (key, value) in parse_passthrough_params(passthrough) {
        if CORE_KEYS.iter().any(|c| c.eq_ignore_ascii_case(&key))
            || params.iter().any(|(k, _)| k.eq_ignore_ascii_case(&key))
        {
            continue;
        }
        params.push((key, value));
    }
    params
        .into_iter()
        .map(|(k, v)| format!("&{}={}", form_encode(&k), form_encode(&v)))
        .collect()
}

/// The engine's default startup portable: its baked public world host is
/// CORS-blocked from a foreign web-explorer origin, so `portables=` repoints
/// it at the realm's own same-origin `/world/…` mirror.
pub const CONTROLLER_WORLD: &str = "basiccontroller.dcl.eth";

/// Percent-encodes everything a query value must not carry raw (`&`, `=`, `?`,
/// `#`, `%`, space, …) while leaving `:` `/` `,` `-` `.` `_` `~` alone, so the
/// printed URL stays copy-pasteable. `form_encode` would escape the `://` of
/// every realm and make these rows unreadable.
fn query_value_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b':'
            | b'/'
            | b',' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn web_join_url(web_explorer: &str, realm: &str, position: (i64, i64)) -> String {
    let encoded_realm = query_value_encode(realm);
    let base = format!(
        "{web_explorer}/?preview=true&realm={encoded_realm}&position={},{}",
        position.0, position.1
    );
    match crate::start::world_base_configured() {
        true => format!(
            "{base}&portables={}",
            form_encode(&format!("{realm}/world/{CONTROLLER_WORLD}"))
        ),
        false => base,
    }
}

pub fn desktop_deep_link(
    realm: &str,
    position: (i64, i64),
    optimized_assets_url: Option<&str>,
    extra: &str,
) -> String {
    let ab = match optimized_assets_url {
        Some(url) => format!("&optimized-assets-url={}", form_encode(url)),
        None => String::new(),
    };
    format!(
        "{}&local-scene=true&dclenv=org{ab}{extra}",
        catalyrst_types::realm_deep_link(realm, position),
    )
}

pub fn mobile_deep_link(realm: &str, position: (i64, i64)) -> String {
    format!(
        "decentraland://open?preview={realm}&position={},{}",
        position.0, position.1
    )
}

pub fn swap_url_host(url: &str, host: impl std::fmt::Display) -> String {
    match url.rsplit_once(':') {
        Some((_, port)) if port.chars().all(|c| c.is_ascii_digit()) => {
            format!("http://{host}:{port}")
        }
        _ => url.to_string(),
    }
}

impl JoinBlock {
    pub fn heading(&self) -> String {
        format!(
            "Preview server ready \u{2014} scene \"{}\" at {},{}",
            self.title, self.position.0, self.position.1
        )
    }

    pub fn body(&self) -> String {
        let mut out = String::new();
        self.push_interface_rows(&mut out);
        self.push_warnings(&mut out);
        self.push_local_section(&mut out);
        self.push_lan_section(&mut out);
        self.push_tunnel_hint(&mut out);
        out
    }

    pub fn compact_body(&self) -> String {
        let mut out = String::new();
        self.push_interface_rows(&mut out);
        self.push_warnings(&mut out);
        let realm = format!("http://127.0.0.1:{}", self.port);
        out.push('\n');
        if self.editor {
            out.push_str(&format!("  editor:   {realm}/inspector/\n"));
        }
        out.push_str(&format!("  desktop:  {}\n", self.desktop_link(&realm)));
        if self.native_bin.is_some() {
            out.push_str(&format!("  native:   {}\n", self.native_cmd(&realm)));
        }
        if self.qr == QrMode::Print {
            if let Some(ip) = share_ip(&self.ifaces) {
                let lan_realm = self.realm(ip);
                out.push_str("  mobile:   scan to open in the Decentraland mobile app:\n");
                self.push_mobile_qr(&mut out, &lan_realm);
            }
        }
        out
    }

    pub fn render(&self) -> String {
        format!("{}\n{}", self.heading(), self.body())
    }

    fn realm(&self, ip: Ipv4Addr) -> String {
        format!("http://{ip}:{}", self.port)
    }

    fn web_url(&self, realm: &str) -> String {
        web_join_url(&self.web_explorer, realm, self.position)
    }

    fn desktop_link(&self, realm: &str) -> String {
        self.desktop_link_with(realm, "")
    }

    fn desktop_link_with(&self, realm: &str, extra: &str) -> String {
        let row_keys: Vec<&str> = extra
            .split('&')
            .filter(|kv| !kv.is_empty())
            .filter_map(|kv| kv.split('=').next())
            .collect();
        let shared: String = self
            .deep_link_extra
            .split('&')
            .filter(|kv| !kv.is_empty())
            .filter(|kv| !row_keys.contains(&kv.split('=').next().unwrap_or("")))
            .map(|kv| format!("&{kv}"))
            .collect();
        desktop_deep_link(
            realm,
            self.position,
            self.optimized_assets_url.as_deref(),
            &format!("{extra}{shared}"),
        )
    }

    fn native_cmd(&self, realm: &str) -> String {
        let bin = self.native_bin.as_deref().unwrap_or("bevy-explorer");
        // --hud is the fork's webkit-overlay flag; upstream decentra-bevy
        // errors out on flags it does not know.
        let hud = if self.native_hud && bin == "bevy-explorer" {
            " --hud"
        } else {
            ""
        };
        format!(
            "{bin} --server {realm} --location {},{} --preview{hud}",
            self.position.0, self.position.1
        )
    }

    fn mobile_link(&self, realm: &str) -> String {
        mobile_deep_link(realm, self.position)
    }

    fn rows(&self) -> Vec<(String, String, &'static str)> {
        let mut rows = Vec::new();
        for i in &self.ifaces {
            let (label, note) = match i.class {
                IfaceClass::Loopback => ("Local:", ""),
                IfaceClass::Lan => ("Network:", ""),
                IfaceClass::Overlay => ("Network:", "overlay/VPN network"),
                IfaceClass::Bridge => (
                    "Network:",
                    "virtual bridge \u{2014} usually unreachable from your LAN",
                ),
                IfaceClass::LinkLocal => continue,
            };
            rows.push((label.to_string(), self.realm(i.ip), note));
        }
        rows
    }

    fn push_interface_rows(&self, out: &mut String) {
        let rows = self.rows();
        let width = rows.iter().map(|(_, url, _)| url.len()).max().unwrap_or(0);
        for (label, url, note) in &rows {
            if note.is_empty() {
                out.push_str(&format!("  {label:<9} {url}\n"));
            } else {
                out.push_str(&format!("  {label:<9} {url:<width$}  ({note})\n"));
            }
        }
    }

    fn push_warnings(&self, out: &mut String) {
        let port = self.port;
        if nat_vm_guest(&self.ifaces) {
            out.push_str(&format!(
                "\n  ! 10.0.2.15 is a NAT-VM guest address \u{2014} nothing outside this VM can\n    reach it. Fixes: switch the VM to bridged networking (it gets its own\n    LAN address), or keep NAT and forward a host port to this VM's port\n    {port}, then share http://<host-lan-ip>:<forwarded-port>\n    Self-test from the joining device:  curl http://<ip>:{port}/about\n"
            ));
        } else if share_ip(&self.ifaces).is_none() {
            out.push_str(
                "\n  ! no LAN address found \u{2014} other devices cannot reach this preview.\n    If this is a VM, switch its network to bridged mode (or add a host\n    port-forward); otherwise check Wi-Fi/Ethernet.\n",
            );
        }
        for ip in &self.unreachable {
            out.push_str(&format!(
                "\n  ! could not reach {ip}:{port} from this host itself \u{2014} a local firewall\n    may be filtering inbound connections; other devices will likely fail\n    too. Self-test from another device:  curl http://{ip}:{port}/about\n"
            ));
        }
    }

    fn push_local_section(&self, out: &mut String) {
        let realm = format!("http://127.0.0.1:{}", self.port);
        out.push_str("\nJoin from THIS machine\n");
        if self.editor {
            out.push_str(&format!("  editor:   {realm}/inspector/\n"));
        }
        out.push_str(&format!("  web:      {}\n", self.web_url(&realm)));
        self.push_local_network_access_note(out);
        out.push_str(&format!("  desktop:  {}\n", self.desktop_link(&realm)));
        out.push_str(&format!(
            "  desktop (2nd instance): {}\n",
            self.desktop_link_with(&realm, "&multi-instance=true")
        ));
        out.push_str(&format!("  native:   {}\n", self.native_cmd(&realm)));
        out.push_str(
            "  note: an Explorer already running SWALLOWS the plain desktop link \u{2014} it comes\n        to the front still on its old realm. Quit it first, or use the 2nd-instance\n        link above.\n",
        );
        out.push_str(
            "  note: a second player needs a second identity \u{2014} use another browser\n        profile (new guest) or another account; same address = kicked.\n",
        );
    }

    fn push_lan_section(&self, out: &mut String) {
        if nat_vm_guest(&self.ifaces) {
            return;
        }
        let Some(ip) = share_ip(&self.ifaces) else {
            return;
        };
        let realm = self.realm(ip);
        let port = self.port;
        out.push_str("\nJoin from another device on this network\n");
        if self.editor {
            out.push_str(&format!("  editor:   {realm}/inspector/\n"));
        }
        let lan_assets = self
            .optimized_assets_url
            .as_deref()
            .map(|u| swap_url_host(u, ip));
        out.push_str(&format!(
            "  desktop:  {}\n",
            desktop_deep_link(
                &realm,
                self.position,
                lan_assets.as_deref(),
                &self.deep_link_extra
            )
        ));
        out.push_str(&format!("  native:   {}\n", self.native_cmd(&realm)));
        out.push_str(&format!("  web:      {}\n", self.web_url(&realm)));
        if web_origin(&self.web_explorer).is_some() {
            out.push_str(
                "  ! the Local Network Access \"Allow\" described above must be granted on\n    the JOINING device's Chrome/Edge \u{2014} that browser is the one blocked.\n",
            );
        }
        out.push_str(&format!(
            "  ! browsers other than Chrome/Edge block http:// realms from the https\n    explorer (mixed content). Workarounds: the mobile-app QR, a native\n    client, or on the joining PC run\n    ssh -L {port}:127.0.0.1:{port} <user>@<this-machine>\n    and join with realm=http://127.0.0.1:{port}\n"
        ));
        match self.qr {
            QrMode::Print => {
                out.push_str("  mobile:   scan to open in the Decentraland mobile app:\n");
                self.push_mobile_qr(out, &realm);
            }
            QrMode::Hint => {
                out.push_str(&format!(
                    "  mobile:   re-run with --mobile for a scan-to-join QR code\n            (also served at http://{ip}:{port}/mobile-preview)\n"
                ));
            }
        }
    }

    fn push_mobile_qr(&self, out: &mut String, realm: &str) {
        let link = self.mobile_link(realm);
        match qr_unicode(&link) {
            Some(qr) => {
                out.push('\n');
                for line in qr.lines() {
                    out.push_str(&format!("    {line}\n"));
                }
                out.push_str(&format!("    this QR opens {link} on your phone\n"));
            }
            None => out.push_str(&format!("            {link}\n")),
        }
    }

    /// Chrome/Edge gate a site's requests to local addresses behind the Local
    /// Network Access permission, granted per web-explorer origin.
    fn push_local_network_access_note(&self, out: &mut String) {
        let Some(origin) = web_origin(&self.web_explorer) else {
            return;
        };
        out.push_str(&format!(
            "  ! Chrome/Edge require permission for websites to reach local addresses\n    (Local Network Access) \u{2014} when the browser asks to access apps on your\n    device, click \"Allow\". If the scene never loads and no prompt appears,\n    enable it manually and reload:\n    chrome://settings/content/siteDetails?site={}\n    \u{2192} \"Apps on device\" (Chrome 145+) or \"Local network access\" (Chrome 142-144) \u{2192} Allow\n",
            form_encode(&origin)
        ));
    }

    fn push_tunnel_hint(&self, out: &mut String) {
        if !self.tunnel_hint {
            return;
        }
        out.push_str("\nJoin from the internet\n");
        out.push_str(
            "  tunnel:   dcl-one-sdk start --tunnel wss://<tunnel-host>   (public https realm)\n",
        );
        out.push_str(
            "  no tunnel service? dcl-one-sdk start --tunnel help   prints a zero-infra ssh -R recipe\n",
        );
    }

    pub fn internet_section(&self, public_url: &str) -> String {
        let realm = public_url.trim_end_matches('/');
        let mut out = String::new();
        out.push_str("\nJoin from the INTERNET \u{2014} tunnel connected\n");
        out.push_str(&format!("  realm:    {realm}\n"));
        out.push_str(&format!("  web:      {}\n", self.web_url(realm)));
        out.push_str(&format!("  desktop:  {}\n", self.desktop_link(realm)));
        out.push_str(&format!("  native:   {}\n", self.native_cmd(realm)));
        match self.qr {
            QrMode::Print => {
                out.push_str("  mobile:   scan to open in the Decentraland mobile app:\n");
                self.push_mobile_qr(&mut out, realm);
            }
            QrMode::Hint => {
                out.push_str(&format!("  mobile:   {}\n", self.mobile_link(realm)));
            }
        }
        out
    }
}

fn web_origin(web_explorer: &str) -> Option<String> {
    let (scheme, rest) = web_explorer.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    (!authority.is_empty()).then(|| format!("{scheme}://{authority}"))
}

pub fn qr_unicode(data: &str) -> Option<String> {
    let code = qrcode::QrCode::new(data.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::unicode::Dense1x2>()
            .dark_color(qrcode::render::unicode::Dense1x2::Light)
            .light_color(qrcode::render::unicode::Dense1x2::Dark)
            .build(),
    )
}

pub fn qr_svg_data_url(data: &str) -> Option<String> {
    use base64::Engine;
    let code = qrcode::QrCode::new(data.as_bytes()).ok()?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(200, 200)
        .build();
    Some(format!(
        "data:image/svg+xml;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(svg.as_bytes())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn iface(name: &str, ip: &str) -> Iface {
        Iface::new(name, ip.parse().unwrap())
    }

    fn block(ifaces: Vec<Iface>, qr: QrMode) -> JoinBlock {
        JoinBlock {
            title: "cube spawner".to_string(),
            position: (52, -68),
            port: 5600,
            ifaces,
            web_explorer: "https://decentraland.org/bevy-web".to_string(),
            qr,
            unreachable: Vec::new(),
            tunnel_hint: false,
            editor: false,
            native_bin: None,
            optimized_assets_url: None,
            deep_link_extra: String::new(),
            native_hud: true,
        }
    }

    fn full_ifaces() -> Vec<Iface> {
        vec![
            iface("lo", "127.0.0.1"),
            iface("wlan0", "10.1.2.20"),
            iface("wg0", "100.101.102.103"),
            iface("docker0", "172.17.0.1"),
            iface("eth1", "169.254.7.42"),
        ]
    }

    #[test]
    fn editor_rows_print_only_with_data_layer() {
        let plain = block(full_ifaces(), QrMode::Hint).render();
        assert!(!plain.contains("editor:"));
        let mut b = block(full_ifaces(), QrMode::Hint);
        b.editor = true;
        let out = b.render();
        assert!(out.contains("  editor:   http://127.0.0.1:5600/inspector/\n"));
        assert!(out.contains("  editor:   http://10.1.2.20:5600/inspector/\n"));
    }

    #[test]
    fn heading_names_scene_and_position() {
        assert_eq!(
            block(full_ifaces(), QrMode::Hint).heading(),
            "Preview server ready \u{2014} scene \"cube spawner\" at 52,-68"
        );
    }

    #[test]
    fn rows_classify_and_skip_link_local() {
        let out = block(full_ifaces(), QrMode::Hint).render();
        assert!(out.contains("  Local:    http://127.0.0.1:5600\n"));
        assert!(out.contains("  Network:  http://10.1.2.20:5600"));
        assert!(out.contains("(overlay/VPN network)"));
        assert!(out.contains("(virtual bridge \u{2014} usually unreachable from your LAN)"));
        assert!(!out.contains("169.254.7.42"));
    }

    #[test]
    fn compact_body_is_addresses_and_one_deeplink() {
        let out = block(full_ifaces(), QrMode::Hint).compact_body();
        assert!(out.contains("  Local:    http://127.0.0.1:5600\n"));
        assert!(out.contains("  Network:  http://10.1.2.20:5600"));
        assert!(out.contains(
            "desktop:  decentraland://realm=http%3A%2F%2F127.0.0.1%3A5600&position=52%2C-68&local-scene=true&dclenv=org"
        ));
        assert!(!out.contains("more:"));
        assert!(!out.contains("every join option"));
        assert!(!out.contains("--verbose"));
        assert!(!out.contains("2nd instance"));
        assert!(!out.contains("Join from another device"));
        assert!(!out.contains("mixed content"));
        assert!(!out.contains("Join from the internet"));
    }

    #[test]
    fn compact_body_keeps_warnings_and_explicit_qr() {
        let out = block(vec![iface("lo", "127.0.0.1")], QrMode::Hint).compact_body();
        assert!(out.contains("! no LAN address found"));
        let out = block(full_ifaces(), QrMode::Print).compact_body();
        assert!(out.contains("scan to open in the Decentraland mobile app"));
        assert!(out.contains('\u{2588}') || out.contains('\u{2580}') || out.contains('\u{2584}'));
    }

    #[test]
    fn swap_url_host_keeps_the_port_and_tolerates_bad_urls() {
        assert_eq!(
            swap_url_host("http://127.0.0.1:5147", "10.1.2.20"),
            "http://10.1.2.20:5147"
        );
        assert_eq!(swap_url_host("not-a-url", "10.1.2.20"), "not-a-url");
    }

    #[test]
    fn lan_desktop_link_rehosts_the_optimized_assets_url() {
        let mut b = block(full_ifaces(), QrMode::Hint);
        b.optimized_assets_url = Some("http://127.0.0.1:5147".to_string());
        let out = b.render();
        assert!(out.contains(
            "desktop:  decentraland://realm=http%3A%2F%2F10.1.2.20%3A5600&position=52%2C-68&local-scene=true&dclenv=org&optimized-assets-url=http%3A%2F%2F10.1.2.20%3A5147"
        ));
    }

    #[test]
    fn desktop_link_carries_the_optimized_assets_url_when_set() {
        let mut b = block(vec![iface("lo", "127.0.0.1")], QrMode::Hint);
        b.optimized_assets_url = Some("http://127.0.0.1:5147".to_string());
        let out = b.render();
        assert!(out.contains(
            "decentraland://realm=http%3A%2F%2F127.0.0.1%3A5600&position=52%2C-68&local-scene=true&dclenv=org&optimized-assets-url=http%3A%2F%2F127.0.0.1%3A5147"
        ));
    }

    /// Pins the mirror's encoding for the configured case: flipping the real
    /// worlds-host switch would race every other test in this binary.
    #[test]
    fn portables_pin_grammar() {
        let realm = "http://10.1.2.20:5600";
        let pinned = format!(
            "&portables={}",
            form_encode(&format!("{realm}/world/{CONTROLLER_WORLD}"))
        );
        assert_eq!(
            pinned,
            "&portables=http%3A%2F%2F10.1.2.20%3A5600%2Fworld%2Fbasiccontroller.dcl.eth"
        );
    }

    #[test]
    fn deep_link_grammars_are_pinned() {
        let out = block(full_ifaces(), QrMode::Hint).render();
        assert!(out.contains(
            "desktop:  decentraland://realm=http%3A%2F%2F127.0.0.1%3A5600&position=52%2C-68&local-scene=true&dclenv=org"
        ));
        assert!(out.contains(
            "desktop:  decentraland://realm=http%3A%2F%2F10.1.2.20%3A5600&position=52%2C-68&local-scene=true&dclenv=org"
        ));
        assert!(out.contains(
            "desktop (2nd instance): decentraland://realm=http%3A%2F%2F127.0.0.1%3A5600&position=52%2C-68&local-scene=true&dclenv=org&multi-instance=true"
        ));
        assert!(out.contains(
            "native:   bevy-explorer --server http://10.1.2.20:5600 --location 52,-68 --preview --hud"
        ));
        assert!(out.contains(
            "web:      https://decentraland.org/bevy-web/?preview=true&realm=http://10.1.2.20:5600&position=52,-68"
        ));
        assert!(
            !out.contains("&portables="),
            "unconfigured worlds host must not advertise the mirror: {out}"
        );
        assert!(out.contains("same address = kicked"));
    }

    #[test]
    fn lan_web_row_carries_the_mixed_content_warning() {
        let out = block(full_ifaces(), QrMode::Hint).render();
        assert!(out.contains("browsers other than Chrome/Edge block http:// realms"));
        assert!(out.contains("ssh -L 5600:127.0.0.1:5600 <user>@<this-machine>"));
        assert!(out.contains("realm=http://127.0.0.1:5600"));
    }

    #[test]
    fn hint_mode_points_at_mobile_flag_and_endpoint() {
        let out = block(full_ifaces(), QrMode::Hint).render();
        assert!(out.contains("re-run with --mobile for a scan-to-join QR code"));
        assert!(out.contains("http://10.1.2.20:5600/mobile-preview"));
        assert!(!out.contains("decentraland://open?preview="));
    }

    #[test]
    fn print_mode_renders_a_unicode_qr_of_the_lan_deep_link() {
        let out = block(full_ifaces(), QrMode::Print).render();
        assert!(out.contains("scan to open in the Decentraland mobile app"));
        assert!(out.contains(
            "this QR opens decentraland://open?preview=http://10.1.2.20:5600&position=52,-68 on your phone"
        ));
        assert!(out.contains('\u{2588}') || out.contains('\u{2580}') || out.contains('\u{2584}'));
    }

    #[test]
    fn loopback_only_warns_and_drops_lan_section() {
        let out = block(vec![iface("lo", "127.0.0.1")], QrMode::Print).render();
        assert!(out.contains("! no LAN address found"));
        assert!(out.contains("switch its network to bridged mode"));
        assert!(out.contains("Join from THIS machine"));
        assert!(!out.contains("Join from another device"));
        assert!(!out.contains("decentraland://open?preview="));
    }

    #[test]
    fn nat_vm_guest_gets_vm_guidance_instead_of_lan_rows() {
        let out = block(
            vec![iface("lo", "127.0.0.1"), iface("enp0s3", "10.0.2.15")],
            QrMode::Print,
        )
        .render();
        assert!(out.contains("! 10.0.2.15 is a NAT-VM guest address"));
        assert!(out.contains("bridged networking"));
        assert!(out.contains("forward a host port"));
        assert!(out.contains("curl http://<ip>:5600/about"));
        assert!(!out.contains("Join from another device"));
    }

    #[test]
    fn unreachable_probe_result_prints_firewall_warning() {
        let mut b = block(full_ifaces(), QrMode::Hint);
        b.unreachable = vec!["10.1.2.20".parse().unwrap()];
        let out = b.render();
        assert!(out.contains("! could not reach 10.1.2.20:5600 from this host itself"));
        assert!(out.contains("local firewall"));
        assert!(out.contains("curl http://10.1.2.20:5600/about"));
    }

    #[test]
    fn base_coords_from_base_then_parcels_then_zero() {
        assert_eq!(
            base_coords(&json!({"scene": {"base": "52,-68", "parcels": ["52,-68"]}})),
            (52, -68)
        );
        assert_eq!(
            base_coords(&json!({"scene": {"parcels": ["1,2", "3,4"]}})),
            (1, 2)
        );
        assert_eq!(base_coords(&json!({})), (0, 0));
    }

    #[test]
    fn scene_title_falls_back_to_untitled() {
        assert_eq!(
            scene_title(&json!({"display": {"title": "cube spawner"}})),
            "cube spawner"
        );
        assert_eq!(scene_title(&json!({})), "untitled");
    }

    #[test]
    fn qr_svg_data_url_is_base64_svg() {
        let url = qr_svg_data_url("decentraland://open?preview=http://10.1.2.20:5600").unwrap();
        assert!(url.starts_with("data:image/svg+xml;base64,"));
        use base64::Engine;
        let svg = base64::engine::general_purpose::STANDARD
            .decode(url.strip_prefix("data:image/svg+xml;base64,").unwrap())
            .unwrap();
        assert!(String::from_utf8(svg).unwrap().contains("<svg"));
    }

    #[test]
    fn tunnel_hint_prints_only_when_enabled() {
        let mut b = block(full_ifaces(), QrMode::Hint);
        assert!(!b.render().contains("Join from the internet"));
        b.tunnel_hint = true;
        let out = b.render();
        assert!(out.contains("Join from the internet"));
        assert!(out.contains("--tunnel wss://<tunnel-host>"));
        assert!(out.contains("--tunnel help"));
    }

    #[test]
    fn internet_section_pins_public_realm_grammars() {
        let b = block(full_ifaces(), QrMode::Hint);
        let out = b.internet_section("https://tunnel.example/t/abc123defg/");
        assert!(out.contains("Join from the INTERNET \u{2014} tunnel connected"));
        assert!(out.contains("  realm:    https://tunnel.example/t/abc123defg\n"));
        assert!(out.contains(
            "web:      https://decentraland.org/bevy-web/?preview=true&realm=https://tunnel.example/t/abc123defg&position=52,-68"
        ));
        assert!(out.contains(
            "desktop:  decentraland://realm=https%3A%2F%2Ftunnel.example%2Ft%2Fabc123defg&position=52%2C-68&local-scene=true&dclenv=org"
        ));
        assert!(out.contains(
            "native:   bevy-explorer --server https://tunnel.example/t/abc123defg --location 52,-68 --preview --hud"
        ));
        assert!(out.contains(
            "mobile:   decentraland://open?preview=https://tunnel.example/t/abc123defg&position=52,-68"
        ));
    }

    #[test]
    fn native_hud_flag_is_omitted_when_disabled() {
        let mut b = block(full_ifaces(), QrMode::Hint);
        b.native_hud = false;
        let out = b.render();
        assert!(out.contains(
            "native:   bevy-explorer --server http://10.1.2.20:5600 --location 52,-68 --preview\n"
        ));
        assert!(!out.contains("--hud"));
        let internet = b.internet_section("https://tunnel.example/t/abc123defg");
        assert!(internet.contains(
            "native:   bevy-explorer --server https://tunnel.example/t/abc123defg --location 52,-68 --preview\n"
        ));
    }

    #[test]
    fn internet_section_qr_mode_renders_a_qr() {
        let b = block(full_ifaces(), QrMode::Print);
        let out = b.internet_section("https://tunnel.example/t/abc123defg");
        assert!(out.contains("scan to open in the Decentraland mobile app"));
        assert!(out.contains(
            "this QR opens decentraland://open?preview=https://tunnel.example/t/abc123defg&position=52,-68 on your phone"
        ));
    }

    #[test]
    fn passthrough_parser_matches_upstream_forms() {
        let toks = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            parse_passthrough_params(&toks(&["--paramA", "--paramX", "valueX"])),
            vec![
                ("paramA".to_string(), "true".to_string()),
                ("paramX".to_string(), "valueX".to_string()),
            ]
        );
        assert_eq!(
            parse_passthrough_params(&toks(&["--key=value", "stray", "-flag"])),
            vec![
                ("key".to_string(), "value".to_string()),
                ("flag".to_string(), "true".to_string()),
            ]
        );
        assert_eq!(
            parse_passthrough_params(&toks(&["--=x", "--k", "one", "--k=two"])),
            vec![("k".to_string(), "two".to_string())]
        );
    }

    #[test]
    fn deep_link_extra_orders_declared_flags_before_passthrough() {
        let extra = deep_link_extra(
            true,
            true,
            Some(8123),
            &["--speed".to_string(), "2".to_string()],
        );
        assert_eq!(extra, "&local-ab=true&mcp=true&mcp-port=8123&speed=2");
    }

    #[test]
    fn deep_link_extra_passthrough_cannot_override_declared_or_core_keys() {
        let extra = deep_link_extra(
            false,
            true,
            None,
            &[
                "--mcp=false".to_string(),
                "--realm".to_string(),
                "evil".to_string(),
                "--position=9,9".to_string(),
                "--custom".to_string(),
                "a b".to_string(),
            ],
        );
        assert_eq!(extra, "&mcp=true&custom=a+b");
    }

    /// The client parses the deep link with `HttpUtility.ParseQueryString`,
    /// which is case-insensitive: a surviving `REALM=` would not sit beside our
    /// `realm=`, it would merge into it as `ours,theirs`.
    #[test]
    fn deep_link_extra_case_does_not_bypass_the_core_or_declared_guards() {
        let extra = deep_link_extra(
            true,
            true,
            Some(8123),
            &[
                "--REALM=http://evil".to_string(),
                "--Position=9,9".to_string(),
                "--Local-Scene=false".to_string(),
                "--DCLENV=zone".to_string(),
                "--Optimized-Assets-Url=http://evil".to_string(),
                "--MCP=false".to_string(),
                "--MCP-PORT=1".to_string(),
                "--Local-Ab=false".to_string(),
                "--custom=ok".to_string(),
                "--CUSTOM=twice".to_string(),
            ],
        );
        assert_eq!(extra, "&local-ab=true&mcp=true&mcp-port=8123&custom=ok");
    }

    /// A realm reflected from a proxy header must not be able to open a second
    /// query param in the web-explorer href.
    #[test]
    fn web_join_url_encodes_the_realm() {
        let url = web_join_url(
            "https://decentraland.org/bevy-web",
            "https://evil.example/?realm=http://attacker&x=",
            (52, -68),
        );
        assert_eq!(
            url,
            "https://decentraland.org/bevy-web/?preview=true&realm=https://evil.example/%3Frealm%3Dhttp://attacker%26x%3D&position=52,-68"
        );
        assert_eq!(url.matches("realm=").count(), 1);
        assert_eq!(url.matches('&').count(), 2, "only preview/realm/position");
        assert_eq!(
            web_join_url(
                "https://decentraland.org/bevy-web",
                "http://10.1.2.20:5600",
                (52, -68)
            ),
            "https://decentraland.org/bevy-web/?preview=true&realm=http://10.1.2.20:5600&position=52,-68"
        );
    }

    #[test]
    fn passthrough_multi_instance_reaches_the_plain_row_without_duplicating() {
        let mut b = block(full_ifaces(), QrMode::Hint);
        b.deep_link_extra = deep_link_extra(false, false, None, &["--multi-instance".to_string()]);
        let out = b.render();
        assert!(out.contains(
            "desktop:  decentraland://realm=http%3A%2F%2F127.0.0.1%3A5600&position=52%2C-68&local-scene=true&dclenv=org&multi-instance=true"
        ));
        let second = out
            .lines()
            .find(|l| l.contains("2nd instance"))
            .expect("second-instance row");
        assert_eq!(second.matches("multi-instance=true").count(), 1);
    }

    #[test]
    fn desktop_links_carry_the_deep_link_extra_on_every_row() {
        let mut b = block(full_ifaces(), QrMode::Hint);
        b.deep_link_extra = deep_link_extra(true, false, None, &[]);
        let out = b.render();
        assert!(out.contains(
            "desktop:  decentraland://realm=http%3A%2F%2F127.0.0.1%3A5600&position=52%2C-68&local-scene=true&dclenv=org&local-ab=true"
        ));
        assert!(out.contains(
            "desktop:  decentraland://realm=http%3A%2F%2F10.1.2.20%3A5600&position=52%2C-68&local-scene=true&dclenv=org&local-ab=true"
        ));
        assert!(out.contains(
            "desktop (2nd instance): decentraland://realm=http%3A%2F%2F127.0.0.1%3A5600&position=52%2C-68&local-scene=true&dclenv=org&multi-instance=true&local-ab=true"
        ));
        assert!(b.compact_body().contains("&local-ab=true"));
    }

    #[test]
    fn local_network_access_note_follows_the_web_explorer_origin() {
        let out = block(full_ifaces(), QrMode::Hint).render();
        assert!(out.contains("Local Network Access"));
        assert!(out
            .contains("chrome://settings/content/siteDetails?site=https%3A%2F%2Fdecentraland.org"));
        assert!(out.contains("\"Apps on device\" (Chrome 145+)"));
        assert!(out.contains("granted on\n    the JOINING device's Chrome/Edge"));
        let mut b = block(full_ifaces(), QrMode::Hint);
        b.web_explorer = "https://play.example.net/play".to_string();
        assert!(b
            .render()
            .contains("chrome://settings/content/siteDetails?site=https%3A%2F%2Fplay.example.net"));
        b.web_explorer = "not-a-url".to_string();
        let out = b.render();
        assert!(!out.contains("chrome://settings"));
        assert!(!out.contains("JOINING device"));
    }

    #[test]
    fn web_explorer_base_trims_trailing_slash() {
        assert_eq!(DEFAULT_WEB_EXPLORER, "https://decentraland.org/bevy-web");
        let b = block(full_ifaces(), QrMode::Hint);
        assert!(!b.web_url("http://127.0.0.1:5600").contains("web//?"));
    }
}
