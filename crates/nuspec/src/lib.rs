//! Implementation of the [NuGet package specification](https://learn.microsoft.com/en-us/nuget/reference/nuspec)
//! (`.nuspec`): read, validate and write `.nuspec` documents.
//!
//! With the `generate` feature (enabled by default), it can also generate a `.nuspec` file
//! for a Cargo package from a build script.
//!
//! With the `cargo` feature, the [`cargo`] module builds packages from `cargo metadata` and
//! `cargo build --message-format=json` output.

#[cfg(feature = "cargo")]
pub mod cargo;
#[cfg(feature = "generate")]
mod generate;
mod spec;
mod validate;
mod xml;

#[cfg(feature = "generate")]
pub use generate::*;
pub use spec::*;
pub use validate::*;
pub use xml::*;
