use crate::{Dependency, License, Package};
use std::{error, fmt};

/// The longest package id `nuget pack` accepts.
pub const MAX_ID_LENGTH: usize = 100;
/// The longest version nuget.org accepts.
pub const MAX_VERSION_LENGTH: usize = 64;
/// The longest description nuget.org accepts.
pub const MAX_DESCRIPTION_LENGTH: usize = 4000;

/// A single problem found by [`Package::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationError {
    /// A required field is missing or empty.
    Missing(&'static str),
    /// A field exceeds the length NuGet accepts.
    TooLong {
        /// The field name, as in the nuspec.
        field: &'static str,
        /// The maximum number of characters.
        max: usize,
        /// The actual number of characters.
        actual: usize,
    },
    /// The package id contains characters NuGet does not allow.
    InvalidId(String),
    /// The version is not a valid NuGet version.
    InvalidVersion(String),
    /// `requireLicenseAcceptance` is set, but neither `license` nor `licenseUrl` is.
    LicenseAcceptanceWithoutLicense,
    /// A `<file>` element has an empty `src`.
    EmptyFileSource {
        /// The position of the element within `<files>`.
        index: usize,
    },
    /// A `<dependency>` element has an empty `id`.
    EmptyDependencyId,
    /// A `<frameworkReference>` element has an empty `name`.
    EmptyFrameworkReferenceName,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::Missing(field) => write!(f, "`{field}` is required"),
            ValidationError::TooLong { field, max, actual } => {
                write!(
                    f,
                    "`{field}` is {actual} characters long, the maximum is {max}"
                )
            }
            ValidationError::InvalidId(id) => write!(
                f,
                "`id` {id:?} must consist of letters, digits and underscores, separated by single dots or dashes"
            ),
            ValidationError::InvalidVersion(version) => write!(
                f,
                "`version` {version:?} must be 1 to 4 dot-separated numbers, optionally followed by `-prerelease` and `+metadata`"
            ),
            ValidationError::LicenseAcceptanceWithoutLicense => write!(
                f,
                "`requireLicenseAcceptance` requires `license` or `licenseUrl` to be set"
            ),
            ValidationError::EmptyFileSource { index } => {
                write!(f, "`file` #{index} has an empty `src`")
            }
            ValidationError::EmptyDependencyId => write!(f, "a `dependency` has an empty `id`"),
            ValidationError::EmptyFrameworkReferenceName => {
                write!(f, "a `frameworkReference` has an empty `name`")
            }
        }
    }
}

impl error::Error for ValidationError {}

/// All problems found by [`Package::validate`], in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationErrors(pub Vec<ValidationError>);

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid nuspec:")?;
        for error in &self.0 {
            write!(f, "\n  - {error}")?;
        }
        Ok(())
    }
}

impl error::Error for ValidationErrors {}

impl Package {
    /// Checks the rules `nuget pack` and nuget.org enforce that the type system does not:
    /// required fields, id and version syntax, length limits and a few cross-field rules.
    ///
    /// Returns every problem found rather than stopping at the first one.
    pub fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = Vec::new();
        let m = &self.metadata;

        if m.id.is_empty() {
            errors.push(ValidationError::Missing("id"));
        } else {
            check_length(&mut errors, "id", &m.id, MAX_ID_LENGTH);
            if !is_valid_id(&m.id) {
                errors.push(ValidationError::InvalidId(m.id.clone()));
            }
        }

        if m.version.is_empty() {
            errors.push(ValidationError::Missing("version"));
        } else {
            check_length(&mut errors, "version", &m.version, MAX_VERSION_LENGTH);
            if !is_valid_version(&m.version) {
                errors.push(ValidationError::InvalidVersion(m.version.clone()));
            }
        }

        if m.description.trim().is_empty() {
            errors.push(ValidationError::Missing("description"));
        } else {
            check_length(
                &mut errors,
                "description",
                &m.description,
                MAX_DESCRIPTION_LENGTH,
            );
        }

        if m.authors.iter().all(|a| a.trim().is_empty()) {
            errors.push(ValidationError::Missing("authors"));
        }

        match &m.license {
            Some(License::Expression(v) | License::File(v)) if v.trim().is_empty() => {
                errors.push(ValidationError::Missing("license"));
            }
            _ => {}
        }
        if m.require_license_acceptance == Some(true)
            && m.license.is_none()
            && m.license_url.is_none()
        {
            errors.push(ValidationError::LicenseAcceptanceWithoutLicense);
        }

        if let Some(dependencies) = &m.dependencies {
            let flat = dependencies.dependency.iter().flatten();
            let grouped = dependencies
                .group
                .iter()
                .flatten()
                .flat_map(|g| g.dependency.iter());
            if flat.chain(grouped).any(|d: &Dependency| d.id.is_empty()) {
                errors.push(ValidationError::EmptyDependencyId);
            }
        }

        if let Some(references) = &m.framework_references {
            let empty = references
                .group
                .iter()
                .flat_map(|g| g.framework_reference.iter())
                .any(|r| r.name.is_empty());
            if empty {
                errors.push(ValidationError::EmptyFrameworkReferenceName);
            }
        }

