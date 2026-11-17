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

#[test]
fn build_cache_baut_unveraenderte_programme_nicht_neu() {
    let dir = std::env::temp_dir().join(format!("volt-cache-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("cache.vt");
    fs::write(&file, "fn main()\n    print(\"eins\")\n").unwrap();
    let run = || Command::new(env!("CARGO_BIN_EXE_volt")).arg("run").arg(&file).output().unwrap();
    let bin = dir.join(".volt-cache").join("cache");
    assert_eq!(String::from_utf8_lossy(&run().stdout), "eins\n");
    let first = fs::metadata(&bin).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(String::from_utf8_lossy(&run().stdout), "eins\n");
    assert_eq!(fs::metadata(&bin).unwrap().modified().unwrap(), first, "unverändertes Programm wurde neu gebaut");
    fs::write(&file, "fn main()\n    print(\"zwei\")\n").unwrap();
    assert_eq!(String::from_utf8_lossy(&run().stdout), "zwei\n", "Änderung muss neu gebaut werden");
    assert!(fs::metadata(&bin).unwrap().modified().unwrap() > first);
    fs::remove_dir_all(&dir).ok();
}

fn volt(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_volt")).args(args).output().unwrap()
}

#[test]
fn volt_test_fuehrt_tests_einzeln_aus() {
    let dir = std::env::temp_dir().join(format!("volt-test-cmd-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("t.vt");
    fs::write(&file, "test \"gut\"\n    assert(1 + 1 == 2)\n\ntest \"schlecht\"\n    assert(1 + 1 == 3)\n\ntest \"gut2\"\n    assert(true)\n").unwrap();
    let out = volt(&["test", file.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("ok       gut") && text.contains("FEHLER   schlecht: Prüfung fehlgeschlagen (Zeile 5)") && text.contains("ok       gut2"), "{text}");
    assert!(text.contains("2 bestanden, 1 fehlgeschlagen"));
    assert_eq!(out.status.code(), Some(1));
    fs::write(&file, "test \"nur gut\"\n    assert(true)\n").unwrap();
    assert_eq!(volt(&["test", file.to_str().unwrap()]).status.code(), Some(0));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn volt_fmt_und_check() {
    let dir = std::env::temp_dir().join(format!("volt-fmt-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("f.vt");
    fs::write(&file, "fn main()  \n\tprint(\"a  \")   \n\n\n\n    # Kommentar  \n").unwrap();
    assert_eq!(volt(&["fmt", "--check", file.to_str().unwrap()]).status.code(), Some(1));
    assert!(volt(&["fmt", file.to_str().unwrap()]).status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), "fn main()\n    print(\"a  \")\n\n    # Kommentar\n", "Text in Strings muss bleiben");
    assert_eq!(volt(&["fmt", "--check", file.to_str().unwrap()]).status.code(), Some(0));
    assert!(volt(&["check", file.to_str().unwrap()]).status.success());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn volt_new_legt_lauffaehige_projekte_an() {
    let dir = std::env::temp_dir().join(format!("volt-new-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for template in ["cli", "game", "lib"] {
        let proj = dir.join(template);
        assert!(volt(&["new", proj.to_str().unwrap(), "--template", template]).status.success(), "{template}");
        let main = proj.join("main.vt");
        assert!(volt(&["check", main.to_str().unwrap()]).status.success(), "{template} besteht die Prüfung nicht");
    }
    let lib_test = volt(&["test", dir.join("lib/mathe.vt").to_str().unwrap()]);
    assert!(lib_test.status.success(), "{}", String::from_utf8_lossy(&lib_test.stdout));
    assert!(!volt(&["new", dir.join("cli").to_str().unwrap()]).status.success(), "vorhandener Ordner darf nicht überschrieben werden");
    assert!(!volt(&["new", dir.join("x").to_str().unwrap(), "--template", "gibtsnicht"]).status.success());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn standardbibliothek_besteht_ihre_eigenen_tests() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let out = volt(&["test", root.join("tests/std.vt").to_str().unwrap()]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && text.contains("0 fehlgeschlagen") && text.contains("ok       sortiere zahlen"), "{text}");
}

#[test]
fn eigene_datei_hat_vorrang_vor_der_standardbibliothek() {
    let dir = std::env::temp_dir().join(format!("volt-std-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("text.vt"), "fn wiederhole(s: str, n: int) -> str\n    return \"eigene\"\n").unwrap();
    fs::write(dir.join("m.vt"), "use text\n\nfn main()\n    print(text.wiederhole(\"a\", 3))\n").unwrap();
    let out = volt(&["run", dir.join("m.vt").to_str().unwrap()]);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "eigene\n");
    fs::remove_dir_all(&dir).ok();
}
