use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_dcl-one-sdk");

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("dcl-one-sdk-initgate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Fixture(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn dir_arg(&self) -> String {
        self.0.display().to_string()
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.0.join(rel)).unwrap_or_else(|e| panic!("reading {rel}: {e}"))
    }

    fn json(&self, rel: &str) -> serde_json::Value {
        serde_json::from_str(&self.read(rel)).unwrap_or_else(|e| panic!("parsing {rel}: {e}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    let mut cmd = Command::new(BIN);
    cmd.args(args).stdin(Stdio::null());
    for k in ["RUST_LOG", "NO_COLOR"] {
        cmd.env_remove(k);
    }
    cmd.output().unwrap()
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const SCENE_FILES: &[&str] = &[
    "scene.json",
    "package.json",
    "tsconfig.json",
    "src/index.ts",
    ".gitignore",
    ".dclignore",
    "README.md",
    "images/scene-thumbnail.png",
];

#[test]
fn scene_scaffold_is_complete_and_loadable() {
    let f = Fixture::new("scene");
    let dir = f.dir_arg();
    let out = run(&["init", "--dir", &dir]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("Scaffolded a scene project"), "{stdout}");
    assert!(stdout.contains("default scene project"), "{stdout}");
    assert!(!stdout.contains("npm install"), "{stdout}");
    assert!(
        stdout.contains("Installed node_modules from the vendored SDK"),
        "{stdout}"
    );
    assert!(stdout.contains("dcl-one-sdk start"), "{stdout}");
    for rel in SCENE_FILES {
        assert!(f.path().join(rel).is_file(), "missing {rel}");
    }
    let scene = f.json("scene.json");
    assert_eq!(scene["runtimeVersion"], "7");
    assert_eq!(scene["main"], "bin/index.js");
    assert_eq!(scene["scene"]["parcels"][0], "0,0");
    let title = scene["display"]["title"].as_str().unwrap();
    assert!(title.starts_with("dcl-one-sdk-initgate-scene"), "{title}");
    let pkg = f.json("package.json");
    for (name, version) in pkg["devDependencies"].as_object().unwrap() {
        let v = version.as_str().unwrap();
        assert_ne!(v, "latest", "{name} is unpinned");
        assert!(
            v.chars().next().unwrap().is_ascii_digit(),
            "{name}={v} is not an exact pin"
        );
    }
    for script in ["start", "build", "deploy"] {
        assert!(
            pkg["scripts"][script].is_string(),
            "missing script {script}"
        );
    }
    assert!(f
        .path()
        .join("node_modules/@dcl/sdk/package.json")
        .is_file());
    assert!(f
        .path()
        .join("node_modules/typescript/lib/tsc.js")
        .is_file());
    assert!(f.read(".dclignore").lines().any(|l| l == "node_modules"));
    assert!(f.read(".gitignore").lines().any(|l| l == "bin/"));
    let png = std::fs::read(f.path().join("images/scene-thumbnail.png")).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let project = dcl_one_sdk::scene::Project::load(f.path()).expect("scaffold must load");
    assert_eq!(project.main_output().unwrap(), "bin/index.js");
    assert_eq!(project.parcels(), vec!["0,0".to_string()]);
    assert!(dcl_one_sdk::scene::min_cli_warning(f.path()).is_none());
}

#[test]
fn refuses_a_non_empty_directory_with_next_steps() {
    let f = Fixture::new("nonempty");
    std::fs::write(f.path().join("existing.txt"), "hi").unwrap();
    let dir = f.dir_arg();
    let out = run(&["init", "--dir", &dir]);
    assert!(!out.status.success());
    let err = stderr_of(&out);
    let error_lines: Vec<&str> = err.lines().filter(|l| l.starts_with("Error: ")).collect();
    assert_eq!(error_lines.len(), 1, "stderr: {err}");
    assert!(error_lines[0].contains("not empty"), "stderr: {err}");
    assert!(err.contains("\u{2192} try: "), "stderr: {err}");
    assert!(err.contains("--yes"), "stderr: {err}");
    assert!(!err.contains('\u{1b}'), "ANSI leaked: {err}");
    assert!(!f.path().join("scene.json").exists());
}

#[test]
fn yes_scaffolds_into_a_non_empty_directory() {
    let f = Fixture::new("yes");
    std::fs::write(f.path().join("existing.txt"), "hi").unwrap();
    let dir = f.dir_arg();
    let out = run(&["init", "--dir", &dir, "--yes"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(f.path().join("scene.json").is_file());
    assert_eq!(f.read("existing.txt"), "hi");
}

#[test]
fn smart_wearable_scaffold_has_wearable_skeleton() {
    let f = Fixture::new("sw");
    let dir = f.dir_arg();
    let out = run(&["init", "--dir", &dir, "--project", "smart-wearable"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("Scaffolded a smart wearable project"),
        "{stdout}"
    );
    assert!(stdout.contains("thumbnail.png"), "{stdout}");
    let wearable = f.json("wearable.json");
    let id = wearable["id"].as_str().unwrap();
    assert_eq!(id.len(), 36);
    assert_eq!(id.as_bytes()[14], b'4');
    assert_eq!(wearable["data"]["category"], "eyewear");
    assert_eq!(wearable["rarity"], "mythic");
    let rep = &wearable["data"]["representations"][0];
    assert_eq!(rep["mainFile"], "model.glb");
    assert_eq!(rep["contents"][0], "model.glb");
    assert_eq!(rep["bodyShapes"].as_array().unwrap().len(), 2);
    let scene = f.json("scene.json");
    assert_eq!(scene["isPortableExperience"], true);
    assert_eq!(scene["runtimeVersion"], "7");
    assert_eq!(scene["scene"]["parcels"].as_array().unwrap().len(), 100);
    assert_eq!(scene["scene"]["base"], "0,0");
    let pkg = f.json("package.json");
    assert!(pkg["scripts"]["pack"].is_string());
    assert!(f.read("README.md").contains("model.glb"));
    let second = Fixture::new("sw2");
    let dir2 = second.dir_arg();
    let out2 = run(&["init", "--dir", &dir2, "--project", "smart-wearable"]);
    assert!(out2.status.success());
    assert_ne!(second.json("wearable.json")["id"], wearable["id"]);
}

#[test]
fn init_creates_a_missing_target_directory() {
    let f = Fixture::new("mkdir");
    let nested = f.path().join("brand/new/scene");
    let dir = nested.display().to_string();
    let out = run(&["init", "--dir", &dir]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(nested.join("scene.json").is_file());
    let scene: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(nested.join("scene.json")).unwrap()).unwrap();
    assert_eq!(scene["display"]["title"], "scene");
}

#[test]
fn init_scene_is_immediately_buildable_with_provisioned_node_modules() {
    let Some(src) = std::env::var_os("DCL_ONE_SDK_TEST_SCENE") else {
        eprintln!(
            "skipped: set DCL_ONE_SDK_TEST_SCENE to a scene checkout with node_modules installed"
        );
        return;
    };
    let src = PathBuf::from(src);
    assert!(
        src.join("node_modules/@dcl/sdk").is_dir(),
        "DCL_ONE_SDK_TEST_SCENE has no installed @dcl/sdk"
    );
    let f = Fixture::new("buildable");
    let dir = f.dir_arg();
    let out = run(&["init", "--dir", &dir]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    std::os::unix::fs::symlink(src.join("node_modules"), f.path().join("node_modules")).unwrap();
    let out = run(&["build", "--dir", &dir]);
    let stdout = stdout_of(&out);
    assert!(
        out.status.success(),
        "build failed\nstdout: {stdout}\nstderr: {}",
        stderr_of(&out)
    );
    assert!(stdout.contains("Bundle saved"), "{stdout}");
    assert!(stdout.contains("Type check passed"), "{stdout}");
    let bundle = std::fs::read(f.path().join("bin/index.js")).unwrap();
    assert!(
        bundle.len() > 10_000,
        "bundle suspiciously small: {}",
        bundle.len()
    );
}
