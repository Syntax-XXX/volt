//! Typprüfung: trägt die Typen in den AST ein und sammelt alle Fehler.

use std::collections::HashMap;
use volt_lexer::{Diag, Span};
use volt_parser::ast::*;

#[derive(Clone)]
struct FnSig {
    params: Vec<Type>,
    ret: Type,
}

#[derive(Clone, Copy)]
enum VarKind {
    Let,
    Var,
    Param,
    Loop,
}

#[derive(Clone, Copy)]
struct Var {
    ty: Type,
    kind: VarKind,
}

struct Checker {
    fns: HashMap<String, FnSig>,
    /// Sichtbare Variablen, innerster Block zuletzt.
    scopes: Vec<HashMap<String, Var>>,
    /// Rückgabetyp der Funktion, die gerade geprüft wird.
    ret: Type,
    diags: Vec<Diag>,
}

/// Prüft das Programm. Ohne Fehler sind danach alle `ty`-Felder im AST gesetzt.
pub fn check(m: &mut Module) -> Vec<Diag> {
    let mut c = Checker { fns: HashMap::new(), scopes: Vec::new(), ret: Type::Void, diags: Vec::new() };
    // Zuerst alle Signaturen: Funktionen dürfen sich in beliebiger Reihenfolge aufrufen.
    for f in &mut m.fns {
        for p in &mut f.params {
            p.ty = c.resolve(&p.ty_ref);
        }
        f.ret = f.ret_ref.as_ref().map_or(Type::Void, |r| c.resolve(r));
        let sig = FnSig { params: f.params.iter().map(|p| p.ty).collect(), ret: f.ret };
        if f.name == "print" {
            c.diags.push(Diag::new(f.span, "`print` ist schon eingebaut").tip("gib deiner Funktion einen anderen Namen"));
        } else if c.fns.insert(f.name.clone(), sig).is_some() {
            c.diags.push(Diag::new(f.span, format!("die Funktion `{}` gibt es schon", f.name)));
        }
    }
    match m.fns.iter().find(|f| f.name == "main") {
        None => c.diags.push(Diag::new(Span { line: 1, col: 1, len: 1 }, "keine `fn main()` gefunden").tip("jedes Programm startet in `fn main()`")),
        Some(f) if !f.params.is_empty() || f.ret_ref.is_some() => {
            c.diags.push(Diag::new(f.span, "`main` hat keine Parameter und keinen Rückgabewert").tip("schreibe `fn main()`"));
        }
        Some(_) => {}
    }
    for f in &mut m.fns {
        c.function(f);
    }
    // `x += 1` mit unbekanntem `x` meldet denselben Fehler für Ziel und Wert.
    c.diags.dedup_by(|a, b| a.span == b.span && a.msg == b.msg);
    c.diags
}

/// Gibt dieser Block auf jedem Weg einen Wert zurück?
fn returns(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Return { .. } => true,
        Stmt::If { then, els, .. } => returns(then) && returns(els),
        _ => false,
    })
}

