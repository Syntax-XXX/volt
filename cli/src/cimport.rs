//! `import c "header.h" as name`: liest Funktionsprototypen und Zahlenkonstanten aus einem C-Header.
//! Der C-Präprozessor löst Includes und Makros auf (`cc -E`, `cc -E -dM`), danach wird der Text zerlegt.
//! Übernommen wird nur, was Volt versteht: Ganzzahlen, Kommazahlen, bool, `char*` als Text und `void`.
//! Alles andere (Structs, andere Zeiger, Funktionszeiger, variadische Funktionen) wird übersprungen.
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use volt_parser::ast::{CConst, CFunc, Type};

/// Präprozessor-Ausgabe für `#include "header"`; `dm` = nur die Makrodefinitionen.
fn preprocess(header: &str, dir: &Path, dm: bool) -> Result<String, String> {
    let wanted = std::env::var("VOLT_CC").ok();
    let candidates: Vec<&str> = match &wanted {
        Some(cc) => vec![cc.as_str()],
        None => vec!["cc", "gcc", "clang", "tcc"],
    };
    for cc in candidates {
        let mut cmd = Command::new(cc);
        cmd.args(["-E", "-P", "-x", "c"]).arg(format!("-I{}", dir.display())).arg(format!("-I{}/include", dir.display()));
        if dm {
            cmd.arg("-dM");
        }
        cmd.arg("-").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let Ok(mut child) = cmd.spawn() else { continue };
        child.stdin.take().unwrap().write_all(format!("#include \"{header}\"\n").as_bytes()).ok();
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().find(|l| l.contains("error")).unwrap_or("Header nicht gefunden").trim().to_string());
    }
    Err("kein C-Compiler gefunden".into())
}

pub fn scan(header: &str, dir: &Path) -> Result<(Vec<CFunc>, Vec<(String, CConst)>), String> {
    let text = preprocess(header, dir, false)?;
    let funcs = functions(&text);
    // Makros: wenn der Compiler -dM nicht kann (tcc), gibt es eben keine Konstanten.
    let consts = preprocess(header, dir, true).map(|m| constants(&m)).unwrap_or_default();
    Ok((funcs, consts))
}

/// Entfernt `name (...)` samt balancierter Klammer, z. B. `__attribute__ ((noreturn))`.
fn strip_call(text: &str, name: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(name) {
        out += &rest[..i];
        let after = &rest[i + name.len()..];
        let trimmed = after.trim_start();
        if !trimmed.starts_with('(') {
            rest = after;
            continue;
        }
        let mut depth = 0;
        let mut end = 0;
        for (k, c) in trimmed.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = k + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &trimmed[end..];
    }
    out + rest
}

fn clean(text: &str) -> String {
    let mut t = text.to_string();
    for n in ["__attribute__", "__asm__", "__asm", "__declspec"] {
        t = strip_call(&t, n);
    }
    for w in ["__extension__", "__restrict__", "__restrict", "__inline__", "__inline", "extern", "static", "inline", "restrict"] {
        t = replace_word(&t, w);
    }
    t
}

