use std::process::Command;

/// The host's `@dcl/sdk/server` client keeps upstream's storage semantics
/// (scripts/host-storage.test.mjs). node is a hard requirement of this tool,
/// so its absence is a skip only here, where the crate's own code is not
/// what is being exercised.
#[test]
fn host_storage_client_keeps_upstream_semantics() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: node is not on PATH");
        return;
    }
    let output = Command::new("node")
        .args(["--test", "scripts/host-storage.test.mjs"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the host storage client tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
