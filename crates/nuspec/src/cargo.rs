//! Builds [`Package`]s for a Cargo package from its metadata and build artifacts.
//!
//! The inputs are what Cargo reports itself: a [`cargo_metadata::Package`] from
//! `cargo metadata`, and the [`Artifact`]s from `cargo build --message-format=json`.
//! Using the artifacts means the exact output files are known, whatever the target, profile
//! or platform naming rules.
//!
//! Two kinds of NuGet packages are produced:
//!
//! - [`Kind::Native`]: `cdylib` libraries for .NET to call through P/Invoke, placed in
//!   `runtimes/<rid>/native/`, where the .NET SDK picks them up for the matching platform.
//! - [`Kind::App`]: executables, placed in `tools/` (or the package root), one package per
//!   platform, with the RID appended to the id by default (`MyApp.linux-x64`).
//!
//! Without configuration, a crate with a `cdylib` gets a native package and a crate with
//! binaries gets an app package. Both can be configured in `Cargo.toml`:
//!
//! ```toml
//! # Applies to every package of this crate.
//! [package.metadata.nuspec.metadata]
//! copyright = "Copyright (c) Contoso"
//!
//! [[package.metadata.nuspec.packages]]
//! kind = "native"
//! id = "Contoso.Engine.Native"
//!
//! [[package.metadata.nuspec.packages]]
//! kind = "app"
//! id = "Contoso.Engine.Cli"
//! bins = ["engine"]
//! layout = "root"
//! files = [{ src = "deploy/*.ps1", target = "scripts" }]
//!
//! # Applies to the last package above only.
//! [package.metadata.nuspec.packages.metadata]
//! title = "Contoso Engine CLI"
//! ```
//!
//! The `metadata` tables accept any [`Metadata`] field, as `camelCase` (as in the nuspec) or
//! `snake_case`. Values set there win over the ones derived from Cargo.

use crate::{File, Files, License, Metadata, Package, Repository, ValidationErrors};
use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::{Artifact, CrateType, TargetKind};
use serde::Deserialize;
use std::{error, fmt};

/// The `[package.metadata.nuspec]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Metadata applied to every package, overriding values derived from Cargo.
    #[serde(default)]
    pub metadata: Option<Metadata>,
    /// Extra files added to every package.
    #[serde(default)]
    pub files: Vec<File>,
    /// The packages to build. When empty, the packages are chosen from the crate's targets.
    #[serde(default)]
    pub packages: Vec<PackageConfig>,
}

/// One entry of `[[package.metadata.nuspec.packages]]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PackageConfig {
    /// What the package contains.
    pub kind: Kind,
    /// The package id; defaults to the crate name.
    #[serde(default)]
    pub id: Option<String>,
    /// Whether to append `.<rid>` to the id. Defaults to `true` for apps and `false` for
    /// native packages.
    #[serde(default)]
    pub id_suffix_rid: Option<bool>,
    /// For apps: the binaries to include; defaults to all of them.
    #[serde(default)]
    pub bins: Option<Vec<String>>,
    /// For apps: where the binaries go.
    #[serde(default)]
    pub layout: Layout,
    /// Whether to include the `.pdb` debug symbols that Cargo produces on Windows.
    #[serde(default)]
    pub include_pdb: bool,
    /// Metadata for this package, overriding [`Config::metadata`].
    #[serde(default)]
    pub metadata: Option<Metadata>,
    /// Extra files for this package. Relative `src` paths are resolved against the crate's
    /// directory, and may contain the `*` and `**` wildcards.
    #[serde(default)]
    pub files: Vec<File>,
}

/// The kind of a NuGet package built from a crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// `cdylib` libraries in `runtimes/<rid>/native/`.
    Native,
    /// Executables in `tools/` or the package root.
    App,
}

/// Where an app package puts its binaries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    /// In `tools/`, NuGet's folder for executables.
    #[default]
    Tools,
    /// In the root of the package.
    Root,
}

