//! Byte packing and digest math shared by every hand-rolled EIP-712 encoder in
//! the workspace. Type strings, field order and input parsing stay at the call
//! site: they are per-contract wire facts, and two callers of the same contract
//! do not always agree on them (see the tests).

use alloy_primitives::{keccak256, Address, U256};

const DOMAIN_TYPE_CHAIN_CONTRACT: &str =
    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";

const DOMAIN_TYPE_CONTRACT_SALT: &str =
    "EIP712Domain(string name,string version,address verifyingContract,bytes32 salt)";

pub fn word_address(address: Address) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[12..].copy_from_slice(address.as_slice());
    word
}

pub fn word_u256(value: U256) -> [u8; 32] {
    value.to_be_bytes::<32>()
}

pub fn word_u64(value: u64) -> [u8; 32] {
    U256::from(value).to_be_bytes::<32>()
}

pub fn hash_dynamic(bytes: &[u8]) -> [u8; 32] {
    keccak256(bytes).0
}

pub fn hash_array_of_structs(members: &[[u8; 32]]) -> [u8; 32] {
    let mut cat = Vec::with_capacity(members.len() * 32);
    for member in members {
        cat.extend_from_slice(member);
    }
    keccak256(&cat).0
}

pub fn struct_hash(type_hash: [u8; 32], fields: &[[u8; 32]]) -> [u8; 32] {
    let mut enc = Vec::with_capacity((fields.len() + 1) * 32);
    enc.extend_from_slice(&type_hash);
    for field in fields {
        enc.extend_from_slice(field);
    }
    keccak256(&enc).0
}

pub fn domain_separator(
    name: &str,
    version: &str,
    chain_id: u64,
    verifying_contract: Address,
) -> [u8; 32] {
    struct_hash(
        hash_dynamic(DOMAIN_TYPE_CHAIN_CONTRACT.as_bytes()),
        &[
            hash_dynamic(name.as_bytes()),
            hash_dynamic(version.as_bytes()),
            word_u64(chain_id),
            word_address(verifying_contract),
        ],
    )
}

pub fn domain_separator_salted(
    name: &str,
    version: &str,
    verifying_contract: Address,
    salt: [u8; 32],
) -> [u8; 32] {
    struct_hash(
        hash_dynamic(DOMAIN_TYPE_CONTRACT_SALT.as_bytes()),
        &[
            hash_dynamic(name.as_bytes()),
            hash_dynamic(version.as_bytes()),
            word_address(verifying_contract),
            salt,
        ],
    )
}

