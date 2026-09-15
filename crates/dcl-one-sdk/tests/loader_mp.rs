use std::process::Command;

/// The split loader's authoritative-multiplayer arming, exercised in a node
/// vm the way the engine's sandbox evaluates it (scripts/loader-mp.test.mjs).
#[test]
fn split_loader_arms_the_authority_relabel_only_with_the_flag() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: node is not on PATH");
        return;
    }
    let output = Command::new("node")
        .args(["--test", "scripts/loader-mp.test.mjs"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("LOADER_TEMPLATE")
        .output()
        .expect("run the split-loader multiplayer tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
