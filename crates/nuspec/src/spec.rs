use quick_xml::SeError;
use quick_xml::se::Serializer;
use serde::{Deserialize, Serialize};
use std::fmt::Display;

/// A representation of a NuGet package specification (nuspec).
///
/// See [NuGet documentation](https://docs.microsoft.com/en-us/nuget/reference/nuspec) for more details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename = "package", deny_unknown_fields)]
pub struct Package {
    #[serde(
        rename = "@xmlns",
        default,
        skip_serializing_if = "Option::is_none",
        alias = "xmlns"
    )]
    pub namespace: Option<String>,
    #[serde(default)]
    pub metadata: Metadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Files>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    /// The case-insensitive package identifier, which must be unique across nuget.org or
    /// whatever gallery the package resides in.
    /// IDs may not contain spaces or characters that are not valid for a URL, and
    /// generally follow .NET namespace rules.
    /// See [Choosing a unique package identifier](https://learn.microsoft.com/en-us/nuget/create-packages/creating-a-package#choose-a-unique-package-identifier-and-setting-the-version-number)
    /// for guidance.
    ///
    /// When uploading a package to nuget.org, the id field is limited to 128 characters.
    #[serde(default)]
    pub id: String,
    /// The version of the package, following the major.minor.patch pattern.
    /// Version numbers may include a pre-release suffix as described in [Package versioning](https://learn.microsoft.com/en-us/nuget/concepts/package-versioning#pre-release-versions).
    ///
    /// When uploading a package to nuget.org, the version field is limited to 64 characters.
    #[serde(default)]
    pub version: String,
    /// A description of the package for UI display.
    ///
    /// When uploading a package to nuget.org, the description field is limited to 4000 characters.
    #[serde(default, alias = "summary")]
    pub description: String,
    /// A comma-separated list of package authors. The authors and the owners from the nuspec are
    /// ignored when uploading the package to nuget.org.
    /// For setting package ownership on nuget.org, see [Managing package owners on nuget.org](https://learn.microsoft.com/en-us/nuget/nuget-org/publish-a-package#managing-package-owners-on-nugetorg).
    #[serde(default, alias = "owners", with = "comma_separated")]
    pub authors: Vec<String>,

    /// A URL for the package's home page, often shown in UI displays as well as nuget.org.
    ///
    /// When uploading a package to nuget.org, the projectUrl field is limited to 4000 characters.
    #[serde(
        rename = "projectUrl",
        skip_serializing_if = "Option::is_none",
        alias = "project_url"
    )]
    pub project_url: Option<String>,

    /// An SPDX license expression or path to a license file within the package,
    /// often shown in UIs like nuget.org.
    /// If you're licensing the package under a common license, like MIT or BSD-2-Clause,
    /// use the associated [SPDX license identifier](https://spdx.org/licenses/).If your package is licensed under multiple common licenses, you can specify a composite license using the SPDX expression syntax version 2.0. For example:
    ///
    /// For example:
    ///
    /// ```xml
    /// <license type="expression">MIT</license>
    /// ```
    ///
    /// If your package is licensed under multiple common licenses, you can specify a
    /// composite license using the [SPDX expression syntax version 2.0](https://spdx.github.io/spdx-spec/v2-draft/SPDX-license-expressions/#d4-composite-license-expressions).
    /// For example:
    ///
    /// ```xml
    /// <license type="expression">BSD-2-Clause OR MIT</license>
    /// ```
    ///
    /// If you use a custom license that isn't supported by license expressions,
    /// you can package a .txt or .md file with the license's text.
    /// For example:
    ///
    /// ```xml
    /// <package>
    ///   <metadata>
    ///     ...
    ///     <license type="file">LICENSE.txt</license>
    ///     ...
    ///   </metadata>
    ///   <files>
    ///     ...
    ///     <file src="licenses\LICENSE.txt" target="" />
    ///     ...
    ///   </files>
    /// </package>
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<License>,

    /// It is a path to an image file within the package, often shown in UIs like nuget.org as
    /// the package icon.
    /// Image file size is limited to 1 MB.
    /// Supported file formats include JPEG and PNG.
    /// We recommend an image resolution of 128x128.
    ///
    /// For example, you would add the following to your nuspec when creating a package using nuget.exe:
    ///
    /// ```xml
    /// <package>
    ///   <metadata>
    ///     ...
    ///     <icon>images\icon.png</icon>
    ///     ...
    ///   </metadata>
    ///   <files>
    ///     ...
    ///     <file src="..\icon.png" target="images" />
    ///     ...
    ///   </files>
    /// </package>
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// When packing a readme file, you need to use the readme element to specify the package path,
    /// relative to the root of the package.
    /// In addition to this, you need to make sure that the file is included in the package.
    /// Supported file formats include only Markdown (.md).
    ///
    /// For example, you would add the following to your nuspec in order to pack a readme file with
    /// your project:
    ///
    /// ```xml
    /// <package>
    ///   <metadata>
    ///     ...
    ///     <readme>docs\readme.md</readme>
    ///     ...
    ///   </metadata>
    ///   <files>
    ///     ...
    ///     <file src="..\readme.md" target="docs" />
    ///     ...
    ///   </files>
    /// </package>
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readme: Option<String>,

    /// A Boolean value specifying whether the client must prompt the consumer to accept
    /// the package license before installing the package.
    #[serde(
        rename = "requireLicenseAcceptance",
        skip_serializing_if = "Option::is_none",
        alias = "require_license_acceptance"
    )]
    pub require_license_acceptance: Option<bool>,

    /// A Boolean value specifying whether the package is be marked as a development-only-dependency,
    /// which prevents the package from being included as a dependency in other packages.
    /// With PackageReference (NuGet 4.8+), this flag also means that it will exclude compile-time
    /// assets from compilation.
    /// See [DevelopmentDependency support for PackageReference](https://github.com/NuGet/Home/wiki/DevelopmentDependency-support-for-PackageReference).
    #[serde(
        rename = "developmentDependency",
        skip_serializing_if = "Option::is_none",
        alias = "development_dependencies"
    )]
    pub development_dependency: Option<bool>,

    /// A description of the changes made in this release of the package,
    /// often used in UI like the Updates tab of the Visual Studio Package Manager in place of
    /// the package description.
    #[serde(
        rename = "releaseNotes",
        skip_serializing_if = "Option::is_none",
        alias = "release_notes"
    )]
    pub release_notes: Option<String>,

    /// Copyright details for the package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,

    /// The locale ID for the package.
    /// See [Creating localized packages](https://learn.microsoft.com/en-us/nuget/create-packages/creating-localized-packages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// A space-delimited list of tags and keywords that describe the package and
    /// aid discoverability of packages through search and filtering.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "optional_space_separated"
    )]
    pub tags: Option<Vec<String>>,

    /// Repository metadata, consisting of four optional attributes: type and url (4.0+),
    /// and branch and commit (4.6+).
    /// These attributes allow you to map the .nupkg to the repository that built it,
    /// with the potential to get as detailed as the individual branch name and / or
    /// commit SHA-1 hash that built the package.
    /// This should be a publicly available url that can be invoked directly by
    /// a version control software.
    /// It should not be an html page as this is meant for the computer.
    /// For linking to project page, use the `projectUrl` field, instead.
    ///
    /// For example:
    ///
    /// ```xml
    /// <?xml version="1.0"?>
    /// <package xmlns="http://schemas.microsoft.com/packaging/2010/07/nuspec.xsd">
    ///     <metadata>
    ///         ...
    ///         <repository type="git" url="https://github.com/NuGet/NuGet.Client.git" branch="dev" commit="e1c65e4524cd70ee6e22abe33e6cb6ec73938cb3" />
    ///         ...
    ///     </metadata>
    /// </package>
    /// ```
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<Repository>,

    /// A human-friendly title of the package which may be used in some UI displays.
    /// (nuget.org and the Package Manager in Visual Studio do not show title)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Specifies the minimum version of the NuGet client that can install this package,
    /// enforced by nuget.exe and the Visual Studio Package Manager.
    /// This is used whenever the package depends on specific features of the .nuspec file that
    /// were added in a particular version of the NuGet client.
    /// For example, a package using the developmentDependency attribute should specify "2.8" for
    /// minClientVersion.
    /// Similarly, a package using the contentFiles element should set minClientVersion to "3.3".
    /// Note also that because NuGet clients prior to 2.5 do not recognize this flag,
    /// they always refuse to install the package no matter what minClientVersion contains.
    ///
    /// ```xml
    /// <?xml version="1.0" encoding="utf-8"?>
    /// <package xmlns="http://schemas.microsoft.com/packaging/2010/07/nuspec.xsd">
    ///     <metadata minClientVersion="100.0.0.1">
    ///         <id>dasdas</id>
    ///         <version>2.0.0</version>
    ///         <title />
    ///         <authors>dsadas</authors>
    ///         <owners />
    ///         <requireLicenseAcceptance>false</requireLicenseAcceptance>
    ///         <description>My package description.</description>
    ///     </metadata>
    ///     <files>
    ///         <file src="content\one.txt" target="content\one.txt" />
    ///     </files>
    /// </package>
    /// ```
    #[serde(
        rename = "@minClientVersion",
        skip_serializing_if = "Option::is_none",
        alias = "min_client_version"
    )]
    pub min_client_version: Option<String>,

    /// A collection of zero or more <packageType> elements specifying the type of the package
    /// if other than a traditional dependency package.
    /// Each packageType has attributes of name and version
    #[serde(
        rename = "packageTypes",
        skip_serializing_if = "Option::is_none",
        alias = "package_types"
    )]
    pub package_types: Option<PackageTypes>,

    /// A collection of zero or more <dependency> elements specifying the dependencies for the package.
    /// Each dependency has attributes of id, version, include (3.x+), and exclude (3.x+).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Dependencies>,

    /// A collection of zero or more <frameworkAssembly> elements identifying .NET Framework
    /// assembly references that this package requires, which ensures that references are added to
    /// projects consuming the package.
    /// Each frameworkAssembly has `assemblyName` and `targetFramework` attributes.
    #[serde(
        rename = "frameworkAssemblies",
        skip_serializing_if = "Option::is_none",
        alias = "framework_assemblies"
    )]
    pub framework_assemblies: Option<FrameworkAssemblies>,

    /// A collection of zero or more <reference> elements naming assemblies in the package's lib
    /// folder that are added as project references.
    /// Each reference has a file attribute.
    /// <references> can also contain a <group> element with a targetFramework attribute,
    /// that then contains <reference> elements.
    /// If omitted, all references in lib are included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<References>,

    /// A collection of <files> elements that identify content files to include in the consuming project.
    /// These files are specified with a set of attributes that describe how they should be used
    /// within the project system.
    #[serde(
        rename = "contentFiles",
        skip_serializing_if = "Option::is_none",
        alias = "content_files"
    )]
    pub content_files: Option<ContentFiles>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "@type", content = "$text", deny_unknown_fields)]
