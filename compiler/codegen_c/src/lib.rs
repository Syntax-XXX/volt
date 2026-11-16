//! C-Backend: geprüfter AST → ein C-Programm (Runtime, Funktionen, `main`).
//! Namen aus Volt bekommen das Präfix `v_`, damit sie nie mit C-Schlüsselwörtern oder der Runtime (`volt_…`) kollidieren.

use volt_parser::ast::*;

const RUNTIME: &str = include_str!("../../../runtime/volt_rt.h");

/// Erwartet einen AST, den der Typchecker ohne Fehler geprüft hat.
pub fn emit(m: &Module) -> String {
    let mut g = Gen { out: String::from(RUNTIME), tmp: 0 };
    g.out.push('\n');
    for f in &m.fns {
        g.out += &format!("{};\n", signature(f));
    }
    for f in &m.fns {
        g.out += &format!("\n{} {{\n", signature(f));
        g.block(&f.body, 1);
        g.out += "}\n";
    }
    g.out += "\nint main(void) {\n    v_main();\n    return 0;\n}\n";
    g.out
}

fn ctype(t: Type) -> &'static str {
    match t {
        Type::Int => "int64_t",
        Type::F64 => "double",
        Type::Bool => "bool",
        Type::Str => "volt_str",
        Type::Void | Type::Unknown => "void",
    }
}

fn signature(f: &FnDecl) -> String {
    let params: Vec<String> = f.params.iter().map(|p| format!("{} v_{}", ctype(p.ty), p.name)).collect();
    let params = if params.is_empty() { "void".to_string() } else { params.join(", ") };
    format!("static {} v_{}({params})", ctype(f.ret), f.name)
}

/// String-Literal für C. Alles außer druckbarem ASCII wird oktal geschrieben (UTF-8 bleibt Byte für Byte erhalten).
fn c_string(text: &str) -> String {
    let mut out = String::from("\"");
    for b in text.bytes() {
        match b {
            b'"' => out += "\\\"",
            b'\\' => out += "\\\\",
            0x20..=0x7e => out.push(b as char),
            _ => out += &format!("\\{b:03o}"),
        }
    }
    out + "\""
}

fn has_call(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Call { .. } => true,
        ExprKind::Unary(_, x) => has_call(x),
        ExprKind::Binary(_, l, r) => has_call(l) || has_call(r),
        ExprKind::Str(segs) => segs.iter().any(|s| matches!(s, StrSeg::Expr(e) if has_call(e))),
        _ => false,
    }
}

struct Gen {
    out: String,
    /// Zähler für Hilfsvariablen.
    tmp: u32,
}

impl Gen {
    fn line(&mut self, depth: usize, text: &str) {
        self.out += &"    ".repeat(depth);
        self.out += text;
        self.out.push('\n');
    }

    fn block(&mut self, stmts: &[Stmt], depth: usize) {
        for s in stmts {
            self.stmt(s, depth);
        }
    }

    fn stmt(&mut self, s: &Stmt, d: usize) {
        match s {
            Stmt::Let { name, ty, init, .. } => {
                let value = self.expr(init);
                self.line(d, &format!("{} v_{name} = {value};", ctype(*ty)));
            }
            Stmt::Assign { name, value, .. } => {
                let value = self.expr(value);
                self.line(d, &format!("v_{name} = {value};"));
            }
            Stmt::If { cond, then, els } => {
                let cond = self.expr(cond);
                self.line(d, &format!("if ({cond}) {{"));
                self.block(then, d + 1);
                if !els.is_empty() {
                    self.line(d, "} else {");
                    self.block(els, d + 1);
                }
                self.line(d, "}");
            }
            Stmt::While { cond, body } => {
                let cond = self.expr(cond);
                self.line(d, &format!("while ({cond}) {{"));
                self.block(body, d + 1);
                self.line(d, "}");
            }
            Stmt::For { var, start, end, body, .. } => {
                // Das Ende wird einmal vor der Schleife berechnet, nach dem Anfang.
                let (start, end) = (self.expr(start), self.expr(end));
                self.tmp += 1;
                let stop = format!("volt_end{}", self.tmp);
                self.line(d, &format!("for (int64_t v_{var} = {start}, {stop} = {end}; v_{var} < {stop}; v_{var}++) {{"));
                self.block(body, d + 1);
                self.line(d, "}");
            }
            Stmt::Return { value: Some(v), .. } => {
                let value = self.expr(v);
                self.line(d, &format!("return {value};"));
            }
            Stmt::Return { value: None, .. } => self.line(d, "return;"),
            Stmt::Expr(e) => {
                let call = self.expr(e);
                self.line(d, &format!("{call};"));
            }
        }
    }

    /// Volt wertet Operanden von links nach rechts aus, C legt die Reihenfolge nicht fest.
    /// Enthält mehr als ein Operand einen Aufruf, landen deshalb alle der Reihe nach in Hilfsvariablen
    /// (GNU-Statement-Expression `({ … })`, die tcc, gcc und clang verstehen).
    fn seq(&mut self, operands: &[&Expr], build: impl FnOnce(&[String]) -> String) -> String {
        let codes: Vec<String> = operands.iter().map(|e| self.expr(e)).collect();
        if operands.iter().filter(|e| has_call(e)).count() < 2 {
            return build(&codes);
        }
        let mut out = String::from("({ ");
        let mut names = Vec::new();
        for (e, code) in operands.iter().zip(&codes) {
            self.tmp += 1;
            let name = format!("volt_t{}", self.tmp);
            out += &format!("{} {name} = {code}; ", ctype(e.ty));
            names.push(name);
        }
        out + &build(&names) + "; })"
    }

