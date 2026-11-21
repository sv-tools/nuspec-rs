# nuspec

Package Rust crates as NuGet packages: `cdylib` libraries for .NET to call through P/Invoke,
and binaries to distribute as apps or services.

## `cargo nuspec`

```sh
cargo install cargo-nuspec

# Build in release mode and write target/nuspec/<id>.nuspec for every package
cargo nuspec --release

# Also write the .nupkg files, no nuget or dotnet needed
cargo nuspec --release --pack

# Copy everything into a self-contained directory per package, e.g. to sign the
# binaries in another CI job before packing
cargo nuspec --release --stage --out-dir dist
```

`cargo nuspec` runs `cargo build` and uses the exact files Cargo reports, so it works with
`--target`, custom profiles and workspaces (`-p`, `--workspace`). It prints the paths of the
files it writes.

Without configuration, a crate gets:

- a **native package** for a `cdylib`: the library in `runtimes/<rid>/native/`, where the .NET
  SDK picks it up for the matching platform;
- an **app package** for binaries, with the runtime identifier appended to the id
  (`my-app.linux-x64`): the binaries in `tools/`, executable after unzipping.

The id, version, description, authors, license, readme, project URL, repository and tags come
from `Cargo.toml`, including workspace-inherited fields. Everything can be overridden:

```toml
[package.metadata.nuspec]
stage = true                        # same as --stage
out-dir = "dist/nuget"              # relative to the workspace root

[package.metadata.nuspec.metadata]  # any nuspec field, for every package
copyright = "Copyright (c) Contoso"

[[package.metadata.nuspec.packages]]
kind = "native"
id = "Contoso.Engine.Native"

[[package.metadata.nuspec.packages]]
kind = "app"
id = "Contoso.Engine"
bins = ["engine"]
layout = "root"                     # binaries in the package root instead of tools/
files = [{ src = "deploy/*.ps1", target = "scripts" }]
```

## Crates

- [`cargo-nuspec`](crates/cargo-nuspec): the `cargo nuspec` subcommand.
- [`nuspec`](crates/nuspec): the NuGet package specification. Reads (`Package::from_xml`),
  validates (`Package::validate`) and writes (`Package::to_xml`) `.nuspec` files. With the
  `cargo` feature, builds packages from Cargo metadata, stages them and writes `.nupkg` files.
- [`nuspec-test`](crates/nuspec-test): a test crate with a `cdylib` and binaries, packaged and
  consumed from .NET in CI.
