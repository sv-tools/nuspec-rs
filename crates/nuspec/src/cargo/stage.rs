use crate::{File, Files, Package};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::{error, fmt, fs, io};

/// An error staging a package.
#[derive(Debug)]
#[non_exhaustive]
pub enum StageError {
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
    /// Two different files would be staged at the same path in the package.
    Conflict(String),
    /// Writing the `.nuspec` failed.
    Write(crate::Error),
}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            StageError::Pattern { pattern, source } => {
                write!(f, "invalid pattern {pattern:?}: {source}")
            }
            StageError::NoMatches(pattern) => write!(f, "no files match {pattern:?}"),
            StageError::Conflict(path) => {
                write!(f, "more than one file would be packed as {path:?}")
            }
            StageError::Write(e) => write!(f, "{e}"),
        }
    }
}

impl error::Error for StageError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            StageError::Io { source, .. } => Some(source),
            StageError::Pattern { source, .. } => Some(source),
            StageError::Write(e) => Some(e),
            _ => None,
        }
    }
}

/// Copies every file of `package` into `dir`, laid out as in the final package, and writes
/// `dir/<id>.nuspec` referring to them with relative paths.
///
/// The result is self-contained: the directory can be moved to another machine, its files
/// signed in place, and then packed with `nuget pack dir/<id>.nuspec`.
///
/// `src` paths may contain the `*`, `?` and `**` wildcards; the part of the path matched by
/// them is preserved under `target`, as `nuget pack` does. A `target` with the same extension
/// as its `src` file names the file itself rather than a folder. Existing files in `dir` are
/// overwritten but not removed.
///
/// Returns the staged package and the path of the written `.nuspec`.
pub fn stage(package: &Package, dir: &Path) -> Result<(Package, PathBuf), StageError> {
    let io_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source| StageError::Io { path, source }
    };
    fs::create_dir_all(dir).map_err(io_err(dir))?;

    let mut staged: Vec<File> = Vec::new();
    let mut sources: HashMap<String, PathBuf> = HashMap::new();
    for file in package.files.iter().flat_map(|f| f.file.iter()) {
        let target = file.target.as_deref().unwrap_or_default();
        for (source, dest) in expand(file, target)? {
            let key = dest.to_lowercase();
            match sources.get(&key) {
                Some(existing) if *existing == source => continue,
                Some(_) => return Err(StageError::Conflict(dest)),
                None => {}
            }
            let to = dir.join(&dest);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(io_err(parent))?;
            }
            fs::copy(&source, &to).map_err(io_err(&source))?;
            sources.insert(key, source);

            let folder = dest.rsplit_once('/').map_or("", |(folder, _)| folder);
            staged.push(File {
                src: dest.clone(),
                target: Some(folder.to_string()),
                exclude: None,
            });
        }
    }

    let package = Package {
        files: Some(Files { file: staged }),
        ..package.clone()
    };
    let nuspec = dir.join(format!("{}.nuspec", package.metadata.id));
    let writer = fs::File::create(&nuspec).map_err(io_err(&nuspec))?;
    package
        .write_to(io::BufWriter::new(writer))
        .map_err(StageError::Write)?;
    Ok((package, nuspec))
}

