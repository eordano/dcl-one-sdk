use std::io::Result;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=proto");
    let proto_files = [
        "proto/decentraland/sdk/development/local_development.proto",
        "proto/decentraland/kernel/comms/rfc5/ws_comms.proto",
    ];
    let mut config = prost_build::Config::new();
    config.compile_protos(&proto_files, &["proto"])?;
    generate_abgen_embed()?;
    Ok(())
}

fn generate_abgen_embed() -> Result<()> {
    println!("cargo:rerun-if-env-changed=ABGEN_EMBED_BIN");
    let dest =
        PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("abgen_embed_data.rs");
    let bin = std::env::var("ABGEN_EMBED_BIN").unwrap_or_default();
    if bin.is_empty() {
        return std::fs::write(
            &dest,
            "pub static FILES: &[(&str, &[u8])] = &[];\npub const BIN_NAME: &str = \"\";\npub const TAG: &str = \"\";\n",
        );
    }
    let bin_path = PathBuf::from(&bin);
    let bin_name = bin_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = bin_path.parent().map(Path::to_path_buf).unwrap_or_default();
    if !bin_path.is_file() || !dir.join("template").is_dir() || !dir.join("shader").is_dir() {
        panic!(
            "ABGEN_EMBED_BIN={bin} must point at the abgen binary inside an unpacked abgen release archive (template/ and shader/ next to the binary); unset it to build without the embed"
        );
    }
    let mut rels = vec![bin_name.clone()];
    for sub in ["template", "shader"] {
        for entry in std::fs::read_dir(dir.join(sub))?.flatten() {
            if entry.path().is_file() {
                rels.push(format!("{sub}/{}", entry.file_name().to_string_lossy()));
            }
        }
    }
    for entry in std::fs::read_dir(&dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".dll") && entry.path().is_file() {
            rels.push(name);
        }
    }
    rels.sort();
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut code = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for rel in &rels {
        let abs = dir.join(rel);
        println!("cargo:rerun-if-changed={}", abs.display());
        fnv1a64(&mut hash, rel.as_bytes());
        fnv1a64(&mut hash, &std::fs::read(&abs)?);
        code.push_str(&format!(
            "    ({rel:?}, include_bytes!({:?})),\n",
            abs.display().to_string()
        ));
    }
    code.push_str("];\n");
    code.push_str(&format!("pub const BIN_NAME: &str = {bin_name:?};\n"));
    code.push_str(&format!("pub const TAG: &str = \"{hash:016x}\";\n"));
    std::fs::write(&dest, code)
}

fn fnv1a64(hash: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *hash ^= b as u64;
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}
