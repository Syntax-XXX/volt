//! `volt` – eine Binary für alles. Phase 1: `run`, `build`, `parse`.

use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};
use volt_parser::ast::{dump, Module};
use volt_types::Unit;

const HELP: &str = "\
volt – die Programmiersprache Volt ⚡

  volt run <datei.vt> [--watch]                   kompilieren und starten; mit --watch bei jeder Änderung neu (Strg+C beendet)
  volt build <datei.vt> [--release] [-o <ziel>]   Programm bauen
  volt parse <datei.vt> [--ast]                   Syntax prüfen, mit --ast den Syntaxbaum zeigen
  volt <skript.vts>                               Volt Script direkt starten (auch per `#!/usr/bin/env volt`)

Dateien: `.vt` ist ein Programm mit `fn main()`, `.vts` (Volt Script) läuft von oben nach unten, ohne main.

C-Compiler: Debug-Builds nehmen tcc (sonst cc, gcc, clang), --release nimmt clang oder gcc mit -O2.
Mit der Umgebungsvariable VOLT_CC legst du den Compiler selbst fest.";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let file = args.get(1).filter(|a| !a.starts_with('-')).map(Path::new);
    let result = match (args.first().map(String::as_str), file) {
        (Some(script), _) if script.ends_with(".vts") || script.ends_with(".vt") => run(Path::new(script)),
        (Some("run"), Some(file)) if flag("--watch") => watch(file),
        (Some("run"), Some(file)) => run(file),
        (Some("build"), Some(file)) => {
            let out = args.iter().position(|a| a == "-o").and_then(|i| args.get(i + 1)).map(PathBuf::from);
            let default = PathBuf::from(file.file_stem().unwrap_or_default());
            compile(file, flag("--release"), Some(out.unwrap_or(default))).map(|_| 0)
        }
        (Some("parse"), Some(file)) => frontend(file, false).map(|units| {
            print!("{}", if flag("--ast") { dump(&units[0].module) } else { "Syntax ok\n".to_string() });
            0
        }),
        (Some("--version"), _) => {
            println!("volt {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        (None | Some("help" | "--help" | "-h"), _) => {
            println!("{HELP}");
            Ok(0)
        }
        _ => {
            eprintln!("{HELP}");
            Err(())
        }
    };
    ExitCode::from(result.unwrap_or(1))
}

/// FNV-1a über mehrere Teile – stabil über Rust-Versionen hinweg (anders als `DefaultHasher`).
fn fnv1a(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for part in parts {
        for b in *part {
            h = (h ^ *b as u64).wrapping_mul(0x100000001b3);
        }
        h = (h ^ 0xff).wrapping_mul(0x100000001b3); // Trenner, damit „ab“+„c“ ≠ „a“+„bc“
    }
    h
}

/// Eine Quelldatei: Hauptprogramm (Name leer) oder Modul aus `use name` (`name.vt` im selben Ordner).
struct Source {
    file: String,
    src: String,
}

fn lex_parse(src: &str, script: bool) -> (Module, Vec<volt_lexer::Diag>) {
    match volt_lexer::lex(src) {
        Ok(tokens) if script => volt_parser::parse_script(tokens),
        Ok(tokens) => volt_parser::parse(tokens),
        Err(diag) => (Module::default(), vec![diag]),
    }
}