/// Resolves a `<file>` into `(source path, path inside the package)` pairs.
fn expand(file: &File, target: &str) -> Result<Vec<(PathBuf, String)>, StageError> {
    let target = target.trim_matches(['/', '\\']).replace('\\', "/");
    let src = Path::new(&file.src);

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
            glob::Pattern::new(p).map_err(|source| StageError::Pattern {
                pattern: p.clone(),
                source,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let base: PathBuf = src
        .components()
        .take_while(|c| !matches!(c, Component::Normal(s) if has_wildcard(&s.to_string_lossy())))
        .collect();
    let matches = glob::glob(&file.src).map_err(|source| StageError::Pattern {
        pattern: file.src.clone(),
        source,
    })?;

    let mut result = Vec::new();
    for entry in matches {
        let path = entry.map_err(|e| StageError::Io {
            path: e.path().to_path_buf(),
            source: e.into(),
        })?;
        if !path.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(&base)
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
        return Err(StageError::NoMatches(file.src.clone()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Metadata;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("nuspec-stage-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, content: &str) -> String {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
            path.to_string_lossy().into_owned()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn file(src: &str, target: &str) -> File {
        File {
            src: src.to_string(),
            target: Some(target.to_string()),
            exclude: None,
        }
    }

    fn package(files: Vec<File>) -> Package {
        Package {
            metadata: Metadata {
                id: "demo".to_string(),
                version: "1.0.0".to_string(),
                description: "Demo".to_string(),
                authors: vec!["Jane".to_string()],
                ..Default::default()
            },
            files: Some(Files { file: files }),
            ..Default::default()
        }
    }

    fn staged_files(pkg: &Package) -> Vec<(String, String)> {
        pkg.files
            .iter()
            .flat_map(|f| f.file.iter())
            .map(|f| (f.src.clone(), f.target.clone().unwrap_or_default()))
            .collect()
    }

    fn pair(src: &str, target: &str) -> (String, String) {
        (src.to_string(), target.to_string())
    }

    #[test]
    fn test_stage() {
        let tmp = TempDir::new("stage");
        let dll = tmp.write("target/release/demo.dll", "dll");
        let readme = tmp.write("crate/README.md", "readme");
        tmp.write("crate/deploy/a.ps1", "a");
        tmp.write("crate/deploy/sub/b.ps1", "b");
        tmp.write("crate/deploy/sub/skip.ps1", "skip");
        tmp.write("crate/deploy/c.txt", "c");
        let notice = tmp.write("crate/NOTICE", "notice");
        let pattern = tmp
            .0
            .join("crate/deploy/**/*.ps1")
            .to_string_lossy()
            .into_owned();

        let pkg = package(vec![
            file(&readme, ""),
            file(&dll, "runtimes/win-x64/native"),
            File {
                exclude: Some(vec!["skip.*".to_string()]),
                ..file(&pattern, "scripts")
            },
            file(&notice, "/docs/"),
            file(&readme, ""),
        ]);
        let out = tmp.0.join("out/demo");
        let (staged, nuspec) = stage(&pkg, &out).unwrap();

        assert_eq!(nuspec, out.join("demo.nuspec"));
        assert_eq!(
            staged_files(&staged),
            vec![
                pair("README.md", ""),
                pair(
                    "runtimes/win-x64/native/demo.dll",
                    "runtimes/win-x64/native"
                ),
                pair("scripts/a.ps1", "scripts"),
                pair("scripts/sub/b.ps1", "scripts/sub"),
                pair("docs/NOTICE", "docs"),
            ]
        );
        assert_eq!(
            fs::read_to_string(out.join("runtimes/win-x64/native/demo.dll")).unwrap(),
            "dll"
        );
        assert_eq!(
            fs::read_to_string(out.join("scripts/sub/b.ps1")).unwrap(),
            "b"
        );
        assert!(!out.join("scripts/sub/skip.ps1").exists());

        let written = Package::from_xml(&fs::read_to_string(&nuspec).unwrap()).unwrap();
        assert_eq!(written, staged);
        assert_eq!(written.metadata, pkg.metadata);
    }

    #[test]
    fn test_target_as_file_name() {
        let tmp = TempDir::new("rename");
        let src = tmp.write("in/readme.md", "x");
        let (staged, _) = stage(
            &package(vec![file(&src, "docs/README.md")]),
            &tmp.0.join("out"),
        )
        .unwrap();
        assert_eq!(staged_files(&staged), vec![pair("docs/README.md", "docs")]);
    }

    #[test]
    fn test_errors() {
        let tmp = TempDir::new("errors");
        let out = tmp.0.join("out");

        let missing = tmp.0.join("missing.dll").to_string_lossy().into_owned();
        let err = stage(&package(vec![file(&missing, "lib")]), &out).unwrap_err();
        assert!(matches!(err, StageError::Io { .. }), "{err}");

        let pattern = tmp.0.join("nothing/*.dll").to_string_lossy().into_owned();
        let err = stage(&package(vec![file(&pattern, "lib")]), &out).unwrap_err();
        assert!(matches!(err, StageError::NoMatches(_)), "{err}");

        let a = tmp.write("a/x.dll", "a");
        let b = tmp.write("b/X.dll", "b");
        let err = stage(&package(vec![file(&a, "lib"), file(&b, "lib")]), &out).unwrap_err();
        assert_eq!(
            err.to_string(),
            "more than one file would be packed as \"lib/X.dll\""
        );
    }
}
