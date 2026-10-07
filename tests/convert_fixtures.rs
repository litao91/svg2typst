use std::fs;
use std::process::Command;

/// Every fixture must convert without panicking or erroring and produce
/// non-empty CeTZ output. (Full Typst compilation is checked by
/// `scripts/verify.sh`, which needs `typst` on PATH.)
#[test]
fn all_fixtures_convert_without_error() {
    let bin = env!("CARGO_BIN_EXE_svg2cetz");
    let mut count = 0;
    for entry in fs::read_dir("tests/fixtures").expect("tests/fixtures missing") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("svg") {
            continue;
        }
        let out = Command::new(bin)
            .arg(&path)
            .output()
            .expect("failed to run svg2cetz");
        assert!(
            out.status.success(),
            "{:?} exited with {}: {}",
            path,
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(!stdout.trim().is_empty(), "{:?} produced no output", path);
        count += 1;
    }
    assert!(count > 0, "no svg fixtures found");
}

#[test]
fn standalone_output_wraps_in_canvas() {
    let bin = env!("CARGO_BIN_EXE_svg2cetz");
    let out = Command::new(bin)
        .arg("--standalone")
        .arg("tests/fixtures/robot.svg")
        .output()
        .expect("failed to run svg2cetz");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(stdout.starts_with("#import \"@preview/cetz:"));
    assert!(stdout.contains("#cetz.canvas(length: 1cm, {"));
    assert!(stdout.contains("import cetz.draw: *"));
}
