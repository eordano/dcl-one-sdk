use std::path::Path;

const FIXTURE_DIR_ENV: &str = "CATALYRST_HASHING_FIXTURES";

fn fixture_root() -> std::path::PathBuf {
    let env = std::env::var(FIXTURE_DIR_ENV).unwrap_or_else(|_| {
        panic!(
            "{} not set — point at a checkout of the upstream @dcl/hashing fixtures \
             (test/fixtures/hashes) before running these tests.",
            FIXTURE_DIR_ENV
        )
    });
    std::path::PathBuf::from(env)
}

fn require_fixture(path: &Path) {
    if !path.exists() {
        panic!(
            "fixture not found at {}. Set {} to a directory that contains it.",
            path.display(),
            FIXTURE_DIR_ENV
        );
    }
}

#[test]
#[ignore = "requires CATALYRST_HASHING_FIXTURES env var pointing at upstream @dcl/hashing fixtures"]
fn cidv0_fixture_qm() {
    let path = fixture_root().join("QmSYpJEQLQc82USvtavzxEiBR57nyb5RdMzecBTR3Qg6qn");
    require_fixture(&path);
    let data = std::fs::read(&path).unwrap();
    assert_eq!(data.len(), 444_317);
    let hash = catalyrst_hashing::hash_bytes(&data);
    assert_eq!(hash, "QmSYpJEQLQc82USvtavzxEiBR57nyb5RdMzecBTR3Qg6qn");
}

#[test]
#[ignore = "requires CATALYRST_HASHING_FIXTURES env var pointing at upstream @dcl/hashing fixtures"]
fn cidv1_fixture_bafy() {
    let path = fixture_root().join("bafybeibdik2ihfpcdi7aaaguptwcoc5msav7uhn5hu54xlq2pdwkh5arzy");
    require_fixture(&path);
    let data = std::fs::read(&path).unwrap();
    assert_eq!(data.len(), 1_615_462);
    let hash = catalyrst_hashing::hash_bytes_v1(&data);
    assert_eq!(
        hash,
        "bafybeibdik2ihfpcdi7aaaguptwcoc5msav7uhn5hu54xlq2pdwkh5arzy"
    );
}

#[test]
#[ignore = "requires CATALYRST_HASHING_FIXTURES env var pointing at upstream @dcl/hashing fixtures"]
fn verify_fixture_files() {
    let qm_path = fixture_root().join("QmSYpJEQLQc82USvtavzxEiBR57nyb5RdMzecBTR3Qg6qn");
    let bafy_path =
        fixture_root().join("bafybeibdik2ihfpcdi7aaaguptwcoc5msav7uhn5hu54xlq2pdwkh5arzy");

    require_fixture(&qm_path);
    require_fixture(&bafy_path);

    let qm_data = std::fs::read(&qm_path).unwrap();
    assert!(catalyrst_hashing::verify_hash(
        &qm_data,
        "QmSYpJEQLQc82USvtavzxEiBR57nyb5RdMzecBTR3Qg6qn"
    ));
    assert!(!catalyrst_hashing::verify_hash(
        &qm_data,
        "QmSYpJEQLQc82USvtavzxEiBR57nyb5RdMzecBTR3QgAAAA"
    ));

    let bafy_data = std::fs::read(&bafy_path).unwrap();
    assert!(catalyrst_hashing::verify_hash(
        &bafy_data,
        "bafybeibdik2ihfpcdi7aaaguptwcoc5msav7uhn5hu54xlq2pdwkh5arzy"
    ));
    assert!(!catalyrst_hashing::verify_hash(
        &bafy_data,
        "bafybeibdik2ihfpcdi7aaaguptwcoc5msav7uhn5hu54xlq2pdwkh5aAAA"
    ));
}
