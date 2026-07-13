use crate::netinfo::{nat_vm_guest, share_ip, Iface, IfaceClass};
use serde_json::Value;
use std::net::Ipv4Addr;

pub const DEFAULT_WEB_EXPLORER: &str = "https://decentraland.org/play";

pub fn web_explorer_base() -> String {
    std::env::var("DCL_ONE_SDK_WEB_EXPLORER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_WEB_EXPLORER.to_string())
        .trim_end_matches('/')
        .to_string()
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
    parse_coords(base.unwrap_or_default()).unwrap_or((0, 0))
}

fn parse_coords(s: &str) -> Option<(i64, i64)> {
    let (x, y) = s.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
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
}

fn form_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
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

    pub fn render(&self) -> String {
        format!("{}\n{}", self.heading(), self.body())
    }

    fn realm(&self, ip: Ipv4Addr) -> String {
        format!("http://{ip}:{}", self.port)
    }

    fn web_url(&self, realm: &str) -> String {
        format!(
            "{}/?realm={realm}&preview=true&position={},{}",
            self.web_explorer, self.position.0, self.position.1
        )
    }

    fn desktop_link(&self, realm: &str) -> String {
        self.desktop_link_with(realm, "")
    }

    fn desktop_link_with(&self, realm: &str, extra: &str) -> String {
        let ab = match &self.optimized_assets_url {
            Some(url) => format!("&optimized-assets-url={}", form_encode(url)),
            None => String::new(),
        };
        format!(
            "decentraland://\"realm={realm}&position={},{}&local-scene=true&dclenv=org{ab}{extra}\"",
            self.position.0, self.position.1
        )
    }

    fn native_cmd(&self, realm: &str) -> String {
        format!(
            "bevy-explorer --server {realm} --location {},{} --preview",
            self.position.0, self.position.1
        )
    }

    fn mobile_link(&self, realm: &str) -> String {
        format!(
            "decentraland://open?preview={realm}&position={}%2C{}",
            self.position.0, self.position.1
        )
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
        out.push_str(&format!("  desktop:  {}\n", self.desktop_link(&realm)));
        out.push_str(&format!(
            "  desktop (2nd instance): {}\n",
            self.desktop_link_with(&realm, "&multi-instance=true")
        ));
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
        out.push_str(&format!("  desktop:  {}\n", self.desktop_link(&realm)));
        out.push_str(&format!("  native:   {}\n", self.native_cmd(&realm)));
        out.push_str(&format!("  web:      {}\n", self.web_url(&realm)));
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
            web_explorer: "https://decentraland.org/play".to_string(),
            qr,
            unreachable: Vec::new(),
            tunnel_hint: false,
            editor: false,
            optimized_assets_url: None,
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
    fn desktop_link_carries_the_optimized_assets_url_when_set() {
        let mut b = block(vec![iface("lo", "127.0.0.1")], QrMode::Hint);
        b.optimized_assets_url = Some("http://127.0.0.1:5147".to_string());
        let out = b.render();
        assert!(out.contains(
            "decentraland://\"realm=http://127.0.0.1:5600&position=52,-68&local-scene=true&dclenv=org&optimized-assets-url=http%3A%2F%2F127.0.0.1%3A5147\""
        ));
    }

    #[test]
    fn deep_link_grammars_are_pinned() {
        let out = block(full_ifaces(), QrMode::Hint).render();
        assert!(out.contains(
            "desktop:  decentraland://\"realm=http://127.0.0.1:5600&position=52,-68&local-scene=true&dclenv=org\""
        ));
        assert!(out.contains(
            "desktop:  decentraland://\"realm=http://10.1.2.20:5600&position=52,-68&local-scene=true&dclenv=org\""
        ));
        assert!(out.contains(
            "desktop (2nd instance): decentraland://\"realm=http://127.0.0.1:5600&position=52,-68&local-scene=true&dclenv=org&multi-instance=true\""
        ));
        assert!(out.contains(
            "native:   bevy-explorer --server http://10.1.2.20:5600 --location 52,-68 --preview"
        ));
        assert!(out.contains(
            "web:      https://decentraland.org/play/?realm=http://10.1.2.20:5600&preview=true&position=52,-68"
        ));
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
            "this QR opens decentraland://open?preview=http://10.1.2.20:5600&position=52%2C-68 on your phone"
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
            "web:      https://decentraland.org/play/?realm=https://tunnel.example/t/abc123defg&preview=true&position=52,-68"
        ));
        assert!(out.contains(
            "desktop:  decentraland://\"realm=https://tunnel.example/t/abc123defg&position=52,-68&local-scene=true&dclenv=org\""
        ));
        assert!(out.contains(
            "native:   bevy-explorer --server https://tunnel.example/t/abc123defg --location 52,-68 --preview"
        ));
        assert!(out.contains(
            "mobile:   decentraland://open?preview=https://tunnel.example/t/abc123defg&position=52%2C-68"
        ));
    }

    #[test]
    fn internet_section_qr_mode_renders_a_qr() {
        let b = block(full_ifaces(), QrMode::Print);
        let out = b.internet_section("https://tunnel.example/t/abc123defg");
        assert!(out.contains("scan to open in the Decentraland mobile app"));
        assert!(out.contains(
            "this QR opens decentraland://open?preview=https://tunnel.example/t/abc123defg&position=52%2C-68 on your phone"
        ));
    }

    #[test]
    fn web_explorer_base_trims_trailing_slash() {
        assert_eq!(DEFAULT_WEB_EXPLORER, "https://decentraland.org/play");
        let b = block(full_ifaces(), QrMode::Hint);
        assert!(!b.web_url("http://127.0.0.1:5600").contains("play//?"));
    }
}