pub enum License {
    #[serde(rename = "expression")]
    Expression(String),
    #[serde(rename = "file")]
    File(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    /// The type of the repository, such as "git", "svn", etc.
    #[serde(
        rename = "@type",
        skip_serializing_if = "Option::is_none",
        alias = "type"
    )]
    pub repository_type: Option<String>,
    /// The URL of the repository.
    #[serde(
        rename = "@url",
        skip_serializing_if = "Option::is_none",
        alias = "url"
    )]
    pub url: Option<String>,
    /// The branch name in the repository.
    #[serde(
        rename = "@branch",
        skip_serializing_if = "Option::is_none",
        alias = "branch"
    )]
    pub branch: Option<String>,
    /// The commit SHA-1 hash in the repository.
    #[serde(
        rename = "@commit",
        skip_serializing_if = "Option::is_none",
        alias = "commit"
    )]
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageTypes {
    #[serde(rename = "packageType", alias = "package_type")]
    pub package_type: Vec<PackageType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageType {
    /// The type of the package, such as "Dependency", "DotnetTool", etc.
    #[serde(rename = "@name", with = "know_package_type", alias = "name")]
    pub name: KnownPackageType,
    /// The version of the package type.
    #[serde(
        rename = "@version",
        skip_serializing_if = "Option::is_none",
        alias = "version"
    )]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum KnownPackageType {
    Dependency,
    DotnetTool,
    MSBuildSdk,
    Template,
    Custom(String),
}

