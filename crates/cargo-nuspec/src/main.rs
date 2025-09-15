use anyhow::{Context, Result};
use cargo_metadata::{MetadataCommand, Package};
use clap::{Args, Parser, Subcommand};
use nuspec::{self, Package as NuspecPackage};
use std::path::PathBuf;
use std::fs;

#[derive(Parser)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Nuspec(NuspecArgs),
}

#[derive(Args)]
struct NuspecArgs {
    /// The name of the package to generate .nuspec for
    package_name: String,
    
    /// Output directory for the .nuspec file
    #[arg(short, long)]
    output: Option<PathBuf>,
    
    /// Path to the Cargo.toml file of the workspace
    #[arg(short, long, default_value = "./Cargo.toml")]
    manifest_path: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Nuspec(args) => generate_nuspec(args),
    }
}

fn generate_nuspec(args: NuspecArgs) -> Result<()> {
    // Get cargo metadata
    let mut cmd = MetadataCommand::new();
    cmd.manifest_path(&args.manifest_path);
    let metadata = cmd.exec()
        .context("Failed to get cargo metadata")?;
    
    // Find the package
    let package = metadata
        .packages
        .iter()
        .find(|pkg| pkg.name == args.package_name)
        .context(format!("Package '{}' not found", args.package_name))?;
    
    // Generate nuspec package
    let nuspec_package = create_nuspec_package(package)?;
    
    // Determine output directory
    let output_dir = args.output.unwrap_or_else(|| {
        package.manifest_path.parent().unwrap().into()
    });
    
    // Ensure output directory exists
    fs::create_dir_all(&output_dir)
        .context("Failed to create output directory")?;
    
    // Write nuspec file
    let file_name = output_dir.join(format!("{}.nuspec", package.name));
    let serialized = nuspec::to_string_indent(&nuspec_package, ' ', 2)
        .context("Failed to serialize nuspec package")?;
    
    let mut file_content = String::new();
    file_content.push_str(r#"<?xml version="1.0" encoding="utf-8"?>"#);
    file_content.push('\n');
    file_content.push_str(&serialized);
    
    fs::write(&file_name, file_content)
        .context("Failed to write nuspec file")?;
    
    println!("Generated .nuspec file: {}", file_name.display());
    
    Ok(())
}

pub fn create_nuspec_package(package: &Package) -> Result<NuspecPackage> {
    let mut nuspec_pkg = NuspecPackage::default();
    
    // Set basic metadata
    nuspec_pkg.metadata.id = package.name.clone();
    nuspec_pkg.metadata.version = package.version.to_string();
    nuspec_pkg.metadata.description = package.description.clone().unwrap_or_default();
    nuspec_pkg.metadata.authors = package.authors.clone();
    
    // Set optional fields
    if let Some(homepage) = &package.homepage {
        nuspec_pkg.metadata.project_url = Some(homepage.clone());
    }
    
    if let Some(license) = &package.license {
        nuspec_pkg.metadata.license = Some(nuspec::License::Expression(license.clone()));
    }
    
    if let Some(repository) = &package.repository {
        nuspec_pkg.metadata.repository = Some(nuspec::Repository {
            url: Some(repository.clone()),
            ..Default::default()
        });
    }
    
    if !package.keywords.is_empty() {
        nuspec_pkg.metadata.tags = Some(package.keywords.clone());
    }
    
    Ok(nuspec_pkg)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_create_nuspec_package_basic() {
        // Create a mock package by using cargo_metadata to get a real one from our workspace
        let mut cmd = MetadataCommand::new();
        cmd.manifest_path("./Cargo.toml");
        let metadata = cmd.exec().unwrap();
        
        // Get the nuspec package as our test subject
        let package = metadata
            .packages
            .iter()
            .find(|pkg| pkg.name == "nuspec")
            .unwrap();
        
        let nuspec_package = create_nuspec_package(package).unwrap();
        
        assert_eq!(nuspec_package.metadata.id, "nuspec");
        assert_eq!(nuspec_package.metadata.version, "0.2.0");
        assert!(!nuspec_package.metadata.description.is_empty());
        assert!(!nuspec_package.metadata.authors.is_empty());
    }
}