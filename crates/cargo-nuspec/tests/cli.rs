//! Runs `cargo nuspec` on the `nuspec-test` crate with a real `cargo build`.

use nuspec::Package;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../nuspec-test/Cargo.toml")
}

/// Runs `cargo nuspec` with its own target directory, shared between tests so the crate is
/// built once. The nested `cargo build` drops the coverage instrumentation flags of the outer
/// build, but `LLVM_PROFILE_FILE` is kept so `cargo llvm-cov` still collects the coverage of
/// the `cargo-nuspec` process itself.
fn cargo_nuspec(args: &[&str]) -> Vec<PathBuf> {
    let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("e2e-target");
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-nuspec"))
        .arg("nuspec")
        .arg("--manifest-path")
        .arg(manifest())
        .args(args)
        .env("CARGO_TARGET_DIR", &target_dir)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo nuspec failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(PathBuf::from)
        .collect()
}

fn out_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn read(path: &Path) -> Package {
    Package::from_xml(&fs::read_to_string(path).unwrap()).unwrap()
}

fn targets(pkg: &Package) -> Vec<String> {
    let mut targets: Vec<String> = pkg
        .files
        .iter()
        .flat_map(|f| f.file.iter())
        .map(|f| {
            let name = Path::new(&f.src).file_name().unwrap().to_string_lossy();
            match f.target.as_deref().unwrap_or_default() {
                "" => name.into_owned(),
                target if target.ends_with(&*name) => target.to_string(),
                target => format!("{target}/{name}"),
            }
        })
        .collect();
    targets.sort();
    targets
}

fn native_library() -> &'static str {
    if cfg!(windows) {
        "nuspec_test_lib.dll"
    } else if cfg!(target_os = "macos") {
        "libnuspec_test_lib.dylib"
    } else {
        "libnuspec_test_lib.so"
    }
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn native_and_app(paths: &[PathBuf]) -> (&PathBuf, &PathBuf, String) {
    let [native, app] = paths else {
        panic!("expected two files, got {paths:?}");
    };
    let app_name = app.file_name().unwrap().to_string_lossy();
    let rid = app_name
        .strip_prefix("nuspec-test.")
        .and_then(|s| s.split(['.']).next())
        .unwrap()
        .to_string();
    (native, app, rid)
}

