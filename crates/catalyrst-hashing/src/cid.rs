pub fn cid_v0_to_string(digest: &[u8]) -> String {
    assert_eq!(
        digest.len(),
        32,
        "cid_v0_to_string: SHA-256 digest must be 32 bytes (got {})",
        digest.len()
    );
    let mut multihash = Vec::with_capacity(34);
    multihash.push(0x12);
    multihash.push(0x20);
    multihash.extend_from_slice(digest);
    bs58::encode(&multihash).into_string()
}

pub fn encode_cid_v1(codec: u64, digest: &[u8]) -> Vec<u8> {
    assert_eq!(
        digest.len(),
        32,
        "encode_cid_v1: SHA-256 digest must be 32 bytes (got {})",
        digest.len()
    );
    let mut buf = Vec::with_capacity(2 + 5 + 34);
    encode_varint(&mut buf, 1);
    encode_varint(&mut buf, codec);
    buf.push(0x12);
    buf.push(0x20);
    buf.extend_from_slice(digest);
    buf
}

pub fn cid_v1_to_string(codec: u64, digest: &[u8]) -> String {
    let cid_bytes = encode_cid_v1(codec, digest);
    multibase_base32lower(&cid_bytes)
}

pub fn multibase_base32lower(data: &[u8]) -> String {
    let encoded = data_encoding::BASE32_NOPAD
        .encode(data)
        .to_ascii_lowercase();
    format!("b{encoded}")
}

fn encode_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            buf.push(byte);
            break;
        } else {
            buf.push(byte | 0x80);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_single_byte() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 1);
        assert_eq!(buf, vec![1]);
    }

    #[test]
    fn varint_two_bytes() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 0x55);
        assert_eq!(buf, vec![0x55]);
    }

    #[test]
    fn varint_larger() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 0x70);
        assert_eq!(buf, vec![0x70]);
    }

    #[test]
    fn varint_multi_byte() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 300);
        assert_eq!(buf, vec![0xAC, 0x02]);
    }

    #[test]
    fn cidv0_format() {
        let digest: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ];
        let s = cid_v0_to_string(&digest);
        assert_eq!(s, "QmdfTbBqBPQ7VNxZEYEj14VmRuZBkqFbiwReogJgS1zR1n");
    }

    #[test]
    fn cidv1_raw_format() {
        let digest: [u8; 32] = [
            0xb9, 0x4d, 0x27, 0xb9, 0x93, 0x4d, 0x3e, 0x08, 0xa5, 0x2e, 0x52, 0xd7, 0xda, 0x7d,
            0xab, 0xfa, 0xc4, 0x84, 0xef, 0xe3, 0x7a, 0x53, 0x80, 0xee, 0x90, 0x88, 0xf7, 0xac,
            0xe2, 0xef, 0xcd, 0xe9,
        ];
        let s = cid_v1_to_string(0x55, &digest);
        assert_eq!(
            s,
            "bafkreifzjut3te2nhyekklss27nh3k72ysco7y32koao5eei66wof36n5e"
        );
    }
}
