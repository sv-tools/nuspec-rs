use super::layout::{LayoutError, resolve};
use crate::Package;
use quick_xml::escape::escape;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

/// The content type NuGet uses for every file in a package.
const OCTET: &str = "application/octet";

/// Writes `package` as `out_dir/<id>.<version>.nupkg` and returns its path.
///
/// Relative `src` paths are resolved against `base`, as `nuget pack` resolves them against
/// the directory of the `.nuspec` (pass the directory given to [`stage`](super::stage) to pack
/// a staged package). The package is checked with [`Package::validate`] first.
///
/// The archive has the layout NuGet clients, `nuget sign` and nuget.org expect: the `.nuspec`
/// (without its `<files>`) at the root, `[Content_Types].xml`, `_rels/.rels` and the core
/// properties part. Unix file permissions are kept, so binaries stay executable when the
/// package is unzipped. Entries are timestamped with `SOURCE_DATE_EPOCH` when it is set, for
/// reproducible packages, and with the current time otherwise.
pub fn pack(package: &Package, base: &Path, out_dir: &Path) -> Result<PathBuf, LayoutError> {
    let seconds = env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        });
    pack_at(package, base, out_dir, seconds)
}

fn pack_at(
    package: &Package,
    base: &Path,
    out_dir: &Path,
    seconds: u64,
) -> Result<PathBuf, LayoutError> {
    package.validate().map_err(LayoutError::Invalid)?;
    let metadata = &package.metadata;
    let entries = resolve(package, base)?;
    let nuspec_name = format!("{}.nuspec", metadata.id);
    for (_, dest) in &entries {
        let lower = dest.to_lowercase();
        if lower == "[content_types].xml"
            || lower.starts_with("_rels/")
            || lower.starts_with("package/")
            || (!lower.contains('/') && lower.ends_with(".nuspec"))
        {
            return Err(LayoutError::Reserved(dest.clone()));
        }
    }

    let io_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source| LayoutError::Io { path, source }
    };
    fs::create_dir_all(out_dir).map_err(io_err(out_dir))?;
    // Build metadata is not part of the file name, as with `nuget pack`.
    let version = metadata.version.split('+').next().unwrap_or_default();
    let path = out_dir.join(format!("{}.{version}.nupkg", metadata.id));
    let partial = path.with_extension("nupkg.partial");

    let result = (|| {
        let file = fs::File::create(&partial).map_err(io_err(&partial))?;
        let mut zip = ZipWriter::new(io::BufWriter::new(file));
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .last_modified_time(zip_time(seconds))
            .unix_permissions(0o644);
        let zip_err = LayoutError::Zip;

        let id_hash = fnv(&format!("{}\0{}", metadata.id, metadata.version));
        let psmdcp = format!(
            "package/services/metadata/core-properties/{id_hash:016x}{:016x}.psmdcp",
            fnv(&metadata.id)
        );

        zip.start_file("_rels/.rels", options).map_err(zip_err)?;
        write_str(&mut zip, &relationships(&nuspec_name, &psmdcp))?;

        let nuspec = Package {
            files: None,
            ..package.clone()
        }
        .to_xml()
        .map_err(LayoutError::Write)?;
        zip.start_file(escape_part(&nuspec_name), options)
            .map_err(zip_err)?;
        write_str(&mut zip, &nuspec)?;

        let mut extensions = BTreeSet::from(["nuspec".to_string()]);
        let mut extensionless = Vec::new();
        for (source, dest) in &entries {
            let name = escape_part(dest);
            match Path::new(dest).extension() {
                Some(ext) => {
                    extensions.insert(ext.to_string_lossy().to_lowercase());
                }
                None => extensionless.push(name.clone()),
            }
            let mode = file_mode(source).map_err(io_err(source))?;
            zip.start_file(name, options.unix_permissions(mode))
                .map_err(zip_err)?;
            let mut input = fs::File::open(source).map_err(io_err(source))?;
            io::copy(&mut input, &mut zip).map_err(io_err(source))?;
        }

        zip.start_file(psmdcp.as_str(), options).map_err(zip_err)?;
        write_str(&mut zip, &core_properties(package))?;

        zip.start_file("[Content_Types].xml", options)
            .map_err(zip_err)?;
        write_str(&mut zip, &content_types(&extensions, &extensionless))?;

        let mut writer = zip.finish().map_err(zip_err)?;
        writer.flush().map_err(io_err(&partial))?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            fs::rename(&partial, &path).map_err(io_err(&path))?;
            Ok(path)
        }
        Err(e) => {
            let _ = fs::remove_file(&partial);
            Err(e)
        }
    }
}

fn write_str<W: Write>(writer: &mut W, s: &str) -> Result<(), LayoutError> {
    writer
        .write_all(s.as_bytes())
        .map_err(|source| LayoutError::Io {
            path: PathBuf::new(),
            source,
        })
}

