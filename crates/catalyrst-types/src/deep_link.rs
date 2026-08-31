/// Launch links for the desktop explorer.
///
/// `https://decentraland.org/play/?realm=...` cannot reach a self-hosted node:
/// the website forwards `realm` only for realms it trusts (the client's
/// `deeplink-whitelisted-worlds` flag) and silently drops it otherwise, so the
/// visitor boots the default Genesis realm at the requested coordinates -- a
/// failure that looks like a realm bug because the position survives. Handing
/// the protocol handler the link directly bypasses that: `realm` is a tier-1
/// always-permitted deep-link param (`DeepLinkAllowlist`), so the launcher
/// passes it through untouched.
///
/// The emitted shape matches what the launcher itself emits
/// (`launcher-rust` `DownloadOriginMetadata::deeplink`). The client normalises
/// `^decentraland:/+` before parsing, so `decentraland://realm=` and
/// `decentraland:///?realm=` are equivalent on the receiving end; keeping one
/// form here keeps the encoding rules in one place.
fn form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `realm` accepts either a full realm URL (`https://host/world/name`) or a
/// bare ENS world name; pass the URL for anything self-hosted, since a bare
/// name resolves against Decentraland's own worlds server.
pub fn realm_deep_link(realm: &str, position: (i64, i64)) -> String {
    format!(
        "decentraland://realm={}&position={}",
        form_encode(realm),
        form_encode(&format!("{},{}", position.0, position.1)),
    )
}

/// The realm URL a world is entered through: worlds are addressed by path off
/// the worlds server's public base, not by their name alone.
pub fn world_realm_url(worlds_base_url: &str, world_name: &str) -> String {
    format!(
        "{}/world/{}",
        worlds_base_url.trim_end_matches('/'),
        world_name.to_lowercase()
    )
}

/// Parses `"x,y"` as written in scene metadata and world spawn settings.
/// Anything unparseable lands the visitor at the origin rather than failing the
/// link, since a missing spawn must not cost them the walk-in.
pub fn parse_position(raw: Option<&str>) -> (i64, i64) {
    let Some(raw) = raw else { return (0, 0) };
    let Some((x, y)) = raw.trim().split_once(',') else {
        return (0, 0);
    };
    match (x.trim().parse::<i64>(), y.trim().parse::<i64>()) {
        (Ok(x), Ok(y)) => (x, y),
        _ => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_realm_url_and_the_position_separator() {
        assert_eq!(
            realm_deep_link("http://127.0.0.1:5600", (52, -68)),
            "decentraland://realm=http%3A%2F%2F127.0.0.1%3A5600&position=52%2C-68"
        );
    }

    #[test]
    fn encodes_a_path_addressed_world_realm() {
        assert_eq!(
            realm_deep_link(
                "https://realm.example.org/worlds-content-server/world/swiss-cube",
                (0, 0)
            ),
            "decentraland://realm=https%3A%2F%2Frealm.example.org%2Fworlds-content-server%2Fworld%2Fswiss-cube&position=0%2C0"
        );
    }

    #[test]
    fn world_realm_url_lowercases_and_trims() {
        assert_eq!(
            world_realm_url("https://host/worlds-content-server/", "Swiss.DCL.eth"),
            "https://host/worlds-content-server/world/swiss.dcl.eth"
        );
    }

    #[test]
    fn position_falls_back_to_origin() {
        assert_eq!(parse_position(Some("-36,122")), (-36, 122));
        assert_eq!(parse_position(Some(" 4 , 5 ")), (4, 5));
        assert_eq!(parse_position(Some("nonsense")), (0, 0));
        assert_eq!(parse_position(None), (0, 0));
    }
}
