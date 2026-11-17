//! `volt lsp`: Language-Server für Editoren (stdin/stdout, JSON-RPC). Fehler beim Tippen, Vervollständigung, Hover.
use std::{
    collections::HashMap,
    io::{BufRead, Read, Write},
    path::PathBuf,
};
use volt_parser::ast::{FnDecl, Module};

#[derive(Clone, Debug)]
enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn get(&self, k: &str) -> &J {
        if let J::Obj(o) = self {
            if let Some((_, v)) = o.iter().find(|(n, _)| n == k) {
                return v;
            }
        }
        &J::Null
    }
    fn str(&self) -> &str {
        if let J::Str(s) = self { s } else { "" }
    }
    fn num(&self) -> f64 {
        if let J::Num(n) = self { *n } else { 0.0 }
    }
    fn write(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Num(n) => out.push_str(&n.to_string()),
            J::Str(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            J::Arr(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            J::Obj(o) => {
                out.push('{');
                for (i, (k, v)) in o.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    J::Str(k.clone()).write(out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn obj(pairs: Vec<(&str, J)>) -> J {
    J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
fn s(x: &str) -> J {
    J::Str(x.to_string())
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn parse(&mut self) -> Option<J> {
        self.ws();
        match *self.b.get(self.i)? {
            b'n' => (self.i += 4, Some(J::Null)).1,
            b't' => (self.i += 4, Some(J::Bool(true))).1,
            b'f' => (self.i += 5, Some(J::Bool(false))).1,
            b'"' => self.string().map(J::Str),
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                loop {
                    self.ws();
                    if self.b.get(self.i) == Some(&b']') {
                        self.i += 1;
                        return Some(J::Arr(a));
                    }
                    a.push(self.parse()?);
                    self.ws();
                    if self.b.get(self.i) == Some(&b',') {
                        self.i += 1;
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut o = Vec::new();
                loop {
                    self.ws();
                    if self.b.get(self.i) == Some(&b'}') {
                        self.i += 1;
                        return Some(J::Obj(o));
                    }
                    let k = self.string()?;
                    self.ws();
                    self.i += 1; // :
                    o.push((k, self.parse()?));
                    self.ws();
                    if self.b.get(self.i) == Some(&b',') {
                        self.i += 1;
                    }
                }
            }
            _ => {
                let st = self.i;
                while self.i < self.b.len() && (self.b[self.i].is_ascii_digit() || b"+-.eE".contains(&self.b[self.i])) {
                    self.i += 1;
                }
                std::str::from_utf8(&self.b[st..self.i]).ok()?.parse().ok().map(J::Num)
            }
        }
    }
    fn string(&mut self) -> Option<String> {
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        while *self.b.get(self.i)? != b'"' {
            if self.b[self.i] == b'\\' {
                self.i += 1;
                match self.b[self.i] {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'u' => {
                        let h = std::str::from_utf8(&self.b[self.i + 1..self.i + 5]).ok()?;
                        let mut cp = u32::from_str_radix(h, 16).ok()?;
                        self.i += 4;
                        if (0xd800..0xdc00).contains(&cp) && self.b.get(self.i + 1) == Some(&b'\\') {
                            let lo = u32::from_str_radix(std::str::from_utf8(&self.b[self.i + 4..self.i + 8]).ok()?, 16).ok()?;
                            cp = 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00);
                            self.i += 6;
                        }
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(char::from_u32(cp).unwrap_or('?').encode_utf8(&mut buf).as_bytes());
                    }
                    c => out.push(c),
                }
            } else {
                out.push(self.b[self.i]);
            }
            self.i += 1;
        }
        self.i += 1;
        String::from_utf8(out).ok()
    }
}

fn send(v: &J) {
    let mut body = String::new();
    v.write(&mut body);
    let mut o = std::io::stdout().lock();
    let _ = write!(o, "Content-Length: {}\r\n\r\n{body}", body.len()).and_then(|_| o.flush());
}

fn path_of(uri: &str) -> PathBuf {
    let p = uri.strip_prefix("file://").unwrap_or(uri);
    let mut out = Vec::new();
    let b = p.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&p[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    PathBuf::from(String::from_utf8_lossy(&out).into_owned())
}

const KEYWORDS: &[&str] = &["fn", "let", "var", "if", "else", "while", "for", "in", "return", "struct", "enum", "match", "none", "use", "mut", "break", "continue", "defer", "true", "false", "and", "or", "not", "test", "trait", "import"];
const BUILTINS: &[(&str, &str)] = &[
    ("print", "print(x) – gibt einen Wert mit Zeilenumbruch aus"),
    ("input", "input(prompt) -> str? – liest eine Zeile"),
    ("error", "error(text) – Fehler für T!"),
    ("int", "int(x: f64) -> int"),
    ("float", "float(x: int) -> f64"),
    ("sqrt", "sqrt(x: f64) -> f64"),
    ("abs", "abs(x) – Betrag"),
    ("min", "min(a, b)"),
    ("max", "max(a, b)"),
    ("pow", "pow(a: f64, b: f64) -> f64"),
    ("floor", "floor(x: f64) -> f64"),
    ("ceil", "ceil(x: f64) -> f64"),
    ("sin", "sin(x: f64) -> f64"),
    ("cos", "cos(x: f64) -> f64"),
    ("assert", "assert(bedingung) – bricht ab, wenn falsch (für test-Blöcke)"),
    ("exit", "exit(code)"),
];
const SNIPPETS: &[(&str, &str)] = &[
    ("fn", "fn ${1:name}(${2})\n    ${0}"),
    ("for", "for ${1:i} in 0..${2:n}\n    ${0}"),
    ("if", "if ${1:bedingung}\n    ${0}"),
    ("while", "while ${1:bedingung}\n    ${0}"),
    ("struct", "struct ${1:Name}\n    ${2:feld}: ${3:int}\n"),
    ("enum", "enum ${1:Name}\n    ${2:Fall}\n"),
    ("match", "match ${1:wert}\n    ${2:Fall} => ${0}"),
    ("test", "test \"${1:name}\"\n    assert(${0})"),
    ("main", "fn main()\n    ${0}"),
];

fn sig(f: &FnDecl) -> String {
    let mut ps: Vec<String> = Vec::new();
    if f.this.is_some() {
        ps.push("self".into());
    }
    ps.extend(f.params.iter().map(|p| format!("{}: {}", p.name, p.ty_ref.text())));
    let gen = if f.generics.is_empty() { String::new() } else { format!("[{}]", f.generics.join(", ")) };
    format!("fn {}{gen}({}){}", f.name, ps.join(", "), f.ret_ref.as_ref().map(|r| format!(" -> {}", r.text())).unwrap_or_default())
}

/// Quelltext eines Moduls: Datei neben dem Dokument oder eingebaute Standardbibliothek.
fn module_source(name: &str, dir: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(dir.join(format!("{name}.vt"))).ok().or_else(|| crate::std_module(name).map(String::from))
}

fn parse_module(src: &str, script: bool) -> Module {
    crate::lex_parse(src, script).0
}

fn diagnose(uri: &str, text: &str) -> Vec<J> {
    let path = path_of(uri);
    let mut out = Vec::new();
    if let Err((_, diags)) = crate::load(&path, text.to_string(), true, false) {
        for (u, d) in diags {
            if u != 0 {
                continue;
            }
            let (line, col) = (d.span.line.saturating_sub(1) as f64, d.span.col.saturating_sub(1) as f64);
            let msg = match &d.tip {
                Some(t) => format!("{}\ntipp: {t}", d.msg),
                None => d.msg.clone(),
            };
            out.push(obj(vec![
                ("range", obj(vec![("start", obj(vec![("line", J::Num(line)), ("character", J::Num(col))])), ("end", obj(vec![("line", J::Num(line)), ("character", J::Num(col + d.span.len.max(1) as f64))]))])),
                ("severity", J::Num(1.0)),
                ("source", s("volt")),
                ("message", J::Str(msg)),
            ]));
        }
    }
    out
}

fn word_at(line: &str, ch: usize) -> (String, String) {
    let chars: Vec<char> = line.chars().collect();
    let is = |c: char| c.is_alphanumeric() || c == '_';
    let mut a = ch.min(chars.len());
    while a > 0 && is(chars[a - 1]) {
        a -= 1;
    }
    let mut b = ch.min(chars.len());
    while b < chars.len() && is(chars[b]) {
        b += 1;
    }
    let qualifier = if a > 0 && chars[a - 1] == '.' {
        let mut q = a - 1;
        while q > 0 && is(chars[q - 1]) {
            q -= 1;
        }
        chars[q..a - 1].iter().collect()
    } else {
        String::new()
    };
    (chars[a..b].iter().collect(), qualifier)
}

fn item(label: &str, kind: f64, detail: &str, insert: Option<&str>) -> J {
    let mut v = vec![("label", s(label)), ("kind", J::Num(kind)), ("detail", s(detail))];
    if let Some(i) = insert {
        v.push(("insertText", s(i)));
        v.push(("insertTextFormat", J::Num(2.0)));
    }
    obj(v)
}

fn completion(uri: &str, text: &str, line: usize, ch: usize) -> J {
    let path = path_of(uri);
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let cur = text.lines().nth(line).unwrap_or("");
    let before: String = cur.chars().take(ch).collect();
    let (prefix, qualifier) = word_at(cur, ch);
    let _ = prefix;
    let mut items = Vec::new();
    let m = parse_module(text, path.extension().is_some_and(|e| e == "vts"));
    if !qualifier.is_empty() && before.trim_end_matches(|c: char| c.is_alphanumeric() || c == '_').ends_with('.') {
        // `modul.` → Funktionen und Typen des Moduls; `Typ.` → Methoden/Fälle
        if m.uses.iter().any(|(n, _)| *n == qualifier) {
            if let Some(src) = module_source(&qualifier, &dir) {
                let mm = parse_module(&src, false);
                items.extend(mm.fns.iter().filter(|f| !f.name.starts_with("__")).map(|f| item(&f.name, 3.0, &sig(f), None)));
                items.extend(mm.structs.iter().map(|x| item(&x.name, 22.0, "struct", None)));
                items.extend(mm.enums.iter().map(|x| item(&x.name, 13.0, "enum", None)));
            }
        } else if let Some(e) = m.enums.iter().find(|e| e.name == qualifier) {
            items.extend(e.variants.iter().map(|v| item(&v.name, 20.0, &format!("Fall von {}", e.name), None)));
        } else if let Some(st) = m.structs.iter().find(|x| x.name == qualifier) {
            items.extend(st.methods.iter().map(|f| item(&f.name, 2.0, &sig(f), None)));
        }
        if !items.is_empty() {
            return J::Arr(items);
        }
    }
    // nach `wert.` ohne bekannten Typ: alle Methoden-Namen der Datei und der Standardtypen
    if before.ends_with('.') || (!qualifier.is_empty() && before.ends_with(&format!(".{}", word_at(cur, ch).0))) {
        let mut seen = std::collections::HashSet::new();
        for st in &m.structs {
            for f in &st.methods {
                if seen.insert(f.name.clone()) {
                    items.push(item(&f.name, 2.0, &sig(f), None));
                }
            }
            for fl in &st.fields {
                items.push(item(&fl.name, 5.0, &format!("Feld: {}", fl.ty_ref.text()), None));
            }
        }
        for n in ["len", "push", "pop", "remove", "insert", "clear", "contains", "join", "upper", "lower", "trim", "split", "replace", "starts_with", "ends_with", "to_int", "to_f64", "has", "get", "keys", "or"] {
            if seen.insert(n.to_string()) {
                items.push(item(n, 2.0, "eingebaut", None));
            }
        }
        return J::Arr(items);
    }
    items.extend(KEYWORDS.iter().map(|k| item(k, 14.0, "Schlüsselwort", None)));
    items.extend(BUILTINS.iter().map(|(n, d)| item(n, 3.0, d, None)));
    items.extend(SNIPPETS.iter().map(|(n, b)| item(&format!("{n} …"), 15.0, "Snippet", Some(b)).clone()));
    items.extend(m.fns.iter().filter(|f| !f.name.starts_with("__")).map(|f| item(&f.name, 3.0, &sig(f), None)));
    items.extend(m.structs.iter().map(|x| item(&x.name, 22.0, "struct", None)));
    items.extend(m.enums.iter().map(|x| item(&x.name, 13.0, "enum", None)));
    items.extend(m.uses.iter().map(|(n, _)| item(n, 9.0, "Modul", None)));
    for g in &m.globals {
        if let volt_parser::ast::Stmt::Let { name, .. } = g {
            items.push(item(name, 6.0, "globale Variable", None));
        }
    }
    J::Arr(items)
}

fn hover(uri: &str, text: &str, line: usize, ch: usize) -> J {
    let path = path_of(uri);
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let (word, qualifier) = word_at(text.lines().nth(line).unwrap_or(""), ch);
    if word.is_empty() {
        return J::Null;
    }
    let m = parse_module(text, path.extension().is_some_and(|e| e == "vts"));
    let mut found: Option<String> = None;
    if !qualifier.is_empty() && m.uses.iter().any(|(n, _)| *n == qualifier) {
        if let Some(src) = module_source(&qualifier, &dir) {
            found = parse_module(&src, false).fns.iter().find(|f| f.name == word).map(sig);
        }
    }
    if found.is_none() {
        found = m.fns.iter().find(|f| f.name == word).map(sig).or_else(|| m.structs.iter().flat_map(|x| x.methods.iter()).find(|f| f.name == word).map(sig));
    }
    if found.is_none() {
        found = m.structs.iter().find(|x| x.name == word).map(|x| format!("struct {}\n{}", x.name, x.fields.iter().map(|f| format!("    {}: {}", f.name, f.ty_ref.text())).collect::<Vec<_>>().join("\n")));
    }
    if found.is_none() {
        found = m.enums.iter().find(|x| x.name == word).map(|x| format!("enum {}\n{}", x.name, x.variants.iter().map(|v| format!("    {}", v.name)).collect::<Vec<_>>().join("\n")));
    }
    if found.is_none() {
        found = BUILTINS.iter().find(|(n, _)| *n == word).map(|(_, d)| d.to_string());
    }
    match found {
        Some(t) => obj(vec![("contents", obj(vec![("kind", s("markdown")), ("value", J::Str(format!("```volt\n{t}\n```")))]))]),
        None => J::Null,
    }
}

pub fn run() {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut docs: HashMap<String, String> = HashMap::new();
    loop {
        let mut len = 0usize;
        loop {
            let mut line = String::new();
            if input.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let line = line.trim();
            if line.is_empty() {
                break;
            }
            if let Some(v) = line.strip_prefix("Content-Length:") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0u8; len];
        if input.read_exact(&mut body).is_err() {
            return;
        }
        let Some(msg) = (P { b: &body, i: 0 }).parse() else { continue };
        let (method, id, params) = (msg.get("method").str().to_string(), msg.get("id").clone(), msg.get("params").clone());
        let reply = |result: J| send(&obj(vec![("jsonrpc", s("2.0")), ("id", id.clone()), ("result", result)]));
        let uri = params.get("textDocument").get("uri").str().to_string();
        let publish = |docs: &HashMap<String, String>, uri: &str| {
            if let Some(t) = docs.get(uri) {
                send(&obj(vec![("jsonrpc", s("2.0")), ("method", s("textDocument/publishDiagnostics")), ("params", obj(vec![("uri", s(uri)), ("diagnostics", J::Arr(diagnose(uri, t)))]))]));
            }
        };
        match method.as_str() {
            "initialize" => reply(obj(vec![
                ("capabilities", obj(vec![("textDocumentSync", J::Num(1.0)), ("completionProvider", obj(vec![("triggerCharacters", J::Arr(vec![s(".")]))])), ("hoverProvider", J::Bool(true))])),
                ("serverInfo", obj(vec![("name", s("volt lsp")), ("version", s(env!("CARGO_PKG_VERSION")))])),
            ])),
            "shutdown" => reply(J::Null),
            "exit" => return,
            "textDocument/didOpen" => {
                docs.insert(uri.clone(), params.get("textDocument").get("text").str().to_string());
                publish(&docs, &uri);
            }
            "textDocument/didChange" => {
                if let J::Arr(ch) = params.get("contentChanges") {
                    if let Some(last) = ch.last() {
                        docs.insert(uri.clone(), last.get("text").str().to_string());
                    }
                }
                publish(&docs, &uri);
            }
            "textDocument/didSave" => publish(&docs, &uri),
            "textDocument/didClose" => {
                docs.remove(&uri);
                send(&obj(vec![("jsonrpc", s("2.0")), ("method", s("textDocument/publishDiagnostics")), ("params", obj(vec![("uri", s(&uri)), ("diagnostics", J::Arr(vec![]))]))]));
            }
            "textDocument/completion" | "textDocument/hover" => {
                let (line, ch) = (params.get("position").get("line").num() as usize, params.get("position").get("character").num() as usize);
                let text = docs.get(&uri).cloned().unwrap_or_default();
                reply(if method.ends_with("completion") { completion(&uri, &text, line, ch) } else { hover(&uri, &text, line, ch) });
            }
            _ if !matches!(id, J::Null) => send(&obj(vec![("jsonrpc", s("2.0")), ("id", id.clone()), ("error", obj(vec![("code", J::Num(-32601.0)), ("message", s("nicht unterstützt"))]))])),
            _ => {}
        }
    }
}
