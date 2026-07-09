use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i32),
    Str(String),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i32> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub id: u32,
    pub is_request: bool,
    pub value: Value,
}

pub fn encode_frame(id: u32, is_request: bool, value: &Value) -> Vec<u8> {
    let mut payload = Vec::with_capacity(256);
    write_u32(&mut payload, (id << 1) | u32::from(!is_request));
    encode_value(&mut payload, value);
    let mut frame = Vec::with_capacity(payload.len() + 4);
    write_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    frame
}

fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_len_prefixed(out: &mut Vec<u8>, bytes: &[u8]) {
    write_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
}

fn encode_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => out.push(0),
        Value::Bool(b) => {
            out.push(1);
            out.push(u8::from(*b));
        }
        Value::Int(n) => {
            out.push(2);
            out.extend_from_slice(&n.to_le_bytes());
        }
        Value::Str(s) => {
            out.push(3);
            write_len_prefixed(out, s.as_bytes());
        }
        Value::Bytes(b) => {
            out.push(4);
            write_len_prefixed(out, b);
        }
        Value::Array(items) => {
            out.push(5);
            write_u32(out, items.len() as u32);
            for item in items {
                encode_value(out, item);
            }
        }
        Value::Object(pairs) => {
            out.push(6);
            write_u32(out, pairs.len() as u32);
            for (key, v) in pairs {
                write_len_prefixed(out, key.as_bytes());
                encode_value(out, v);
            }
        }
    }
}

pub fn decode_payload(bytes: &[u8]) -> Result<Packet> {
    let mut r = Reader { buf: bytes, pos: 0 };
    let id_word = r.u32()?;
    let is_request = id_word & 1 == 0;
    let id = id_word >> 1;
    let value = decode_value(&mut r)?;
    if r.pos != bytes.len() {
        bail!("invalid packet: {} trailing bytes", bytes.len() - r.pos);
    }
    Ok(Packet {
        id,
        is_request,
        value,
    })
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| anyhow::anyhow!("invalid packet: truncated"))?;
        self.pos += 1;
        Ok(b)
    }

    fn u32(&mut self) -> Result<u32> {
        let end = self.pos + 4;
        if end > self.buf.len() {
            bail!("invalid packet: truncated u32");
        }
        let v = u32::from_le_bytes(self.buf[self.pos..end].try_into()?);
        self.pos = end;
        Ok(v)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos + n;
        if end > self.buf.len() {
            bail!("invalid packet: truncated byte run of {n}");
        }
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn len_prefixed(&mut self) -> Result<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
}

fn decode_value(r: &mut Reader) -> Result<Value> {
    match r.u8()? {
        0 => Ok(Value::Null),
        1 => Ok(Value::Bool(r.u8()? != 0)),
        2 => Ok(Value::Int(r.u32()? as i32)),
        3 => Ok(Value::Str(String::from_utf8(r.len_prefixed()?.to_vec())?)),
        4 => Ok(Value::Bytes(r.len_prefixed()?.to_vec())),
        5 => {
            let n = r.u32()? as usize;
            let mut items = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                items.push(decode_value(r)?);
            }
            Ok(Value::Array(items))
        }
        6 => {
            let n = r.u32()? as usize;
            let mut pairs = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                let key = String::from_utf8(r.len_prefixed()?.to_vec())?;
                pairs.push((key, decode_value(r)?));
            }
            Ok(Value::Object(pairs))
        }
        tag => bail!("invalid packet: unknown type tag {tag}"),
    }
}

