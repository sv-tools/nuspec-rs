use super::layout::{LayoutError, resolve};
use crate::{File, Files, Package};
use std::path::{Path, PathBuf};
use std::{fs, io};

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
/// Relative `src` paths are resolved against the current directory. Returns the staged package
/// and the path of the written `.nuspec`, which [`pack`](super::pack) accepts with `dir` as its
/// base directory.
pub fn stage(package: &Package, dir: &Path) -> Result<(Package, PathBuf), LayoutError> {
    let io_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source| LayoutError::Io { path, source }
    };
    fs::create_dir_all(dir).map_err(io_err(dir))?;

    let mut staged: Vec<File> = Vec::new();
    for (source, dest) in resolve(package, Path::new(""))? {
        let to = dir.join(&dest);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        fs::copy(&source, &to).map_err(io_err(&source))?;

        let folder = dest.rsplit_once('/').map_or("", |(folder, _)| folder);
        staged.push(File {
            src: dest.clone(),
            target: Some(folder.to_string()),
            exclude: None,
        });
    }

    let package = Package {
        files: Some(Files { file: staged }),
        ..package.clone()
    };
    let nuspec = dir.join(format!("{}.nuspec", package.metadata.id));
    let writer = fs::File::create(&nuspec).map_err(io_err(&nuspec))?;
    package
        .write_to(io::BufWriter::new(writer))
        .map_err(LayoutError::Write)?;
    Ok((package, nuspec))
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
        assert!(matches!(err, LayoutError::Io { .. }), "{err}");

        let pattern = tmp.0.join("nothing/*.dll").to_string_lossy().into_owned();
        let err = stage(&package(vec![file(&pattern, "lib")]), &out).unwrap_err();
        assert!(matches!(err, LayoutError::NoMatches(_)), "{err}");

        let a = tmp.write("a/x.dll", "a");
        let b = tmp.write("b/X.dll", "b");
        let err = stage(&package(vec![file(&a, "lib"), file(&b, "lib")]), &out).unwrap_err();
        assert_eq!(
            err.to_string(),
            "more than one file would be packed as \"lib/X.dll\""
        );
    }
}