/// An error building packages from Cargo output.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// `[package.metadata.nuspec]` is malformed.
    Config {
        /// The Cargo package name.
        package: String,
        /// The parse error.
        source: serde_json::Error,
    },
    /// The crate has neither a `cdylib` nor binaries, and no packages are configured.
    NothingToPackage {
        /// The Cargo package name.
        package: String,
    },
    /// A package needs a build artifact that is not among the given artifacts.
    MissingArtifact {
        /// The NuGet package id.
        id: String,
        /// What is missing, such as `cdylib` or `bin "foo"`.
        artifact: String,
    },
    /// Two packages would get the same id.
    DuplicateId(String),
    /// A package failed [`Package::validate`].
    Invalid {
        /// The NuGet package id.
        id: String,
        /// The problems found.
        errors: ValidationErrors,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Config { package, source } => {
                write!(
                    f,
                    "invalid `package.metadata.nuspec` in {package}: {source}"
                )
            }
            Error::NothingToPackage { package } => write!(
                f,
                "{package} has neither a cdylib nor binaries; configure `package.metadata.nuspec.packages`"
            ),
            Error::MissingArtifact { id, artifact } => write!(
                f,
                "package {id}: no {artifact} build artifact; was it built for this target?"
            ),
            Error::DuplicateId(id) => write!(f, "more than one package has the id {id}"),
            Error::Invalid { id, errors } => write!(f, "package {id}: {errors}"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Config { source, .. } => Some(source),
            Error::Invalid { errors, .. } => Some(errors),
            _ => None,
        }
    }
}

impl Config {
    /// Reads `[package.metadata.nuspec]`; a missing table is the default configuration.
    pub fn from_package(package: &cargo_metadata::Package) -> Result<Self, Error> {
        match package.metadata.get("nuspec") {
            None | Some(serde_json::Value::Null) => Ok(Self::default()),
            Some(value) => Self::deserialize(value).map_err(|source| Error::Config {
                package: package.name.to_string(),
                source,
            }),
        }
    }
}

/// Builds the NuGet packages for a Cargo package built for the platform `rid`.
///
/// `artifacts` may include artifacts of other packages (dependencies); they are ignored.
/// Every file in the result has an absolute `src`, so the `.nuspec` can be written anywhere.
/// Each package is checked with [`Package::validate`].
pub fn packages(
    package: &cargo_metadata::Package,
    artifacts: &[Artifact],
    rid: &str,
) -> Result<Vec<Package>, Error> {
    let config = Config::from_package(package)?;
    let artifacts: Vec<&Artifact> = artifacts
        .iter()
        .filter(|a| a.package_id == package.id)
        .collect();

    let configs = if config.packages.is_empty() {
        default_packages(package)?
    } else {
        config.packages.clone()
    };

    let mut result: Vec<Package> = Vec::with_capacity(configs.len());
    for pkg_config in &configs {
        let pkg = build_package(package, &config, pkg_config, &artifacts, rid)?;
        if result.iter().any(|p| p.metadata.id == pkg.metadata.id) {
            return Err(Error::DuplicateId(pkg.metadata.id));
        }
        pkg.validate().map_err(|errors| Error::Invalid {
            id: pkg.metadata.id.clone(),
            errors,
        })?;
        result.push(pkg);
    }
    Ok(result)
}

fn default_packages(package: &cargo_metadata::Package) -> Result<Vec<PackageConfig>, Error> {
    let config = |kind| PackageConfig {
        kind,
        id: None,
        id_suffix_rid: None,
        bins: None,
        layout: Layout::default(),
        include_pdb: false,
        metadata: None,
        files: Vec::new(),
    };
    let mut configs = Vec::new();
    if package.targets.iter().any(is_cdylib) {
        configs.push(config(Kind::Native));
    }
    if package.targets.iter().any(is_bin) {
        configs.push(config(Kind::App));
    }
    if configs.is_empty() {
        return Err(Error::NothingToPackage {
            package: package.name.to_string(),
        });
    }
    Ok(configs)
}

fn is_cdylib(target: &cargo_metadata::Target) -> bool {
    target.crate_types.contains(&CrateType::CDyLib)
}