impl Display for KnownPackageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KnownPackageType::Dependency => write!(f, "Dependency"),
            KnownPackageType::DotnetTool => write!(f, "DotnetTool"),
            KnownPackageType::MSBuildSdk => write!(f, "MSBuildSdk"),
            KnownPackageType::Template => write!(f, "Template"),
            KnownPackageType::Custom(name) => write!(f, "{name}"),
        }
    }
}

// It should be an enum type, but quick_xml + serde don't properly serialize/deserialize such a combination
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Dependencies {
    /// A collection of dependencies for the package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency: Option<Vec<Dependency>>,
    /// A collection of dependency groups, each with a target framework.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Vec<DependencyGroup>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    /// The package ID of the dependency, such as "EntityFramework" and "NUnit",
    /// which is the name of the package nuget.org shows on a package page.
    #[serde(rename = "@id", alias = "id")]
    pub id: String,
    /// The range of versions acceptable as a dependency.
    /// See Package versioning for exact syntax.
    /// Floating versions are not supported.
    #[serde(rename = "@version", alias = "version")]
    pub version: String,

    /// A comma-delimited list of include tags indicating of the dependency to include in
    /// the final package.
    /// The default value is `all`.
    #[serde(
        rename = "@include",
        default,
        skip_serializing_if = "Option::is_none",
        with = "optional_comma_separated",
        alias = "include"
    )]
    pub include: Option<Vec<String>>,

    /// A comma-delimited list of exclude tags indicating of the dependency to exclude in
    /// the final package.
    /// The default value is `build,analyzers`, which can be over-written.
    /// But `content/ContentFiles` are also implicitly excluded in the final package, which
    /// can't be over-written.
    /// Tags specified with exclude take precedence over those specified with include.
    ///
    /// For example, `include="runtime, compile" exclude="compile"` is the same as `include="runtime"`.
    #[serde(
        rename = "@exclude",
        default,
        skip_serializing_if = "Option::is_none",
        with = "optional_comma_separated",
        alias = "exclude"
    )]
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DependencyGroup {
    /// The target framework for the dependencies in this group, such as "netstandard2.0" or "net5.0".
    #[serde(
        rename = "@targetFramework",
        skip_serializing_if = "Option::is_none",
        alias = "target_framework"
    )]
    pub target_framework: Option<String>,
    /// A collection of dependencies for the target framework.
    pub dependency: Vec<Dependency>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameworkAssemblies {
    /// A collection of framework assemblies required by the package.
    #[serde(rename = "frameworkAssembly", alias = "framework_assembly")]
    pub framework_assembly: Vec<FrameworkAssembly>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FrameworkAssembly {
    /// The fully qualified assembly name.
    #[serde(rename = "@assemblyName", alias = "assembly_name")]
    pub assembly_name: String,
    /// Specifies the target framework to which this reference applies.
    /// If omitted, indicates that the reference applies to all frameworks.
    /// See [Target frameworks](https://learn.microsoft.com/en-us/nuget/reference/target-frameworks)
    /// for the exact framework identifiers.
    #[serde(
        rename = "@targetFramework",
        skip_serializing_if = "Option::is_none",
        alias = "target_framework"
    )]
    pub target_framework: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct References {
    /// A collection of references for the package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<Vec<Reference>>,
    /// A collection of reference groups, each with a target framework.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Vec<ReferenceGroup>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    /// The file name of the assembly reference, such as "EntityFramework.dll" or "NUnit.Framework.dll".
    #[serde(rename = "@file", alias = "file")]
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ReferenceGroup {
    /// The target framework for the references in this group, such as "netstandard2.0" or "net5.0".
    #[serde(
        rename = "@targetFramework",
        skip_serializing_if = "Option::is_none",
        alias = "target_framework"
    )]
    pub target_framework: Option<String>,
    /// A collection of references for the target framework.
    pub reference: Vec<Reference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ContentFiles {
    /// A collection of content files for the package.
    #[serde(rename = "files")]
    pub files: Vec<ContentFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ContentFile {
    /// The location of the file or files to include, subject to exclusions specified by
    /// the exclude attribute.
    /// The path is relative to the contentFiles folder unless an absolute path is specified.
    /// The wildcard character * is allowed, and the double wildcard ** implies a recursive folder search.
    #[serde(rename = "@include", alias = "include")]
    pub include: String,

    /// A semicolon-delimited list of files or file patterns to exclude from the src location.
    /// The wildcard character * is allowed, and the double wildcard ** implies a recursive folder search.
    #[serde(
        rename = "@exclude",
        default,
        skip_serializing_if = "Option::is_none",
        with = "optional_semicolon_separated",
        alias = "exclude"
    )]
    pub exclude: Option<Vec<String>>,

    /// The build action to assign to the content item for MSBuild, such as Content, None,
    /// Embedded Resource, Compile, etc.
    /// The default is Compile.
    #[serde(rename = "@buildAction", alias = "build_action")]
    pub build_action: Option<BuildAction>,

    /// A Boolean indicating whether to copy content items to the build (or publish) output folder.
    /// The default is false.
    #[serde(
        rename = "@copyToOutput",
        skip_serializing_if = "Option::is_none",
        alias = "copy_to_output"
    )]
    pub copy_to_output: Option<bool>,

    /// A Boolean indicating whether to copy content items to a single folder in the build output (true),
    /// or to preserve the folder structure in the package (false).
    /// This flag only works when copyToOutput flag is set to true.
    /// The default is false.
    #[serde(
        rename = "@flatten",
        skip_serializing_if = "Option::is_none",
        alias = "flatten"
    )]
    pub flatten: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub enum BuildAction {
    Content,
    None,
    EmbeddedResource,
    #[default]
    Compile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Files {
    /// A collection of files included in the package.
    pub file: Vec<File>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct File {
    /// The location of the file or files to include, subject to exclusions specified by
    /// the exclude attribute.
    /// The path is relative to the .nuspec file unless an absolute path is specified.
    /// The wildcard character * is allowed, and the double wildcard ** implies a recursive folder search.
    #[serde(rename = "@src", alias = "src")]
    pub src: String,
    /// The relative path to the folder within the package where the source files are placed,
    /// which must begin with lib, content, build, or tools.
    /// See [Creating a .nuspec from a convention-based working directory](https://learn.microsoft.com/en-us/nuget/create-packages/creating-a-package#from-a-convention-based-working-directory).
    #[serde(
        rename = "@target",
        skip_serializing_if = "Option::is_none",
        alias = "target"
    )]
    pub target: Option<String>,
    /// A semicolon-delimited list of files or file patterns to exclude from the src location.
    /// The wildcard character * is allowed, and the double wildcard ** implies a recursive folder search.
    #[serde(
        rename = "@exclude",
        default,
        skip_serializing_if = "Option::is_none",
        with = "optional_semicolon_separated",
        alias = "exclude"
    )]
    pub exclude: Option<Vec<String>>,
}

