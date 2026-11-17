//! Snapshot-Tests: jedes Programm in `examples/` und `tests/` (Unterordner: nur `main.vt`) wird gestartet.
//! Ein Ordner mit `main.vt` ist ein Projekt mit Modulen (nur `main.vt` starten), sonst zählt jede Datei.
//! `<datei>.out` = erwartete Ausgabe, `<datei>.in` = Eingabe, `<datei>.err` = erwarteter Text in stderr (dann Exit ≠ 0).
use std::{fs, io::Write, path::Path, process::{Command, Stdio}};

fn programme(dir: &Path, out: &mut Vec<std::path::PathBuf>, top: bool) {
    let projekt = !top && dir.join("main.vt").exists();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if !path.ends_with(".volt-cache") {
                programme(&path, out, false);
            }
        } else if matches!(path.extension().and_then(|e| e.to_str()), Some("vt" | "vts")) && (!projekt || path.file_name().unwrap() == "main.vt") {
            out.push(path);
        }
    }
}

#[test]
fn programme_liefern_die_erwartete_ausgabe() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut list = Vec::new();
    programme(&root.join("examples"), &mut list, true);
    programme(&root.join("tests"), &mut list, true);
    list.sort();
    assert!(list.len() >= 15, "zu wenige Programme gefunden: {list:?}");
    let mut fehler = Vec::new();
    for path in &list {
        let p = path.display();
        let mut child = Command::new(env!("CARGO_BIN_EXE_volt")).arg("run").arg(path)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let input = fs::read(format!("{p}.in")).unwrap_or_default();
        child.stdin.take().unwrap().write_all(&input).unwrap();
        let out = child.wait_with_output().unwrap();
        let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        match fs::read_to_string(format!("{p}.err")) {
            Ok(want) if out.status.success() || !stderr.contains(want.trim()) => fehler.push(format!("{p}: erwartet Fehler „{}“, stderr: {stderr}", want.trim())),
            Err(_) if !out.status.success() => fehler.push(format!("{p}: Exit {:?}\n{stderr}", out.status.code())),
            _ => {}
        }
        if let Ok(want) = fs::read_to_string(format!("{p}.out")) {
            if stdout != want {
                fehler.push(format!("{p}: Ausgabe weicht ab\n--- erwartet\n{want}--- bekommen\n{stdout}"));
            }
        }
    }
    assert!(fehler.is_empty(), "{}", fehler.join("\n\n"));
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
