//! Runs `cargo nuspec` on the `nuspec-test` crate with a real `cargo build`.

use nuspec::Package;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../nuspec-test/Cargo.toml")
}

/// Runs `cargo nuspec` with its own target directory, shared between tests so the crate is
/// built once, and without the coverage instrumentation of the outer build.
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
        .env_remove("LLVM_PROFILE_FILE")
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