/// Ersetzt ganze Wörter (nicht Teile von Namen) durch ein Leerzeichen.
fn replace_word(text: &str, word: &str) -> String {
    let mut out = String::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut String| {
        if cur == word {
            out.push(' ');
        } else {
            out.push_str(cur);
        }
        cur.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            flush(&mut cur, &mut out);
            out.push(c);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Aussagen auf oberster Ebene: Text bis `;`; geschweifte Blöcke fallen weg (Structs, Enums, Funktionskörper).
fn statements(text: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0);
    for c in text.chars() {
        match c {
            '{' => {
                if depth == 0 && cur.trim_end().ends_with(')') {
                    out.push(cur.trim().to_string()); // Funktionsdefinition: der Kopf zählt als Prototyp
                }
                depth += 1;
            }
            '}' => {
                depth = (depth as i32 - 1).max(0) as usize;
                if depth == 0 {
                    cur.clear();
                }
            }
            ';' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ if depth == 0 => cur.push(c),
            _ => {}
        }
    }
    out
}

fn functions(text: &str) -> Vec<CFunc> {
    let mut found: Vec<CFunc> = Vec::new();
    for st in statements(&clean(text)) {
        if st.starts_with("typedef") || !st.ends_with(')') {
            continue;
        }
        let Some(open) = st.find('(') else { continue };
        let (head, params) = (&st[..open], &st[open + 1..st.len() - 1]);
        if params.contains('(') || head.contains('(') {
            continue; // Funktionszeiger
        }
        let words: Vec<&str> = head.split_whitespace().collect();
        let Some(name) = words.last().map(|w| w.trim_start_matches('*')) else { continue };
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') || name.starts_with("__") {
            continue;
        }
        let ret_text = head[..head.rfind(name).unwrap_or(0)].to_string();
        let Some(ret) = ctype(&ret_text, true) else { continue };
        let mut ps = Vec::new();
        let mut ok = true;
        let params = params.trim();
        if !(params.is_empty() || params == "void") {
            for p in params.split(',') {
                match ctype(p, false) {
                    Some(t) => ps.push(t),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if ok && !found.iter().any(|f| f.name == name) {
            found.push(CFunc { name: name.to_string(), params: ps, ret });
        }
    }
    found
}

/// Ganzzahl-Typnamen aus C (inklusive der üblichen typedefs) – alle werden in Volt zu `int`.
const INT_NAMES: &[&str] = &["int", "long", "short", "char", "size_t", "ssize_t", "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t", "intptr_t", "uintptr_t", "time_t", "off_t", "useconds_t", "suseconds_t", "pid_t", "uid_t", "gid_t", "mode_t", "socklen_t", "clock_t", "wint_t", "ptrdiff_t"];
const TYPE_WORDS: &[&str] = &["double", "float", "unsigned", "signed", "void", "_Bool", "bool"];

/// Typ aus C-Text; `ret` = Rückgabe (dort ist `void` erlaubt und kein Parametername zu entfernen).
fn ctype(text: &str, ret: bool) -> Option<Type> {
    let spaced = text.replace('*', " * ");
    // glibc schreibt Typen intern mit Unterstrichen (`__useconds_t`, `__off_t`): für den Vergleich weglassen
    let mut words: Vec<&str> = spaced.split_whitespace().filter(|w| !matches!(*w, "const" | "volatile" | "register")).map(|w| if w.starts_with("__") && INT_NAMES.contains(&w.trim_start_matches('_')) { w.trim_start_matches('_') } else { w }).collect();
    let stars = words.iter().filter(|w| **w == "*").count();
    words.retain(|w| *w != "*");
    // `double x` → Parametername entfernen (aber nicht bei `unsigned int` oder `long long`)
    if !ret && words.len() > 1 && !TYPE_WORDS.contains(words.last().unwrap()) && !INT_NAMES.contains(words.last().unwrap()) {
        words.pop();
    }
    if words.is_empty() {
        return None;
    }
    let base: Vec<&str> = words.iter().copied().filter(|w| !matches!(*w, "unsigned" | "signed")).collect();
    let base = if base.is_empty() { vec!["int"] } else { base };
    let all_ints = base.iter().all(|w| INT_NAMES.contains(w));
    match stars {
        0 if base == ["void"] => ret.then_some(Type::Void),
        0 if base.iter().all(|w| matches!(*w, "double" | "float" | "long")) && base.iter().any(|w| matches!(*w, "double" | "float")) => Some(Type::F64),
        0 if base == ["_Bool"] || base == ["bool"] => Some(Type::Bool),
        0 if all_ints => Some(Type::Int),
        1 if base == ["char"] => Some(Type::Str),
        _ => None,
    }
}

fn constants(dm: &str) -> Vec<(String, CConst)> {
    let mut out = Vec::new();
    for line in dm.lines() {
        let Some(rest) = line.strip_prefix("#define ") else { continue };
        let Some((name, value)) = rest.split_once(' ') else { continue };
        if name.starts_with('_') || name.contains('(') || out.iter().any(|(n, _)| n == name) {
            continue;
        }
        let v = value.trim().trim_start_matches('(').trim_end_matches(')').trim();
        let neg = v.starts_with('-');
        let digits = v.trim_start_matches('-').trim();
        let int_text = digits.trim_end_matches(|c| matches!(c, 'u' | 'U' | 'l' | 'L'));
        let parsed = if let Some(h) = int_text.strip_prefix("0x").or_else(|| int_text.strip_prefix("0X")) {
            i64::from_str_radix(h, 16).ok().map(CConst::Int)
        } else if !int_text.is_empty() && int_text.chars().all(|c| c.is_ascii_digit()) {
            int_text.parse::<i64>().ok().map(CConst::Int)
        } else if digits.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.') && (digits.contains('.') || digits.contains('e') || digits.contains('E')) {
            digits.trim_end_matches(|c| matches!(c, 'f' | 'F' | 'l' | 'L')).parse::<f64>().ok().map(CConst::Float)
        } else {
            None
        };
        if let Some(c) = parsed {
            out.push((name.to_string(), match (c, neg) { (CConst::Int(n), true) => CConst::Int(-n), (CConst::Float(x), true) => CConst::Float(-x), (c, _) => c }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typen_aus_c_text() {
        assert_eq!(ctype("double x", false), Some(Type::F64));
        assert_eq!(ctype("unsigned int", false), Some(Type::Int));
        assert_eq!(ctype("long long n", false), Some(Type::Int));
        assert_eq!(ctype("useconds_t us", false), Some(Type::Int));
        assert_eq!(ctype("pid_t", true), Some(Type::Int));
        assert_eq!(ctype("__useconds_t __us", false), Some(Type::Int));
        assert_eq!(ctype("const char *s", false), Some(Type::Str));
        assert_eq!(ctype("void", true), Some(Type::Void));
        assert_eq!(ctype("void", false), None);
        assert_eq!(ctype("FILE *f", false), None);
        assert_eq!(ctype("int *p", false), None);
    }

    #[test]
    fn prototypen_und_konstanten() {
        let text = "extern double sin (double __x) __attribute__ ((__nothrow__)); typedef struct { int a; } S; int puts(const char *s); int printf(const char *f, ...); static inline int twice(int x) { return x * 2; } void qsort(void *b, int n, int (*cmp)(const void*, const void*));";
        let f = functions(text);
        let names: Vec<&str> = f.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["sin", "puts", "twice"], "{names:?}");
        assert_eq!(f[0].params, vec![Type::F64]);
        assert_eq!(f[1].ret, Type::Int);
        let c = constants("#define M_PI 3.14159265358979323846\n#define EOF (-1)\n#define BUFSIZ 8192\n#define X 0xFF\n#define __HIDDEN 1\n#define F(x) x\n#define NAME \"text\"\n");
        assert_eq!(c.len(), 4);
        assert!(c.contains(&("EOF".to_string(), CConst::Int(-1))));
        assert!(c.contains(&("X".to_string(), CConst::Int(255))));
    }

    #[test]
    fn echter_header() {
        let dir = std::env::temp_dir();
        let (f, c) = scan("math.h", &dir).expect("math.h");
        assert!(f.iter().any(|f| f.name == "cbrt" && f.params == vec![Type::F64] && f.ret == Type::F64));
        assert!(c.iter().any(|(n, v)| n == "M_PI" && matches!(v, CConst::Float(x) if (x - std::f64::consts::PI).abs() < 1e-12)));
        assert!(scan("gibt_es_nicht.h", &dir).is_err());
    }
}
