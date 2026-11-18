//! Snapshot-Tests: jedes Programm in `examples/` und `tests/` (Unterordner: nur `main.vt`) wird gestartet.
//! Ein Ordner mit `main.vt` ist ein Projekt mit Modulen (nur `main.vt` starten), sonst zählt jede Datei.
//! Server-Programme (mit `http.diene`) laufen ewig und werden übersprungen; sie testet `http_server_antwortet_gleichzeitig`.
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
        } else if matches!(path.extension().and_then(|e| e.to_str()), Some("vt" | "vts")) && (!projekt || path.file_name().unwrap() == "main.vt") && !fs::read_to_string(&path).map_or(false, |t| t.contains("http.diene")) {
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

#[test]
fn web_ziel_meldet_fehlendes_clang_verstaendlich() {
    let dir = std::env::temp_dir().join(format!("volt-web-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("w.vt");
    fs::write(&file, "use web\n\nfn main()\n    web.setze_text(\"#a\", \"x\")\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_volt")).args(["build", file.to_str().unwrap(), "--target", "web", "-o", dir.join("o").to_str().unwrap()]).env("VOLT_WASM_CC", "gibt-es-nicht-clang").output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("nicht gefunden") && err.contains("wasm"), "{err}");
    // das Programm läuft auch ohne Browser (Aufrufe werden ausgegeben)
    let run = volt(&["run", file.to_str().unwrap()]);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "[web] setze_text #a = x\n");
    fs::remove_dir_all(&dir).ok();
}

