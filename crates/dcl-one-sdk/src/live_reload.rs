use crate::scene::b64_hash;
use prost::Message;
use serde_json::json;
use std::path::{Path, PathBuf};

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/decentraland.sdk.development.rs"));
}

use proto::{ws_scene_message, UpdateModel, UpdateModelType, UpdateScene, WsSceneMessage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadEvent {
    Scene,
    Model(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadFrame {
    Text(String),
    Binary(Vec<u8>),
}

pub fn scene_update_json(scene_id: &str) -> String {
    json!({
        "type": "SCENE_UPDATE",
        "payload": { "sceneId": scene_id, "sceneType": "scene" }
    })
    .to_string()
}

pub fn update_scene_frame(scene_id: &str) -> Vec<u8> {
    WsSceneMessage {
        message: Some(ws_scene_message::Message::UpdateScene(UpdateScene {
            scene_id: scene_id.to_string(),
        })),
    }
    .encode_to_vec()
}

pub fn update_model_frame(scene_id: &str, src: &str, hash: &str) -> Vec<u8> {
    WsSceneMessage {
        message: Some(ws_scene_message::Message::UpdateModel(UpdateModel {
            scene_id: scene_id.to_string(),
            src: src.to_string(),
            hash: hash.to_string(),
            r#type: UpdateModelType::UmtChange as i32,
        })),
    }
    .encode_to_vec()
}

pub fn model_src(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn reload_frames(
    root: &Path,
    scene_id: &str,
    machine: &str,
    event: &ReloadEvent,
) -> Vec<ReloadFrame> {
    let binary = match event {
        ReloadEvent::Scene => update_scene_frame(scene_id),
        ReloadEvent::Model(path) => {
            let src = model_src(root, path);
            let hash = b64_hash(&src, machine);
            update_model_frame(scene_id, &src, &hash)
        }
    };
    vec![
        ReloadFrame::Text(scene_update_json(scene_id)),
        ReloadFrame::Binary(binary),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_scene_golden_bytes() {
        let bytes = update_scene_frame("abc");
        assert_eq!(bytes, vec![0x0a, 0x05, 0x0a, 0x03, 0x61, 0x62, 0x63]);
    }

    #[test]
    fn update_scene_round_trip() {
        let bytes = update_scene_frame("b64-c2NlbmU=");
        let decoded = WsSceneMessage::decode(bytes.as_slice()).unwrap();
        match decoded.message {
            Some(ws_scene_message::Message::UpdateScene(u)) => {
                assert_eq!(u.scene_id, "b64-c2NlbmU=");
            }
            other => panic!("unexpected oneof: {other:?}"),
        }
    }

    #[test]
    fn update_model_round_trip() {
        let bytes = update_model_frame("scene-1", "assets/tree.glb", "b64-aGFzaA==");
        let decoded = WsSceneMessage::decode(bytes.as_slice()).unwrap();
        match decoded.message {
            Some(ws_scene_message::Message::UpdateModel(u)) => {
                assert_eq!(u.scene_id, "scene-1");
                assert_eq!(u.src, "assets/tree.glb");
                assert_eq!(u.hash, "b64-aGFzaA==");
                assert_eq!(u.r#type, UpdateModelType::UmtChange as i32);
                assert_eq!(u.r#type(), UpdateModelType::UmtChange);
            }
            other => panic!("unexpected oneof: {other:?}"),
        }
    }

    #[test]
    fn scene_frames_text_first_and_unchanged() {
        let frames = reload_frames(Path::new("/proj"), "scene-1", "host", &ReloadEvent::Scene);
        assert_eq!(frames.len(), 2);
        let expected_json = json!({
            "type": "SCENE_UPDATE",
            "payload": { "sceneId": "scene-1", "sceneType": "scene" }
        })
        .to_string();
        assert_eq!(frames[0], ReloadFrame::Text(expected_json));
        assert_eq!(
            frames[1],
            ReloadFrame::Binary(update_scene_frame("scene-1"))
        );
    }

    #[test]
    fn model_frames_use_relative_src_and_b64_hash() {
        let root = Path::new("/proj");
        let event = ReloadEvent::Model(root.join("assets/tree.glb"));
        let frames = reload_frames(root, "scene-1", "host", &event);
        assert_eq!(frames.len(), 2);
        assert!(matches!(&frames[0], ReloadFrame::Text(t) if t.contains("SCENE_UPDATE")));
        let ReloadFrame::Binary(bytes) = &frames[1] else {
            panic!("second frame must be binary");
        };
        let decoded = WsSceneMessage::decode(bytes.as_slice()).unwrap();
        match decoded.message {
            Some(ws_scene_message::Message::UpdateModel(u)) => {
                assert_eq!(u.src, "assets/tree.glb");
                assert_eq!(u.hash, b64_hash("assets/tree.glb", "host"));
                assert_eq!(u.scene_id, "scene-1");
                assert_eq!(u.r#type(), UpdateModelType::UmtChange);
            }
            other => panic!("unexpected oneof: {other:?}"),
        }
    }
}
