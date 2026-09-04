use crate::scene::Project;
use anyhow::Result;
use std::path::PathBuf;

pub struct EsbuildOptions {
    pub production: bool,
    pub entrypoint: PathBuf,
    pub outfile: PathBuf,
    pub tsconfig: PathBuf,
    pub aliases: Vec<(String, PathBuf)>,
    pub externals: Vec<String>,
}

#[cfg(feature = "rolldown")]
pub async fn bundle(project: &Project, opts: &EsbuildOptions) -> Result<()> {
    crate::rolldown_backend::run(project, opts).await
}

#[cfg(not(feature = "rolldown"))]
pub async fn bundle(_project: &Project, _opts: &EsbuildOptions) -> Result<()> {
    use crate::ux::{TrySteps, UserError};
    Err(UserError::new(
        "this binary was built without the rolldown backend",
        TrySteps::one("rebuild with cargo build -p dcl-one-sdk --features rolldown"),
    )
    .into())
}

/// `@dcl/sdk` is required; the rest alias to whichever of two install
/// locations exists, if either does.
pub fn resolve_aliases(project: &Project) -> Result<Vec<(String, PathBuf)>> {
    let mut aliases = vec![(
        "@dcl/sdk".to_string(),
        project.require_node_module("@dcl/sdk")?,
    )];
    for (name, first, second) in [
        ("@dcl/ecs", "@dcl/sdk/node_modules/@dcl/ecs", "@dcl/ecs"),
        ("react", "react", "@dcl/react-ecs/node_modules/react"),
        (
            "@dcl/asset-packs",
            "@dcl/asset-packs",
            "@dcl/inspector/node_modules/@dcl/asset-packs",
        ),
    ] {
        if let Some(path) = project
            .node_module(first)
            .or_else(|| project.node_module(second))
        {
            aliases.push((name.to_string(), path));
        }
    }
    Ok(aliases)
}