/// Datei samt aller `use`-Module lesen, lexen, parsen und (für run/build) Typen prüfen. Fehler werden hier ausgegeben.
fn frontend(path: &Path, check: bool) -> Result<Vec<Unit>, ()> {
    let script = match path.extension().and_then(|e| e.to_str()) {
        Some("vt") => false,
        Some("vts") => true,
        _ => {
            eprintln!("error: `{}` ist keine Volt-Datei\n     = tipp: Volt-Programme enden auf .vt, Volt-Skripte auf .vts", path.display());
            return Err(());
        }
    };
    let src = fs::read_to_string(path).map_err(|e| eprintln!("error: `{}` lässt sich nicht lesen: {e}", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new(""));
    let (module, d) = lex_parse(&src, script);
    let mut units = vec![Unit { name: String::new(), module }];
    let mut sources = vec![Source { file: path.display().to_string(), src }];
    let mut diags: Vec<(usize, volt_lexer::Diag)> = d.into_iter().map(|d| (0, d)).collect();
    // Module der Reihe nach nachladen; jedes nur einmal (auch bei gegenseitigem `use`).
    let mut i = 0;
    while i < units.len() {
        let uses = units[i].module.uses.clone();
        for (name, span) in uses {
            if units.iter().any(|u| u.name == name) {
                continue;
            }
            let file = dir.join(format!("{name}.vt"));
            match fs::read_to_string(&file) {
                Ok(src) => {
                    let (module, d) = lex_parse(&src, false);
                    diags.extend(d.into_iter().map(|d| (units.len(), d)));
                    units.push(Unit { name: name.clone(), module });
                    sources.push(Source { file: file.display().to_string(), src });
                }
                Err(_) => diags.push((i, volt_lexer::Diag::new(span, format!("das Modul `{name}` gibt es nicht")).tip(format!("lege `{}` an", file.display())))),
            }
        }
        i += 1;
    }
    if diags.is_empty() && check {
        diags = volt_types::check_program(&mut units);
    }
    if diags.is_empty() {
        return Ok(units);
    }
    for (u, d) in &diags {
        eprintln!("{}", d.render(&sources[*u].file, &sources[*u].src));
    }
    eprintln!("{} Fehler – nichts gebaut", diags.len());
    Err(())
}

/// Volt → C → Programm. Ohne `out` landet das Programm im Cache neben der Quelldatei.
fn compile(path: &Path, release: bool, out: Option<PathBuf>) -> Result<PathBuf, ()> {
    let units = frontend(path, true)?;
    let cache = path.parent().unwrap_or(Path::new("")).join(".volt-cache");
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let c_file = cache.join(format!("{stem}.c"));
    let bin = out.unwrap_or_else(|| cache.join(&*stem));
    // `volt build prog` (Quelldatei ohne Endung) würde sonst die Quelldatei überschreiben.
    if fs::canonicalize(&bin).ok() == fs::canonicalize(path).ok() {
        eprintln!("error: das Ziel `{}` ist die Quelldatei selbst\n     = tipp: gib mit `-o <name>` ein anderes Ziel an", bin.display());
        return Err(());
    }
    let c_code = volt_codegen_c::emit_program(&units.iter().map(|u| &u.module).collect::<Vec<_>>());
    fs::create_dir_all(&cache)
        .and_then(|_| fs::write(&c_file, &c_code))
        .map_err(|e| eprintln!("error: `{}` lässt sich nicht schreiben: {e}", c_file.display()))?;

    // Build-Cache: derselbe C-Code (enthält die Runtime) + dieselben Compiler-Einstellungen + dasselbe Ziel → Programm ist schon da.
    let chosen = env::var("VOLT_CC").ok();
    let key = format!("{:016x} {} {} {}", fnv1a(&[c_code.as_bytes(), chosen.as_deref().unwrap_or("").as_bytes(), &[release as u8], env!("CARGO_PKG_VERSION").as_bytes()]), release, chosen.as_deref().unwrap_or("-"), bin.display());
    let key_file = cache.join(format!("{stem}.key"));
    if bin.exists() && fs::read_to_string(&key_file).is_ok_and(|k| k == key) {
        return Ok(bin);
    }

    let candidates: Vec<&str> = match &chosen {
        Some(cc) => vec![cc],
        None if release => vec!["clang", "gcc", "cc", "tcc"],
        None => vec!["tcc", "cc", "gcc", "clang"],
    };
    for cc in candidates {
        let mut cmd = Command::new(cc);
        cmd.arg(&c_file).arg("-o").arg(&bin);
        if release {
            cmd.arg("-O2");
        }
        cmd.arg("-lm"); // sqrt, fabs
        if !cc.contains("tcc") {
            cmd.arg("-fwrapv"); // int läuft bei Überlauf definiert über; tcc macht das von sich aus
        }
        match cmd.output() {
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => eprintln!("error: `{cc}` lässt sich nicht starten: {e}"),
            Ok(done) if done.status.success() => {
                // Schlüssel erst nach erfolgreichem Bauen schreiben (kaputte Builds gelten nie als „aktuell“).
                let _ = fs::write(&key_file, &key);
                return Ok(bin);
            }
            Ok(done) => eprintln!(
                "interner Fehler: `{cc}` nimmt den erzeugten C-Code nicht an – das ist ein Fehler in Volt, nicht in deinem Programm.\nC-Datei: {}\n{}",
                c_file.display(),
                String::from_utf8_lossy(&done.stderr)
            ),
        }
        return Err(());
    }
    eprintln!("error: kein C-Compiler gefunden\n     = tipp: installiere tcc (am schnellsten), gcc oder clang – oder setze VOLT_CC");
    Err(())
}

/// Zeitstempel aller Volt-Dateien im Ordner der Quelle – ändert er sich, hat jemand gespeichert.
fn stamp(path: &Path) -> (usize, Option<std::time::SystemTime>) {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let files: Vec<_> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("vt" | "vts"))).collect();
    (files.len(), files.iter().filter_map(|p| fs::metadata(p).and_then(|m| m.modified()).ok()).max())
}

/// `volt run --watch`: bauen, starten, bei Änderung stoppen und neu bauen. Fehler stoppen die Schleife nicht.
// shortcut: Neustart statt Hot-Reload mit erhaltenem Zustand, Phase 4 braucht dafür dynamisches Laden; Abfrage alle 250 ms statt inotify.
fn watch(path: &Path) -> Result<u8, ()> {
    let mut last = stamp(path);
    eprintln!("[watch] beobachte {} (Strg+C beendet)", path.display());
    loop {
        let mut child = match compile(path, false, None) {
            Ok(bin) => Command::new(&bin).spawn().map_err(|e| eprintln!("error: `{}` lässt sich nicht starten: {e}", bin.display())).ok(),
            Err(()) => {
                eprintln!("[watch] Fehler – warte auf die nächste Änderung");
                None
            }
        };
        while stamp(path) == last {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        last = stamp(path);
        if let Some(c) = child.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
        eprintln!("[watch] Änderung erkannt – neu bauen");
    }
}

fn run(path: &Path) -> Result<u8, ()> {
    let bin = compile(path, false, None)?;
    let status = Command::new(&bin).status().map_err(|e| eprintln!("error: `{}` lässt sich nicht starten: {e}", bin.display()))?;
    status.code().map(|code| code as u8).ok_or_else(|| eprintln!("error: das Programm wurde vom System abgebrochen ({status})"))
}
