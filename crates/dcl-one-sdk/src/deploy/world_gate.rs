use crate::jsjson::JsValue;
use crate::ux::{TrySteps, UserError};

/// A `worldConfiguration` section whose name is blank or absent names no
/// destination at all: a worlds server wants the name, and a Genesis catalyst
/// refuses the section outright (ADR-173). Caught here so the deploy stops
/// before the build and the wallet prompt, not after them.
pub fn nameless_world_section(metadata: &JsValue) -> bool {
    metadata.get("worldConfiguration").is_some() && super::world_name(metadata).is_none()
}

pub fn refuse_nameless_world() -> anyhow::Error {
    UserError::new(
        "scene.json has a worldConfiguration section that names no world",
        TrySteps::one(
            "to deploy to Genesis City parcels: remove the worldConfiguration section from scene.json (the /target page's \"Point at Genesis City LAND\" does this)",
        )
        .and("to publish a World: set worldConfiguration.name, e.g. \"myname.dcl.eth\""),
    )
    .why("a World needs the name, and a Genesis City catalyst refuses any worldConfiguration (ADR-173), so this scene can be published nowhere")
    .into()
}

/// The worlds server refused a scene for naming no world: the mirror of
/// `world_at_genesis`, reached when `--target-server` / `DCL_ONE_SDK_TARGET_SERVER`
/// points a parcel scene at a worlds server. Both the public server's
/// wording and catalyrst-worlds' are recognised.
pub(super) fn plain_scene_at_worlds(body: &str) -> bool {
    body.contains("worldConfiguration")
        && (body.contains("needs to specify")
            || body.contains("required to deploy a scene to a World"))
}

pub(super) fn refuse_plain_scene_at_worlds() -> anyhow::Error {
    UserError::new(
        "this scene names no world, but it was sent to a worlds server, which only takes World scenes",
        TrySteps::one(
            "to deploy to Genesis City parcels: drop --target-server / DCL_ONE_SDK_TARGET_SERVER so the scene routes to a Genesis catalyst",
        )
        .and("to publish a World: set worldConfiguration.name in scene.json (the /target page's world picker does this)"),
    )
    .why("worlds and Genesis parcels are different deploy destinations; this target is a worlds server")
    .into()
}

/// A World sent to a Genesis catalyst: the same refusal whether a catalyst
/// answered ADR-173 or the target was recognised by host before the build.
pub(super) fn refuse_world_at_genesis() -> anyhow::Error {
    UserError::new(
        "this scene is a World, but it was sent to a Genesis City content server, which only takes parcel scenes",
        TrySteps::one(format!(
            "to publish the World: drop --target-server / DCL_ONE_SDK_TARGET_SERVER so it routes to the worlds server ({}), or point the target at a worlds server",
            super::WORLDS_CONTENT_SERVER
        ))
        .and("to deploy to Genesis City parcels instead: remove worldConfiguration from scene.json (the /target page's \"Point at Genesis City LAND\" does this)"),
    )
    .why("worlds and Genesis parcels are different deploy destinations; this target is a Genesis catalyst")
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::net::rejected;
    use crate::jsjson;

    fn meta(s: &str) -> JsValue {
        jsjson::parse(s).unwrap()
    }

    #[test]
    fn a_world_section_that_names_no_world_is_flagged() {
        assert!(nameless_world_section(&meta(
            r#"{"worldConfiguration":{"name":""}}"#
        )));
        assert!(nameless_world_section(&meta(
            r#"{"worldConfiguration":{"name":"   "}}"#
        )));
        assert!(nameless_world_section(&meta(
            r#"{"worldConfiguration":{}}"#
        )));
        assert!(!nameless_world_section(&meta(
            r#"{"worldConfiguration":{"name":"gather.dcl.eth"}}"#
        )));
        assert!(!nameless_world_section(&meta("{}")));
    }

    /// The refusal reads as the dead end it is and names both ways out, with
    /// the ADR reasoning below the headline rather than in it.
    #[test]
    fn the_refusal_names_both_ways_out() {
        let e = crate::ux::render(&refuse_nameless_world(), false, false);
        assert!(
            e.starts_with(
                "Error: scene.json has a worldConfiguration section that names no world\n"
            ),
            "{e}"
        );
        assert!(e.contains("ADR-173"), "{e}");
        assert!(e.contains("Point at Genesis City LAND"), "{e}");
        assert!(e.contains("worldConfiguration.name"), "{e}");
    }

    /// A parcel scene at a worlds server reads as the routing mistake it is
    /// — the mirror of the ADR-173 case — under either server's wording.
    #[test]
    fn a_plain_scene_at_a_worlds_server_names_the_routing_fix() {
        for body in [
            r#"{"error":"Bad request","message":"Deployment failed: scene.json needs to specify a worldConfiguration section with a valid name inside."}"#,
            r#"{"error":"Bad request","message":"The metadata.worldConfiguration.name is required to deploy a scene to a World"}"#,
        ] {
            let e = crate::ux::render(&rejected(400, body, &[]), false, false);
            assert!(e.contains("names no world"), "{e}");
            assert!(e.contains("Genesis catalyst"), "{e}");
            assert!(e.contains("worldConfiguration.name"), "{e}");
            assert!(!e.contains("read the server message above"), "{e}");
        }
        let adr = r#"{"errors":["The scene.json contains a worldConfiguration section, which is not allowed for Genesis City scenes (see ADR-173)"]}"#;
        assert!(!plain_scene_at_worlds(adr));
    }
}