pub fn take_frame(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    if buf.len() < 4 {
        return None;
    }
    let n = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if buf.len() < 4 + n {
        return None;
    }
    let payload = buf[4..4 + n].to_vec();
    buf.drain(..4 + n);
    Some(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDSHAKE_HEX: &str = "07000000302e31382e3230";

    const BUILD_REQUEST_HEX: &str = concat!(
        "3c01000000000000060a000000070000",
        "00636f6d6d616e640305000000627569",
        "6c64030000006b657902000000000700",
        "0000656e747269657305010000000502",
        "000000030000000003200000002f746d",
        "702f65736275696c642d70726f746f2d",
        "746573742f656e7472792e6a73050000",
        "00666c61677305040000000313000000",
        "2d2d6c6f672d6c6576656c3d7761726e",
        "696e67030d0000002d2d6c6f672d6c69",
        "6d69743d3003080000002d2d62756e64",
        "6c65030c0000002d2d666f726d61743d",
        "636a7305000000777269746501000d00",
        "0000737464696e436f6e74656e747300",
        "0f000000737464696e5265736f6c7665",
        "446972000d000000616273576f726b69",
        "6e6744697203170000002f746d702f65",
        "736275696c642d70726f746f2d746573",
        "74090000006e6f646550617468730500",
        "00000007000000636f6e746578740100",
    );

    fn build_vector_value() -> Value {
        Value::Object(vec![
            ("command".into(), Value::Str("build".into())),
            ("key".into(), Value::Int(0)),
            (
                "entries".into(),
                Value::Array(vec![Value::Array(vec![
                    Value::Str(String::new()),
                    Value::Str("/tmp/esbuild-proto-test/entry.js".into()),
                ])]),
            ),
            (
                "flags".into(),
                Value::Array(vec![
                    Value::Str("--log-level=warning".into()),
                    Value::Str("--log-limit=0".into()),
                    Value::Str("--bundle".into()),
                    Value::Str("--format=cjs".into()),
                ]),
            ),
            ("write".into(), Value::Bool(false)),
            ("stdinContents".into(), Value::Null),
            ("stdinResolveDir".into(), Value::Null),
            (
                "absWorkingDir".into(),
                Value::Str("/tmp/esbuild-proto-test".into()),
            ),
            ("nodePaths".into(), Value::Array(vec![])),
            ("context".into(), Value::Bool(false)),
        ])
    }

    #[test]
    fn handshake_frame_peels_to_version_string() {
        let mut buf = hex::decode(HANDSHAKE_HEX).unwrap();
        let payload = take_frame(&mut buf).unwrap();
        assert_eq!(payload, b"0.18.20");
        assert!(buf.is_empty());
    }

    #[test]
    fn build_request_encodes_to_reference_vector() {
        let expected = hex::decode(BUILD_REQUEST_HEX).unwrap();
        assert_eq!(expected.len(), 320);
        let encoded = encode_frame(0, true, &build_vector_value());
        assert_eq!(encoded, expected);
    }

    #[test]
    fn build_request_vector_round_trips() {
        let wire = hex::decode(BUILD_REQUEST_HEX).unwrap();
        let mut buf = wire.clone();
        let payload = take_frame(&mut buf).unwrap();
        assert!(buf.is_empty());
        let pkt = decode_payload(&payload).unwrap();
        assert_eq!(pkt.id, 0);
        assert!(pkt.is_request);
        assert_eq!(pkt.value, build_vector_value());
        assert_eq!(encode_frame(pkt.id, pkt.is_request, &pkt.value), wire);
    }

    #[test]
    fn all_types_round_trip() {
        let value = Value::Object(vec![
            ("n".into(), Value::Null),
            ("t".into(), Value::Bool(true)),
            ("f".into(), Value::Bool(false)),
            ("neg".into(), Value::Int(-42)),
            ("big".into(), Value::Int(i32::MAX)),
            ("s".into(), Value::Str("héllo ▲".into())),
            ("b".into(), Value::Bytes(vec![0, 1, 254, 255])),
            (
                "a".into(),
                Value::Array(vec![Value::Int(1), Value::Str("x".into()), Value::Null]),
            ),
            (
                "o".into(),
                Value::Object(vec![("inner".into(), Value::Array(vec![]))]),
            ),
        ]);
        let frame = encode_frame(7, false, &value);
        let mut buf = frame.clone();
        let payload = take_frame(&mut buf).unwrap();
        let pkt = decode_payload(&payload).unwrap();
        assert_eq!(pkt.id, 7);
        assert!(!pkt.is_request);
        assert_eq!(pkt.value, value);
    }

    #[test]
    fn partial_frames_are_not_consumed() {
        let wire = hex::decode(BUILD_REQUEST_HEX).unwrap();
        let mut buf = wire[..100].to_vec();
        assert!(take_frame(&mut buf).is_none());
        assert_eq!(buf.len(), 100);
        buf.extend_from_slice(&wire[100..]);
        buf.extend_from_slice(&wire[..10]);
        let payload = take_frame(&mut buf).unwrap();
        assert_eq!(payload, wire[4..]);
        assert_eq!(buf.len(), 10);
        assert!(take_frame(&mut buf).is_none());
    }

    #[test]
    fn trailing_bytes_rejected() {
        let mut payload = encode_frame(0, true, &Value::Int(1))[4..].to_vec();
        payload.push(0);
        assert!(decode_payload(&payload).is_err());
    }
}