    fn expr(&mut self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(n) => format!("INT64_C({n})"),
            ExprKind::Float(x) => format!("{x:?}"),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Ident(name) => format!("v_{name}"),
            ExprKind::Str(segs) => {
                if let [StrSeg::Lit(text)] = segs.as_slice() {
                    return c_string(text);
                }
                let inserts: Vec<&Expr> = segs.iter().filter_map(|s| if let StrSeg::Expr(e) = s { Some(e) } else { None }).collect();
                self.seq(&inserts, |values| {
                    let (mut tags, mut args, mut values) = (String::new(), String::new(), values.iter());
                    for seg in segs {
                        let (tag, arg) = match seg {
                            StrSeg::Lit(text) => ('s', c_string(text)),
                            StrSeg::Expr(e) => {
                                let tag = match e.ty {
                                    Type::Int => 'i',
                                    Type::F64 => 'f',
                                    Type::Bool => 'b',
                                    _ => 's',
                                };
                                (tag, values.next().expect("ein Wert je Einfügung").clone())
                            }
                        };
                        tags.push(tag);
                        args += ", ";
                        args += &arg;
                    }
                    format!("volt_fmt(\"{tags}\"{args})")
                })
            }
            ExprKind::Unary(op, operand) => format!("({}{})", if *op == UnOp::Neg { "-" } else { "!" }, self.expr(operand)),
            // `and`/`or` werten rechts nur bei Bedarf aus – genau wie && und || in C.
            ExprKind::Binary(op @ (BinOp::And | BinOp::Or), l, r) => {
                format!("({} {} {})", self.expr(l), if *op == BinOp::And { "&&" } else { "||" }, self.expr(r))
            }
            ExprKind::Binary(op, l, r) => self.seq(&[&**l, &**r], |v| match op {
                BinOp::Div if e.ty == Type::Int => format!("volt_div({}, {}, {})", v[0], v[1], e.span.line),
                BinOp::Mod => format!("volt_mod({}, {}, {})", v[0], v[1], e.span.line),
                BinOp::Eq | BinOp::Ne if l.ty == Type::Str => format!("(strcmp({}, {}) {} 0)", v[0], v[1], op.sym()),
                _ => format!("({} {} {})", v[0], op.sym(), v[1]),
            }),
            ExprKind::Call { name, args } if name == "print" => {
                let kind = match args[0].ty {
                    Type::Int => "int",
                    Type::F64 => "f64",
                    Type::Bool => "bool",
                    _ => "str",
                };
                format!("volt_print_{kind}({})", self.expr(&args[0]))
            }
            ExprKind::Call { name, args } => self.seq(&args.iter().collect::<Vec<_>>(), |v| format!("v_{name}({})", v.join(", "))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C-Code für den Rumpf von `fn main()`, ohne Runtime davor.
    fn c(body: &str) -> String {
        let src = format!("fn f(a: int) -> int\n    return a\n\nfn main()\n{}", body.lines().map(|l| format!("    {l}\n")).collect::<String>());
        let (mut m, diags) = volt_parser::parse(volt_lexer::lex(&src).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        let diags = volt_types::check(&mut m);
        assert!(diags.is_empty(), "{diags:?}");
        emit(&m)[RUNTIME.len()..].to_string()
    }

    #[test]
    fn strings_werden_sicher_nach_c_geschrieben() {
        assert_eq!(c_string("a\"b\\c\n"), r#""a\"b\\c\012""#);
        assert_eq!(c_string("ä7"), r#""\303\2447""#);
    }

    #[test]
    fn namen_bekommen_ein_praefix() {
        let out = c("let int = 1\nprint(int)");
        assert!(out.contains("int64_t v_int = INT64_C(1);"), "{out}");
        assert!(out.contains("static void v_main(void)") && out.contains("v_main();"));
    }

    #[test]
    fn mehrere_aufrufe_werden_der_reihe_nach_ausgewertet() {
        assert!(c("print(f(1) - f(2))").contains("({ int64_t volt_t1 = v_f(INT64_C(1)); int64_t volt_t2 = v_f(INT64_C(2)); (volt_t1 - volt_t2); })"));
        // Ein einzelner Aufruf braucht keine Hilfsvariablen.
        assert!(c("print(f(1) - 2)").contains("volt_print_int((v_f(INT64_C(1)) - INT64_C(2)));"));
        // `and` darf die rechte Seite nicht vorab ausrechnen.
        assert!(c("print(f(1) > 0 and f(2) > 0)").contains("((v_f(INT64_C(1)) > INT64_C(0)) && (v_f(INT64_C(2)) > INT64_C(0)))"));
    }

    #[test]
    fn division_und_strings() {
        assert!(c("print(7 / 2)").contains("volt_div(INT64_C(7), INT64_C(2), 5)"));
        assert!(c("print(7 / 2.0)").contains("(INT64_C(7) / 2.0)"));
        assert!(c("let a = \"x\"\nprint(a == \"y\")").contains("(strcmp(v_a, \"y\") == 0)"));
        assert!(c("let n = 3\nprint(\"n = {n}!\")").contains("volt_fmt(\"sis\", \"n = \", v_n, \"!\")"));
    }
}