fn is_bin(target: &cargo_metadata::Target) -> bool {
    target.kind.contains(&TargetKind::Bin)
}

fn build_package(
    package: &cargo_metadata::Package,
    config: &Config,
    pkg_config: &PackageConfig,
    artifacts: &[&Artifact],
    rid: &str,
) -> Result<Package, Error> {
    let mut metadata = pkg_config.metadata.clone().unwrap_or_default();
    if let Some(shared) = &config.metadata {
        fill(&mut metadata, shared.clone());
    }
    let mut files = Vec::new();
    fill(&mut metadata, from_cargo(package, &mut files));

    if let Some(id) = &pkg_config.id {
        metadata.id = id.clone();
    }
    let suffix = pkg_config
        .id_suffix_rid
        .unwrap_or(pkg_config.kind == Kind::App);
    if suffix {
        metadata.id = format!("{}.{rid}", metadata.id);
    }

    match pkg_config.kind {
        Kind::Native => {
            let target = format!("runtimes/{rid}/native");
            let found: Vec<_> = artifacts
                .iter()
                .filter(|a| is_cdylib(&a.target))
                .flat_map(|a| a.filenames.iter())
                .filter(|f| is_native_library(f) || (pkg_config.include_pdb && is_pdb(f)))
                .collect();
            if !found.iter().any(|f| is_native_library(f)) {
                return Err(Error::MissingArtifact {
                    id: metadata.id,
                    artifact: "cdylib".to_string(),
                });
            }
            for file in found {
                push_file(&mut files, file, &target);
            }
        }
        Kind::App => {
            let target = match pkg_config.layout {
                Layout::Tools => "tools",
                Layout::Root => "",
            };
            let bins: Vec<String> = match &pkg_config.bins {
                Some(bins) => bins.clone(),
                None => package
                    .targets
                    .iter()
                    .filter(|t| is_bin(t))
                    .map(|t| t.name.clone())
                    .collect(),
            };
            for bin in &bins {
                let artifact = artifacts
                    .iter()
                    .find(|a| is_bin(&a.target) && &a.target.name == bin)
                    .and_then(|a| a.executable.as_ref().map(|exe| (a, exe)));
                let Some((artifact, executable)) = artifact else {
                    return Err(Error::MissingArtifact {
                        id: metadata.id,
                        artifact: format!("bin {bin:?}"),
                    });
                };
                push_file(&mut files, executable, target);
                if pkg_config.include_pdb {
                    for pdb in artifact.filenames.iter().filter(|f| is_pdb(f)) {
                        push_file(&mut files, pdb, target);
                    }
                }
            }
        }
    }

    let manifest_dir = package
        .manifest_path
        .parent()
        .unwrap_or_else(|| Utf8Path::new(""));
    for extra in config.files.iter().chain(&pkg_config.files) {
        let src = Utf8Path::new(&extra.src);
        let src = if src.is_absolute() {
            src.to_path_buf()
        } else {
            manifest_dir.join(src)
        };
        files.push(File {
            src: src.into_string(),
            ..extra.clone()
        });
    }

    Ok(Package {
        metadata,
        files: Some(Files { file: files }),
        ..Default::default()
    })
}

/// The metadata Cargo knows about, plus the readme and license files it refers to.
fn from_cargo(package: &cargo_metadata::Package, files: &mut Vec<File>) -> Metadata {
    let mut metadata = Metadata {
        id: package.name.to_string(),
        version: package.version.to_string(),
        description: package.description.clone().unwrap_or_default(),
        authors: package
            .authors
            .iter()
            .map(|a| strip_email(a))
            .filter(|a| !a.is_empty())
            .collect(),
        project_url: non_empty(&package.homepage).or_else(|| non_empty(&package.repository)),
        repository: non_empty(&package.repository).map(|url| Repository {
            url: Some(url),
            ..Default::default()
        }),
        tags: (!package.keywords.is_empty()).then(|| package.keywords.clone()),
        ..Default::default()
    };
    // NuGet requires authors; `dotnet pack` falls back to the project name, so do the same.
    if metadata.authors.is_empty() {
        metadata.authors = vec![package.name.to_string()];
    }

    if let Some(license) = non_empty(&package.license) {
        metadata.license = Some(License::Expression(license));
    } else if let Some(path) = package.license_file() {
        if let Some(name) = path.file_name() {
            metadata.license = Some(License::File(name.to_string()));
            push_file(files, &path, "");
        }
    }

    // NuGet only renders Markdown readmes.
    if let Some(path) = package.readme() {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            if let Some(name) = path.file_name() {
                metadata.readme = Some(name.to_string());
                push_file(files, &path, "");
            }
        }
    }
    metadata
}

