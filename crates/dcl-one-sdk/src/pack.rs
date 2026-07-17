use crate::build;
use crate::deploy;
use crate::ux::{self, TrySteps, UserError};
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct PackOptions {
    pub dir: PathBuf,
    pub skip_build: bool,
}

pub const MAX_WEARABLE_SIZE_BYTES: u64 = 2_097_152;
pub const ZIP_NAME: &str = "smart-wearable.zip";

const CATEGORIES: [&str; 18] = [
    "eyebrows",
    "eyes",
    "facial_hair",
    "hair",
    "body_shape",
    "mouth",
    "upper_body",
    "lower_body",
    "feet",
    "earring",
    "eyewear",
    "hat",
    "helmet",
    "mask",
    "tiara",
    "top_head",
    "skin",
    "hands_wear",
];

const RARITIES: [&str; 7] = [
    "unique",
    "mythic",
    "legendary",
    "epic",
    "rare",
    "uncommon",
    "common",
];

pub async fn pack(opts: &PackOptions) -> Result<()> {
    if !opts.dir.is_dir() {
        return Err(UserError::new(
            format!("the directory {} does not exist", opts.dir.display()),
            TrySteps::one("check the path passed to --dir"),
        )
        .into());
    }
    let root = dunce::canonicalize(&opts.dir)
        .with_context(|| format!("resolving project dir {}", opts.dir.display()))?;
    if !root.join("wearable.json").is_file() {
        return Err(UserError::new(
            "this project is not a smart wearable",
            TrySteps::one("pack runs on projects that have a wearable.json next to scene.json")
                .and("start one with: dcl-one-sdk init --project smart-wearable"),
        )
        .why(format!("no wearable.json in {}", root.display()))
        .into());
    }
    validate_wearable(&root)?;

    if !opts.skip_build {
        build::build(&build::BuildOptions {
            dir: root.clone(),
            production: true,
            ignore_composite: false,
            custom_entry_point: false,
            skip_type_check: false,
        })
        .await?;
    }

    let rel_paths = deploy::collect_publishable_files(&root)?;
    let mut seen_lower = HashSet::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total: u64 = 0;
    for rel in &rel_paths {
        if !seen_lower.insert(rel.to_lowercase()) {
            return Err(UserError::new(
                format!("the file {rel} collides case-insensitively with another packed file"),
                TrySteps::one("rename one of the colliding files"),
            )
            .into());
        }
        let p = root.join(rel);
        let bytes =
            std::fs::read(&p).with_context(|| format!("reading content file {}", p.display()))?;
        total += bytes.len() as u64;
        files.push((rel.clone(), bytes));
    }
    if files.is_empty() {
        return Err(UserError::new(
            "no publishable files found to pack",
            TrySteps::one("check .dclignore is not excluding everything")
                .and("run without --skip-build so the bundle exists"),
        )
        .into());
    }
    if let Some(warning) = size_warning(total) {
        tracing::warn!("{warning}");
    }

    let zip_path = root.join(ZIP_NAME);
    if zip_path.exists() {
        std::fs::remove_file(&zip_path)
            .with_context(|| format!("removing the previous {}", zip_path.display()))?;
    }
    write_zip(&zip_path, &files)?;

    let mut steps = ux::Steps::new(1);
    ux::note(zip_path.display().to_string());
    steps.done(format!(
        "Smart wearable packed \u{2014} {} files, {} bytes ({ZIP_NAME})",
        files.len(),
        total
    ));
    Ok(())
}

pub fn size_warning(total: u64) -> Option<String> {
    if total <= MAX_WEARABLE_SIZE_BYTES {
        return None;
    }
    Some(format!(
        "the packed files total {total} bytes, above the {MAX_WEARABLE_SIZE_BYTES}-byte (2 MiB) smart-wearable limit \u{2014} the zip is still written, but the builder/worlds server will reject it; shrink assets or exclude files via .dclignore"
    ))
}