impl Checker {
    fn resolve(&mut self, t: &TypeRef) -> Type {
        match t.name.as_str() {
            "int" => Type::Int,
            "f64" => Type::F64,
            "bool" => Type::Bool,
            "str" => Type::Str,
            other => {
                self.diags.push(Diag::new(t.span, format!("den Typ `{other}` gibt es nicht")).tip("Volt kennt bisher: int, f64, bool, str"));
                Type::Unknown
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<Var> {
        self.scopes.iter().rev().find_map(|s| s.get(name)).copied()
    }

    fn declare(&mut self, name: &str, span: Span, ty: Type, kind: VarKind) {
        if self.lookup(name).is_some() {
            self.diags.push(
                Diag::new(span, format!("`{name}` gibt es hier schon")).tip("nimm einen anderen Namen – in Volt steht ein Name immer für genau eine Variable"),
            );
        } else if name == "print" || self.fns.contains_key(name) {
            self.diags.push(Diag::new(span, format!("`{name}` ist schon eine Funktion")).tip("nimm für die Variable einen anderen Namen"));
        }
        self.scopes.last_mut().expect("Variablen gibt es nur in Funktionen").insert(name.to_string(), Var { ty, kind });
    }

    /// Meldet einen Fehler, wenn `got` nicht dorthin passt, wo `want` erwartet wird. int wird automatisch zu f64.
    fn expect(&mut self, want: Type, got: Type, span: Span, msg: impl FnOnce(&str, &str) -> String) {
        if want == got || want == Type::Unknown || got == Type::Unknown || (want == Type::F64 && got == Type::Int) {
            return;
        }
        let mut d = Diag::new(span, msg(want.name(), got.name()));
        if want == Type::Int && got == Type::F64 {
            d = d.tip("f64 wird nicht automatisch zu int – die Nachkommastellen gingen verloren");
        }
        self.diags.push(d);
    }

    fn function(&mut self, f: &mut FnDecl) {
        self.ret = f.ret;
        self.scopes.push(HashMap::new());
        for p in &f.params {
            self.declare(&p.name, p.span, p.ty, VarKind::Param);
        }
        self.block(&mut f.body);
        self.scopes.pop();
        if !matches!(f.ret, Type::Void | Type::Unknown) && !returns(&f.body) {
            self.diags.push(
                Diag::new(f.span, format!("`{}` gibt nicht auf jedem Weg einen Wert zurück", f.name))
                    .tip(format!("beende die Funktion mit `return …` ({})", f.ret.name())),
            );
        }
    }

    fn block(&mut self, stmts: &mut [Stmt]) {
        self.scopes.push(HashMap::new());
        for s in stmts {
            self.stmt(s);
        }
        self.scopes.pop();
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match s {
            Stmt::Let { name, span, mutable, ty_ref, ty, init } => {
                let got = self.value(init);
                *ty = match ty_ref {
                    Some(r) => {
                        let want = self.resolve(r);
                        self.expect(want, got, init.span, |w, g| format!("`{name}` soll {w} sein, der Wert ist aber {g}"));
                        want
                    }
                    None => got,
                };
                self.declare(name, *span, *ty, if *mutable { VarKind::Var } else { VarKind::Let });
            }
            Stmt::Assign { name, span, value } => {
                let target = self.lookup(name);
                match target.map(|v| v.kind) {
                    None => self.diags.push(Diag::new(*span, format!("`{name}` gibt es hier nicht")).tip(format!("lege die Variable zuerst an: `var {name} = …`"))),
                    Some(VarKind::Var) => {}
                    Some(kind) => {
                        let tip = match kind {
                            VarKind::Param => format!("Parameter sind fest – kopiere den Wert in eine eigene Variable, z. B. `var wert = {name}`"),
                            VarKind::Loop => "die Schleifenvariable setzt Volt selbst – nutze dafür eine eigene `var`".to_string(),
                            _ => format!("nutze `var {name} = …`, wenn sich der Wert ändern soll"),
                        };
                        self.diags.push(Diag::new(*span, format!("`{name}` ist unveränderlich")).tip(tip));
                    }
                }
                let got = self.value(value);
                if let Some(var) = target {
                    self.expect(var.ty, got, value.span, |w, g| format!("`{name}` ist {w}, der neue Wert ist aber {g}"));
                }
            }
            Stmt::If { cond, then, els } => {
                self.cond(cond);
                self.block(then);
                self.block(els);
            }
            Stmt::While { cond, body } => {
                self.cond(cond);
                self.block(body);
            }
            Stmt::For { var, span, start, end, body } => {
                for bound in [start, end] {
                    let got = self.value(bound);
                    self.expect(Type::Int, got, bound.span, |_, g| format!("der Bereich braucht int-Werte, das hier ist {g}"));
                }
                self.scopes.push(HashMap::new());
                self.declare(var, *span, Type::Int, VarKind::Loop);
                self.block(body);
                self.scopes.pop();
            }
            Stmt::Return { span, value } => match value {
                Some(v) => {
                    let got = self.value(v);
                    if self.ret == Type::Void {
                        let mut d = Diag::new(v.span, "diese Funktion gibt nichts zurück");
                        if got != Type::Unknown {
                            d = d.tip(format!("schreibe `-> {}` hinter die Parameter, wenn sie einen Wert liefern soll", got.name()));
                        }
                        self.diags.push(d);
                    } else {
                        self.expect(self.ret, got, v.span, |w, g| format!("die Funktion gibt {w} zurück, das hier ist aber {g}"));
                    }
                }
                None => {
                    if !matches!(self.ret, Type::Void | Type::Unknown) {
                        self.diags.push(Diag::new(*span, format!("hier fehlt der Rückgabewert ({})", self.ret.name())));
                    }
                }
            },
            Stmt::Expr(e) => {
                self.expr(e);
                if !matches!(e.kind, ExprKind::Call { .. }) {
                    let mut d = Diag::new(e.span, "dieser Wert wird nicht benutzt");
                    if matches!(e.kind, ExprKind::Binary(BinOp::Eq, ..)) {
                        d = d.tip("zum Zuweisen nutze `=` statt `==`");
                    }
                    self.diags.push(d);
                }
            }
        }
    }

    fn cond(&mut self, e: &mut Expr) {
        let got = self.value(e);
        if !matches!(got, Type::Bool | Type::Unknown) {
            let mut d = Diag::new(e.span, format!("die Bedingung muss bool sein, ist aber {}", got.name()));
            if matches!(got, Type::Int | Type::F64) {
                d = d.tip("vergleiche die Zahl, z. B. mit `!= 0`");
            }
            self.diags.push(d);
        }
    }

    /// Wie `expr`, aber der Ausdruck muss einen Wert liefern.
    fn value(&mut self, e: &mut Expr) -> Type {
        if self.expr(e) == Type::Void {
            self.diags.push(Diag::new(e.span, "dieser Aufruf gibt keinen Wert zurück"));
            e.ty = Type::Unknown;
        }
        e.ty
    }

    fn expr(&mut self, e: &mut Expr) -> Type {
        let span = e.span;
        e.ty = match &mut e.kind {
            ExprKind::Int(_) => Type::Int,
            ExprKind::Float(_) => Type::F64,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Str(segs) => {
                for seg in segs {
                    if let StrSeg::Expr(inner) = seg {
                        self.value(inner);
                    }
                }
                Type::Str
            }
            ExprKind::Ident(name) => match self.lookup(name) {
                Some(var) => var.ty,
                None => {
                    let mut d = Diag::new(span, format!("`{name}` gibt es hier nicht"));
                    if *name == "print" || self.fns.contains_key(name.as_str()) {
                        d = d.tip(format!("`{name}` ist eine Funktion – rufe sie mit `{name}(…)` auf"));
                    }
                    self.diags.push(d);
                    Type::Unknown
                }
            },
            ExprKind::Unary(op, operand) => {
                let got = self.value(operand);
                let (sym, want, ok) = match op {
                    UnOp::Neg => ("-", "eine Zahl", matches!(got, Type::Int | Type::F64)),
                    UnOp::Not => ("not", "bool", got == Type::Bool),
                };
                if ok {
                    got
                } else {
                    if got != Type::Unknown {
                        self.diags.push(Diag::new(span, format!("`{sym}` braucht {want}, bekommt aber {}", got.name())));
                    }
                    Type::Unknown
                }
            }
            ExprKind::Binary(op, l, r) => {
                let (lt, rt) = (self.value(l), self.value(r));
                self.binary(*op, lt, rt, span)
            }
            ExprKind::Call { name, args } => {
                let got: Vec<(Type, Span)> = args.iter_mut().map(|a| (self.value(a), a.span)).collect();
                if *name == "print" {
                    if got.len() != 1 {
                        self.diags.push(
                            Diag::new(span, format!("`print` gibt genau einen Wert aus, bekommen hat es {}", got.len()))
                                .tip("mehrere Werte passen in einen String: `print(\"{a} {b}\")`"),
                        );
                    }
                    Type::Void
                } else if let Some(sig) = self.fns.get(name.as_str()).cloned() {
                    if sig.params.len() != got.len() {
                        self.diags.push(Diag::new(span, format!("`{name}` erwartet {} Argument(e), bekommen hat es {}", sig.params.len(), got.len())));
                    }
                    for (i, (want, (got, at))) in sig.params.iter().zip(&got).enumerate() {
                        self.expect(*want, *got, *at, |w, g| format!("Argument {} von `{name}` soll {w} sein, ist aber {g}", i + 1));
                    }
                    sig.ret
                } else {
                    let mut d = Diag::new(span, format!("die Funktion `{name}` gibt es nicht"));
                    if self.lookup(name).is_some() {
                        d = d.tip(format!("`{name}` ist eine Variable, keine Funktion"));
                    }
                    self.diags.push(d);
                    Type::Unknown
                }
            }
        };
        e.ty
    }

    fn binary(&mut self, op: BinOp, l: Type, r: Type, span: Span) -> Type {
        use {BinOp::*, Type::*};
        let arithmetic = matches!(op, Add | Sub | Mul | Div | Mod);
        let num = |t| matches!(t, Int | F64);
        let ty = match op {
            _ if l == Unknown || r == Unknown => Some(if arithmetic { Unknown } else { Bool }),
            Add | Sub | Mul | Div if num(l) && num(r) => Some(if l == F64 || r == F64 { F64 } else { Int }),
            Mod if l == Int && r == Int => Some(Int),
            Lt | Le | Gt | Ge if num(l) && num(r) => Some(Bool),
            Eq | Ne if l == r || (num(l) && num(r)) => Some(Bool),
            And | Or if l == Bool && r == Bool => Some(Bool),
            _ => None,
        };
        ty.unwrap_or_else(|| {
            let mut d = Diag::new(span, format!("`{}` geht nicht mit {} und {}", op.sym(), l.name(), r.name()));
            if op == Add && (l == Str || r == Str) {
                d = d.tip("Strings setzt man mit Einfügungen zusammen: `\"{a}{b}\"`");
            } else if op == Mod && num(l) && num(r) {
                d = d.tip("`%` gibt es bisher nur für int");
            } else if matches!(op, And | Or) && (num(l) || num(r)) {
                d = d.tip("`and` und `or` verknüpfen Bedingungen, z. B. `a > 0 and b > 0`");
            }
            self.diags.push(d);
            if arithmetic { Unknown } else { Bool }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> (Module, Vec<Diag>) {
        let (mut m, diags) = volt_parser::parse(volt_lexer::lex(src).unwrap());
        assert!(diags.is_empty(), "Syntaxfehler im Test: {diags:?}");
        let diags = check(&mut m);
        (m, diags)
    }

    /// Fehlermeldungen für den Rumpf von `fn main()`.
    fn errors(body: &str) -> Vec<String> {
        let body: String = body.lines().map(|l| format!("    {l}\n")).collect();
        run(&format!("fn main()\n{body}")).1.into_iter().map(|d| d.msg).collect()
    }

    fn tip(body: &str) -> String {
        let body: String = body.lines().map(|l| format!("    {l}\n")).collect();
        run(&format!("fn main()\n{body}")).1.remove(0).tip.unwrap_or_default()
    }

    #[test]
    fn gueltiges_programm_bekommt_typen() {
        let src = "\
fn half(x: f64) -> f64
    return x / 2

fn main()
    let a = 1 + 2 * 3
    let b = a / 2.0
    let c = a < 3 and not (b == 1)
    let d = \"wert {a} {half(a)}\"
    var e: f64 = a
    e += 1
    for i in 0..a
        print(i)
    print(d)
";
        let (m, diags) = run(src);
        assert!(diags.is_empty(), "{diags:?}");
        let types: Vec<Type> = m.fns[1]
            .body
            .iter()
            .filter_map(|s| match s {
                Stmt::Let { ty, .. } => Some(*ty),
                _ => None,
            })
            .collect();
        assert_eq!(types, vec![Type::Int, Type::F64, Type::Bool, Type::Str, Type::F64]);
    }

    #[test]
    fn unveraenderliche_variablen() {
        assert_eq!(errors("let score = 0\nscore += 10"), vec!["`score` ist unveränderlich"]);
        assert_eq!(tip("let score = 0\nscore += 10"), "nutze `var score = …`, wenn sich der Wert ändern soll");
        assert_eq!(errors("for i in 0..3\n    i = 5"), vec!["`i` ist unveränderlich"]);
        let (_, diags) = run("fn f(n: int)\n    n = 1\n\nfn main()\n    f(1)\n");
        assert!(diags[0].tip.as_ref().unwrap().contains("Parameter sind fest"));
    }

    #[test]
    fn typen_muessen_passen() {
        assert_eq!(errors("let x: int = 1.5"), vec!["`x` soll int sein, der Wert ist aber f64"]);
        assert_eq!(errors("var x = 1\nx = \"a\""), vec!["`x` ist int, der neue Wert ist aber str"]);
        assert_eq!(errors("let x = 1 + true"), vec!["`+` geht nicht mit int und bool"]);
        assert_eq!(errors("let x = \"a\" + \"b\""), vec!["`+` geht nicht mit str und str"]);
        assert!(tip("let x = \"a\" + \"b\"").contains("Einfügungen"));
        assert_eq!(errors("let x = 1.5 % 2"), vec!["`%` geht nicht mit f64 und int"]);
        assert_eq!(errors("let x = 1 == \"1\""), vec!["`==` geht nicht mit int und str"]);
        assert_eq!(errors("let x = not 1"), vec!["`not` braucht bool, bekommt aber int"]);
        assert_eq!(errors("let x = -\"a\""), vec!["`-` braucht eine Zahl, bekommt aber str"]);
        assert_eq!(errors("let x: zahl = 1"), vec!["den Typ `zahl` gibt es nicht"]);
        assert_eq!(errors("for i in 0..1.5\n    print(i)"), vec!["der Bereich braucht int-Werte, das hier ist f64"]);
    }

    #[test]
    fn bedingungen_sind_bool() {
        assert_eq!(errors("let n = 1\nif n\n    print(n)"), vec!["die Bedingung muss bool sein, ist aber int"]);
        assert_eq!(errors("while \"ja\"\n    print(1)"), vec!["die Bedingung muss bool sein, ist aber str"]);
    }

    #[test]
    fn namen() {
        assert_eq!(errors("print(x)"), vec!["`x` gibt es hier nicht"]);
        assert_eq!(errors("x += 1"), vec!["`x` gibt es hier nicht"]);
        assert_eq!(errors("let a = 1\nlet a = 2"), vec!["`a` gibt es hier schon"]);
        assert_eq!(errors("let a = 1\nif a > 0\n    let a = 2\n    print(a)"), vec!["`a` gibt es hier schon"]);
        assert_eq!(errors("if true\n    let a = 1\n    print(a)\nlet a = 2\nprint(a)"), Vec::<String>::new());
        assert_eq!(errors("let main = 1"), vec!["`main` ist schon eine Funktion"]);
        assert_eq!(errors("let f = print"), vec!["`print` gibt es hier nicht"]);
        assert_eq!(errors("foo(1)"), vec!["die Funktion `foo` gibt es nicht"]);
    }

    #[test]
    fn aufrufe() {
        let with = |call: &str| -> Vec<String> {
            let src = format!("fn add(a: int, b: f64) -> f64\n    return a + b\n\nfn log(x: int)\n    print(x)\n\nfn main()\n    {call}\n");
            run(&src).1.into_iter().map(|d| d.msg).collect()
        };
        assert_eq!(with("print(add(1, 2))"), Vec::<String>::new());
        assert_eq!(with("print(add(1))"), vec!["`add` erwartet 2 Argument(e), bekommen hat es 1"]);
        assert_eq!(with("print(add(1.5, 2))"), vec!["Argument 1 von `add` soll int sein, ist aber f64"]);
        assert_eq!(with("let x = log(1)"), vec!["dieser Aufruf gibt keinen Wert zurück"]);
        assert_eq!(with("print(1, 2)"), vec!["`print` gibt genau einen Wert aus, bekommen hat es 2"]);
        assert_eq!(with("add(1, 2) == 3.0"), vec!["dieser Wert wird nicht benutzt"]);
    }

    #[test]
    fn rueckgabewerte() {
        let msgs = |src: &str| -> Vec<String> { run(src).1.into_iter().map(|d| d.msg).collect() };
        let main = "\nfn main()\n    print(1)\n";
        assert_eq!(msgs(&format!("fn f(n: int) -> int\n    if n > 0\n        return 1\n{main}")), vec!["`f` gibt nicht auf jedem Weg einen Wert zurück"]);
        assert_eq!(msgs(&format!("fn f(n: int) -> int\n    if n > 0\n        return 1\n    else\n        return 2\n{main}")), Vec::<String>::new());
        assert_eq!(msgs(&format!("fn f() -> int\n    return \"a\"\n{main}")), vec!["die Funktion gibt int zurück, das hier ist aber str"]);
        assert_eq!(msgs(&format!("fn f() -> int\n    return\n{main}")), vec!["hier fehlt der Rückgabewert (int)"]);
        assert_eq!(msgs(&format!("fn f()\n    return 1\n{main}")), vec!["diese Funktion gibt nichts zurück"]);
        assert_eq!(msgs("fn f()\n    print(1)\n"), vec!["keine `fn main()` gefunden"]);
        assert_eq!(msgs("fn main(a: int)\n    print(a)\n"), vec!["`main` hat keine Parameter und keinen Rückgabewert"]);
        assert_eq!(msgs(&format!("fn main()\n    print(2)\n{main}")), vec!["die Funktion `main` gibt es schon"]);
    }

    #[test]
    fn mehrere_fehler_ohne_folgefehler() {
        // Der unbekannte Name `y` darf keine weiteren Meldungen nach sich ziehen.
        assert_eq!(errors("let a = y + 1\nlet b = a * 2\nlet c: int = \"x\""), vec!["`y` gibt es hier nicht", "`c` soll int sein, der Wert ist aber str"]);
    }
}