/// Fills every empty field of `target` from `from`.
fn fill(target: &mut Metadata, from: Metadata) {
    // Destructured so that a new `Metadata` field fails to compile until it is handled here.
    let Metadata {
        id,
        version,
        description,
        authors,
        owners,
        summary,
        project_url,
        license,
        license_url,
        icon,
        icon_url,
        readme,
        require_license_acceptance,
        development_dependency,
        serviceable,
        release_notes,
        copyright,
        language,
        tags,
        repository,
        title,
        min_client_version,
        package_types,
        dependencies,
        framework_assemblies,
        framework_references,
        references,
        content_files,
    } = from;

    fn string(target: &mut String, from: String) {
        if target.is_empty() {
            *target = from;
        }
    }
    fn option<T>(target: &mut Option<T>, from: Option<T>) {
        if target.is_none() {
            *target = from;
        }
    }

    string(&mut target.id, id);
    string(&mut target.version, version);
    string(&mut target.description, description);
    if target.authors.is_empty() {
        target.authors = authors;
    }
    option(&mut target.owners, owners);
    option(&mut target.summary, summary);
    option(&mut target.project_url, project_url);
    option(&mut target.license, license);
    option(&mut target.license_url, license_url);
    option(&mut target.icon, icon);
    option(&mut target.icon_url, icon_url);
    option(&mut target.readme, readme);
    option(
        &mut target.require_license_acceptance,
        require_license_acceptance,
    );
    option(&mut target.development_dependency, development_dependency);
    option(&mut target.serviceable, serviceable);
    option(&mut target.release_notes, release_notes);
    option(&mut target.copyright, copyright);
    option(&mut target.language, language);
    option(&mut target.tags, tags);
    option(&mut target.repository, repository);
    option(&mut target.title, title);
    option(&mut target.min_client_version, min_client_version);
    option(&mut target.package_types, package_types);
    option(&mut target.dependencies, dependencies);
    option(&mut target.framework_assemblies, framework_assemblies);
    option(&mut target.framework_references, framework_references);
    option(&mut target.references, references);
    option(&mut target.content_files, content_files);
}

fn push_file(files: &mut Vec<File>, src: &Utf8Path, target: &str) {
    let src = src.to_string();
    if files.iter().any(|f| f.src == src) {
        return;
    }
    files.push(File {
        src,
        target: Some(target.to_string()),
        exclude: None,
    });
}

fn is_native_library(path: &Utf8PathBuf) -> bool {
    matches!(path.extension(), Some("dll" | "so" | "dylib"))
}

fn is_pdb(path: &Utf8PathBuf) -> bool {
    path.extension() == Some("pdb")
}

fn non_empty(value: &Option<String>) -> Option<String> {
    value.as_ref().filter(|v| !v.trim().is_empty()).cloned()
}

/// `"Jane Doe <jane@example.com>"` becomes `"Jane Doe"`.
fn strip_email(author: &str) -> String {
    match author.find('<') {
        Some(start) if author.trim_end().ends_with('>') => author[..start].trim().to_string(),
        _ => author.trim().to_string(),
    }
}