pub fn write_zip(zip_path: &Path, files: &[(String, Vec<u8>)]) -> Result<()> {
    let f = std::fs::File::create(zip_path)
        .with_context(|| format!("creating {}", zip_path.display()))?;
    let mut zw = zip::ZipWriter::new(f);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (rel, bytes) in files {
        if rel.is_empty() {
            continue;
        }
        zw.start_file(rel, options)
            .with_context(|| format!("adding {rel} to the zip"))?;
        zw.write_all(bytes)
            .with_context(|| format!("writing {rel} into the zip"))?;
    }
    zw.finish().context("finalizing the zip")?;
    Ok(())
}

pub fn validate_wearable(root: &Path) -> Result<()> {
    let path = root.join("wearable.json");
    let raw =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| {
        anyhow::Error::from(
            UserError::new(
                format!(
                    "wearable.json is not valid JSON (line {}, column {})",
                    e.line(),
                    e.column()
                ),
                TrySteps::one(format!(
                    "fix the syntax error at wearable.json:{}:{}",
                    e.line(),
                    e.column()
                )),
            )
            .caused_by(e),
        )
    })?;
    validate_wearable_value(&v, root)
}

pub fn validate_wearable_value(v: &Value, root: &Path) -> Result<()> {
    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or_default();
    if name.is_empty() {
        return Err(field_error(
            "wearable.json needs a non-empty \"name\"",
            "add \"name\": \"My Wearable\"",
        ));
    }
    let rarity = v.get("rarity").and_then(|r| r.as_str()).unwrap_or_default();
    if !RARITIES.contains(&rarity) {
        return Err(field_error(
            &format!("wearable.json \"rarity\" is \"{rarity}\", which is not a rarity"),
            &format!("use one of: {}", RARITIES.join(", ")),
        ));
    }
    let Some(data) = v.get("data") else {
        return Err(field_error(
            "wearable.json is missing the \"data\" object",
            "add \"data\": { \"category\": ..., \"representations\": [...] }",
        ));
    };
    let category = data
        .get("category")
        .and_then(|c| c.as_str())
        .unwrap_or_default();
    if !CATEGORIES.contains(&category) {
        return Err(field_error(
            &format!(
                "wearable.json data.category is \"{category}\", which is not a wearable category"
            ),
            &format!("use one of: {}", CATEGORIES.join(", ")),
        ));
    }
    let reps = data
        .get("representations")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    if reps.is_empty() {
        return Err(field_error(
            "wearable.json data.representations must be a non-empty array",
            "add one representation with bodyShapes, mainFile and contents",
        ));
    }
    for (i, rep) in reps.iter().enumerate() {
        let body_shapes = rep
            .get("bodyShapes")
            .and_then(|b| b.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if body_shapes == 0 {
            return Err(field_error(
                &format!("representation {i} has no bodyShapes"),
                "list at least one body shape URN (BaseMale / BaseFemale)",
            ));
        }
        let main_file = rep
            .get("mainFile")
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        if main_file.is_empty() {
            return Err(field_error(
                &format!("representation {i} has no mainFile"),
                "set mainFile to the wearable model, e.g. \"model.glb\"",
            ));
        }
        let contents: Vec<String> = rep
            .get("contents")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if contents.is_empty() {
            return Err(field_error(
                &format!("representation {i} has no contents"),
                "list every file the representation ships, including mainFile",
            ));
        }
        if !contents.iter().any(|c| c == main_file) {
            return Err(field_error(
                &format!("representation {i}: mainFile \"{main_file}\" is not listed in contents"),
                "add the mainFile to the contents array",
            ));
        }
        for c in &contents {
            if !root.join(c).is_file() {
                return Err(UserError::new(
                    format!("representation {i} references \"{c}\", which does not exist in the project"),
                    TrySteps::one(format!("add the file {c} to the project"))
                        .and("or fix the contents entry in wearable.json"),
                )
                .into());
            }
        }
    }
    Ok(())
}

fn field_error(what: &str, step: &str) -> anyhow::Error {
    UserError::new(what.to_string(), TrySteps::one(step.to_string())).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "dcl-one-sdk-pack-test-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempTree(dir)
        }

        fn write(&self, rel: &str, contents: &[u8]) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contents).unwrap();
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn valid_wearable() -> Value {
        json!({
            "id": "0f0e0d0c-0b0a-4900-8807-060504030201",
            "name": "Test Glasses",
            "description": "test",
            "rarity": "mythic",
            "data": {
                "replaces": [],
                "hides": [],
                "tags": ["special"],
                "category": "eyewear",
                "representations": [{
                    "bodyShapes": [
                        "urn:decentraland:off-chain:base-avatars:BaseMale",
                        "urn:decentraland:off-chain:base-avatars:BaseFemale"
                    ],
                    "mainFile": "model.glb",
                    "contents": ["model.glb"],
                    "overrideHides": [],
                    "overrideReplaces": []
                }]
            }
        })
    }

    fn wearable_fixture(tag: &str) -> TempTree {
        let t = TempTree::new(tag);
        t.write(
            "wearable.json",
            serde_json::to_string_pretty(&valid_wearable())
                .unwrap()
                .as_bytes(),
        );
        t.write(
            "scene.json",
            b"{\"runtimeVersion\":\"7\",\"main\":\"bin/game.js\",\"scene\":{\"parcels\":[\"0,0\"],\"base\":\"0,0\"}}",
        );
        t.write("model.glb", b"GLBFIXTURE");
        t.write("thumbnail.png", b"PNGFIXTURE");
        t.write("bin/game.js", b"console.log(1);\n");
        t.write("src/game.ts", b"const x = 1;\n");
        t.write("README.md", b"docs");
        t
    }

    #[test]
    fn zip_golden_layout_flat_project_relative_glob9_order() {
        let t = wearable_fixture("golden");
        t.write(ZIP_NAME, b"stale zip to be replaced");
        let rel_paths = deploy::collect_publishable_files(&t.0).unwrap();
        let files: Vec<(String, Vec<u8>)> = rel_paths
            .iter()
            .map(|r| (r.clone(), std::fs::read(t.0.join(r)).unwrap()))
            .collect();
        let zip_path = t.0.join(ZIP_NAME);
        std::fs::remove_file(&zip_path).unwrap();
        write_zip(&zip_path, &files).unwrap();

        let f = std::fs::File::open(&zip_path).unwrap();
        let mut ar = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..ar.len())
            .map(|i| ar.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "wearable.json",
                "thumbnail.png",
                "scene.json",
                "model.glb",
                "bin/game.js"
            ]
        );
        let mut model = ar.by_name("model.glb").unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut model, &mut bytes).unwrap();
        assert_eq!(bytes, b"GLBFIXTURE");
    }

    #[test]
    fn size_cap_is_a_warning_not_an_error() {
        assert_eq!(size_warning(MAX_WEARABLE_SIZE_BYTES), None);
        let w = size_warning(MAX_WEARABLE_SIZE_BYTES + 1).unwrap();
        assert!(w.contains("2097152"));
        assert!(w.contains(".dclignore"));
    }

    #[test]
    fn wearable_validation_accepts_the_scaffold_shape() {
        let t = wearable_fixture("valid");
        assert!(validate_wearable(&t.0).is_ok());
    }

    #[test]
    fn wearable_validation_names_the_broken_field() {
        let t = wearable_fixture("broken");

        let mut bad = valid_wearable();
        bad["rarity"] = json!("shiny");
        let err = validate_wearable_value(&bad, &t.0).unwrap_err();
        assert!(err.to_string().contains("shiny"));

        let mut bad = valid_wearable();
        bad["data"]["category"] = json!("sunglasses");
        let err = validate_wearable_value(&bad, &t.0).unwrap_err();
        assert!(err.to_string().contains("sunglasses"));

        let mut bad = valid_wearable();
        bad["data"]["representations"] = json!([]);
        assert!(validate_wearable_value(&bad, &t.0).is_err());

        let mut bad = valid_wearable();
        bad["data"]["representations"][0]["contents"] = json!(["missing.glb"]);
        let err = validate_wearable_value(&bad, &t.0).unwrap_err();
        assert!(err.to_string().contains("mainFile"));

        let mut bad = valid_wearable();
        bad["data"]["representations"][0]["mainFile"] = json!("missing.glb");
        bad["data"]["representations"][0]["contents"] = json!(["missing.glb"]);
        let err = validate_wearable_value(&bad, &t.0).unwrap_err();
        assert!(err.to_string().contains("missing.glb"));
    }
}
