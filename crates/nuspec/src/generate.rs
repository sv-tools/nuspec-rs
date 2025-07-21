use crate::{File, Files, License, Package, Repository, to_string_indent};
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::{env, error, fs};

/// Generates a NuSpec file based on the Cargo package metadata.
/// The generated file will be placed in the output directory next to the Cargo build artifacts,
/// such as executables or libraries.
///
/// All file paths in the generated NuSpec file are relative to the output directory.
/// If you want to use absolute paths, you need to set them explicitly in the metadata.
///
/// The generated NuSpec file without any explicit modifications is supposed to stay in
/// the same directory to be used by the `nuget` command line tool.
pub fn generate() -> Result<(), Box<dyn error::Error>> {
    let out_dir = get_build_artifacts_path()?;
    generate_to(out_dir)
}

/// Generates a NuSpec file and writes it to the specified output directory.
pub fn generate_to(out_dir: PathBuf) -> Result<(), Box<dyn error::Error>> {
    let pkg = load_package_config(out_dir.clone())?;
    let file_name = out_dir
        .join(pkg.metadata.id.clone())
        .with_extension("nuspec");
    let serialized = to_string_indent(&pkg, ' ', 2)?;
    let mut file = fs::File::create(file_name.clone())?;
    file.write_all(r#"<?xml version="1.0" encoding="utf-8"?>"#.as_bytes())?;
    file.write_all(b"\n")?;
    file.write_all(serialized.as_bytes())?;

    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    pub package: Option<ManifestPackage>,
    #[serde(rename = "bin")]
    pub binary: Option<Vec<ManifestBinary>>,
    pub lib: Option<ManifestLibrary>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ManifestPackage {
    pub keywords: Option<ManifestPackageKeywords>,
    pub metadata: Option<ManifestPackageMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ManifestPackageKeywords {
    Keywords(Vec<String>),
    Workspace(ManifestPackageKeywordsWorkspace),
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestPackageKeywordsWorkspace {
    pub workspace: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ManifestPackageMetadata {
    pub nuspec: Option<ManifestPackageMetadataNuspec>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ManifestPackageMetadataNuspec {
    pub package: Option<Package>,
    pub out_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestBinary {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestLibrary {
    pub name: Option<String>,
    #[serde(rename = "crate-type")]
    pub crate_type: Option<Vec<ManifestCrateType>>,
}

#[derive(Debug, Clone, Deserialize)]
enum ManifestCrateType {
    #[serde(rename = "bin")]
    Bin,
    #[serde(rename = "lib")]
    Lib,
    #[serde(rename = "rlib")]
    Rlib,
    #[serde(rename = "dylib")]
    Dylib,
    #[serde(rename = "cdylib")]
    Cdylib,
    #[serde(rename = "staticlib")]
    Staticlib,
    #[serde(rename = "proc-macro")]
    Procmacro,
}

fn load_package_config(out_dir: PathBuf) -> Result<Package, Box<dyn error::Error>> {
    let manifest_file = PathBuf::from(env::var("CARGO_MANIFEST_PATH")?);
    let manifest_content = fs::read_to_string(manifest_file)?;
    let manifest: Manifest = toml::from_str(&manifest_content)?;
    let build_artifacts_path = get_build_artifacts_path()?;
    let nuspec_config = &manifest
        .package
        .clone()
        .and_then(|p| p.metadata)
        .and_then(|m| m.nuspec)
        .unwrap_or_default();
    let out_dir = nuspec_config
        .out_dir
        .clone()
        .and_then(|dir| {
            if dir.is_empty() {
                None
            } else {
                Some(PathBuf::from(dir))
            }
        })
        .unwrap_or(out_dir);
    if !out_dir.exists() {
        fs::create_dir_all(out_dir.clone())?;
    }
    if !out_dir.is_dir() {
        return Err(format!("The `out_dir` is not a directory: {out_dir:?}").into());
    }
    let mut pkg = nuspec_config.package.clone().unwrap_or_default();
    let mut files = pkg.files.unwrap_or_default().file;
    for file in files.iter_mut() {
        let file_path = PathBuf::from(&file.src);
        if file_path.is_relative() {
            file.src = get_relative_path(&out_dir, file.src.clone())?;
        }
    }

    let pkg_name = env::var("CARGO_PKG_NAME")?;
    if pkg.metadata.id.is_empty() {
        pkg.metadata.id = pkg_name.clone();
    }
    if pkg.metadata.version.is_empty() {
        pkg.metadata.version = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    }
    if pkg.metadata.description.is_empty() {
        pkg.metadata.description = env::var("CARGO_PKG_DESCRIPTION").unwrap_or_default();
    }
    if pkg.metadata.authors.is_empty() {
        pkg.metadata.authors = env::var("CARGO_PKG_AUTHORS")
            .unwrap_or_default()
            .split(':')
            .map(|s| s.trim().to_string())
            .collect();
    }
    if pkg.metadata.project_url.is_none() {
        pkg.metadata.project_url = match env::var("CARGO_PKG_HOMEPAGE") {
            Ok(url) => {
                if url.is_empty() {
                    None
                } else {
                    Some(url)
                }
            }
            _ => None,
        };
    }
    if pkg.metadata.license.is_none() {
        pkg.metadata.license = match env::var("CARGO_PKG_LICENSE") {
            Ok(expression) => {
                if expression.is_empty() {
                    None
                } else {
                    Some(License::Expression(expression))
                }
            }
            _ => None,
        };
    }
    if pkg.metadata.license.is_none() {
        pkg.metadata.license = match env::var("CARGO_PKG_LICENSE_FILE") {
            Ok(path) => {
                if path.is_empty() {
                    None
                } else {
                    let license_path = get_relative_path(&out_dir, path)?;
                    let license_file_name = Path::new(&license_path).file_name().ok_or(format!(
                        "Failed to get the file name from the license: {license_path}"
                    ))?;
                    push_file(&mut files, license_path.clone(), "");
                    Some(License::File(
                        license_file_name.to_string_lossy().to_string(),
                    ))
                }
            }
            _ => None,
        };
    }

    if pkg.metadata.tags.is_none() {
        pkg.metadata.tags = if let Some(p) = manifest.package {
            if let Some(k) = p.keywords {
                match k {
                    ManifestPackageKeywords::Keywords(keywords) => Some(keywords),
                    ManifestPackageKeywords::Workspace(w) => {
                        if w.workspace {
                            let workspace_manifest = get_workspace_manifest_path()?;
                            if let Some(workspace_manifest) = workspace_manifest.workspace {
                                workspace_manifest
                                    .package
                                    .and_then(|p| p.keywords)
                                    .and_then(|k| match k {
                                        ManifestPackageKeywords::Keywords(keywords) => {
                                            Some(keywords)
                                        }
                                        ManifestPackageKeywords::Workspace(_) => None,
                                    })
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
    }

    if pkg.metadata.repository.is_none() {
        pkg.metadata.repository = match env::var("CARGO_PKG_REPOSITORY") {
            Ok(url) => {
                if url.is_empty() {
                    None
                } else {
                    Some(Repository {
                        url: Some(url),
                        ..Default::default()
                    })
                }
            }
            _ => None,
        };
    }
    if pkg.metadata.readme.is_none() {
        pkg.metadata.readme = match env::var("CARGO_PKG_README") {
            Ok(path) => {
                if path.is_empty() {
                    None
                } else {
                    let readme_path = get_relative_path(&out_dir, path)?;
                    let readme_file_name = Path::new(&readme_path).file_name().ok_or(format!(
                        "Failed to get the file name from readme: {readme_path}"
                    ))?;
                    push_file(&mut files, readme_path.clone(), "");
                    Some(readme_file_name.to_string_lossy().to_string())
                }
            }
            _ => None,
        };
    }

    manifest.binary.unwrap_or_default().iter().for_each(|b| {
        let base_name = build_artifacts_path.join(b.name.clone());
        let relative_path =
            get_relative_path(&out_dir, base_name.as_path().to_string_lossy().to_string())
                .unwrap_or(b.name.clone());
        let relative_path = Path::new(&relative_path);
        #[cfg(target_os = "windows")]
        {
            push_file(
                &mut files,
                relative_path
                    .with_extension("exe")
                    .to_string_lossy()
                    .to_string(),
                "tools",
            );

            let name = b.name.clone().replace("-", "_");
            let base_name = build_artifacts_path.join(name.clone());
            let relative_path =
                get_relative_path(&out_dir, base_name.as_path().to_string_lossy().to_string())
                    .unwrap_or(name.clone());
            let relative_path = Path::new(&relative_path);
            push_file(
                &mut files,
                relative_path
                    .with_extension("pdb")
                    .to_string_lossy()
                    .to_string(),
                "tools",
            );
        }
        #[cfg(not(target_os = "windows"))]
        {
            push_file(
                &mut files,
                relative_path.to_string_lossy().to_string(),
                "tools",
            );
        }
    });
    if let Some(l) = manifest.lib {
        let name = l.name.unwrap_or(pkg_name.clone().replace("-", "_"));
        let base_name = build_artifacts_path.join(name.clone());
        let relative_path =
            get_relative_path(&out_dir, base_name.as_path().to_string_lossy().to_string())
                .unwrap_or(name.clone());
        let relative_path = Path::new(&relative_path);
        if l.crate_type.is_none() {
            println!(
                "cargo:warning=No `crate-type` specified for the `lib` crate, please choose a more specific or configure a files section manually."
            );
        }
        if let Some(crate_types) = l.crate_type {
            if crate_types.is_empty() {
                println!(
                    "cargo:warning=No `crate-type` specified for the `lib` crate, please choose a more specific or configure a files section manually."
                );
            }
            for crate_type in crate_types {
                match crate_type {
                    ManifestCrateType::Bin => {
                        #[cfg(target_os = "windows")]
                        {
                            push_file(
                                &mut files,
                                relative_path
                                    .with_extension("exe")
                                    .to_string_lossy()
                                    .to_string(),
                                "tools",
                            );
                            push_file(
                                &mut files,
                                relative_path
                                    .with_extension("pdb")
                                    .to_string_lossy()
                                    .to_string(),
                                "tools",
                            );
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            push_file(
                                &mut files,
                                relative_path.to_string_lossy().to_string(),
                                "tools",
                            );
                        }
                    }
                    ManifestCrateType::Lib => {
                        println!(
                            "cargo:warning=A `lib` crate-type is not supported, please choose a more specific or configure a files section manually."
                        );
                    }
                    ManifestCrateType::Rlib => {
                        push_file(
                            &mut files,
                            relative_path
                                .with_file_name(format!("lib{name}.rlib"))
                                .to_string_lossy()
                                .to_string(),
                            "lib",
                        );
                    }
                    ManifestCrateType::Cdylib | ManifestCrateType::Dylib => {
                        #[cfg(target_os = "windows")]
                        {
                            push_file(
                                &mut files,
                                relative_path
                                    .with_extension("dll")
                                    .to_string_lossy()
                                    .to_string(),
                                "lib",
                            );
                            push_file(
                                &mut files,
                                relative_path
                                    .with_extension("pdb")
                                    .to_string_lossy()
                                    .to_string(),
                                "lib",
                            );
                        }
                        #[cfg(target_os = "macos")]
                        {
                            push_file(
                                &mut files,
                                relative_path
                                    .with_file_name(format!("lib{name}.dylib"))
                                    .to_string_lossy()
                                    .to_string(),
                                "lib",
                            );
                        }
                        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                        {
                            push_file(
                                &mut files,
                                relative_path
                                    .with_file_name(format!("lib{name}.so"))
                                    .to_string_lossy()
                                    .to_string(),
                                "lib",
                            );
                        }
                    }
                    ManifestCrateType::Staticlib => {
                        #[cfg(all(target_os = "windows", target_env = "msvc"))]
                        push_file(
                            &mut files,
                            relative_path
                                .with_extension("lib")
                                .to_string_lossy()
                                .to_string(),
                            "lib",
                        );
                        #[cfg(not(all(target_os = "windows", target_env = "msvc")))]
                        push_file(
                            &mut files,
                            relative_path
                                .with_file_name(format!("lib{name}.a"))
                                .to_string_lossy()
                                .to_string(),
                            "lib",
                        );
                    }
                    ManifestCrateType::Procmacro => {
                        println!(
                            "cargo:warning=A `proc-macro` crate-type is not supported, please choose a more specific or configure a files section manually."
                        );
                    }
                }
            }
        }
    };

    pkg.files = if files.is_empty() {
        None
    } else {
        Some(Files { file: files })
    };

    Ok(pkg)
}

fn push_file(files: &mut Vec<File>, src: String, target: &str) {
    let src_file_name = Path::new(&src).file_name();
    if src_file_name.is_none() {
        return;
    }
    let src_file_name = src_file_name.unwrap().to_string_lossy().to_string();
    for file in files.iter() {
        if file.src.ends_with(&src_file_name) {
            // If the file already exists, we do not add it again
            return;
        }
    }
    files.push(File {
        src,
        target: Some(
            PathBuf::from(target)
                .join(src_file_name)
                .to_string_lossy()
                .to_string(),
        ),
        ..Default::default()
    });
}

// Retrieves the output directory path from the environment variable `OUT_DIR`
// and navigates up to the directory that matches the current build profile.
fn get_build_artifacts_path() -> Result<PathBuf, Box<dyn error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let mut out_path = out_dir.as_path();
    let profile = env::var("PROFILE")?;
    while let Some(parent) = out_path.parent() {
        if parent.ends_with(&profile) {
            return Ok(parent.into());
        }
        out_path = parent;
    }
    Err("No output directory found".into())
}

// finds the relative path from `from_dir` to `to_file` if it is in the same directory tree
// otherwise returns the absolute path to `to_file`
fn get_relative_path(from_dir: &PathBuf, to_file: String) -> Result<String, Box<dyn error::Error>> {
    let to_file = fs::canonicalize(to_file)?;
    let from_dir = fs::canonicalize(from_dir)?;
    let mut from_dir_components = from_dir.components();
    let mut to_file_components = to_file.components();

    // Check if the `to_file` is in the same directory tree as `from_dir`
    if !from_dir_components.next().eq(&to_file_components.next()) {
        return Ok(to_file.to_string_lossy().to_string());
    }

    // Skip the common components
    let mut from_dir_component = from_dir_components.next();
    let mut to_file_component = to_file_components.next();
    while from_dir_component.eq(&to_file_component) {
        from_dir_component = from_dir_components.next();
        to_file_component = to_file_components.next();
    }

    let mut relative_path = PathBuf::new();
    if from_dir_component.is_some() {
        // Add `..` for each component in `from_dir` that is not in `to_file`
        // Manually add one `..` because it was skipped in the loop above
        relative_path.push("..");
        for _ in from_dir_components {
            relative_path.push("..");
        }
    }
    // Add the remaining components of `to_file`
    if let Some(component) = to_file_component {
        relative_path.push(component.as_os_str());
        for component in to_file_components {
            relative_path.push(component.as_os_str());
        }
    }

    Ok(relative_path.as_path().to_string_lossy().to_string())
}

#[derive(Debug, Clone, Deserialize)]
struct WorkspaceManifest {
    pub workspace: Option<Manifest>,
}

fn get_workspace_manifest_path() -> Result<WorkspaceManifest, Box<dyn error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let mut workspace_path = manifest_dir.as_path();
    while let Some(parent) = workspace_path.parent() {
        let manifest_file = parent.join("Cargo.toml");
        if manifest_file.exists() {
            let serialized = fs::read_to_string(&manifest_file)?;
            let manifest = toml::from_str::<WorkspaceManifest>(&serialized)?;
            return Ok(manifest);
        }
        workspace_path = parent;
    }
    Err("No workspace manifest found".into())
}
