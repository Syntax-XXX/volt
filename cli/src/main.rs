//! `volt` – eine Binary für alles. Phase 1: `run`, `build`, `parse`.

use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};
use volt_parser::ast::{dump, Module};

const HELP: &str = "\
volt – die Programmiersprache Volt ⚡

  volt run <datei.vt>                             kompilieren und starten
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
        (Some("run"), Some(file)) => run(file),
        (Some("build"), Some(file)) => {
            let out = args.iter().position(|a| a == "-o").and_then(|i| args.get(i + 1)).map(PathBuf::from);
            let default = PathBuf::from(file.file_stem().unwrap_or_default());
            compile(file, flag("--release"), Some(out.unwrap_or(default))).map(|_| 0)
        }
        (Some("parse"), Some(file)) => frontend(file, false).map(|module| {
            print!("{}", if flag("--ast") { dump(&module) } else { "Syntax ok\n".to_string() });
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

/// Datei lesen, lexen, parsen und (für run/build) Typen prüfen. Fehler werden hier ausgegeben.
fn frontend(path: &Path, check: bool) -> Result<Module, ()> {
    let script = match path.extension().and_then(|e| e.to_str()) {
        Some("vt") => false,
        Some("vts") => true,
        _ => {
            eprintln!("error: `{}` ist keine Volt-Datei\n     = tipp: Volt-Programme enden auf .vt, Volt-Skripte auf .vts", path.display());
            return Err(());
        }
    };
    let src = fs::read_to_string(path).map_err(|e| eprintln!("error: `{}` lässt sich nicht lesen: {e}", path.display()))?;
    let (mut module, mut diags) = match volt_lexer::lex(&src) {
        Ok(tokens) if script => volt_parser::parse_script(tokens),
        Ok(tokens) => volt_parser::parse(tokens),
        Err(diag) => (Module { fns: Vec::new() }, vec![diag]),
    };
    if diags.is_empty() && check {
        diags = volt_types::check(&mut module);
    }
    if diags.is_empty() {
        return Ok(module);
    }
    let name = path.display().to_string();
    for d in &diags {
        eprintln!("{}", d.render(&name, &src));
    }
    eprintln!("{} Fehler – nichts gebaut", diags.len());
    Err(())
}

/// Volt → C → Programm. Ohne `out` landet das Programm im Cache neben der Quelldatei.
fn compile(path: &Path, release: bool, out: Option<PathBuf>) -> Result<PathBuf, ()> {
    let module = frontend(path, true)?;
    let cache = path.parent().unwrap_or(Path::new("")).join(".volt-cache");
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let c_file = cache.join(format!("{stem}.c"));
    let bin = out.unwrap_or_else(|| cache.join(&*stem));
    // `volt build prog` (Quelldatei ohne Endung) würde sonst die Quelldatei überschreiben.
    if fs::canonicalize(&bin).ok() == fs::canonicalize(path).ok() {
        eprintln!("error: das Ziel `{}` ist die Quelldatei selbst\n     = tipp: gib mit `-o <name>` ein anderes Ziel an", bin.display());
        return Err(());
    }
    fs::create_dir_all(&cache)
        .and_then(|_| fs::write(&c_file, volt_codegen_c::emit(&module)))
        .map_err(|e| eprintln!("error: `{}` lässt sich nicht schreiben: {e}", c_file.display()))?;

    let chosen = env::var("VOLT_CC").ok();
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
        if !cc.contains("tcc") {
            cmd.arg("-fwrapv"); // int läuft bei Überlauf definiert über; tcc macht das von sich aus
        }
        match cmd.output() {
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => eprintln!("error: `{cc}` lässt sich nicht starten: {e}"),
            Ok(done) if done.status.success() => return Ok(bin),
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

fn run(path: &Path) -> Result<u8, ()> {
    let bin = compile(path, false, None)?;
    let status = Command::new(&bin).status().map_err(|e| eprintln!("error: `{}` lässt sich nicht starten: {e}", bin.display()))?;
    status.code().map(|code| code as u8).ok_or_else(|| eprintln!("error: das Programm wurde vom System abgebrochen ({status})"))
}
