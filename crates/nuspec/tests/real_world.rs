//! Parses `.nuspec` files taken from real packages on nuget.org, writes them back and checks
//! that nothing is lost.

use nuspec::Package;
use std::{fs, path::Path};

fn fixtures() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "nuspec"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read_to_string(&p).unwrap())
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures found");
    files
}

#[test]
fn parse_validate_and_roundtrip() {
    for (name, xml) in fixtures() {
        let pkg = Package::from_xml(&xml).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            pkg.namespace
                .as_deref()
                .is_some_and(|ns| ns.starts_with("http://schemas.microsoft.com/packaging/")),
            "{name}: unexpected namespace {:?}",
            pkg.namespace
        );
        pkg.validate().unwrap_or_else(|e| panic!("{name}: {e}"));

        let written = pkg.to_xml().unwrap_or_else(|e| panic!("{name}: {e}"));
        let reparsed = Package::from_xml(&written).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(reparsed, pkg, "{name}: roundtrip changed the package");
    }
}

#[test]
fn fields_are_read() {
    let xml = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/microsoft.aspnetcore.authentication.jwtbearer.nuspec"),
    )
    .unwrap();
    let m = Package::from_xml(&xml).unwrap().metadata;
    assert_eq!(m.id, "Microsoft.AspNetCore.Authentication.JwtBearer");
    assert_eq!(m.serviceable, Some(true));
    assert_eq!(
        m.license_url.as_deref(),
        Some("https://licenses.nuget.org/MIT")
    );
    let references = m.framework_references.unwrap();
    assert_eq!(references.group[0].target_framework, "net9.0");
    assert_eq!(
        references.group[0].framework_reference[0].name,
        "Microsoft.AspNetCore.App"
    );
}
