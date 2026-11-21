//! `cargo nuspec`: builds a crate and packages it as NuGet packages.
//!
//! See the [`nuspec::cargo`] module for the package layout and the `Cargo.toml` configuration.

use anyhow::{Context, Result, bail};
use cargo_metadata::camino::Utf8Path;
use cargo_metadata::{Artifact, Message, Metadata, MetadataCommand, Package};
use clap::{Args, Parser};
use nuspec::cargo::{Config, pack, packages, rid_for_target, stage};
use std::ffi::OsString;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs, io};

#[derive(Debug, Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cli {
    Nuspec(NuspecArgs),
}

/// Build a crate and generate NuGet packages for it.
///
/// Runs `cargo build`, then writes a `.nuspec` for each package configured in (or derived from)
/// `[package.metadata.nuspec]`: a native package for a `cdylib`, an app package for binaries.
/// The paths of the written `.nuspec` and `.nupkg` files are printed to stdout.
#[derive(Debug, Default, Args)]
#[command(version)]
struct NuspecArgs {
    /// Package(s) to build and package
    #[arg(short, long = "package", value_name = "SPEC")]
    packages: Vec<String>,

    /// Package all members of the workspace
    #[arg(long, conflicts_with = "packages")]
    workspace: bool,

    /// Path to Cargo.toml
    #[arg(long, value_name = "PATH")]
    manifest_path: Option<PathBuf>,

    /// Build for the target triple; also selects the .NET runtime identifier
    #[arg(long, value_name = "TRIPLE")]
    target: Option<String>,

    /// The .NET runtime identifier (e.g. `linux-x64`), instead of deriving it from the target
    #[arg(long, value_name = "RID")]
    rid: Option<String>,

    /// Build artifacts in release mode, with optimizations
    #[arg(short, long, conflicts_with = "profile")]
    release: bool,

    /// Build artifacts with the specified profile
    #[arg(long, value_name = "PROFILE-NAME")]
    profile: Option<String>,

    /// Space or comma separated list of features to activate
    #[arg(short = 'F', long, value_name = "FEATURES")]
    features: Vec<String>,

    /// Activate all available features
    #[arg(long)]
    all_features: bool,

    /// Do not activate the `default` feature
    #[arg(long)]
    no_default_features: bool,

    /// Assert that `Cargo.lock` will remain unchanged
    #[arg(long)]
    locked: bool,

    /// Run without accessing the network
    #[arg(long)]
    offline: bool,

    /// Equivalent to specifying both --locked and --offline
    #[arg(long)]
    frozen: bool,

    /// Copy the files into a self-contained directory per package, next to its `.nuspec`
    /// (default: `stage` in `package.metadata.nuspec`, or off)
    #[arg(long, overrides_with = "no_stage")]
    stage: bool,

    /// Refer to the files where cargo built them, even if `stage = true` is configured
    #[arg(long, overrides_with = "stage")]
    no_stage: bool,

    /// Also write the `.nupkg` packages
    #[arg(long)]
    pack: bool,

    /// Directory for the generated files
    /// (default: `out-dir` in `package.metadata.nuspec`, or `<target-dir>/nuspec`)
    #[arg(long, value_name = "DIR")]
    out_dir: Option<PathBuf>,

    /// Extra arguments passed to `cargo build`
    #[arg(last = true, value_name = "CARGO_BUILD_ARGS")]
    cargo_args: Vec<String>,
}

