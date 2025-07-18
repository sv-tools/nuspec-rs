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
    let mut file = fs::File::create(file_name)?;
    file.write_all(r#"<?xml version="1.0" encoding="UTF-8"?>"#.as_bytes())?;
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
    pub metadata: Option<ManifestPackageMetadata>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ManifestPackageMetadata {
    pub nuspec: Option<Package>,
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
    let manifest_content = std::fs::read_to_string(manifest_file)?;
    let manifest: Manifest = toml::from_str(&manifest_content)?;
    let build_artifacts_path = get_build_artifacts_path()?;

    let mut pkg = manifest
        .package
        .unwrap_or_default()
        .metadata
        .unwrap_or_default()
        .nuspec
        .unwrap_or_default();
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
                    Some(License::File(get_relative_path(out_dir.clone(), path)?))
                }
            }
            _ => None,
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
    let mut files = vec![];
    if pkg.metadata.readme.is_none() && pkg.files.is_none() {
        pkg.metadata.readme = match env::var("CARGO_PKG_README") {
            Ok(path) => {
                if path.is_empty() {
                    None
                } else {
                    let readme_path = get_relative_path(out_dir.clone(), path)?;
                    let readme_file_name = Path::new(&readme_path)
                        .file_name()
                        .ok_or("Failed to get file name from readme")?;
                    let docs_readme = Path::new("docs").join(readme_file_name);
                    files.push(File {
                        src: readme_path,
                        target: Some("docs".to_string()),
                        ..Default::default()
                    });
                    Some(docs_readme.to_string_lossy().to_string())
                }
            }
            _ => None,
        };
    }
    if pkg.files.is_none() {
        pkg.files = {
            manifest.binary.unwrap_or_default().iter().for_each(|b| {
                let base_name = build_artifacts_path.join(b.name.clone());
                let relative_path = get_relative_path(
                    out_dir.clone(),
                    base_name.as_path().to_string_lossy().to_string(),
                )
                .unwrap_or(b.name.clone());
                let relative_path = Path::new(&relative_path);
                #[cfg(target_os = "windows")]
                {
                    files.push(File {
                        src: relative_path
                            .with_extension("exe")
                            .to_str()
                            .unwrap()
                            .to_string(),
                        target: Some("tools".to_string()),
                        ..Default::default()
                    });

                    let name = b.name.clone().replace("-", "_");
                    let base_name = build_artifacts_path.join(name.clone());
                    let relative_path = get_relative_path(
                        out_dir.clone(),
                        base_name.as_path().to_string_lossy().to_string(),
                    )
                    .unwrap_or(name.clone());
                    let relative_path = Path::new(&relative_path);
                    files.push(File {
                        src: relative_path
                            .with_extension("pdb")
                            .to_str()
                            .unwrap()
                            .to_string(),
                        target: Some("tools".to_string()),
                        ..Default::default()
                    });
                }
                #[cfg(not(target_os = "windows"))]
                {
                    files.push(File {
                        src: relative_path.to_string_lossy().to_string(),
                        target: Some("tools".to_string()),
                        ..Default::default()
                    });
                }
            });
            if let Some(l) = manifest.lib {
                let name = l.name.unwrap_or(pkg_name.clone().replace("-", "_"));
                let base_name = build_artifacts_path.join(name.clone());
                let relative_path = get_relative_path(
                    out_dir.clone(),
                    base_name.as_path().to_string_lossy().to_string(),
                )
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
                                    files.push(File {
                                        src: relative_path
                                            .with_extension("exe")
                                            .to_str()
                                            .unwrap()
                                            .to_string(),
                                        target: Some("tools".to_string()),
                                        ..Default::default()
                                    });
                                    files.push(File {
                                        src: relative_path
                                            .with_extension("pdb")
                                            .to_str()
                                            .unwrap()
                                            .to_string(),
                                        target: Some("tools".to_string()),
                                        ..Default::default()
                                    });
                                }
                                #[cfg(not(target_os = "windows"))]
                                {
                                    files.push(File {
                                        src: relative_path.to_string_lossy().to_string(),
                                        target: Some("tools".to_string()),
                                        ..Default::default()
                                    });
                                }
                            }
                            ManifestCrateType::Lib => {
                                println!(
                                    "cargo:warning=A `lib` crate-type is not supported, please choose a more specific or configure a files section manually."
                                );
                            }
                            ManifestCrateType::Rlib => {
                                files.push(File {
                                    src: relative_path
                                        .with_file_name(format!("lib{name}.rlib"))
                                        .to_str()
                                        .unwrap()
                                        .to_string(),
                                    target: Some("lib".to_string()),
                                    ..Default::default()
                                });
                            }
                            ManifestCrateType::Cdylib | ManifestCrateType::Dylib => {
                                #[cfg(target_os = "windows")]
                                {
                                    files.push(File {
                                        src: relative_path
                                            .with_extension("dll")
                                            .to_str()
                                            .unwrap()
                                            .to_string(),
                                        target: Some("lib".to_string()),
                                        ..Default::default()
                                    });
                                    files.push(File {
                                        src: relative_path
                                            .with_extension("pdb")
                                            .to_str()
                                            .unwrap()
                                            .to_string(),
                                        target: Some("lib".to_string()),
                                        ..Default::default()
                                    });
                                }
                                #[cfg(target_os = "macos")]
                                {
                                    files.push(File {
                                        src: relative_path
                                            .with_extension("dylib")
                                            .to_str()
                                            .unwrap()
                                            .to_string(),
                                        target: Some("lib".to_string()),
                                        ..Default::default()
                                    });
                                }
                                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                                {
                                    files.push(File {
                                        src: relative_path
                                            .with_file_name(format!("lib{name}.so"))
                                            .to_str()
                                            .unwrap()
                                            .to_string(),
                                        target: Some("lib".to_string()),
                                        ..Default::default()
                                    });
                                }
                            }
                            ManifestCrateType::Staticlib => {
                                #[cfg(all(target_os = "windows", target_env = "msvc"))]
                                files.push(File {
                                    src: relative_path
                                        .with_extension("lib")
                                        .to_str()
                                        .unwrap()
                                        .to_string(),
                                    target: Some("lib".to_string()),
                                    ..Default::default()
                                });
                                #[cfg(not(all(target_os = "windows", target_env = "msvc")))]
                                files.push(File {
                                    src: relative_path
                                        .with_file_name(format!("lib{name}.a"))
                                        .to_str()
                                        .unwrap()
                                        .to_string(),
                                    target: Some("lib".to_string()),
                                    ..Default::default()
                                });
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
            if files.is_empty() {
                None
            } else {
                Some(Files { file: files })
            }
        }
    }

    Ok(pkg)
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
fn get_relative_path(from_dir: PathBuf, to_file: String) -> Result<String, Box<dyn error::Error>> {
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
