//! Web-Ziel: Volt → C → WebAssembly (clang, `--target=wasm32-wasi`, Reaktor-Modus) plus `index.html` und `volt.js`.
//! `volt build x.vt --target web [-o ordner]` baut eine fertige Seite, `volt serve x.vt` baut, startet einen kleinen
//! Server und lädt die Seite im Browser neu, sobald sich eine Volt-Datei ändert.
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

const VOLT_JS: &str = include_str!("../web/volt.js");
const INDEX_HTML: &str = include_str!("../web/index.html");
/// Eingebaute C-Header, die Volt-Module einbinden (`import c "volt_web.h" as w`).
pub const HEADERS: &[(&str, &str)] = &[("volt_web.h", include_str!("../../runtime/volt_web.h")), ("volt_thread.h", include_str!("../../runtime/volt_thread.h")), ("volt_net.h", include_str!("../../runtime/volt_net.h")), ("volt_fs.h", include_str!("../../runtime/volt_fs.h")), ("volt_text.h", include_str!("../../runtime/volt_text.h"))];

/// Legt die eingebauten Header im Cache neben der Quelle ab und gibt den Include-Ordner zurück.
pub fn ensure_headers(dir: &Path) -> PathBuf {
    let inc = dir.join(".volt-cache").join("include");
    if fs::create_dir_all(&inc).is_ok() {
        for (name, text) in HEADERS {
            if fs::read_to_string(inc.join(name)).map_or(true, |t| t != *text) {
                let _ = fs::write(inc.join(name), text);
            }
        }
    }
    inc
}

/// Baut `path` als Web-Seite nach `out` (app.wasm, volt.js, index.html).
pub fn build(path: &Path, out: &Path) -> Result<(), ()> {
    let units = crate::frontend(path, true, false)?;
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let cache = dir.join(".volt-cache");
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let c_file = cache.join(format!("{stem}.web.c"));
    let c_code = volt_codegen_c::emit_web(&units.iter().map(|u| &u.module).collect::<Vec<_>>());
    let inc = ensure_headers(dir);
    fs::create_dir_all(out).and_then(|_| fs::write(&c_file, c_code)).map_err(|e| eprintln!("error: `{}` lässt sich nicht schreiben: {e}", c_file.display()))?;

    let cc = std::env::var("VOLT_WASM_CC").unwrap_or_else(|_| "clang".into());
    let mut cmd = Command::new(&cc);
    cmd.args(["--target=wasm32-wasi", "-O2", "-mexec-model=reactor", "-w"])
        .arg(format!("-I{}", dir.display()))
        .arg(format!("-I{}", inc.display()))
        .arg(&c_file)
        .arg("-o")
        .arg(out.join("app.wasm"))
        .arg("-lm");
    match cmd.output() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("error: `{cc}` nicht gefunden – das Web-Ziel braucht clang mit WebAssembly-Unterstützung\n     = tipp: apt install clang lld wasi-libc libclang-rt-dev-wasm32 – oder setze VOLT_WASM_CC");
            return Err(());
        }
        Err(e) => {
            eprintln!("error: `{cc}` lässt sich nicht starten: {e}");
            return Err(());
        }
        Ok(done) if !done.status.success() => {
            eprintln!("interner Fehler: `{cc}` nimmt den erzeugten C-Code nicht an\nC-Datei: {}\n{}", c_file.display(), String::from_utf8_lossy(&done.stderr));
            return Err(());
        }
        Ok(_) => {}
    }
    let title = stem.replace(['_', '-'], " ");
    fs::write(out.join("volt.js"), VOLT_JS).and_then(|_| fs::write(out.join("index.html"), INDEX_HTML.replace("{{TITEL}}", &title))).map_err(|e| eprintln!("error: Seite lässt sich nicht schreiben: {e}"))?;
    Ok(())
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

fn respond(mut s: TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes()).and_then(|_| s.write_all(body));
}

fn handle(stream: TcpStream, root: &Path, version: &AtomicU64) {
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let target = line.split_whitespace().nth(1).unwrap_or("/");
    let url_path = target.split(['?', '#']).next().unwrap_or("/");
    if url_path == "/__stamp" {
        return respond(stream, "200 OK", "text/plain", version.load(Ordering::Relaxed).to_string().as_bytes());
    }
    // nur Dateien innerhalb des Ausgabeordners, keine `..`
    let rel = url_path.trim_start_matches('/');
    if rel.split('/').any(|p| p == ".." || p.starts_with('.')) {
        return respond(stream, "403 Forbidden", "text/plain", b"nicht erlaubt");
    }
    let file = if rel.is_empty() { root.join("index.html") } else { root.join(rel) };
    match fs::read(&file) {
        Ok(bytes) => respond(stream, "200 OK", mime(&file), &bytes),
        Err(_) => respond(stream, "404 Not Found", "text/plain; charset=utf-8", "nicht gefunden".as_bytes()),
    }
}

/// `volt serve`: bauen, ausliefern, bei Änderungen neu bauen und den Browser neu laden lassen.
pub fn serve(path: &Path, host: &str, port: u16) -> Result<u8, ()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let out = dir.join(".volt-cache").join(format!("web-{stem}"));
    let listener = TcpListener::bind((host, port)).map_err(|e| eprintln!("error: {host}:{port} lässt sich nicht öffnen: {e}\n     = tipp: anderer Port mit --port 8081"))?;
    let version = Arc::new(AtomicU64::new(1));
    if build(path, &out).is_err() {
        eprintln!("[serve] Fehler beim ersten Bauen – der Server startet trotzdem, korrigiere die Datei und speichere");
    }
    println!("[serve] http://{}:{port}  (Strg+C beendet)", if host == "0.0.0.0" { "localhost" } else { host });
    {
        let (path, out, version) = (path.to_path_buf(), out.clone(), version.clone());
        thread::spawn(move || {
            let mut last = crate::stamp(&path);
            loop {
                thread::sleep(Duration::from_millis(300));
                let now = crate::stamp(&path);
                if now != last {
                    last = now;
                    eprintln!("[serve] Änderung erkannt – neu bauen");
                    if build(&path, &out).is_ok() {
                        version.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        });
    }
    for stream in listener.incoming().flatten() {
        let (out, version) = (out.clone(), version.clone());
        thread::spawn(move || handle(stream, &out, &version));
    }
    Ok(0)
}
