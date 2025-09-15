# nuspec
Generating .nuspec file for a Rust project to pack as a Nuget package.

## Crates

### `nuspec`

* Implementation of the full NuGet package specification. Can be used independently to work with `.nuspec` files.
* Implements a `nuspec` generator. It is guarded by the `generate` feature and enabled by default.

### `nuspec-test`

A test crate to test the `nuspec` crate and to publish it as a NuGet package.

### `cargo-nuspec`

A cargo subcommand to generate .nuspec files for Rust packages. Can be installed as a cargo tool:

```bash
cargo install cargo-nuspec
```

Usage:

```bash
cargo nuspec <package-name>
```

This will generate a `.nuspec` file for the specified package in the package's directory. You can also specify an output directory:

```bash
cargo nuspec <package-name> -o /path/to/output
```