fn relationships(nuspec: &str, psmdcp: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            r#"<Relationship Type="http://schemas.microsoft.com/packaging/2010/07/manifest" Target="/{nuspec}" Id="R{nuspec_id:016X}" />"#,
            r#"<Relationship Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="/{psmdcp}" Id="R{psmdcp_id:016X}" />"#,
            r#"</Relationships>"#,
        ),
        nuspec = escape(escape_part(nuspec)),
        nuspec_id = fnv(nuspec),
        psmdcp = psmdcp,
        psmdcp_id = fnv(psmdcp),
    )
}

fn core_properties(package: &Package) -> String {
    let m = &package.metadata;
    let mut xml = String::from(concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<coreProperties xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns="http://schemas.openxmlformats.org/package/2006/metadata/core-properties">"#,
    ));
    let _ = write!(
        xml,
        "<dc:creator>{}</dc:creator><dc:description>{}</dc:description><dc:identifier>{}</dc:identifier><version>{}</version>",
        escape(m.authors.join(", ")),
        escape(&m.description),
        escape(&m.id),
        escape(&m.version),
    );
    if let Some(tags) = m.tags.as_ref().filter(|t| !t.is_empty()) {
        let _ = write!(xml, "<keywords>{}</keywords>", escape(tags.join(" ")));
    }
    if let Some(title) = &m.title {
        let _ = write!(xml, "<dc:title>{}</dc:title>", escape(title));
    }
    let _ = write!(
        xml,
        "<lastModifiedBy>nuspec {}</lastModifiedBy></coreProperties>",
        env!("CARGO_PKG_VERSION")
    );
    xml
}

fn content_types(extensions: &BTreeSet<String>, extensionless: &[String]) -> String {
    let mut xml = String::from(concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" />"#,
        r#"<Default Extension="psmdcp" ContentType="application/vnd.openxmlformats-package.core-properties+xml" />"#,
    ));
    for ext in extensions.iter().filter(|e| *e != "rels" && *e != "psmdcp") {
        let _ = write!(
            xml,
            r#"<Default Extension="{}" ContentType="{OCTET}" />"#,
            escape(ext)
        );
    }
    for name in extensionless {
        let _ = write!(
            xml,
            r#"<Override PartName="/{}" ContentType="{OCTET}" />"#,
            escape(name)
        );
    }
    xml.push_str("</Types>");
    xml
}

/// Percent-encodes a package path for use as an OPC part name, keeping `/` separators.
fn escape_part(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

#[cfg(unix)]
fn file_mode(path: &Path) -> io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::metadata(path)?.permissions().mode() & 0o777 | 0o444)
}

#[cfg(not(unix))]
fn file_mode(path: &Path) -> io::Result<u32> {
    fs::metadata(path)?;
    Ok(0o644)
}