/// Maps a Rust target triple to a .NET [runtime identifier](https://learn.microsoft.com/en-us/dotnet/core/rid-catalog),
/// such as `x86_64-pc-windows-msvc` to `win-x64`.
///
/// Returns `None` for platforms without a portable RID.
pub fn rid_for_target(triple: &str) -> Option<String> {
    let mut parts = triple.split('-');
    let arch = parts.next()?;
    let rest: Vec<&str> = parts.collect();
    let has = |s: &str| rest.contains(&s);
    let env = rest.last().copied().unwrap_or_default();

    let os = if has("windows") {
        "win"
    } else if has("darwin") {
        "osx"
    } else if has("linux") && env.starts_with("android") {
        "android"
    } else if has("linux") && env.starts_with("musl") {
        "linux-musl"
    } else if has("linux") {
        "linux"
    } else if has("freebsd") {
        "freebsd"
    } else {
        return None;
    };

    let arch = match arch {
        "x86_64" => "x64",
        "i586" | "i686" => "x86",
        "aarch64" => "arm64",
        a if a.starts_with("armv7") || a == "arm" || a.starts_with("thumbv7") => "arm",
        "riscv64gc" => "riscv64",
        "loongarch64" => "loongarch64",
        "powerpc64le" => "ppc64le",
        "s390x" => "s390x",
        _ => return None,
    };

    Some(format!("{os}-{arch}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "path+file:///work/demo#0.1.0";

    fn target(name: &str, kind: &str, crate_types: &[&str]) -> serde_json::Value {
        json!({
            "name": name,
            "kind": [kind],
            "crate_types": crate_types,
            "src_path": "/work/demo/src/lib.rs",
            "edition": "2024",
        })
    }

    fn package(
        targets: Vec<serde_json::Value>,
        metadata: serde_json::Value,
    ) -> cargo_metadata::Package {
        serde_json::from_value(json!({
            "name": "demo",
            "version": "0.1.0",
            "id": ID,
            "authors": ["Jane Doe <jane@example.com>", "John"],
            "description": "A demo crate",
            "license": "MIT OR Apache-2.0",
            "repository": "https://github.com/contoso/demo",
            "keywords": ["ffi", "demo"],
            "readme": "README.md",
            "dependencies": [],
            "targets": targets,
            "features": {},
            "manifest_path": "/work/demo/Cargo.toml",
            "metadata": metadata,
            "edition": "2024",
        }))
        .unwrap()
    }

    fn artifact(name: &str, kind: &str, crate_types: &[&str], filenames: &[&str]) -> Artifact {
        let executable = (kind == "bin").then(|| filenames[0]);
        serde_json::from_value(json!({
            "package_id": ID,
            "manifest_path": "/work/demo/Cargo.toml",
            "target": target(name, kind, crate_types),
            "profile": {
                "opt_level": "3",
                "debuginfo": 0,
                "debug_assertions": false,
                "overflow_checks": false,
                "test": false,
            },
            "features": [],
            "filenames": filenames,
            "executable": executable,
            "fresh": false,
        }))
        .unwrap()
    }

    fn files(pkg: &Package) -> Vec<(String, String)> {
        pkg.files
            .as_ref()
            .unwrap()
            .file
            .iter()
            .map(|f| (f.src.clone(), f.target.clone().unwrap_or_default()))
            .collect()
    }

    fn pair(src: &str, target: &str) -> (String, String) {
        (src.to_string(), target.to_string())
    }

    fn lib_and_bin() -> (cargo_metadata::Package, Vec<Artifact>) {
        let pkg = package(
            vec![
                target("demo", "cdylib", &["cdylib", "rlib"]),
                target("demo-cli", "bin", &["bin"]),
            ],
            json!(null),
        );
        let artifacts = vec![
            artifact(
                "demo",
                "cdylib",
                &["cdylib", "rlib"],
                &[
                    "/work/target/release/demo.dll",
                    "/work/target/release/demo.dll.lib",
                    "/work/target/release/demo.pdb",
                    "/work/target/release/libdemo.rlib",
                ],
            ),
            artifact(
                "demo-cli",
                "bin",
                &["bin"],
                &[
                    "/work/target/release/demo-cli.exe",
                    "/work/target/release/demo_cli.pdb",
                ],
            ),
        ];
        (pkg, artifacts)
    }

    #[test]
    fn test_default_packages() {
        let (pkg, artifacts) = lib_and_bin();
        let result = packages(&pkg, &artifacts, "win-x64").unwrap();
        assert_eq!(result.len(), 2);

        let native = &result[0];
        let m = &native.metadata;
        assert_eq!(m.id, "demo");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.description, "A demo crate");
        assert_eq!(m.authors, vec!["Jane Doe", "John"]);
        assert_eq!(
            m.license,
            Some(License::Expression("MIT OR Apache-2.0".to_string()))
        );
        assert_eq!(m.readme.as_deref(), Some("README.md"));
        assert_eq!(
            m.project_url.as_deref(),
            Some("https://github.com/contoso/demo")
        );
        assert_eq!(
            m.repository.as_ref().and_then(|r| r.url.as_deref()),
            Some("https://github.com/contoso/demo")
        );
        assert_eq!(m.tags, Some(vec!["ffi".to_string(), "demo".to_string()]));
        assert_eq!(
            files(native),
            vec![
                pair("/work/demo/README.md", ""),
                pair("/work/target/release/demo.dll", "runtimes/win-x64/native"),
            ]
        );

        let app = &result[1];
        assert_eq!(app.metadata.id, "demo.win-x64");
        assert_eq!(
            files(app),
            vec![
                pair("/work/demo/README.md", ""),
                pair("/work/target/release/demo-cli.exe", "tools"),
            ]
        );
    }

    #[test]
    fn test_configured_packages() {
        let (mut pkg, artifacts) = lib_and_bin();
        pkg.metadata = json!({
            "nuspec": {
                "metadata": { "copyright": "Contoso", "title": "Shared" },
                "files": [{ "src": "NOTICE.txt", "target": "" }],
                "packages": [
                    {
                        "kind": "native",
                        "id": "Contoso.Demo.Native",
                        "include-pdb": true,
                        "metadata": { "title": "Native", "requireLicenseAcceptance": true },
                    },
                    {
                        "kind": "app",
                        "id": "Contoso.Demo",
                        "id-suffix-rid": false,
                        "bins": ["demo-cli"],
                        "layout": "root",
                        "files": [{ "src": "/abs/run.ps1", "target": "scripts" }],
                    },
                ],
            }
        });

        let result = packages(&pkg, &artifacts, "win-x64").unwrap();
        let native = &result[0];
        assert_eq!(native.metadata.id, "Contoso.Demo.Native");
        assert_eq!(native.metadata.title.as_deref(), Some("Native"));
        assert_eq!(native.metadata.copyright.as_deref(), Some("Contoso"));
        assert_eq!(native.metadata.require_license_acceptance, Some(true));
        assert_eq!(
            files(native),
            vec![
                pair("/work/demo/README.md", ""),
                pair("/work/target/release/demo.dll", "runtimes/win-x64/native"),
                pair("/work/target/release/demo.pdb", "runtimes/win-x64/native"),
                pair("/work/demo/NOTICE.txt", ""),
            ]
        );

        let app = &result[1];
        assert_eq!(app.metadata.id, "Contoso.Demo");
        assert_eq!(app.metadata.title.as_deref(), Some("Shared"));
        assert_eq!(
            files(app),
            vec![
                pair("/work/demo/README.md", ""),
                pair("/work/target/release/demo-cli.exe", ""),
                pair("/work/demo/NOTICE.txt", ""),
                pair("/abs/run.ps1", "scripts"),
            ]
        );
    }

    #[test]
    fn test_license_file_and_no_authors() {
        let mut pkg = package(vec![target("demo", "cdylib", &["cdylib"])], json!(null));
        pkg.license = None;
        pkg.license_file = Some("LICENSE.txt".into());
        pkg.readme = Some("README.txt".into());
        pkg.authors = vec![];
        let artifacts = [artifact("demo", "cdylib", &["cdylib"], &["/t/libdemo.so"])];

        let result = packages(&pkg, &artifacts, "linux-x64").unwrap();
        let m = &result[0].metadata;
        assert_eq!(m.license, Some(License::File("LICENSE.txt".to_string())));
        assert_eq!(m.readme, None, "only Markdown readmes are packed");
        assert_eq!(m.authors, vec!["demo"]);
        assert_eq!(
            files(&result[0]),
            vec![
                pair("/work/demo/LICENSE.txt", ""),
                pair("/t/libdemo.so", "runtimes/linux-x64/native"),
            ]
        );
    }

    #[test]
    fn test_errors() {
        let pkg = package(vec![target("demo", "lib", &["lib"])], json!(null));
        assert!(matches!(
            packages(&pkg, &[], "linux-x64"),
            Err(Error::NothingToPackage { .. })
        ));

        let (pkg, _) = lib_and_bin();
        let err = packages(&pkg, &[], "linux-x64").unwrap_err();
        assert_eq!(
            err.to_string(),
            "package demo: no cdylib build artifact; was it built for this target?"
        );

        let (mut pkg, artifacts) = lib_and_bin();
        pkg.metadata = json!({ "nuspec": { "packages": [
            { "kind": "app", "id": "x", "id-suffix-rid": false },
            { "kind": "native", "id": "x" },
        ]}});
        assert!(matches!(
            packages(&pkg, &artifacts, "win-x64"),
            Err(Error::DuplicateId(id)) if id == "x"
        ));

        pkg.metadata = json!({ "nuspec": { "package": {} } });
        let err = packages(&pkg, &artifacts, "win-x64").unwrap_err();
        assert!(err.to_string().contains("unknown field `package`"), "{err}");

        pkg.metadata = json!(null);
        pkg.description = None;
        let err = packages(&pkg, &artifacts, "win-x64").unwrap_err();
        assert!(matches!(err, Error::Invalid { .. }), "{err}");
    }

    #[test]
    fn test_ignores_other_packages_artifacts() {
        let (pkg, mut artifacts) = lib_and_bin();
        for a in &mut artifacts {
            a.package_id = cargo_metadata::PackageId {
                repr: "registry+https://github.com/rust-lang/crates.io-index#dep@1.0.0".to_string(),
            };
        }
        assert!(matches!(
            packages(&pkg, &artifacts, "win-x64"),
            Err(Error::MissingArtifact { .. })
        ));
    }

    #[test]
    fn test_strip_email() {
        assert_eq!(strip_email("Jane Doe <jane@example.com>"), "Jane Doe");
        assert_eq!(strip_email(" Jane "), "Jane");
        assert_eq!(strip_email("<jane@example.com>"), "");
        assert_eq!(strip_email("a < b"), "a < b");
    }

    #[test]
    fn test_rid_for_target() {
        for (triple, rid) in [
            ("x86_64-pc-windows-msvc", "win-x64"),
            ("x86_64-pc-windows-gnu", "win-x64"),
            ("i686-pc-windows-msvc", "win-x86"),
            ("aarch64-pc-windows-msvc", "win-arm64"),
            ("x86_64-unknown-linux-gnu", "linux-x64"),
            ("aarch64-unknown-linux-gnu", "linux-arm64"),
            ("armv7-unknown-linux-gnueabihf", "linux-arm"),
            ("x86_64-unknown-linux-musl", "linux-musl-x64"),
            ("aarch64-unknown-linux-musl", "linux-musl-arm64"),
            ("aarch64-linux-android", "android-arm64"),
            ("x86_64-apple-darwin", "osx-x64"),
            ("aarch64-apple-darwin", "osx-arm64"),
            ("x86_64-unknown-freebsd", "freebsd-x64"),
            ("riscv64gc-unknown-linux-gnu", "linux-riscv64"),
        ] {
            assert_eq!(rid_for_target(triple).as_deref(), Some(rid), "{triple}");
        }
        for triple in [
            "wasm32-unknown-unknown",
            "aarch64-apple-ios",
            "mips-unknown-linux-gnu",
            "",
        ] {
            assert_eq!(rid_for_target(triple), None, "{triple}");
        }
    }
}