/// Mit einem echten WebAssembly-clang (VOLT_WASM_CC gesetzt oder `clang` mit wasm32) wird die ganze Seite gebaut.
#[test]
fn web_ziel_baut_seite_wenn_clang_da_ist() {
    let cc = std::env::var("VOLT_WASM_CC").unwrap_or_else(|_| "clang".into());
    let has_wasm = Command::new(&cc).args(["--target=wasm32-wasi", "--version"]).output().is_ok_and(|o| o.status.success()) && std::env::var("VOLT_WASM_CC").is_ok();
    if !has_wasm {
        eprintln!("übersprungen: kein WebAssembly-clang (VOLT_WASM_CC setzen)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("volt-web-build-{}", std::process::id()));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let out = volt(&["build", root.join("examples/web_zaehler.vt").to_str().unwrap(), "--target", "web", "-o", dir.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    for f in ["index.html", "app.wasm", "volt.js"] {
        assert!(dir.join(f).exists(), "{f} fehlt");
    }
    assert_eq!(&fs::read(dir.join("app.wasm")).unwrap()[..4], b"\0asm");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn language_server_meldet_fehler_und_vervollstaendigt() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_volt")).arg("lsp").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let frame = |json: &str| format!("Content-Length: {}\r\n\r\n{json}", json.len());
    let src = "use liste\\n\\nfn main()\\n    let x: str = 1\\n    liste.so\\n";
    for m in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.to_string(),
        format!(r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///tmp/volt-lsp-test/a.vt","text":"{src}"}}}}}}"#),
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///tmp/volt-lsp-test/a.vt"},"position":{"line":4,"character":10}}}"#.to_string(),
        r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#.to_string(),
        r#"{"jsonrpc":"2.0","method":"exit"}"#.to_string(),
    ] {
        stdin.write_all(frame(&m).as_bytes()).unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("soll str sein") && text.contains("publishDiagnostics"), "{text}");
    assert!(text.contains("\"label\":\"sortiere\""), "{text}");
}

/// Jedes ```volt-Beispiel im Tutorial muss die Prüfung bestehen (und `main` haben, damit es sich wirklich starten lässt).
#[test]
fn tutorial_beispiele_sind_gueltig() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let text = fs::read_to_string(root.join("docs/TUTORIAL.md")).unwrap();
    let dir = std::env::temp_dir().join(format!("volt-tutorial-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let mut count = 0;
    for block in text.split("```volt\n").skip(1) {
        let code = block.split("```").next().unwrap();
        count += 1;
        let file = dir.join(format!("b{count}.vt"));
        fs::write(&file, code).unwrap();
        let out = volt(&["run", file.to_str().unwrap()]);
        assert!(out.status.success(), "Tutorial-Beispiel {count} schlägt fehl:\n{code}\n{}", String::from_utf8_lossy(&out.stderr));
    }
    assert!(count >= 14, "zu wenige Beispiele: {count}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn paketmanager_holt_pakete_per_git() {
    let dir = std::env::temp_dir().join(format!("volt-pkg-{}", std::process::id()));
    let repo = dir.join("geo-repo");
    let proj = dir.join("projekt");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&proj).unwrap();
    fs::write(repo.join("geo.vt"), "fn doppelt(x: int) -> int\n    return x * 2\n").unwrap();
    let git = |args: &[&str], cwd: &Path| {
        let o = Command::new("git").args(args).current_dir(cwd).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["init", "-q"], &repo);
    git(&["add", "."], &repo);
    git(&["commit", "-q", "-m", "x"], &repo);
    fs::write(proj.join("main.vt"), "use geo\n\nfn main()\n    print(geo.doppelt(21))\n").unwrap();
    let volt_in = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_volt")).args(args).current_dir(&proj).output().unwrap();
    let add = volt_in(&["add", "geo", repo.to_str().unwrap()]);
    assert!(add.status.success(), "{}", String::from_utf8_lossy(&add.stderr));
    assert!(fs::read_to_string(proj.join("volt.toml")).unwrap().contains("geo = "));
    assert_eq!(String::from_utf8_lossy(&volt_in(&["run", "main.vt"]).stdout), "42\n");
    // `volt install` holt alles neu, auch nach dem Löschen
    fs::remove_dir_all(proj.join(".volt")).unwrap();
    assert!(volt_in(&["install"]).status.success());
    assert_eq!(String::from_utf8_lossy(&volt_in(&["run", "main.vt"]).stdout), "42\n");
    assert!(!volt_in(&["add", "x", "/gibt/es/nicht"]).status.success());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn http_server_antwortet_gleichzeitig() {
    use std::io::{Read, Write};
    let dir = std::env::temp_dir().join(format!("volt-http-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let port = 20000 + (std::process::id() % 20000) as u16;
    let src = format!("use http\n\nfn behandle(a: http.Anfrage) -> http.Antwort\n    if a.pfad == \"/kaputt\"\n        return http.fehler(500, \"x\")\n    return http.text(\"{{a.methode}} {{a.pfad}} {{a.anfrage}} [{{a.inhalt}}]\")\n\nfn main()\n    http.diene({port}, behandle)\n");
    let file = dir.join("s.vt");
    fs::write(&file, src).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_volt")).args(["run", file.to_str().unwrap()]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let ask = |req: String| -> String {
        for _ in 0..100 {
            if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                s.write_all(req.as_bytes()).unwrap();
                let mut out = String::new();
                s.read_to_string(&mut out).unwrap();
                return out;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("Server startet nicht");
    };
    let r = ask("GET /a?x=1 HTTP/1.1\r\nHost: t\r\n\r\n".into());
    assert!(r.starts_with("HTTP/1.1 200 OK") && r.ends_with("GET /a x=1 []"), "{r}");
    let r = ask("POST /p HTTP/1.1\r\nHost: t\r\nContent-Length: 5\r\n\r\nhallo".into());
    assert!(r.ends_with("POST /p  [hallo]"), "{r}");
    assert!(ask("GET /kaputt HTTP/1.1\r\n\r\n".into()).starts_with("HTTP/1.1 500"));
    let hs: Vec<_> = (0..200).map(|i| std::thread::spawn(move || ask_n(port, i))).collect();
    for h in hs { assert!(h.join().unwrap()); }
    child.kill().ok();
    child.wait().ok();
    fs::remove_dir_all(&dir).ok();
}

fn ask_n(port: u16, i: u32) -> bool {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(format!("GET /n{i} HTTP/1.1\r\n\r\n").as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.ends_with(&format!("GET /n{i}  []"))
}