/// 64-bit FNV-1a, used to derive stable part names and relationship ids.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Converts Unix seconds to a zip timestamp, clamped to the range zip can store.
fn zip_time(seconds: u64) -> DateTime {
    let days = (seconds / 86_400) as i64;
    let rem = seconds % 86_400;
    // Howard Hinnant's `civil_from_days`.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = yoe + era * 400 + i64::from(month <= 2);

    match year {
        ..1980 => DateTime::default(),
        2108.. => DateTime::from_date_and_time(2107, 12, 31, 23, 59, 58).unwrap_or_default(),
        _ => DateTime::from_date_and_time(
            year as u16,
            month,
            day,
            (rem / 3600) as u8,
            (rem % 3600 / 60) as u8,
            (rem % 60) as u8,
        )
        .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{File, Files, Metadata};
    use std::io::Read;
    use zip::ZipArchive;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("nuspec-pack-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, content).unwrap();
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
                id: "Demo.App".to_string(),
                version: "1.2.3-rc.1+abc".to_string(),
                description: "A <demo> & more".to_string(),
                authors: vec!["Jane".to_string(), "John".to_string()],
                tags: Some(vec!["a".to_string(), "b".to_string()]),
                ..Default::default()
            },
            files: Some(Files { file: files }),
            ..Default::default()
        }
    }

    fn read(archive: &mut ZipArchive<fs::File>, name: &str) -> String {
        let mut s = String::new();
        archive
            .by_name(name)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .read_to_string(&mut s)
            .unwrap();
        s
    }

    #[test]
    fn test_pack() {
        let tmp = TempDir::new("pack");
        tmp.write("in/demo-app", "elf");
        tmp.write("in/demo.dll", "dll");
        tmp.write("in/README.md", "readme");
        tmp.write("in/my file.txt", "spaces");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(tmp.0.join("in/demo-app"), fs::Permissions::from_mode(0o755))
                .unwrap();
        }

        let pkg = package(vec![
            file("in/demo-app", "tools/"),
            file("in/demo.dll", "runtimes/win-x64/native"),
            file("in/README.md", ""),
            file("in/my file.txt", "docs"),
        ]);
        let out = tmp.0.join("out");
        let path = pack_at(&pkg, &tmp.0, &out, 1_700_000_000).unwrap();
        assert_eq!(path, out.join("Demo.App.1.2.3-rc.1.nupkg"));
        assert!(!out.join("Demo.App.1.2.3-rc.1.nupkg.partial").exists());

        let mut archive = ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let mut names: Vec<String> = archive
            .file_names()
            .map(|n| n.unwrap().into_owned())
            .collect();
        let psmdcp = names
            .iter()
            .find(|n| n.ends_with(".psmdcp"))
            .unwrap()
            .clone();
        assert!(psmdcp.starts_with("package/services/metadata/core-properties/"));
        names.sort();
        assert_eq!(
            names,
            vec![
                "Demo.App.nuspec",
                "README.md",
                "[Content_Types].xml",
                "_rels/.rels",
                "docs/my%20file.txt",
                psmdcp.as_str(),
                "runtimes/win-x64/native/demo.dll",
                "tools/demo-app",
            ]
        );

        assert_eq!(read(&mut archive, "tools/demo-app"), "elf");
        assert_eq!(read(&mut archive, "docs/my%20file.txt"), "spaces");

        let nuspec = Package::from_xml(&read(&mut archive, "Demo.App.nuspec")).unwrap();
        assert_eq!(nuspec.files, None);
        assert_eq!(nuspec.metadata, pkg.metadata);

        let types = read(&mut archive, "[Content_Types].xml");
        for expected in [
            r#"<Default Extension="nuspec" ContentType="application/octet" />"#,
            r#"<Default Extension="dll" ContentType="application/octet" />"#,
            r#"<Default Extension="md" ContentType="application/octet" />"#,
            r#"<Default Extension="txt" ContentType="application/octet" />"#,
            r#"<Override PartName="/tools/demo-app" ContentType="application/octet" />"#,
        ] {
            assert!(types.contains(expected), "{expected} not in {types}");
        }

        let rels = read(&mut archive, "_rels/.rels");
        assert!(rels.contains(r#"Target="/Demo.App.nuspec""#), "{rels}");
        assert!(rels.contains(&format!(r#"Target="/{psmdcp}""#)), "{rels}");

        let props = read(&mut archive, &psmdcp);
        assert!(
            props.contains("<dc:creator>Jane, John</dc:creator>"),
            "{props}"
        );
        assert!(
            props.contains("<dc:description>A &lt;demo&gt; &amp; more</dc:description>"),
            "{props}"
        );
        assert!(props.contains("<keywords>a b</keywords>"), "{props}");

        let entry = archive.by_name("tools/demo-app").unwrap();
        let modified = entry.last_modified().unwrap();
        assert_eq!(
            (modified.year(), modified.month(), modified.day()),
            (2023, 11, 14)
        );
        #[cfg(unix)]
        assert_eq!(entry.unix_mode().map(|m| m & 0o777), Some(0o755));
    }

    #[test]
    fn test_pack_is_reproducible() {
        let tmp = TempDir::new("repro");
        tmp.write("a.dll", "a");
        let pkg = package(vec![file("a.dll", "lib")]);
        let first =
            fs::read(pack_at(&pkg, &tmp.0, &tmp.0.join("1"), 1_700_000_000).unwrap()).unwrap();
        let second =
            fs::read(pack_at(&pkg, &tmp.0, &tmp.0.join("2"), 1_700_000_000).unwrap()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn test_errors() {
        let tmp = TempDir::new("errors");
        tmp.write("x.xml", "x");
        tmp.write("other.nuspec", "x");
        for (src, target, reserved) in [
            ("x.xml", "[Content_Types].xml", "[Content_Types].xml"),
            ("x.xml", "_rels", "_rels/x.xml"),
            ("x.xml", "package/services", "package/services/x.xml"),
            ("other.nuspec", "", "other.nuspec"),
        ] {
            let err = pack_at(&package(vec![file(src, target)]), &tmp.0, &tmp.0, 0).unwrap_err();
            assert!(
                matches!(&err, LayoutError::Reserved(p) if p == reserved),
                "{target}: {err}"
            );
        }

        let mut invalid = package(vec![]);
        invalid.metadata.version = String::new();
        let err = pack_at(&invalid, &tmp.0, &tmp.0, 0).unwrap_err();
        assert!(matches!(err, LayoutError::Invalid(_)), "{err}");

        let err = pack_at(
            &package(vec![file("missing.dll", "lib")]),
            &tmp.0,
            &tmp.0,
            0,
        )
        .unwrap_err();
        assert!(matches!(err, LayoutError::Io { .. }), "{err}");
        assert!(!tmp.0.join("Demo.App.1.2.3-rc.1.nupkg").exists());
        assert!(!tmp.0.join("Demo.App.1.2.3-rc.1.nupkg.partial").exists());
    }

    #[test]
    fn test_helpers() {
        assert_eq!(escape_part("a b/c+d%.txt"), "a%20b/c%2Bd%25.txt");
        assert_eq!(fnv(""), 0xcbf2_9ce4_8422_2325);
        let t = zip_time(951_782_400); // 2000-02-29T00:00:00Z
        assert_eq!((t.year(), t.month(), t.day()), (2000, 2, 29));
        assert_eq!(zip_time(0), DateTime::default());
    }
}