        if let Some(files) = &self.files {
            for (index, file) in files.file.iter().enumerate() {
                if file.src.is_empty() {
                    errors.push(ValidationError::EmptyFileSource { index });
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ValidationErrors(errors))
        }
    }
}

fn check_length(errors: &mut Vec<ValidationError>, field: &'static str, value: &str, max: usize) {
    let actual = value.chars().count();
    if actual > max {
        errors.push(ValidationError::TooLong { field, max, actual });
    }
}

/// Mirrors NuGet's `^\w+([.-]\w+)*$` package id rule.
fn is_valid_id(id: &str) -> bool {
    id.split(['.', '-'])
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_'))
}

/// Accepts NuGet versions: 1 to 4 numeric parts, then an optional SemVer 2.0 pre-release label
/// and build metadata.
fn is_valid_version(version: &str) -> bool {
    let (rest, metadata) = match version.split_once('+') {
        Some((rest, metadata)) => (rest, Some(metadata)),
        None => (version, None),
    };
    let (core, prerelease) = match rest.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (rest, None),
    };

    let parts: Vec<&str> = core.split('.').collect();
    let core_ok = (1..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    let labels_ok = |labels: &str| {
        labels
            .split('.')
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    };

    core_ok && prerelease.is_none_or(labels_ok) && metadata.is_none_or(labels_ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Dependencies, DependencyGroup, File, Files, FrameworkReference, FrameworkReferenceGroup,
        FrameworkReferences, Metadata,
    };

    fn valid() -> Package {
        Package {
            metadata: Metadata {
                id: "My.Package-Name_1".to_string(),
                version: "1.2.3-beta.1+sha.abc".to_string(),
                description: "Description".to_string(),
                authors: vec!["Author".to_string()],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_valid_package() {
        assert_eq!(valid().validate(), Ok(()));
    }

    #[test]
    fn test_default_package_reports_all_required_fields() {
        let errors = Package::default().validate().unwrap_err();
        assert_eq!(
            errors.0,
            vec![
                ValidationError::Missing("id"),
                ValidationError::Missing("version"),
                ValidationError::Missing("description"),
                ValidationError::Missing("authors"),
            ]
        );
        assert_eq!(
            errors.to_string(),
            "invalid nuspec:\n  - `id` is required\n  - `version` is required\n  - `description` is required\n  - `authors` is required"
        );
    }

    #[test]
    fn test_ids() {
        for id in ["a", "A.B", "a-b.c_d", "Ünïcödé"] {
            assert!(is_valid_id(id), "{id}");
        }
        for id in ["a b", ".a", "a.", "a..b", "a.-b", "a/b", "a@b"] {
            assert!(!is_valid_id(id), "{id}");
        }

        let mut pkg = valid();
        pkg.metadata.id = "a".repeat(MAX_ID_LENGTH + 1);
        assert_eq!(
            pkg.validate().unwrap_err().0,
            vec![ValidationError::TooLong {
                field: "id",
                max: MAX_ID_LENGTH,
                actual: MAX_ID_LENGTH + 1
            }]
        );
    }

    #[test]
    fn test_versions() {
        for v in [
            "1",
            "1.0",
            "1.0.0",
            "1.0.0.0",
            "1.0.0-rc.1",
            "1.0.0+build",
            "0.1.0-a-b.2+x.y",
        ] {
            assert!(is_valid_version(v), "{v}");
        }
        for v in [
            "",
            "v1.0",
            "1.0.0.0.0",
            "1..0",
            "1.0-",
            "1.0+",
            "1.0-a..b",
            "1.0-a_b",
            "1.x",
        ] {
            assert!(!is_valid_version(v), "{v}");
        }
    }

    #[test]
    fn test_license_rules() {
        let mut pkg = valid();
        pkg.metadata.require_license_acceptance = Some(true);
        assert_eq!(
            pkg.validate().unwrap_err().0,
            vec![ValidationError::LicenseAcceptanceWithoutLicense]
        );

        pkg.metadata.license_url = Some("https://example.com/license".to_string());
        assert_eq!(pkg.validate(), Ok(()));

        pkg.metadata.license = Some(License::Expression(" ".to_string()));
        assert_eq!(
            pkg.validate().unwrap_err().0,
            vec![ValidationError::Missing("license")]
        );
    }

    #[test]
    fn test_nested_rules() {
        let mut pkg = valid();
        pkg.metadata.dependencies = Some(Dependencies {
            group: Some(vec![DependencyGroup {
                dependency: vec![Default::default()],
                ..Default::default()
            }]),
            ..Default::default()
        });
        pkg.metadata.framework_references = Some(FrameworkReferences {
            group: vec![FrameworkReferenceGroup {
                target_framework: "net9.0".to_string(),
                framework_reference: vec![FrameworkReference::default()],
            }],
        });
        pkg.files = Some(Files {
            file: vec![
                File {
                    src: "a".to_string(),
                    ..Default::default()
                },
                File::default(),
            ],
        });
        assert_eq!(
            pkg.validate().unwrap_err().0,
            vec![
                ValidationError::EmptyDependencyId,
                ValidationError::EmptyFrameworkReferenceName,
                ValidationError::EmptyFileSource { index: 1 },
            ]
        );
    }
}