fn main() {
    let Cli::Nuspec(args) = Cli::parse();
    if let Err(err) = run(&args) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run(args: &NuspecArgs) -> Result<()> {
    let metadata = metadata(args)?;
    let selected = select_packages(&metadata, args)?;

    let mut build = Command::new(cargo());
    build
        .args(build_args(args, &selected))
        .stdout(Stdio::piped());
    let mut child = build.spawn().context("failed to run `cargo build`")?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let mut artifacts = Vec::new();
    for message in Message::parse_stream(BufReader::new(stdout)) {
        if let Message::CompilerArtifact(artifact) =
            message.context("failed to read cargo output")?
        {
            artifacts.push(artifact);
        }
    }
    let status = child.wait().context("failed to run `cargo build`")?;
    if !status.success() {
        bail!("`cargo build` failed");
    }

    let rid = match &args.rid {
        Some(rid) => rid.clone(),
        None => {
            let triple = match &args.target {
                Some(target) => target.clone(),
                None => match infer_target(&artifacts, &metadata.target_directory) {
                    Some(target) => target,
                    None => host_target()?,
                },
            };
            rid_for_target(&triple).with_context(|| {
                format!("no .NET runtime identifier is known for {triple}; pass --rid")
            })?
        }
    };

    for package in selected {
        let config = Config::from_package(package)?;
        let out_dir = out_dir(args, &config, &metadata);
        let staged = args.stage || (!args.no_stage && config.stage.unwrap_or(false));
        for nuget in
            packages(package, &artifacts, &rid).with_context(|| format!("{}", package.name))?
        {
            let id = nuget.metadata.id.clone();
            let (nuget, base) = if staged {
                let dir = out_dir.join(&id);
                let (staged, nuspec) =
                    stage(&nuget, &dir).with_context(|| format!("failed to stage {id}"))?;
                report("Staged", &nuspec);
                (staged, dir)
            } else {
                fs::create_dir_all(&out_dir)
                    .with_context(|| format!("failed to create {}", out_dir.display()))?;
                let nuspec = out_dir.join(format!("{id}.nuspec"));
                let file = fs::File::create(&nuspec)
                    .with_context(|| format!("failed to create {}", nuspec.display()))?;
                nuget
                    .write_to(io::BufWriter::new(file))
                    .with_context(|| format!("failed to write {}", nuspec.display()))?;
                report("Generated", &nuspec);
                (nuget, PathBuf::new())
            };
            if args.pack {
                let nupkg = pack(&nuget, &base, &out_dir)
                    .with_context(|| format!("failed to pack {id}"))?;
                report("Packed", &nupkg);
            }
        }
    }
    Ok(())
}

fn report(status: &str, path: &Path) {
    eprintln!("{status:>12} {}", path.display());
    println!("{}", path.display());
}

fn cargo() -> OsString {
    env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

fn metadata(args: &NuspecArgs) -> Result<Metadata> {
    let mut command = MetadataCommand::new();
    command.no_deps();
    if let Some(path) = &args.manifest_path {
        command.manifest_path(path);
    }
    let mut other = Vec::new();
    for (flag, set) in [
        ("--locked", args.locked),
        ("--offline", args.offline),
        ("--frozen", args.frozen),
    ] {
        if set {
            other.push(flag.to_string());
        }
    }
    command.other_options(other);
    command.exec().context("failed to run `cargo metadata`")
}

fn select_packages<'a>(metadata: &'a Metadata, args: &NuspecArgs) -> Result<Vec<&'a Package>> {
    let members = metadata.workspace_packages();
    let mut selected: Vec<&Package> = if !args.packages.is_empty() {
        args.packages
            .iter()
            .map(|name| {
                members
                    .iter()
                    .find(|p| p.name.as_str() == name)
                    .copied()
                    .with_context(|| format!("package `{name}` is not a member of the workspace"))
            })
            .collect::<Result<_>>()?
    } else if args.workspace {
        members
    } else if let Some(root) = metadata.root_package() {
        vec![root]
    } else {
        metadata.workspace_default_packages()
    };
    selected.dedup_by_key(|p| &p.id);
    if selected.is_empty() {
        bail!("no packages selected");
    }
    Ok(selected)
}

fn build_args(args: &NuspecArgs, packages: &[&Package]) -> Vec<String> {
    let mut out = vec![
        "build".to_string(),
        "--message-format=json-render-diagnostics".to_string(),
    ];
    if let Some(path) = &args.manifest_path {
        out.push("--manifest-path".to_string());
        out.push(path.display().to_string());
    }
    for package in packages {
        out.push("--package".to_string());
        out.push(package.name.to_string());
    }
    if let Some(target) = &args.target {
        out.push("--target".to_string());
        out.push(target.clone());
    }
    if args.release {
        out.push("--release".to_string());
    }
    if let Some(profile) = &args.profile {
        out.push("--profile".to_string());
        out.push(profile.clone());
    }
    if !args.features.is_empty() {
        out.push("--features".to_string());
        out.push(args.features.join(","));
    }
    for (flag, set) in [
        ("--all-features", args.all_features),
        ("--no-default-features", args.no_default_features),
        ("--locked", args.locked),
        ("--offline", args.offline),
        ("--frozen", args.frozen),
    ] {
        if set {
            out.push(flag.to_string());
        }
    }
    out.extend(args.cargo_args.iter().cloned());
    out
}

/// Cargo puts the final artifacts in `<target-dir>/<profile>/` for the host and in
/// `<target-dir>/<triple>/<profile>/` for an explicit target, including one set in
/// `.cargo/config.toml` or passed after `--`.
fn infer_target(artifacts: &[Artifact], target_dir: &Utf8Path) -> Option<String> {
    artifacts
        .iter()
        .flat_map(|a| a.filenames.iter())
        .find_map(|file| {
            let relative = file.strip_prefix(target_dir).ok()?;
            let parts: Vec<&str> = relative.iter().collect();
            (parts.len() == 3).then(|| parts[0].to_string())
        })
}

