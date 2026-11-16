//! Jedes Beispiel in `examples/` muss genau die Ausgabe aus `<datei>.out` liefern.
use std::{fs, process::Command};

#[test]
fn beispiele_liefern_die_erwartete_ausgabe() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples");
    let mut count = 0;
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !matches!(path.extension().and_then(|e| e.to_str()), Some("vt" | "vts")) {
            continue;
        }
        let out = Command::new(env!("CARGO_BIN_EXE_volt")).arg("run").arg(&path).output().unwrap();
        let want = fs::read_to_string(format!("{}.out", path.display())).unwrap();
        assert!(out.status.success(), "{}: {}", path.display(), String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), want, "{}", path.display());
        count += 1;
    }
    assert!(count >= 4);
}

#[test]
fn fremde_endung_wird_abgelehnt() {
    let out = Command::new(env!("CARGO_BIN_EXE_volt")).args(["run", "Cargo.toml"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains(".vt"));
}

#[test]
fn division_durch_null_bricht_sauber_ab() {
    let dir = std::env::temp_dir().join(format!("volt-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("null.vts");
    fs::write(&file, "let a = 0\nprint(1 / a)\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_volt")).arg(&file).output().unwrap();
    fs::remove_dir_all(&dir).ok();
    assert_eq!(out.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Division durch null (Zeile 2)"));
}
