use crate::{File, Package};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::{error, fmt, io};

/// An error laying out the files of a package, by [`stage`](super::stage) or [`pack`](super::pack).
#[derive(Debug)]
#[non_exhaustive]
pub enum LayoutError {
    /// Reading or copying a file failed.
    Io {
        /// The file or directory involved.
        path: PathBuf,
        /// The underlying error.
        source: io::Error,
    },
    /// A `src` or `exclude` wildcard pattern is malformed.
    Pattern {
        /// The pattern.
        pattern: String,
        /// The parse error.
        source: glob::PatternError,
    },
    /// A `src` wildcard pattern matched no files.
    NoMatches(String),
    /// Two different files would be stored at the same path in the package.
    Conflict(String),
    /// A file would be stored at a path NuGet reserves for the package format.
    Reserved(String),
    /// Writing the `.nuspec` failed.
    Write(crate::Error),
    /// Writing the `.nupkg` archive failed.
    Zip(zip::result::ZipError),
    /// The package failed [`Package::validate`](crate::Package::validate).
    Invalid(crate::ValidationErrors),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            LayoutError::Pattern { pattern, source } => {
                write!(f, "invalid pattern {pattern:?}: {source}")
            }
            LayoutError::NoMatches(pattern) => write!(f, "no files match {pattern:?}"),
            LayoutError::Conflict(path) => {
                write!(f, "more than one file would be packed as {path:?}")
            }
            LayoutError::Reserved(path) => {
                write!(f, "{path:?} is reserved by the NuGet package format")
            }
            LayoutError::Write(e) => write!(f, "{e}"),
            LayoutError::Zip(e) => write!(f, "failed to write the package: {e}"),
            LayoutError::Invalid(e) => write!(f, "{e}"),
        }
    }
}

impl error::Error for LayoutError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            LayoutError::Io { source, .. } => Some(source),
            LayoutError::Pattern { source, .. } => Some(source),
            LayoutError::Write(e) => Some(e),
            LayoutError::Zip(e) => Some(e),
            LayoutError::Invalid(e) => Some(e),
            _ => None,
        }
    }
}

/// Resolves every `<file>` of `package` into `(source path, path inside the package)` pairs,
/// in order. Relative `src` paths are resolved against `base`, as `nuget pack` resolves them
/// against the `.nuspec` directory. A file listed twice is returned once; two different files
/// at the same path (compared case-insensitively, as NuGet does) are a [`LayoutError::Conflict`].
pub(super) fn resolve(
    package: &Package,
    base: &Path,
) -> Result<Vec<(PathBuf, String)>, LayoutError> {
    let mut result = Vec::new();
    let mut seen: HashMap<String, PathBuf> = HashMap::new();
    for file in package.files.iter().flat_map(|f| f.file.iter()) {
        for (source, dest) in expand(file, base)? {
            match seen.get(&dest.to_lowercase()) {
                Some(existing) if *existing == source => continue,
                Some(_) => return Err(LayoutError::Conflict(dest)),
                None => {}
            }
            seen.insert(dest.to_lowercase(), source.clone());
            result.push((source, dest));
        }
    }
    Ok(result)
}

/// Resolves a `<file>` into `(source path, path inside the package)` pairs. Relative `src`
/// paths are resolved against `base`.
fn expand(file: &File, base: &Path) -> Result<Vec<(PathBuf, String)>, LayoutError> {
    let target = file.target.as_deref().unwrap_or_default();
    let target = target.trim_matches(['/', '\\']).replace('\\', "/");
    let relative_src = Path::new(&file.src);
    let src = base.join(relative_src);
    let src = src.as_path();

    if !has_wildcard(&file.src) {
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target_ext = Path::new(&target).extension();
        let dest = if !target.is_empty() && target_ext.is_some() && target_ext == src.extension() {
            target
        } else {
            join(&target, &name)
        };
        return Ok(vec![(src.to_path_buf(), dest)]);
    }

    let excludes = file
        .exclude
        .iter()
        .flatten()
        .map(|p| {
            glob::Pattern::new(p).map_err(|source| LayoutError::Pattern {
                pattern: p.clone(),
                source,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Only `src` may contain wildcards: escape the base directory, which may contain `[`.
    let pattern = if relative_src.is_absolute() || base.as_os_str().is_empty() {
        file.src.clone()
    } else {
        let base = glob::Pattern::escape(&base.to_string_lossy());
        format!("{}/{}", base.trim_end_matches(['/', '\\']), file.src)
    };
    let prefix: PathBuf = base.join(
        relative_src
            .components()
            .take_while(
                |c| !matches!(c, Component::Normal(s) if has_wildcard(&s.to_string_lossy())),
            )
            .collect::<PathBuf>(),
    );
    let matches = glob::glob(&pattern).map_err(|source| LayoutError::Pattern {
        pattern: file.src.clone(),
        source,
    })?;

    let mut result = Vec::new();
    for entry in matches {
        let path = entry.map_err(|e| LayoutError::Io {
            path: e.path().to_path_buf(),
            source: e.into(),
        })?;
        if !path.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(&prefix)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if excludes
            .iter()
            .any(|p| p.matches(&relative) || p.matches(&name))
        {
            continue;
        }
        result.push((path.clone(), join(&target, &relative)));
    }
    if result.is_empty() {
        return Err(LayoutError::NoMatches(file.src.clone()));
    }
    result.sort();
    Ok(result)
}

fn has_wildcard(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_string()
    } else {
        format!("{folder}/{name}")
    }
}
