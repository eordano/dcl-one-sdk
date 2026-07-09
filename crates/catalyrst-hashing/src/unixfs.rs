pub struct PBLink<'a> {
    pub hash: &'a [u8],
    pub name: &'a str,
    pub tsize: u64,
}

pub fn encode_file_node(filesize: u64, blocksizes: &[u64]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);

    buf.push(0x08);
    encode_varint(&mut buf, 2);

    buf.push(0x18);
    encode_varint(&mut buf, filesize);

    for &bs in blocksizes {
        buf.push(0x20);
        encode_varint(&mut buf, bs);
    }

    buf
}

pub fn encode_pb_node(data: &[u8], links: &[PBLink]) -> Vec<u8> {
    let mut size = 0usize;

    let link_sizes: Vec<usize> = links.iter().map(|l| pb_link_size(l)).collect();
    for &ls in &link_sizes {
        size += 1 + varint_size(ls as u64) + ls;
    }

    size += 1 + varint_size(data.len() as u64) + data.len();

    let mut buf = Vec::with_capacity(size);

    for (i, link) in links.iter().enumerate() {
        buf.push(0x12);
        encode_varint(&mut buf, link_sizes[i] as u64);
        encode_pb_link(&mut buf, link);
    }

    buf.push(0x0a);
    encode_varint(&mut buf, data.len() as u64);
    buf.extend_from_slice(data);

    buf
}

fn encode_pb_link(buf: &mut Vec<u8>, link: &PBLink) {
    buf.push(0x0a);
    encode_varint(buf, link.hash.len() as u64);
    buf.extend_from_slice(link.hash);

    if !link.name.is_empty() {
        buf.push(0x12);
        encode_varint(buf, link.name.len() as u64);
        buf.extend_from_slice(link.name.as_bytes());
    } else {
        buf.push(0x12);
        encode_varint(buf, 0);
    }

    buf.push(0x18);
    encode_varint(buf, link.tsize);
}

fn pb_link_size(link: &PBLink) -> usize {
    let mut n = 0usize;

    n += 1 + varint_size(link.hash.len() as u64) + link.hash.len();

    n += 1 + varint_size(link.name.len() as u64) + link.name.len();

    n += 1 + varint_size(link.tsize);

    n
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

fn varint_size(mut value: u64) -> usize {
    let mut n = 1;
    while value >= 0x80 {
        value >>= 7;
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_encoding() {
        let mut buf = Vec::new();
        encode_varint(&mut buf, 0);
        assert_eq!(buf, vec![0x00]);

        buf.clear();
        encode_varint(&mut buf, 1);
        assert_eq!(buf, vec![0x01]);

        buf.clear();
        encode_varint(&mut buf, 127);
        assert_eq!(buf, vec![0x7F]);

        buf.clear();
        encode_varint(&mut buf, 128);
        assert_eq!(buf, vec![0x80, 0x01]);

        buf.clear();
        encode_varint(&mut buf, 300);
        assert_eq!(buf, vec![0xAC, 0x02]);
    }

    #[test]
    fn varint_size_check() {
        assert_eq!(varint_size(0), 1);
        assert_eq!(varint_size(127), 1);
        assert_eq!(varint_size(128), 2);
        assert_eq!(varint_size(300), 2);
        assert_eq!(varint_size(262_144), 3);
    }

    #[test]
    fn unixfs_file_node_smoke() {
        let data = encode_file_node(524_288, &[262_144, 262_144]);
        assert_eq!(data[0], 0x08);
        assert_eq!(data[1], 0x02);
        assert_eq!(data[2], 0x18);
    }
}
