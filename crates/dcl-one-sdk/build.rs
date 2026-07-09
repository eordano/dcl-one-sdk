use std::io::Result;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=proto");
    let proto_files = [
        "proto/decentraland/sdk/development/local_development.proto",
        "proto/decentraland/kernel/comms/rfc5/ws_comms.proto",
    ];
    let mut config = prost_build::Config::new();
    config.compile_protos(&proto_files, &["proto"])?;
    Ok(())
}