fn host_target() -> Result<String> {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(rustc)
        .arg("-vV")
        .output()
        .context("failed to run `rustc -vV`")?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::to_string)
        .context("`rustc -vV` did not report the host target")
}

fn out_dir(args: &NuspecArgs, config: &Config, metadata: &Metadata) -> PathBuf {
    if let Some(dir) = &args.out_dir {
        return dir.clone();
    }
    match &config.out_dir {
        Some(dir) => metadata.workspace_root.join(dir).into_std_path_buf(),
        None => metadata.target_directory.join("nuspec").into_std_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> NuspecArgs {
        let Cli::Nuspec(args) = Cli::try_parse_from(["cargo", "nuspec"].iter().chain(args))
            .unwrap_or_else(|e| panic!("{e}"));
        args
    }

    fn workspace() -> Metadata {
        MetadataCommand::new()
            .manifest_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml"))
            .no_deps()
            .exec()
            .unwrap()
    }

    #[test]
    fn test_cli() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_build_args() {
        let metadata = workspace();
        let args = parse(&[
            "-p",
            "nuspec-test",
            "--target",
            "x86_64-pc-windows-msvc",
            "--release",
            "-F",
            "a,b",
            "--features",
            "c",
            "--locked",
            "--",
            "--jobs",
            "2",
        ]);
        let selected = select_packages(&metadata, &args).unwrap();
        assert_eq!(
            build_args(&args, &selected),
            [
                "build",
                "--message-format=json-render-diagnostics",
                "--package",
                "nuspec-test",
                "--target",
                "x86_64-pc-windows-msvc",
                "--release",
                "--features",
                "a,b,c",
                "--locked",
                "--jobs",
                "2",
            ]
        );
    }

    #[test]
    fn test_select_packages() {
        let metadata = workspace();
        let names = |args: &NuspecArgs| -> Vec<String> {
            select_packages(&metadata, args)
                .unwrap()
                .iter()
                .map(|p| p.name.to_string())
                .collect()
        };
        assert_eq!(names(&parse(&["-p", "nuspec"])), ["nuspec"]);
        assert_eq!(
            names(&parse(&["--workspace"])),
            ["cargo-nuspec", "nuspec", "nuspec-test"]
        );
        let err = select_packages(&metadata, &parse(&["-p", "serde"])).unwrap_err();
        assert_eq!(
            err.to_string(),
            "package `serde` is not a member of the workspace"
        );
    }

    #[test]
    fn test_stage_flags() {
        assert!(parse(&["--stage"]).stage);
        let args = parse(&["--stage", "--no-stage"]);
        assert!(!args.stage && args.no_stage);
        let args = parse(&["--no-stage", "--stage"]);
        assert!(args.stage && !args.no_stage);
        assert!(Cli::try_parse_from(["cargo", "nuspec", "-r", "--profile", "dev"]).is_err());
    }

    #[test]
    fn test_out_dir() {
        let metadata = workspace();
        let default = Config::default();
        assert_eq!(
            out_dir(&NuspecArgs::default(), &default, &metadata),
            metadata.target_directory.join("nuspec").into_std_path_buf()
        );
        let config = Config {
            out_dir: Some("dist".to_string()),
            ..Default::default()
        };
        assert_eq!(
            out_dir(&NuspecArgs::default(), &config, &metadata),
            metadata.workspace_root.join("dist").into_std_path_buf()
        );
        let args = parse(&["--out-dir", "elsewhere"]);
        assert_eq!(
            out_dir(&args, &config, &metadata),
            PathBuf::from("elsewhere")
        );
    }

    #[test]
    fn test_infer_target() {
        let artifact = |file: &str| -> Artifact {
            serde_json::from_value(serde_json::json!({
                "package_id": "path+file:///w#x@0.1.0",
                "manifest_path": "/w/Cargo.toml",
                "target": {
                    "name": "x", "kind": ["bin"], "crate_types": ["bin"],
                    "src_path": "/w/src/main.rs", "edition": "2024",
                },
                "profile": {
                    "opt_level": "0", "debuginfo": 0, "debug_assertions": true,
                    "overflow_checks": true, "test": false,
                },
                "features": [],
                "filenames": [file],
                "executable": file,
                "fresh": true,
            }))
            .unwrap()
        };
        let target_dir = Utf8Path::new("/w/target");
        assert_eq!(
            infer_target(&[artifact("/w/target/debug/x")], target_dir),
            None
        );
        assert_eq!(
            infer_target(
                &[artifact("/w/target/aarch64-apple-darwin/release/x")],
                target_dir
            ),
            Some("aarch64-apple-darwin".to_string())
        );
        assert!(host_target().unwrap().contains('-'));
    }
}