mod know_package_type {
    use super::KnownPackageType;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &KnownPackageType, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<KnownPackageType, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        match s.as_str() {
            "Dependency" => Ok(KnownPackageType::Dependency),
            "DotnetTool" => Ok(KnownPackageType::DotnetTool),
            "MSBuildSdk" => Ok(KnownPackageType::MSBuildSdk),
            "Template" => Ok(KnownPackageType::Template),
            _ => Ok(KnownPackageType::Custom(s)),
        }
    }
}

mod comma_separated {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &[String], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.join(","))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        Ok(if s.is_empty() {
            vec![]
        } else {
            s.split(',').map(|s| s.trim().to_string()).collect()
        })
    }
}

mod optional_comma_separated {
    use super::comma_separated;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<Vec<String>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(v) = value {
            comma_separated::serialize(v, serializer)
        } else {
            serializer.serialize_none()
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        match comma_separated::deserialize(deserializer) {
            Ok(v) => Ok(Some(v)),
            Err(_) => Ok(None),
        }
    }
}

mod space_separated {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &[String], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.join(" "))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        Ok(if s.is_empty() {
            vec![]
        } else {
            s.split(' ').map(|s| s.trim().to_string()).collect()
        })
    }
}

mod optional_space_separated {
    use super::space_separated;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<Vec<String>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(v) = value {
            space_separated::serialize(v, serializer)
        } else {
            serializer.serialize_none()
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        match space_separated::deserialize(deserializer) {
            Ok(v) => Ok(Some(v)),
            Err(_) => Ok(None),
        }
    }
}

