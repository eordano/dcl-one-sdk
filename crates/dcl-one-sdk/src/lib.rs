#![allow(clippy::result_large_err)]

pub mod abgen_embed;
pub mod asset_bundles;
pub mod build;
pub mod comms;
pub mod composite_norm;
pub mod context_files;
pub mod crdt_gen;
pub mod data_layer;
pub mod deploy;
pub mod entity_names;
pub mod entrypoint;
pub mod esbuild;
pub mod host;
pub mod init;
pub mod joinblock;
pub mod jsjson;
pub mod linker;
pub mod live_reload;
pub mod netinfo;
pub mod pack;
pub mod prebuilt;
#[cfg(feature = "rolldown")]
pub mod rolldown_backend;
pub mod scene;
pub mod schema_crdt;
pub mod skills;
pub mod split;
pub mod start;
pub mod tunnel;
pub mod ux;
pub mod watch;
pub mod workspace;
pub mod world;
pub mod world_linker;

#[cfg(test)]
pub(crate) fn random_test_wallet() -> catalyrst_crypto::Wallet {
    loop {
        let bytes: [u8; 32] = rand::random();
        if let Ok(w) = catalyrst_crypto::Wallet::from_hex(&hex::encode(bytes)) {
            return w;
        }
    }
}
