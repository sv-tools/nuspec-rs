use super::layout::{LayoutError, is_safe, is_safe_file_name, resolve};
use crate::{File, Files, Metadata, Package};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Merges packages with the same id, version and metadata into one package containing the
/// files of all of them.
///
/// This combines the native packages built for each platform, each with its library in its own
/// `runtimes/<rid>/native/`, into a single package for every platform. App packages have the
/// runtime identifier in their id, so they are not merged.
///
/// Each input is a package and the base directory its relative `src` paths are resolved
/// against (the directory of its `.nuspec`, as for [`pack`](super::pack)). A file found in
/// several inputs at the same path, such as the readme or the license, is kept once (from the
/// first input) if the contents are identical, ignoring the difference between `\r\n` and `\n`
/// line endings that a checkout on Windows introduces; different contents are a
/// [`LayoutError::Conflict`]. The result
/// lists every file by its absolute (or base-relative) source path and full target path, ready
/// for [`stage`](super::stage) or [`pack`](super::pack) with an empty base.
pub fn merge(inputs: &[(Package, PathBuf)]) -> Result<Package, LayoutError> {
    let Some((first, _)) = inputs.first() else {
        return Err(LayoutError::Mismatch("no packages to merge".to_string()));
    };
    let mut files: Vec<File> = Vec::new();
    let mut seen: HashMap<String, PathBuf> = HashMap::new();
    for (package, base) in inputs {
        check_same(first, package)?;
        for (source, dest) in resolve(package, base)? {
            let key = dest.to_lowercase();
            if let Some(existing) = seen.get(&key) {
                if *existing == source || same_content(existing, &source)? {
                    continue;
                }
                return Err(LayoutError::Conflict(dest));
            }
            seen.insert(key, source.clone());
            files.push(File {
                src: source.to_string_lossy().into_owned(),
                target: Some(dest),
                exclude: None,
            });
        }
    }
    Ok(Package {
        files: Some(Files { file: files }),
        ..first.clone()
    })
}

fn check_same(first: &Package, other: &Package) -> Result<(), LayoutError> {
    let (a, b) = (&first.metadata, &other.metadata);
    if !a.id.eq_ignore_ascii_case(&b.id) || a.version != b.version {
        return Err(LayoutError::Mismatch(format!(
            "cannot merge {} {} with {} {}: packages must have the same id and version",
            a.id, a.version, b.id, b.version
        )));
    }
    if a != b
        && (a.id == b.id
            || *a
                != Metadata {
                    id: a.id.clone(),
                    ..b.clone()
                })
    {
        return Err(LayoutError::Mismatch(format!(
            "cannot merge the {} {} packages: their metadata differs",
            a.id, a.version
        )));
    }
    Ok(())
}

fn same_content(a: &Path, b: &Path) -> Result<bool, LayoutError> {
    let read = |path: &Path| {
        fs::read(path).map_err(|source| LayoutError::Io {
            path: path.to_path_buf(),
            source,
        })
    };
    let (a, b) = (read(a)?, read(b)?);
    Ok(a == b || without_cr(&a).eq(without_cr(&b)))
}

/// The bytes with every `\r` of a `\r\n` removed.
fn without_cr(bytes: &[u8]) -> impl Iterator<Item = u8> + '_ {
    bytes
        .iter()
        .enumerate()
        .filter(|&(i, &b)| !(b == b'\r' && bytes.get(i + 1) == Some(&b'\n')))
        .map(|(_, &b)| b)
}

/// Extracts the `.nupkg` at `nupkg` into `dir`, laid out as in the package, and writes
/// `dir/<id>.nuspec` referring to the files with relative paths: the same result as
/// [`stage`](super::stage), so the directory can be merged, signed or packed again.
///
/// The parts of the package format (`[Content_Types].xml`, `_rels/`, `package/`) and a package
/// signature (`.signature.p7s`, which no longer applies once the package is repacked) are not
/// extracted. Unix file permissions stored in the archive are restored. Returns the package and
/// the path of the written `.nuspec`.
pub fn unpack(nupkg: &Path, dir: &Path) -> Result<(Package, PathBuf), LayoutError> {
    let io_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source| LayoutError::Io { path, source }
    };
    let file = fs::File::open(nupkg).map_err(io_err(nupkg))?;
    let mut archive = ZipArchive::new(io::BufReader::new(file)).map_err(LayoutError::Zip)?;
    fs::create_dir_all(dir).map_err(io_err(dir))?;

    let mut nuspec = None;
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(LayoutError::Zip)?;
        if entry.is_dir() {
            continue;
        }
        let name = unescape_part(&entry.name().map_err(LayoutError::Zip)?);
        let lower = name.to_lowercase();
        if !lower.contains('/') && lower.ends_with(".nuspec") {
            let mut xml = String::new();
            entry.read_to_string(&mut xml).map_err(io_err(nupkg))?;
            let package = Package::from_xml(&xml).map_err(|source| LayoutError::Read {
                path: nupkg.to_path_buf(),
                source,
            })?;
            nuspec = Some(package);
            continue;
        }
        if lower == "[content_types].xml"
            || lower == ".signature.p7s"
            || lower.starts_with("_rels/")
            || lower.starts_with("package/")
        {
            continue;
        }
        if !is_safe(&name) {
            return Err(LayoutError::UnsafePath(name));
        }
        let to = dir.join(&name);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        let mut out = fs::File::create(&to).map_err(io_err(&to))?;
        io::copy(&mut entry, &mut out).map_err(io_err(&to))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&to, fs::Permissions::from_mode(mode & 0o777))
                .map_err(io_err(&to))?;
        }
        files.push(File {
            src: name.clone(),
            target: Some(name),
            exclude: None,
        });
    }

    let package = nuspec
        .ok_or_else(|| LayoutError::Mismatch(format!("{} has no .nuspec", nupkg.display())))?;
    if !is_safe_file_name(&package.metadata.id) {
        return Err(LayoutError::UnsafePath(package.metadata.id));
    }
    let package = Package {
        files: Some(Files { file: files }),
        ..package
    };
    let path = dir.join(format!("{}.nuspec", package.metadata.id));
    let writer = fs::File::create(&path).map_err(io_err(&path))?;
    package
        .write_to(io::BufWriter::new(writer))
        .map_err(LayoutError::Write)?;
    Ok((package, path))
}