mod semicolon_separated {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &[String], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.join(";"))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        Ok(if s.is_empty() {
            vec![]
        } else {
            s.split(';').map(|s| s.trim().to_string()).collect()
        })
    }
}

mod optional_semicolon_separated {
    use super::semicolon_separated;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<Vec<String>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(v) = value {
            semicolon_separated::serialize(v, serializer)
        } else {
            serializer.serialize_none()
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        match semicolon_separated::deserialize(deserializer) {
            Ok(v) => Ok(Some(v)),
            Err(_) => Ok(None),
        }
    }
}

/// Serializes a value to xml format with indentation.
// TODO: create a PR to quick-xml to support indentation
pub fn to_string_indent<T>(
    value: &T,
    indent_char: char,
    indent_size: usize,
) -> Result<String, SeError>
where
    T: ?Sized + Serialize,
{
    let mut writer = String::new();
    let mut serializer = Serializer::new(&mut writer);
    serializer.indent(indent_char, indent_size);
    value.serialize(serializer)?;
    Ok(writer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialization_and_deserialization() {
        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);
    }

    #[test]
    fn test_owners_deserialization() {
        let serialized = r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><owners>Author One,Author Two</owners></metadata></package>"#;
        let deserialized: Package = quick_xml::de::from_str(serialized).unwrap();
        assert_eq!(
            deserialized.metadata.authors,
            vec!["Author One".to_string(), "Author Two".to_string()]
        );

        let serialized = r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><owners>Author One,Author Two</owners><authors>Author Three,Author Four</authors></metadata></package>"#;
        let deserialized_err = quick_xml::de::from_str::<Package>(serialized).unwrap_err();
        assert!(
            deserialized_err
                .to_string()
                .contains("duplicate field `authors`"),
            "must fail on duplicate fields"
        );
    }

    #[test]
    fn test_license_serialization_and_deserialization() {
        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                license: Some(License::Expression("MIT".to_string())),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><license type="expression">MIT</license></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);

        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                license: Some(License::File("LICENSE-MIT".to_string())),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><license type="file">LICENSE-MIT</license></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);
    }

    #[test]
    fn test_tags_serialization_deserialization() {
        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                tags: Some(vec!["tag1".to_string(), "tag2".to_string()]),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><tags>tag1 tag2</tags></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);

        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                tags: Some(vec![]),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><tags/></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);
    }

    #[test]
    fn test_package_types_serialization_deserialization() {
        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                package_types: Some(PackageTypes {
                    package_type: vec![
                        PackageType {
                            name: KnownPackageType::Dependency,
                            version: Some("1.0".to_string()),
                        },
                        PackageType {
                            name: KnownPackageType::DotnetTool,
                            version: None,
                        },
                        PackageType {
                            name: KnownPackageType::MSBuildSdk,
                            version: None,
                        },
                        PackageType {
                            name: KnownPackageType::Template,
                            version: None,
                        },
                        PackageType {
                            name: KnownPackageType::Custom("ContosoExtension".to_string()),
                            version: None,
                        },
                    ],
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><packageTypes><packageType name="Dependency" version="1.0"/><packageType name="DotnetTool"/><packageType name="MSBuildSdk"/><packageType name="Template"/><packageType name="ContosoExtension"/></packageTypes></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);
    }

    #[test]
    fn test_files_serialization_deserialization() {
        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                ..Default::default()
            },
            files: Some(Files {
                file: vec![
                    File {
                        src: "content/one.txt".to_string(),
                        target: Some("content/one.txt".to_string()),
                        exclude: None,
                    },
                    File {
                        src: "content/two.txt".to_string(),
                        target: Some("content/two.txt".to_string()),
                        exclude: Some(vec!["*.tmp".to_string(), "*.bak".to_string()]),
                    },
                ],
            }),
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors></metadata><files><file src="content/one.txt" target="content/one.txt"/><file src="content/two.txt" target="content/two.txt" exclude="*.tmp;*.bak"/></files></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);
    }

    #[test]
    fn test_dependencies_serialization_deserialization() {
        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                dependencies: Some(Dependencies {
                    dependency: Some(vec![
                        Dependency {
                            id: "SomeDependency".to_string(),
                            version: "1.2.3".to_string(),
                            include: Some(vec!["runtime".to_string(), "compile".to_string()]),
                            exclude: Some(vec!["build".to_string()]),
                        },
                        Dependency {
                            id: "AnotherDependency".to_string(),
                            version: "[2.0,3.0)".to_string(),
                            include: None,
                            exclude: None,
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><dependencies><dependency id="SomeDependency" version="1.2.3" include="runtime,compile" exclude="build"/><dependency id="AnotherDependency" version="[2.0,3.0)"/></dependencies></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);

        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                dependencies: Some(Dependencies {
                    group: Some(vec![
                        DependencyGroup {
                            target_framework: Some("netstandard2.0".to_string()),
                            dependency: vec![
                                Dependency {
                                    id: "SomeDependency".to_string(),
                                    version: "1.2.3".to_string(),
                                    include: Some(vec![
                                        "runtime".to_string(),
                                        "compile".to_string(),
                                    ]),
                                    exclude: Some(vec!["build".to_string()]),
                                },
                                Dependency {
                                    id: "AnotherDependency".to_string(),
                                    version: "[2.0,3.0)".to_string(),
                                    ..Default::default()
                                },
                            ],
                        },
                        DependencyGroup {
                            target_framework: Some("net5.0".to_string()),
                            dependency: vec![Dependency {
                                id: "YetAnotherDependency".to_string(),
                                version: "4.5.6".to_string(),
                                include: Some(vec!["runtime".to_string()]),
                                ..Default::default()
                            }],
                        },
                        DependencyGroup {
                            dependency: vec![Dependency {
                                id: "NoTargetFrameworkDependency".to_string(),
                                version: "0.1.0".to_string(),
                                ..Default::default()
                            }],
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><dependencies><group targetFramework="netstandard2.0"><dependency id="SomeDependency" version="1.2.3" include="runtime,compile" exclude="build"/><dependency id="AnotherDependency" version="[2.0,3.0)"/></group><group targetFramework="net5.0"><dependency id="YetAnotherDependency" version="4.5.6" include="runtime"/></group><group><dependency id="NoTargetFrameworkDependency" version="0.1.0"/></group></dependencies></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);
    }

    #[test]
    fn test_framework_assemblies_serialization_deserialization() {
        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                framework_assemblies: Some(FrameworkAssemblies {
                    framework_assembly: vec![
                        FrameworkAssembly {
                            assembly_name: "System.Xml".to_string(),
                            target_framework: Some("netstandard2.0".to_string()),
                        },
                        FrameworkAssembly {
                            assembly_name: "Newtonsoft.Json".to_string(),
                            ..Default::default()
                        },
                    ],
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><frameworkAssemblies><frameworkAssembly assemblyName="System.Xml" targetFramework="netstandard2.0"/><frameworkAssembly assemblyName="Newtonsoft.Json"/></frameworkAssemblies></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);
    }

    #[test]
    fn test_references_serialization_deserialization() {
        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                references: Some(References {
                    reference: Some(vec![
                        Reference {
                            file: "SomeAssembly.dll".to_string(),
                        },
                        Reference {
                            file: "AnotherAssembly.dll".to_string(),
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><references><reference file="SomeAssembly.dll"/><reference file="AnotherAssembly.dll"/></references></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);

        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                references: Some(References {
                    group: Some(vec![
                        ReferenceGroup {
                            target_framework: Some("netstandard2.0".to_string()),
                            reference: vec![Reference {
                                file: "NetStandardAssembly.dll".to_string(),
                            }],
                        },
                        ReferenceGroup {
                            target_framework: Some("net5.0".to_string()),
                            reference: vec![Reference {
                                file: "Net5Assembly.dll".to_string(),
                            }],
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><references><group targetFramework="netstandard2.0"><reference file="NetStandardAssembly.dll"/></group><group targetFramework="net5.0"><reference file="Net5Assembly.dll"/></group></references></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);
    }

    #[test]
    fn test_content_files_serialization_deserialization() {
        let nuspec = Package {
            metadata: Metadata {
                id: "example-package".to_string(),
                version: "1.0.0".to_string(),
                description: "An example NuGet package".to_string(),
                authors: vec!["Author One".to_string(), "Author Two".to_string()],
                content_files: Some(ContentFiles {
                    files: vec![
                        ContentFile {
                            include: "contentFiles/one.txt".to_string(),
                            build_action: Some(BuildAction::Content),
                            copy_to_output: Some(true),
                            flatten: Some(false),
                            ..Default::default()
                        },
                        ContentFile {
                            include: "contentFiles/two.txt".to_string(),
                            exclude: Some(vec!["*.tmp".to_string(), "*.bak".to_string()]),
                            build_action: Some(BuildAction::Compile),
                            ..Default::default()
                        },
                    ],
                }),
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = quick_xml::se::to_string(&nuspec).unwrap();
        assert_eq!(
            serialized,
            r#"<package><metadata><id>example-package</id><version>1.0.0</version><description>An example NuGet package</description><authors>Author One,Author Two</authors><contentFiles><files include="contentFiles/one.txt" buildAction="Content" copyToOutput="true" flatten="false"/><files include="contentFiles/two.txt" exclude="*.tmp;*.bak" buildAction="Compile"/></contentFiles></metadata></package>"#.to_string()
        );

        let deserialized: Package = quick_xml::de::from_str(&serialized).unwrap();
        assert_eq!(deserialized, nuspec);
    }
}