pub fn typed_data_digest(domain_separator: [u8; 32], struct_hash: [u8; 32]) -> [u8; 32] {
    let mut msg = Vec::with_capacity(2 + 64);
    msg.extend_from_slice(&[0x19, 0x01]);
    msg.extend_from_slice(&domain_separator);
    msg.extend_from_slice(&struct_hash);
    keccak256(&msg).0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recover::recover_address_from_digest;

    const TRADE_TYPE: &str = concat!(
        "Trade(Checks checks,AssetWithoutBeneficiary[] sent,Asset[] received)",
        "Asset(uint256 assetType,address contractAddress,uint256 value,bytes extra,address beneficiary)",
        "AssetWithoutBeneficiary(uint256 assetType,address contractAddress,uint256 value,bytes extra)",
        "Checks(uint256 uses,uint256 expiration,uint256 effective,bytes32 salt,uint256 contractSignatureIndex,uint256 signerSignatureIndex,bytes32 allowedRoot,ExternalCheck[] externalChecks)",
        "ExternalCheck(address contractAddress,bytes4 selector,bytes value,bool required)",
    );

    const CHECKS_TYPE_BARE: &str = "Checks(uint256 uses,uint256 expiration,uint256 effective,bytes32 salt,uint256 contractSignatureIndex,uint256 signerSignatureIndex,bytes32 allowedRoot,ExternalCheck[] externalChecks)";

    const CHECKS_TYPE_WITH_REFERENCED: &str = concat!(
        "Checks(uint256 uses,uint256 expiration,uint256 effective,bytes32 salt,uint256 contractSignatureIndex,uint256 signerSignatureIndex,bytes32 allowedRoot,ExternalCheck[] externalChecks)",
        "ExternalCheck(address contractAddress,bytes4 selector,bytes value,bool required)",
    );

    const ASSET_TYPE: &str =
        "Asset(uint256 assetType,address contractAddress,uint256 value,bytes extra,address beneficiary)";

    const ASSET_WITHOUT_BENEFICIARY_TYPE: &str =
        "AssetWithoutBeneficiary(uint256 assetType,address contractAddress,uint256 value,bytes extra)";

    const MARKETPLACE_NAME: &str = "DecentralandMarketplacePolygon";
    const MARKETPLACE_VERSION: &str = "1.0.0";

    fn addr(value: &str) -> Address {
        value.parse().expect("address")
    }

    fn left_padded(bytes: &[u8]) -> [u8; 32] {
        let mut word = [0u8; 32];
        word[32 - bytes.len()..].copy_from_slice(bytes);
        word
    }

    fn trade_struct_hash(
        checks_type: &str,
        uses: u64,
        expiration_secs: u64,
        effective_secs: u64,
        salt: [u8; 32],
        sent: [u8; 32],
        received: [u8; 32],
    ) -> [u8; 32] {
        let checks = struct_hash(
            hash_dynamic(checks_type.as_bytes()),
            &[
                word_u64(uses),
                word_u64(expiration_secs),
                word_u64(effective_secs),
                salt,
                word_u64(0),
                word_u64(0),
                [0u8; 32],
                hash_array_of_structs(&[]),
            ],
        );
        struct_hash(
            hash_dynamic(TRADE_TYPE.as_bytes()),
            &[checks, sent, received],
        )
    }

    #[test]
    fn economy_mainnet_trade_digest_and_signer() {
        let sent = hash_array_of_structs(&[struct_hash(
            hash_dynamic(ASSET_WITHOUT_BENEFICIARY_TYPE.as_bytes()),
            &[
                word_u64(3),
                word_address(addr("0xe9e86941b23fbe9d8f4dd0c5b7e5f89722936878")),
                word_u64(283),
                hash_dynamic(&[]),
            ],
        )]);
        let received = hash_array_of_structs(&[struct_hash(
            hash_dynamic(ASSET_TYPE.as_bytes()),
            &[
                word_u64(1),
                word_address(addr("0xa1c57f48f0deb89f569dfbe6e2b7f46d33606fd4")),
                word_u256(U256::from(1_000_000_000_000_000_000u64)),
                hash_dynamic(&[]),
                word_address(addr("0x02d0bb59a5f04a12d883751dc1605e15b4959b7e")),
            ],
        )]);
        let trade = trade_struct_hash(
            CHECKS_TYPE_WITH_REFERENCED,
            1,
            1_798_783_200,
            1_733_927_535,
            left_padded(&[0x19, 0x9a, 0x40, 0x82, 0xc5]),
            sent,
            received,
        );
        let domain = domain_separator_salted(
            MARKETPLACE_NAME,
            MARKETPLACE_VERSION,
            addr("0x540fb08eDb56AaE562864B390542C97F562825BA"),
            word_u64(137),
        );
        let digest = typed_data_digest(domain, trade);

        assert_eq!(
            hex::encode(digest),
            "d4d6a86e2a1f0ab327b88353ef9cbd59ddde578a73fdcc176d8b07564c6f7718"
        );
        assert_eq!(
            recover_address_from_digest(
                &digest,
                "0x2860a680deb41ba57ee26d6972c21d49d6cca25c74613ca04b9ed15d48a154f205fd3554d71836277e9d3f0143a62afad6f5c1636a7cd3d8f691dc4b0d8ccd011b",
            )
            .expect("recovers"),
            "0x02d0bb59a5f04a12d883751dc1605e15b4959b7e"
        );
    }

    // The market encoder hashes `Checks` WITHOUT the referenced `ExternalCheck`
    // type appended, so it does not agree with the mainnet-anchored vector
    // above; this vector pins its bytes as they are today.
    #[test]
    fn market_fixture_trade_digest() {
        let sent = hash_array_of_structs(&[struct_hash(
            hash_dynamic(ASSET_WITHOUT_BENEFICIARY_TYPE.as_bytes()),
            &[
                word_u64(3),
                word_address(addr("0x1111111111111111111111111111111111111111")),
                word_u64(42),
                hash_dynamic(&[]),
            ],
        )]);
        let received = hash_array_of_structs(&[struct_hash(
            hash_dynamic(ASSET_TYPE.as_bytes()),
            &[
                word_u64(1),
                word_address(addr("0x2222222222222222222222222222222222222222")),
                word_u64(1000),
                hash_dynamic(&[]),
                word_address(addr("0x3333333333333333333333333333333333333333")),
            ],
        )]);
        let salt = left_padded(&[0x12, 0x34]);
        let domain = domain_separator_salted(
            MARKETPLACE_NAME,
            MARKETPLACE_VERSION,
            addr("0xa40b1d129b8906888720686f3a01921ddf37716f"),
            word_u64(137),
        );
        let digest = typed_data_digest(
            domain,
            trade_struct_hash(CHECKS_TYPE_BARE, 1, 4_102_444_800, 0, salt, sent, received),
        );
        assert_eq!(
            hex::encode(digest),
            "dc0df04c7e569fc389d3ac0e83953c19bd3b67a0846ab91e7051028da2b2dba9"
        );

        let with_contract_checks_type = typed_data_digest(
            domain,
            trade_struct_hash(
                CHECKS_TYPE_WITH_REFERENCED,
                1,
                4_102_444_800,
                0,
                salt,
                sent,
                received,
            ),
        );
        assert_ne!(digest, with_contract_checks_type);
    }

    #[test]
    fn signatures_rentals_listing_digest_and_signer() {
        const LISTING_TYPE: &str = "Listing(address signer,address contractAddress,uint256 tokenId,uint256 expiration,uint256[3] indexes,uint256[] pricePerDay,uint256[] maxDays,uint256[] minDays,address target)";

        let listing = struct_hash(
            hash_dynamic(LISTING_TYPE.as_bytes()),
            &[
                word_address(addr("0x19e7e376e7c213b7e7e7e46cc70a5dd086daff2a")),
                word_address(addr("0xf87e31492faf9a91b02ee0deaad50d51d56d5d4d")),
                word_u64(42),
                word_u64(1_893_456_000),
                hash_array_of_structs(&[word_u64(0), word_u64(0), word_u64(0)]),
                hash_array_of_structs(&[word_u256(U256::from(1_000_000_000_000_000_000u64))]),
                hash_array_of_structs(&[word_u64(30)]),
                hash_array_of_structs(&[word_u64(1)]),
                word_address(Address::ZERO),
            ],
        );
        let domain = domain_separator(
            "Rentals",
            "1",
            1,
            addr("0x3a1469499d0be105d4f77045ca403a5f6dc2f3f5"),
        );
        let digest = typed_data_digest(domain, listing);

        assert_eq!(
            hex::encode(digest),
            "d6c00455cf6c7c140ed004ba43dcb049b121ea69c690043c69431815343cbc82"
        );
        assert_eq!(
            recover_address_from_digest(
                &digest,
                "0xab333677b5572585e44bc94d70e60eef3468f9c08c896f44deb920a00599be71531f4a7bbbcff3d06de00a0c399ed09eeb342fb80a2eb898e887568c54ae20071c",
            )
            .expect("recovers"),
            "0x19e7e376e7c213b7e7e7e46cc70a5dd086daff2a"
        );
    }

    #[test]
    fn governance_snapshot_proposal_digest() {
        const PROPOSAL_TYPE: &str = "Proposal(address from,string space,uint64 timestamp,string type,string title,string body,string discussion,string[] choices,uint64 start,uint64 end,uint64 snapshot,string plugins,string app)";
        const BODY: &str = "> by 0x1111111111111111111111111111111111111111\n\nShould the catalyst node with the domain peer.example.org and owner 0x3333333333333333333333333333333333333333 be added to Decentraland's Catalyst Network?\n\n## Description\n\nA new node for the network.";

        let domain = struct_hash(
            hash_dynamic(b"EIP712Domain(string name,string version)"),
            &[hash_dynamic(b"snapshot"), hash_dynamic(b"0.1.4")],
        );
        assert_eq!(
            hex::encode(domain),
            "484fce18f892e8535a4b6700e197a8026f4213f809d23ae117da03b497e18670"
        );

        let proposal = struct_hash(
            hash_dynamic(PROPOSAL_TYPE.as_bytes()),
            &[
                word_address(addr("0x1a642f0E3c3aF545E7AcBD38b07251B3990914F1")),
                hash_dynamic(b"gate.dcl.eth"),
                word_u64(1_700_000_040),
                hash_dynamic(b"single-choice"),
                hash_dynamic(
                    b"Add catalyst node with domain peer.example.org to the catalyst network",
                ),
                hash_dynamic(BODY.as_bytes()),
                hash_dynamic(b""),
                hash_array_of_structs(&[
                    hash_dynamic(b"yes"),
                    hash_dynamic(b"no"),
                    hash_dynamic(b"abstain"),
                ]),
                word_u64(1_700_000_040),
                word_u64(1_700_000_640),
                word_u64(22_000_000),
                hash_dynamic(b"{}"),
                hash_dynamic(b"decentraland-governance"),
            ],
        );
        assert_eq!(
            hex::encode(proposal),
            "b065c4ade9b2bb4675be9e9a99630a04bd43e17b32a00191cd9e5c9b802819c3"
        );
        assert_eq!(
            hex::encode(typed_data_digest(domain, proposal)),
            "9e426671d3aaae26c4c9f72f60a68553900999dcd35f75f090778c90f5c60c25"
        );
    }

    #[test]
    fn credits_purchase_intent_digest_and_signer() {
        const INTENT_TYPE: &str = concat!(
            "PurchaseIntent(address buyer,string items,string totalCredits,",
            "string currency,string nonce,uint256 expiresAt)",
        );
        const ITEMS: &str = r#"[["0x59a90bad9570ecd08895f132daf7b79696337f61","12",2],["0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","3",1]]"#;

        let domain = struct_hash(
            hash_dynamic(b"EIP712Domain(string name,string version,uint256 chainId)"),
            &[
                hash_dynamic(b"dcl.one Checkout"),
                hash_dynamic(b"1"),
                word_u64(137),
            ],
        );
        let intent = struct_hash(
            hash_dynamic(INTENT_TYPE.as_bytes()),
            &[
                word_address(addr("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266")),
                hash_dynamic(ITEMS.as_bytes()),
                hash_dynamic(b"3"),
                hash_dynamic(b"CREDITS"),
                hash_dynamic(b"idem-vector-0001"),
                word_u64(1_767_225_600),
            ],
        );
        let digest = typed_data_digest(domain, intent);

        assert_eq!(
            hex::encode(digest),
            "cc577fb0844b7f8a0163e4daf32481bed2beca29d87c1634aa43b42ed34bca1c"
        );
        assert_eq!(
            recover_address_from_digest(
                &digest,
                "0x29ababcea69bb9464958c8ccd3b34dce8c82c44c52e80ec0c02a49891344d8da6951071404ce24a3975a4111511adf8bd313d3e9ab5072e89299e2f351800ef71b",
            )
            .expect("recovers"),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );
    }

    #[test]
    fn words_pack_left_and_right_of_the_value() {
        assert_eq!(
            hex::encode(word_address(addr(
                "0x1111111111111111111111111111111111111111"
            ))),
            "0000000000000000000000001111111111111111111111111111111111111111"
        );
        assert_eq!(
            hex::encode(word_u64(1)),
            "0000000000000000000000000000000000000000000000000000000000000001"
        );
        assert_eq!(word_u64(7), word_u256(U256::from(7)));
        assert_eq!(
            hex::encode(hash_array_of_structs(&[])),
            hex::encode(hash_dynamic(&[]))
        );
    }
}
