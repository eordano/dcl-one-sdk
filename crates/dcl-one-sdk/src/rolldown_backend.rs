use crate::esbuild::EsbuildOptions;
use crate::scene::Project;
use crate::ux::{TrySteps, UserError};
use anyhow::{anyhow, Result};
use rolldown::Bundler;
use rolldown_common::{
    BundlerOptions, BundlerTransformOptions, ChecksOptions, CodeSplittingMode, Either, InputItem,
    IsExternal, OutputFormat, Platform, RawMinifyOptions, ResolveOptions, SourceMapType, TsConfig,
};
use rolldown_utils::indexmap::FxIndexMap;
use rolldown_utils::pattern_filter::StringOrRegex;
use std::path::PathBuf;

pub async fn run(project: &Project, opts: &EsbuildOptions) -> Result<()> {
    let mut bundler = Bundler::new(bundler_options(project, opts)?).map_err(|e| {
        anyhow::Error::from(
            UserError::new(
                "the rolldown backend failed to initialize",
                TrySteps::one("re-run with --verbose for the backend report"),
            )
            .why(format!("{e:?}")),
        )
    })?;
    let output = bundler.write().await.map_err(|e| {
        anyhow::Error::from(
            UserError::new(
                "build failed \u{2014} rolldown reported errors",
                TrySteps::one("fix the errors above"),
            )
            .why(format!("{e}")),
        )
    })?;
    for warning in &output.warnings {
        let kind = warning.kind().to_string();
        if kind == "INVALID_ANNOTATION" || kind == "IMPORT_IS_UNDEFINED" {
            continue;
        }
        tracing::warn!("rolldown {kind}: {warning}");
    }
    Ok(())
}

fn bundler_options(project: &Project, opts: &EsbuildOptions) -> Result<BundlerOptions> {
    Ok(BundlerOptions {
        input: Some(vec![InputItem {
            name: Some("index".into()),
            import: opts.entrypoint.display().to_string(),
        }]),
        cwd: Some(project.root.clone()),
        file: Some(opts.outfile.display().to_string()),
        code_splitting: Some(CodeSplittingMode::Bool(false)),
        format: Some(OutputFormat::Cjs),
        platform: Some(Platform::Browser),
        external: Some(externals(&opts.externals)?),
        define: Some(defines(opts.production)),
        resolve: Some(ResolveOptions {
            alias: Some(aliases(&opts.aliases)),
            ..Default::default()
        }),
        tsconfig: Some(TsConfig::Manual(opts.tsconfig.clone())),
        transform: Some(BundlerTransformOptions {
            target: Some(Either::Left("es2020".to_string())),
            ..Default::default()
        }),
        checks: Some(ChecksOptions {
            invalid_annotation: Some(false),
            import_is_undefined: Some(false),
            ..Default::default()
        }),
        minify: Some(RawMinifyOptions::Bool(opts.production)),
        sourcemap: if opts.production {
            None
        } else {
            Some(SourceMapType::Inline)
        },
        ..Default::default()
    })
}

fn aliases(list: &[(String, PathBuf)]) -> Vec<(String, Vec<Option<String>>)> {
    list.iter()
        .map(|(name, path)| (name.clone(), vec![Some(path.display().to_string())]))
        .collect()
}

fn externals(extra: &[String]) -> Result<IsExternal> {
    ["~system/*", "@dcl/inspector", "@dcl/inspector/*"]
        .into_iter()
        .chain(extra.iter().map(String::as_str))
        .map(external_pattern)
        .collect::<Result<Vec<_>>>()
        .map(IsExternal::StringOrRegex)
}

fn external_pattern(pattern: &str) -> Result<StringOrRegex> {
    if !pattern.contains('*') {
        return Ok(StringOrRegex::String(pattern.to_string()));
    }
    let mut rx = String::from("^");
    for c in pattern.chars() {
        match c {
            '*' => rx.push_str(".*"),
            c if "\\^$.|?+()[]{}".contains(c) => {
                rx.push('\\');
                rx.push(c);
            }
            c => rx.push(c),
        }
    }
    rx.push('$');
    StringOrRegex::new(rx, Some(&String::new()))
        .map_err(|e| anyhow!("external pattern {pattern}: {e}"))
}

fn defines(production: bool) -> FxIndexMap<String, String> {
    let (debug, env) = match production {
        true => ("false", "\"production\""),
        false => ("true", "\"development\""),
    };
    [
        ("document", "undefined"),
        ("window", "undefined"),
        ("DEBUG", debug),
        ("globalThis.DEBUG", debug),
        ("process.env.NODE_ENV", env),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}
