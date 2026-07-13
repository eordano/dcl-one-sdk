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

pub fn resolve_aliases(project: &Project) -> Result<Vec<(String, PathBuf)>> {
    let mut aliases = Vec::new();
    let sdk = project.require_node_module("@dcl/sdk")?;
    aliases.push(("@dcl/sdk".to_string(), sdk));
    if let Some(ecs) = project
        .node_module("@dcl/sdk/node_modules/@dcl/ecs")
        .or_else(|| project.node_module("@dcl/ecs"))
    {
        aliases.push(("@dcl/ecs".to_string(), ecs));
    }
    if let Some(react) = project
        .node_module("react")
        .or_else(|| project.node_module("@dcl/react-ecs/node_modules/react"))
    {
        aliases.push(("react".to_string(), react));
    }
    if let Some(ap) = project
        .node_module("@dcl/asset-packs")
        .or_else(|| project.node_module("@dcl/inspector/node_modules/@dcl/asset-packs"))
    {
        aliases.push(("@dcl/asset-packs".to_string(), ap));
    }
    Ok(aliases)
}
