# cargo-nuspec

A cargo subcommand to generate .nuspec files for Rust packages.

## Installation

Install as a cargo tool:

```bash
cargo install cargo-nuspec
```

## Usage

Generate a .nuspec file for a specific package in your workspace:

```bash
cargo nuspec <package-name>
```

The generated .nuspec file will be placed in the package's directory (same directory as the package's Cargo.toml file).

### Options

- `-o, --output <OUTPUT>`: Specify a custom output directory for the .nuspec file
- `-m, --manifest-path <MANIFEST_PATH>`: Path to the workspace Cargo.toml file (defaults to `./Cargo.toml`)

### Examples

Generate a .nuspec file for a package named "my-package":

```bash
cargo nuspec my-package
```

Generate a .nuspec file and place it in a specific directory:

```bash
cargo nuspec my-package -o /path/to/output
```

Use a different workspace manifest file:

```bash
cargo nuspec my-package -m /path/to/workspace/Cargo.toml
```

## How it works

The tool reads the package metadata from Cargo.toml and generates a corresponding .nuspec file with the appropriate mappings:

- Package name → NuGet package id
- Version → NuGet package version  
- Description → NuGet package description
- Authors → NuGet package authors
- Homepage → NuGet project URL
- License → NuGet license expression
- Repository → NuGet repository URL
- Keywords → NuGet tags

The generated .nuspec file can then be used with the `nuget pack` command to create a NuGet package.