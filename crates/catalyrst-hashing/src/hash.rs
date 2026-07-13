use sha2::{Digest, Sha256};

use crate::cid::{cid_v0_to_string, cid_v1_to_string, encode_cid_v1};
use crate::unixfs;

const CHUNK_SIZE: usize = 262_144;

const MAX_CHILDREN: usize = 174;

pub fn hash_bytes(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    cid_v0_to_string(&digest)
}

pub fn hash_bytes_v1(data: &[u8]) -> String {
    if data.len() <= CHUNK_SIZE {
        let digest = Sha256::digest(data);
        cid_v1_to_string(0x55, &digest)
    } else {
        hash_chunked(data)
    }
}

struct TreeNode {
    cid_bytes: Vec<u8>,
    file_size: u64,
    tsize: u64,
}

fn hash_chunked(data: &[u8]) -> String {
    let leaves: Vec<TreeNode> = data
        .chunks(CHUNK_SIZE)
        .map(|chunk| {
            let digest = Sha256::digest(chunk);
            let cid_bytes = encode_cid_v1(0x55, &digest);
            TreeNode {
                cid_bytes,
                file_size: chunk.len() as u64,
                tsize: chunk.len() as u64,
            }
        })
        .collect();

    let root = balanced_reduce(leaves);

    cid_v1_to_string_from_bytes(&root.cid_bytes)
}

fn balanced_reduce(mut nodes: Vec<TreeNode>) -> TreeNode {
    while nodes.len() > 1 {
        let mut parents = Vec::new();
        for batch in nodes.chunks(MAX_CHILDREN) {
            parents.push(build_interior_node(batch));
        }
        nodes = parents;
    }
    nodes.into_iter().next().expect("non-empty input")
}

fn build_interior_node(children: &[TreeNode]) -> TreeNode {
    let block_sizes: Vec<u64> = children.iter().map(|c| c.file_size).collect();
    let total_file_size: u64 = block_sizes.iter().sum();

    let unixfs_data = unixfs::encode_file_node(total_file_size, &block_sizes);

    let links: Vec<unixfs::PBLink> = children
        .iter()
        .map(|c| unixfs::PBLink {
            hash: &c.cid_bytes,
            name: "",
            tsize: c.tsize,
        })
        .collect();
    let pb_node = unixfs::encode_pb_node(&unixfs_data, &links);

    let digest = Sha256::digest(&pb_node);
    let cid_bytes = encode_cid_v1(0x70, &digest);

    let children_tsize_sum: u64 = children.iter().map(|c| c.tsize).sum();
    let tsize = pb_node.len() as u64 + children_tsize_sum;

    TreeNode {
        cid_bytes,
        file_size: total_file_size,
        tsize,
    }
}

fn cid_v1_to_string_from_bytes(cid_bytes: &[u8]) -> String {
    crate::cid::multibase_base32lower(cid_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidv0_empty() {
        let hash = hash_bytes(b"");
        assert!(
            hash.starts_with("Qm"),
            "CIDv0 should start with Qm, got {hash}"
        );
    }

    #[test]
    fn cidv1_empty() {
        let hash = hash_bytes_v1(b"");
        assert!(
            hash.starts_with("bafkrei"),
            "CIDv1 raw leaf should start with bafkrei, got {hash}"
        );
        assert_eq!(
            hash,
            "bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku"
        );
    }

    #[test]
    fn cidv0_known_vector() {
        let hash = hash_bytes(b"");
        assert_eq!(hash, "QmdfTbBqBPQ7VNxZEYEj14VmRuZBkqFbiwReogJgS1zR1n");
    }

    #[test]
    fn cidv1_small_data() {
        let hash = hash_bytes_v1(b"hello world");
        assert_eq!(
            hash,
            "bafkreifzjut3te2nhyekklss27nh3k72ysco7y32koao5eei66wof36n5e"
        );
    }

    #[test]
    fn cidv0_hello_world() {
        let hash = hash_bytes(b"hello world");
        assert_eq!(hash, "QmaozNR7DZHQK1ZcU9p7QdrshMvXqWK6gpu5rmrkPdT3L4");
    }

    #[test]
    fn roundtrip_small() {
        let data = b"some entity content";
        let v0 = hash_bytes(data);
        let v1 = hash_bytes_v1(data);
        assert!(v0.starts_with("Qm"));
        assert!(v1.starts_with("bafkrei"));
        assert_ne!(v0, v1);
    }

    #[test]
    fn cidv1_multi_chunk_produces_dagpb_cid() {
        let data = vec![0x42u8; 300_000];
        let hash = hash_bytes_v1(&data);
        assert!(
            hash.starts_with("bafy"),
            "Multi-chunk CIDv1 should start with 'bafy' (dag-pb), got {hash}"
        );
        assert!(
            !hash.starts_with("bafkrei"),
            "Multi-chunk CIDv1 must NOT be a raw leaf (bafkrei), got {hash}"
        );
        assert_eq!(hash, hash_bytes_v1(&data));
    }

    #[test]
    fn cidv1_empty_known_value() {
        let hash = hash_bytes_v1(b"");
        assert_eq!(
            hash, "bafkreihdwdcefgh4dqkjv67uzcmw7ojee6xedzdetojuzjevtenxquvyku",
            "Empty file CIDv1 does not match expected value"
        );
    }

    #[test]
    fn cidv1_boundary_at_chunk_size() {
        let data = vec![0xAAu8; CHUNK_SIZE];
        let hash = hash_bytes_v1(&data);
        assert!(
            hash.starts_with("bafkrei"),
            "Exactly CHUNK_SIZE bytes should be a single raw leaf, got {hash}"
        );

        let data_plus_one = vec![0xAAu8; CHUNK_SIZE + 1];
        let hash_chunked = hash_bytes_v1(&data_plus_one);
        assert!(
            hash_chunked.starts_with("bafy"),
            "CHUNK_SIZE+1 bytes should be dag-pb (chunked), got {hash_chunked}"
        );
    }
}

#[cfg(test)]
mod multilevel_tests {
    use super::*;

    #[test]
    fn cidv1_multi_level_dag_golden() {
        let n = 175 * CHUNK_SIZE + 7;
        let data: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        let hash = hash_bytes_v1(&data);
        assert!(
            hash.starts_with("bafy"),
            "175 chunks must be dag-pb, got {hash}"
        );
        assert_eq!(
            hash, "bafybeihh7afuh5inawukv67gg6vxlpvb3zgw6rkpw7tymous2idoydpxpi",
            "multi-level (>{MAX_CHILDREN} chunks) UnixFS CID regressed; interior nodes must use \
             children file_size for blocksizes/filesize, not DAG tsize"
        );
    }

    #[test]
    fn single_vs_multi_level_boundary() {
        let one_level = vec![7u8; MAX_CHILDREN * CHUNK_SIZE];
        let two_level = vec![7u8; (MAX_CHILDREN + 1) * CHUNK_SIZE];
        assert!(hash_bytes_v1(&one_level).starts_with("bafy"));
        assert!(hash_bytes_v1(&two_level).starts_with("bafy"));
        assert_ne!(hash_bytes_v1(&one_level), hash_bytes_v1(&two_level));
    }
}