#[test]
fn generate_in_place() {
    let out = out_dir("in-place");
    let written = cargo_nuspec(&["--out-dir", out.to_str().unwrap()]);
    let (native, app, rid) = native_and_app(&written);
    assert_eq!(native, &out.join("nuspec-test.nuspec"));
    assert_eq!(app, &out.join(format!("nuspec-test.{rid}.nuspec")));

    let native = read(native);
    assert_eq!(native.metadata.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(native.metadata.require_license_acceptance, Some(false));
    assert_eq!(
        targets(&native),
        [
            "LICENSE-APACHE".to_string(),
            "LICENSE-MIT".to_string(),
            "README.md".to_string(),
            format!("runtimes/{rid}/native/{}", native_library()),
        ]
    );

    let app = read(app);
    assert_eq!(
        targets(&app),
        [
            "LICENSE-APACHE".to_string(),
            "LICENSE-MIT".to_string(),
            "README.md".to_string(),
            format!("tools/{}", exe("nuspec_test_bin_1")),
            format!("tools/{}", exe("nuspec_test_bin_2")),
        ]
    );
    for file in app.files.iter().flat_map(|f| f.file.iter()) {
        assert!(Path::new(&file.src).is_absolute(), "{}", file.src);
        assert!(Path::new(&file.src).is_file(), "{}", file.src);
    }
}

#[test]
fn stage_and_pack() {
    let out = out_dir("staged");
    let written = cargo_nuspec(&["--stage", "--pack", "--out-dir", out.to_str().unwrap()]);
    assert_eq!(written.len(), 4, "{written:?}");
    let version = env!("CARGO_PKG_VERSION");
    let (_, _, rid) = native_and_app(&[written[0].clone(), written[2].clone()]);
    assert_eq!(
        written,
        [
            out.join("nuspec-test/nuspec-test.nuspec"),
            out.join(format!("nuspec-test.{version}.nupkg")),
            out.join(format!("nuspec-test.{rid}/nuspec-test.{rid}.nuspec")),
            out.join(format!("nuspec-test.{rid}.{version}.nupkg")),
        ]
    );

    let staged = read(&written[2]);
    for file in staged.files.iter().flat_map(|f| f.file.iter()) {
        assert!(Path::new(&file.src).is_relative(), "{}", file.src);
        assert!(
            out.join(format!("nuspec-test.{rid}"))
                .join(&file.src)
                .is_file()
        );
    }

    let archive = |path: &Path| zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut native = archive(&written[1]);
    assert!(
        native
            .by_name(&format!("runtimes/{rid}/native/{}", native_library()))
            .is_ok()
    );
    assert!(native.by_name("nuspec-test.nuspec").is_ok());

    let mut app = archive(&written[3]);
    let bin = app
        .by_name(&format!("tools/{}", exe("nuspec_test_bin_1")))
        .unwrap();
    #[cfg(unix)]
    assert_eq!(bin.unix_mode().map(|m| m & 0o111), Some(0o111));
    drop(bin);
    assert!(app.by_name("LICENSE-MIT").is_ok());
}

#[test]
fn merge_platforms() {
    let out = out_dir("merge");
    let version = env!("CARGO_PKG_VERSION");
    // The same build packaged for two runtime identifiers stands in for two platforms.
    let x64 = out.join("x64");
    let arm64 = out.join("arm64");
    let native = |dir: &Path, rid: &str| {
        let written = cargo_nuspec(&[
            "--rid",
            rid,
            "--stage",
            "--pack",
            "--out-dir",
            dir.to_str().unwrap(),
        ]);
        assert_eq!(written[0], dir.join("nuspec-test/nuspec-test.nuspec"));
        assert_eq!(written[1], dir.join(format!("nuspec-test.{version}.nupkg")));
    };
    native(&x64, "linux-x64");
    native(&arm64, "linux-arm64");

    // A packed and a staged package; the shared readme and licenses are kept once.
    let merged = out.join("merged");
    let written = cargo_nuspec_merge(&[
        x64.join(format!("nuspec-test.{version}.nupkg"))
            .to_str()
            .unwrap(),
        arm64.join("nuspec-test").to_str().unwrap(),
        "--pack",
        "--out-dir",
        merged.to_str().unwrap(),
    ]);
    assert_eq!(
        written,
        [
            merged.join("nuspec-test/nuspec-test.nuspec"),
            merged.join(format!("nuspec-test.{version}.nupkg")),
        ]
    );
    let library = native_library();
    assert_eq!(
        targets(&read(&written[0])),
        [
            "LICENSE-APACHE".to_string(),
            "LICENSE-MIT".to_string(),
            "README.md".to_string(),
            format!("runtimes/linux-arm64/native/{library}"),
            format!("runtimes/linux-x64/native/{library}"),
        ]
    );
    let mut archive = zip::ZipArchive::new(fs::File::open(&written[1]).unwrap()).unwrap();
    for rid in ["linux-x64", "linux-arm64"] {
        assert!(
            archive
                .by_name(&format!("runtimes/{rid}/native/{library}"))
                .is_ok()
        );
    }

    // App packages have the runtime identifier in their id.
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-nuspec"))
        .args(["nuspec", "merge", "--out-dir"])
        .arg(out.join("apps"))
        .arg(x64.join("nuspec-test.linux-x64"))
        .arg(arm64.join("nuspec-test.linux-arm64"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("same id and version"), "{stderr}");
}

fn cargo_nuspec_merge(args: &[&str]) -> Vec<PathBuf> {
    let output = Command::new(env!("CARGO_BIN_EXE_cargo-nuspec"))
        .args(["nuspec", "merge"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo nuspec merge failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(PathBuf::from)
        .collect()
}
