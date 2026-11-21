//! Implementation of the [NuGet package specification](https://learn.microsoft.com/en-us/nuget/reference/nuspec)
//! (`.nuspec`): read, validate and write `.nuspec` documents.
//!
//! With the `cargo` feature, the [`cargo`] module builds packages from `cargo metadata` and
//! `cargo build --message-format=json` output, and stages or packs them into `.nupkg` files.
//! The [`cargo-nuspec`](https://crates.io/crates/cargo-nuspec) subcommand is built on it.

#[cfg(feature = "cargo")]
pub mod cargo;
mod spec;
mod validate;
mod xml;

pub use spec::*;
pub use validate::*;
pub use xml::*;