/// Decodes the `%XX` escapes of an OPC part name.
fn unescape_part(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = name.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Metadata;
    use crate::cargo::{pack, stage};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("nuspec-merge-{name}-{}", std::process::id()));
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
                id: "Demo.Native".to_string(),
                version: "1.0.0".to_string(),
                description: "Demo".to_string(),
                authors: vec!["Jane".to_string()],
                ..Default::default()
            },
            files: Some(Files { file: files }),
            ..Default::default()
        }
    }

    fn targets(package: &Package) -> Vec<String> {
        package
            .files
            .iter()
            .flat_map(|f| f.file.iter())
            .map(|f| f.target.clone().unwrap_or_default())
            .collect()
    }

    /// Stages a package for `rid` with a native library and a shared readme.
    fn platform(tmp: &TempDir, rid: &str, library: &str) -> (Package, PathBuf) {
        let lib = tmp.write(&format!("build-{rid}/{library}"), rid);
        let readme = tmp.write("README.md", "readme");
        let dir = tmp.0.join(format!("staged-{rid}"));
        let (staged, _) = stage(
            &package(vec![
                file(&readme, "README.md"),
                file(&lib, &format!("runtimes/{rid}/native/{library}")),
            ]),
            &dir,
        )
        .unwrap();
        (staged, dir)
    }

    #[test]
    fn test_merge_and_pack() {
        let tmp = TempDir::new("merge");
        let inputs = [
            platform(&tmp, "linux-x64", "libdemo.so"),
            platform(&tmp, "win-x64", "demo.dll"),
            platform(&tmp, "osx-arm64", "libdemo.dylib"),
        ];
        let merged = merge(&inputs).unwrap();
        assert_eq!(merged.metadata, inputs[0].0.metadata);
        assert_eq!(
            targets(&merged),
            [
                "README.md",
                "runtimes/linux-x64/native/libdemo.so",
                "runtimes/win-x64/native/demo.dll",
                "runtimes/osx-arm64/native/libdemo.dylib",
            ]
        );

        // Staged and packed, then unpacked again, the files are all there.
        let (staged, _) = stage(&merged, &tmp.0.join("merged")).unwrap();
        let nupkg = pack(&staged, &tmp.0.join("merged"), &tmp.0.join("out")).unwrap();
        let (unpacked, nuspec) = unpack(&nupkg, &tmp.0.join("unpacked")).unwrap();
        assert_eq!(nuspec, tmp.0.join("unpacked/Demo.Native.nuspec"));
        assert_eq!(unpacked.metadata, merged.metadata);
        let mut files = targets(&unpacked);
        files.sort();
        assert_eq!(
            files,
            [
                "README.md",
                "runtimes/linux-x64/native/libdemo.so",
                "runtimes/osx-arm64/native/libdemo.dylib",
                "runtimes/win-x64/native/demo.dll",
            ]
        );
        assert_eq!(
            fs::read_to_string(tmp.0.join("unpacked/runtimes/win-x64/native/demo.dll")).unwrap(),
            "win-x64"
        );
        let written = Package::from_xml(&fs::read_to_string(&nuspec).unwrap()).unwrap();
        assert_eq!(written, unpacked);
    }

    #[test]
    fn test_unpack_keeps_modes_and_names() {
        let tmp = TempDir::new("unpack");
        let bin = tmp.write("in/app", "bin");
        let spaced = tmp.write("in/my file.txt", "text");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let pkg = package(vec![file(&bin, "tools/app"), file(&spaced, "docs/")]);
        let nupkg = pack(&pkg, Path::new(""), &tmp.0.join("out")).unwrap();
        let (unpacked, _) = unpack(&nupkg, &tmp.0.join("x")).unwrap();
        assert_eq!(targets(&unpacked), ["tools/app", "docs/my file.txt"]);
        assert_eq!(
            fs::read_to_string(tmp.0.join("x/docs/my file.txt")).unwrap(),
            "text"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(tmp.0.join("x/tools/app"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    #[test]
    fn test_merge_errors() {
        let tmp = TempDir::new("errors");
        let (linux, linux_dir) = platform(&tmp, "linux-x64", "libdemo.so");

        // Line endings alone are not a difference.
        let crlf = tmp.write("crlf/README.md", "line 1\r\nline 2\r\n");
        let lf = tmp.write("lf/README.md", "line 1\nline 2\n");
        let readme = |src: &str| package(vec![file(src, "README.md")]);
        let merged = merge(&[
            (readme(&crlf), PathBuf::new()),
            (readme(&lf), PathBuf::new()),
        ])
        .unwrap();
        assert_eq!(targets(&merged), ["README.md"]);
        let cr = tmp.write("cr/README.md", "line 1\rline 2\r");
        assert!(matches!(
            merge(&[(readme(&lf), PathBuf::new()), (readme(&cr), PathBuf::new())]),
            Err(LayoutError::Conflict(_))
        ));

        // The same path with different contents.
        let other = tmp.write("other/libdemo.so", "different");
        let clash = package(vec![file(&other, "runtimes/linux-x64/native/libdemo.so")]);
        assert!(matches!(
            merge(&[(linux.clone(), linux_dir.clone()), (clash, PathBuf::new())]),
            Err(LayoutError::Conflict(path)) if path == "runtimes/linux-x64/native/libdemo.so"
        ));

        // A different id or version, as for app packages.
        let mut app = linux.clone();
        app.metadata.id = "Demo.Native.linux-x64".to_string();
        let err = merge(&[(linux.clone(), linux_dir.clone()), (app, linux_dir.clone())])
            .unwrap_err()
            .to_string();
        assert!(err.contains("same id and version"), "{err}");

        // Case-insensitive package id match succeeds and keeps the first id's casing.
        let (mut win, win_dir) = platform(&tmp, "win-x64", "demo.dll");
        win.metadata.id = "demo.native".to_string();
        let merged = merge(&[(linux.clone(), linux_dir.clone()), (win, win_dir)]).unwrap();
        assert_eq!(merged.metadata.id, "Demo.Native");

        // Other metadata differs.
        let mut described = linux.clone();
        described.metadata.description = "Other".to_string();
        let err = merge(&[(linux, linux_dir.clone()), (described, linux_dir)])
            .unwrap_err()
            .to_string();
        assert!(err.contains("metadata differs"), "{err}");

        assert!(merge(&[]).is_err());
    }

    #[test]
    fn test_unpack_rejects_unsafe_paths() {
        let tmp = TempDir::new("unsafe");
        let path = tmp.0.join("evil.nupkg");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("evil.nuspec", options).unwrap();
        io::Write::write_all(&mut zip, package(vec![]).to_xml().unwrap().as_bytes()).unwrap();
        zip.start_file("%2E%2E/escape.txt", options).unwrap();
        io::Write::write_all(&mut zip, b"x").unwrap();
        zip.finish().unwrap();

        let result = unpack(&path, &tmp.0.join("out"));
        assert!(
            matches!(&result, Err(LayoutError::UnsafePath(name)) if name == "../escape.txt"),
            "{result:?}"
        );
        assert!(!tmp.0.join("escape.txt").exists());

        // A malicious <id> inside the .nuspec cannot write <id>.nuspec outside the target dir.
        let id_path = tmp.0.join("evil-id.nupkg");
        let mut zip = zip::ZipWriter::new(fs::File::create(&id_path).unwrap());
        let mut evil_pkg = package(vec![]);
        evil_pkg.metadata.id = "../escape".to_string();
        zip.start_file("evil.nuspec", options).unwrap();
        io::Write::write_all(&mut zip, evil_pkg.to_xml().unwrap().as_bytes()).unwrap();
        zip.finish().unwrap();

        let result = unpack(&id_path, &tmp.0.join("out-id"));
        assert!(
            matches!(&result, Err(LayoutError::UnsafePath(name)) if name == "../escape"),
            "{result:?}"
        );
        assert!(!tmp.0.join("escape.nuspec").exists());
    }

    #[test]
    fn test_unescape_part() {
        assert_eq!(unescape_part("docs/my%20file.txt"), "docs/my file.txt");
        assert_eq!(unescape_part("a%2Fb%zz%4"), "a/b%zz%4");
    }

    #[test]
    fn test_is_safe() {
        assert!(is_safe("runtimes/linux-x64/native/libdemo.so"));
        for name in ["", "/abs", "a//b", "../x", "a/./b", "c:/x", "a\\b"] {
            assert!(!is_safe(name), "{name}");
        }
        assert!(is_safe_file_name("Demo.Native"));
        for name in ["", ".", "..", "a/b", "../x", "a\\b"] {
            assert!(!is_safe_file_name(name), "{name}");
        }
    }
}
