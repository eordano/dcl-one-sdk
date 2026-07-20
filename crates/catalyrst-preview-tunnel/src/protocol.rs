use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    pub id: String,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelKind {
    Http,
    Ws,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Control {
    Hello {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        token: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume: Option<Resume>,
        agent: String,
    },
    Welcome {
        id: String,
        public_url: String,
        resume_key: String,
        ping_s: u64,
    },
    Open {
        ch: u32,
        kind: ChannelKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        method: Option<String>,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        headers: Vec<(String, String)>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subprotocols: Option<Vec<String>>,
    },
    OpenOk {
        ch: u32,
        status: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headers: Option<Vec<(String, String)>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subprotocol: Option<String>,
    },
    OpenErr {
        ch: u32,
        error: String,
    },
    End {
        ch: u32,
    },
    Close {
        ch: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Ping,
    Pong,
    #[serde(other)]
    Unknown,
}

impl Control {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("control message serializes")
    }

    pub fn decode(text: &str) -> Option<Control> {
        serde_json::from_str(text).ok()
    }
}

pub const FLAG_BINARY: u8 = 0b0000_0001;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFrame {
    pub ch: u32,
    pub binary: bool,
    pub payload: Vec<u8>,
}

pub fn encode_data(ch: u32, binary: bool, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.extend_from_slice(&ch.to_be_bytes());
    out.push(if binary { FLAG_BINARY } else { 0 });
    out.extend_from_slice(payload);
    out
}

pub fn decode_data(bytes: &[u8]) -> Option<DataFrame> {
    if bytes.len() < 5 {
        return None;
    }
    let ch = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    Some(DataFrame {
        ch,
        binary: bytes[4] & FLAG_BINARY != 0,
        payload: bytes[5..].to_vec(),
    })
}

pub const HOP_BY_HOP: [&str; 13] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "sec-websocket-key",
    "sec-websocket-version",
    "sec-websocket-extensions",
    "sec-websocket-accept",
    "sec-websocket-protocol",
];

pub fn is_hop_by_hop(name: &str) -> bool {
    HOP_BY_HOP.iter().any(|h| name.eq_ignore_ascii_case(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_tags_match_the_spec_grammar() {
        let hello = Control::Hello {
            token: None,
            resume: Some(Resume {
                id: "abc123defg".into(),
                key: "k1".into(),
            }),
            agent: "dcl-one-sdk/0.1.0".into(),
        };
        assert_eq!(
            hello.encode(),
            r#"{"t":"hello","resume":{"id":"abc123defg","key":"k1"},"agent":"dcl-one-sdk/0.1.0"}"#
        );
        let welcome = Control::Welcome {
            id: "abc123defg".into(),
            public_url: "https://tunnel.example/t/abc123defg".into(),
            resume_key: "rk".into(),
            ping_s: 20,
        };
        assert_eq!(
            welcome.encode(),
            r#"{"t":"welcome","id":"abc123defg","public_url":"https://tunnel.example/t/abc123defg","resume_key":"rk","ping_s":20}"#
        );
        let open = Control::Open {
            ch: 7,
            kind: ChannelKind::Http,
            method: Some("GET".into()),
            path: "/content/contents/xyz".into(),
            query: Some("a=b".into()),
            headers: vec![("accept".into(), "*/*".into())],
            subprotocols: None,
        };
        assert_eq!(
            open.encode(),
            r#"{"t":"open","ch":7,"kind":"http","method":"GET","path":"/content/contents/xyz","query":"a=b","headers":[["accept","*/*"]]}"#
        );
        let ws_ok = Control::OpenOk {
            ch: 9,
            status: 101,
            headers: None,
            subprotocol: Some("rfc5".into()),
        };
        assert_eq!(
            ws_ok.encode(),
            r#"{"t":"open_ok","ch":9,"status":101,"subprotocol":"rfc5"}"#
        );
        assert_eq!(Control::Ping.encode(), r#"{"t":"ping"}"#);
        assert_eq!(Control::End { ch: 7 }.encode(), r#"{"t":"end","ch":7}"#);
    }

    #[test]
    fn unknown_discriminator_decodes_to_unknown() {
        assert_eq!(
            Control::decode(r#"{"t":"future-feature","x":1}"#),
            Some(Control::Unknown)
        );
        assert_eq!(Control::decode("not json"), None);
    }

    #[test]
    fn open_with_subprotocols_round_trips() {
        let open = Control::Open {
            ch: 9,
            kind: ChannelKind::Ws,
            method: None,
            path: "/mini-comms/room-1".into(),
            query: None,
            headers: vec![],
            subprotocols: Some(vec!["rfc5".into(), "rfc4".into()]),
        };
        assert_eq!(Control::decode(&open.encode()), Some(open));
    }

    #[test]
    fn data_frame_round_trips_and_preserves_text_binary_bit() {
        let bin = encode_data(7, true, b"payload");
        assert_eq!(&bin[..4], &7u32.to_be_bytes());
        assert_eq!(bin[4], FLAG_BINARY);
        assert_eq!(
            decode_data(&bin),
            Some(DataFrame {
                ch: 7,
                binary: true,
                payload: b"payload".to_vec()
            })
        );
        let text = encode_data(9, false, b"{}");
        assert_eq!(text[4], 0);
        assert!(!decode_data(&text).unwrap().binary);
        assert_eq!(decode_data(&[0, 0, 0, 1]), None);
        assert_eq!(
            decode_data(&[0, 0, 0, 2, 1]),
            Some(DataFrame {
                ch: 2,
                binary: true,
                payload: vec![]
            })
        );
    }

    #[test]
    fn hop_by_hop_headers_are_case_insensitive() {
        assert!(is_hop_by_hop("Connection"));
        assert!(is_hop_by_hop("sec-websocket-protocol"));
        assert!(!is_hop_by_hop("accept"));
        assert!(!is_hop_by_hop("x-forwarded-proto"));
    }
}
