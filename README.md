# nuspec
Generating .nuspec file for a Rust project to pack as a Nuget package.

## Crates

### `nuspec`

* Implementation of the full NuGet package specification. Can be used independently to work with `.nuspec` files.
* Implements a `nuspec` generator. It is guarded by the `generate` feature and enabled by default.

### `nuspec-test`

A test crate to test the `nuspec` crate and to publish it as a NuGet package.
