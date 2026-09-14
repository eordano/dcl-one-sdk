use std::process::Command;

#[test]
fn split_loader_decodes_utf8_in_explorer_sandboxes() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: node is not on PATH");
        return;
    }
    let output = Command::new("node")
        .args(["--test", "scripts/loader-utf8.test.mjs"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env_remove("LOADER_TEMPLATE")
        .output()
        .expect("run the split-loader UTF-8 regression tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
