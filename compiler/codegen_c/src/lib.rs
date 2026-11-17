//! C-Backend: geprüfter AST → ein C-Programm (Runtime, Typen, Prototypen, Funktionen, `main`).
//!
//! Namen: Funktionen und Variablen bekommen `v_`, Typen `vt_`, Felder `f_` – so kollidiert nichts mit C oder der
//! Runtime (`volt_…`). Volle Namen mit Punkt (`Player.hit`, `geo.neu`) werden je Teil mit Länge davor geschrieben
//! (`v_6Player3hit`), damit `a.b_c` und `a_b.c` nie gleich heißen.
//! Werte: Structs sind C-Structs (Kopie), Enums Tagged Unions, `T?`/`T!` kleine generierte Structs,
//! List/Map Zeiger auf Runtime-Objekte (Referenzen).

use std::collections::{HashMap, HashSet};
use volt_parser::ast::*;

const RUNTIME: &str = include_str!("../../../runtime/volt_rt.h");

/// Ein einzelnes Programm ohne Module.
pub fn emit(m: &Module) -> String {
    emit_program(&[m])
}

/// Hauptprogramm (`mods[0]`, enthält `main`) samt Modulen. Erwartet einen fehlerfrei geprüften AST.
pub fn emit_program(mods: &[&Module]) -> String {
    let mut g = Gen { types: String::new(), protos: String::new(), body: String::new(), done: HashSet::new(), structs: HashMap::new(), enums: HashMap::new(), tmp: 0, mut_self: false, ret: Type::Void };
    for m in mods {
        for s in &m.structs {
            g.structs.insert(s.key.clone(), s);
        }
        for e in &m.enums {
            g.enums.insert(e.key.clone(), e);
        }
    }
    let fns: Vec<&FnDecl> = mods.iter().flat_map(|m| m.structs.iter().flat_map(|s| &s.methods).chain(m.enums.iter().flat_map(|e| &e.methods)).chain(&m.fns)).collect();
    for f in &fns {
        let sig = g.signature(f);
        g.protos += &format!("{sig};\n");
    }
    for f in &fns {
        g.function(f);
    }
    let main_ret = mods[0].fns.iter().find(|f| f.name == "main").map(|f| f.ret.clone()).unwrap_or(Type::Void);
    let main = if is_res_void(&main_ret) {
        "\nint main(void) {\n    volt_res_void r = v_main();\n    if (!r.ok) {\n        fflush(stdout);\n        fprintf(stderr, \"error: %s\\n\", r.err);\n        return 1;\n    }\n    return 0;\n}\n"
    } else {
        "\nint main(void) {\n    v_main();\n    return 0;\n}\n"
    };
    format!("{RUNTIME}\n{}\n{}{}{main}", g.types, g.protos, g.body)
}

fn is_res_void(t: &Type) -> bool {
    matches!(t, Type::Res(inner) if **inner == Type::Void)
}

/// `main` → `main`, `Player.hit` → `6Player3hit`
fn mangle(key: &str) -> String {
    if !key.contains('.') {
        return key.to_string();
    }
    key.split('.').map(|s| format!("{}{s}", s.len())).collect()
}

fn fname(key: &str) -> String {
    format!("v_{}", mangle(key))
}

fn tname(key: &str) -> String {
    format!("vt_{}", mangle(key))
}

/// Kurzname eines Typs für generierte Typnamen (`volt_opt_int`, `volt_res_SPlayer`).
fn tm(t: &Type) -> String {
    match t {
        Type::Int => "int".into(),
        Type::F64 => "f64".into(),
        Type::Bool => "bool".into(),
        Type::Str => "str".into(),
        Type::List(_) => "L".into(),
        Type::Map(..) => "M".into(),
        Type::Struct(k) | Type::Enum(k) => format!("S{}", mangle(k)),
        Type::Opt(t) => format!("O{}", tm(t)),
        Type::Res(t) => format!("R{}", tm(t)),
        _ => "void".into(),
    }
}

/// String-Literal für C. Alles außer druckbarem ASCII wird oktal geschrieben (UTF-8 bleibt Byte für Byte erhalten).
fn c_string(text: &str) -> String {
    let mut out = String::from("\"");
    for b in text.bytes() {
        match b {
            b'"' => out += "\\\"",
            b'\\' => out += "\\\\",
            // `??x` wären in C Trigraphen
            b'?' => out += "\\?",
            0x20..=0x7e => out.push(b as char),
            _ => out += &format!("\\{b:03o}"),
        }
    }
    out + "\""
}

/// Kann die Auswertung Nebenwirkungen haben (Aufruf, `?`, Index mit Abbruch, Listenbau)?
fn has_call(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Invoke { .. } | ExprKind::Try(_) | ExprKind::Index(..) | ExprKind::List(_) | ExprKind::Call { .. } => true,
        ExprKind::Unary(_, x) | ExprKind::Field(x, _) | ExprKind::Wrap(x) => has_call(x),
        ExprKind::Binary(_, l, r) => has_call(l) || has_call(r),
        ExprKind::Variant { args, .. } => args.iter().any(has_call),
        ExprKind::Str(segs) => segs.iter().any(|s| matches!(s, StrSeg::Expr(e) if has_call(e))),
        _ => false,
    }
}

/// Map-Schlüssel als `uint64_t` für die Runtime.
fn map_key(code: &str, key_ty: &Type) -> String {
    if *key_ty == Type::Str { format!("(uint64_t)(uintptr_t)({code})") } else { format!("(uint64_t)({code})") }
}

fn elem(t: &Type) -> &Type {
    match t {
        Type::List(e) | Type::Opt(e) | Type::Res(e) => e,
        _ => t,
    }
}

/// Ein Operand für `seq`: C-Code, C-Typ und ob die Auswertung Nebenwirkungen haben kann.
struct Op {
    code: String,
    cty: String,
    call: bool,
}

struct Gen<'a> {
    types: String,
    protos: String,
    body: String,
    /// Schon geschriebene Typdefinitionen.
    done: HashSet<String>,
    structs: HashMap<String, &'a StructDecl>,
    enums: HashMap<String, &'a EnumDecl>,
    tmp: u32,
    /// Die aktuelle Methode hat `mut self` (dann ist `self` ein Zeiger).
    mut_self: bool,
    ret: Type,
}

impl<'a> Gen<'a> {
    fn fresh(&mut self, what: &str) -> String {
        self.tmp += 1;
        format!("volt_{what}{}", self.tmp)
    }

    /// C-Typ; schreibt dabei fehlende Typdefinitionen (vor allem, was sie brauchen).
    fn ctype(&mut self, t: &Type) -> String {
        match t {
            Type::Int => "int64_t".into(),
            Type::F64 => "double".into(),
            Type::Bool => "bool".into(),
            Type::Str | Type::Error => "volt_str".into(),
            Type::List(_) => "volt_list*".into(),
            Type::Map(..) => "volt_map*".into(),
            Type::Struct(k) => {
                self.define_struct(k);
                tname(k)
            }
            Type::Enum(k) => {
                self.define_enum(k);
                tname(k)
            }
            Type::Opt(inner) => {
                let name = format!("volt_opt_{}", tm(inner));
                if self.done.insert(name.clone()) {
                    let it = self.ctype(inner);
                    self.types += &format!("typedef struct {{ bool ok; {it} val; }} {name};\n");
                }
                name
            }
            Type::Res(inner) => {
                let name = format!("volt_res_{}", tm(inner));
                if self.done.insert(name.clone()) {
                    if **inner == Type::Void {
                        self.types += &format!("typedef struct {{ bool ok; volt_str err; }} {name};\n");
                    } else {
                        let it = self.ctype(inner);
                        self.types += &format!("typedef struct {{ bool ok; {it} val; volt_str err; }} {name};\n");
                    }
                }
                name
            }
            Type::Void | Type::Unknown | Type::None => "void".into(),
        }
    }

    fn fields_c(&mut self, fields: &[Field]) -> String {
        fields.iter().map(|f| format!("{} f_{};", self.ctype(&f.ty), f.name)).collect::<Vec<_>>().join(" ")
    }

    fn define_struct(&mut self, key: &str) {
        let name = tname(key);
        if !self.done.insert(name.clone()) {
            return;
        }
        let s = self.structs[key];
        let mut fields = self.fields_c(&s.fields);
        if fields.is_empty() {
            fields = "char unused_;".into(); // leere Structs gibt es in C nicht
        }
        self.types += &format!("typedef struct {name} {{ {fields} }} {name};\n");
    }

    fn define_enum(&mut self, key: &str) {
        let name = tname(key);
        if !self.done.insert(name.clone()) {
            return;
        }
        let e = self.enums[key];
        let mut cases = String::new();
        for (i, v) in e.variants.iter().enumerate() {
            if !v.fields.is_empty() {
                let f = self.fields_c(&v.fields);
                cases += &format!(" struct {{ {f} }} v{i};");
            }
        }
        let union = if cases.is_empty() { String::new() } else { format!(" union {{{cases} }} as;") };
        self.types += &format!("typedef struct {name} {{ int32_t tag;{union} }} {name};\n");
    }

    fn owner_type(&self, method_key: &str) -> Type {
        let owner = method_key.rsplit_once('.').map(|(o, _)| o).unwrap_or(method_key).to_string();
        if self.structs.contains_key(&owner) { Type::Struct(owner) } else { Type::Enum(owner) }
    }

    fn signature(&mut self, f: &FnDecl) -> String {
        let mut params = Vec::new();
        if let Some(this) = f.this {
            let t = self.owner_type(&f.key);
            let ct = self.ctype(&t);
            params.push(if this == SelfKind::Mut { format!("{ct}* v_self") } else { format!("{ct} v_self") });
        }
        for p in &f.params {
            params.push(format!("{} v_{}", self.ctype(&p.ty), p.name));
        }
        let params = if params.is_empty() { "void".to_string() } else { params.join(", ") };
        format!("static {} {}({params})", self.ctype(&f.ret), fname(&f.key))
    }

    fn function(&mut self, f: &FnDecl) {
        self.mut_self = f.this == Some(SelfKind::Mut);
        self.ret = f.ret.clone();
        let sig = self.signature(f);
        self.body += &format!("\n{sig} {{\n");
        self.block(&f.body, 1);
        if is_res_void(&f.ret) {
            let r = self.ctype(&f.ret);
            self.line(1, &format!("return ({r}){{true}};"));
        }
        self.body += "}\n";
    }

    fn line(&mut self, depth: usize, text: &str) {
        self.body += &"    ".repeat(depth);
        self.body += text;
        self.body.push('\n');
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
                if name == "_" {
                    self.line(d, &format!("(void)({value});"));
                } else {
                    let ct = self.ctype(ty);
                    self.line(d, &format!("{ct} v_{name} = {value};"));
                }
            }
            Stmt::LetElse { name, init, err, els, .. } => {
                let ot = self.ctype(&init.ty);
                let value = self.expr(init);
                let t = self.fresh("t");
                self.line(d, &format!("{ot} {t} = {value};"));
                self.line(d, &format!("if (!{t}.ok) {{"));
                if let (Some((e, _)), Type::Res(_)) = (err, &init.ty) {
                    self.line(d + 1, &format!("volt_str v_{e} = {t}.err;"));
                }
                self.block(els, d + 1);
                self.line(d, "}");
                let inner = elem(&init.ty).clone();
                if name != "_" && inner != Type::Void {
                    let it = self.ctype(&inner);
                    self.line(d, &format!("{it} v_{name} = {t}.val;"));
                }
            }
            Stmt::Assign { target, op, value } => self.assign(target, *op, value, d),
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
            Stmt::IfLet { name, value, then, els, .. } => {
                let ot = self.ctype(&value.ty);
                let v = self.expr(value);
                let t = self.fresh("t");
                self.line(d, "{");
                self.line(d + 1, &format!("{ot} {t} = {v};"));
                self.line(d + 1, &format!("if ({t}.ok) {{"));
                if name != "_" {
                    let it = self.ctype(elem(&value.ty));
                    self.line(d + 2, &format!("{it} v_{name} = {t}.val;"));
                }
                self.block(then, d + 2);
                if !els.is_empty() {
                    self.line(d + 1, "} else {");
                    self.block(els, d + 2);
                }
                self.line(d + 1, "}");
                self.line(d, "}");
            }
            Stmt::While { cond, body } => {
                let cond = self.expr(cond);
                self.line(d, &format!("while ({cond}) {{"));
                self.block(body, d + 1);
                self.line(d, "}");
            }
            Stmt::For { var, start, end: Some(end), body, .. } => {
                // Das Ende wird einmal vor der Schleife berechnet, nach dem Anfang.
                let (start, end) = (self.expr(start), self.expr(end));
                let stop = self.fresh("end");
                self.line(d, &format!("for (int64_t v_{var} = {start}, {stop} = {end}; v_{var} < {stop}; v_{var}++) {{"));
                self.block(body, d + 1);
                self.line(d, "}");
            }
            Stmt::For { var, start, end: None, body, .. } => {
                let et = self.ctype(elem(&start.ty));
                let list = self.expr(start);
                let (l, i) = (self.fresh("l"), self.fresh("i"));
                self.line(d, "{");
                self.line(d + 1, &format!("volt_list* {l} = {list};"));
                self.line(d + 1, &format!("for (int64_t {i} = 0; {i} < {l}->len; {i}++) {{"));
                self.line(d + 2, &format!("{et} v_{var} = (({et}*){l}->data)[{i}];"));
                self.block(body, d + 2);
                self.line(d + 1, "}");
                self.line(d, "}");
            }
            Stmt::Match { value, arms } => self.match_stmt(value, arms, d),
            Stmt::Return { value: Some(v), .. } => {
                let value = self.expr(v);
                self.line(d, &format!("return {value};"));
            }
            Stmt::Return { value: None, .. } => {
                if is_res_void(&self.ret) {
                    let r = self.ctype(&self.ret.clone());
                    self.line(d, &format!("return ({r}){{true}};"));
                } else {
                    self.line(d, "return;");
                }
            }
            Stmt::Break(_) => self.line(d, "break;"),
            Stmt::Continue(_) => self.line(d, "continue;"),
            Stmt::Expr(e) => {
                let code = self.expr(e);
                self.line(d, &format!("{code};"));
            }
        }
    }

    fn assign(&mut self, target: &Expr, op: Option<BinOp>, value: &Expr, d: usize) {
        let tt = self.ctype(&target.ty);
        // Neuer Map-Eintrag: `m[k] = v` legt an (Lesen mit `m[k]` bricht bei fehlendem Schlüssel ab).
        if let (None, ExprKind::Index(m, k)) = (op, &target.kind) {
            if let Type::Map(kt, _) = &m.ty {
                let kt = (**kt).clone();
                let v = self.expr(value);
                let t = self.fresh("t");
                let (mc, kc) = (self.expr(m), self.expr(k));
                let key = map_key(&kc, &kt);
                self.line(d, &format!("{{ {tt} {t} = {v}; *({tt}*)volt_map_slot({mc}, {key}) = {t}; }}"));
                return;
            }
        }
        match op {
            None => {
                let (tc, vc) = (self.expr(target), self.expr(value));
                if has_call(target) && has_call(value) {
                    let t = self.fresh("t");
                    self.line(d, &format!("{{ {tt} {t} = {vc}; {tc} = {t}; }}"));
                } else {
                    self.line(d, &format!("{tc} = {vc};"));
                }
            }
            Some(op) => {
                let vc = self.expr(value);
                let int_op = target.ty == Type::Int && value.ty == Type::Int;
                let combine = |a: &str| match op {
                    BinOp::Div if int_op => format!("volt_div({a}, {vc}, {})", value.span.line),
                    BinOp::Mod => format!("volt_mod({a}, {vc}, {})", value.span.line),
                    _ => format!("({a} {} {vc})", op.sym()),
                };
                if let ExprKind::Ident(_) = target.kind {
                    let tc = self.expr(target);
                    let rhs = combine(&tc);
                    self.line(d, &format!("{tc} = {rhs};"));
                } else {
                    let tc = self.expr(target);
                    let p = self.fresh("p");
                    let rhs = combine(&format!("*{p}"));
                    self.line(d, &format!("{{ {tt}* {p} = &({tc}); *{p} = {rhs}; }}"));
                }
            }
        }
    }

    fn match_stmt(&mut self, value: &Expr, arms: &[Arm], d: usize) {
        let vt = self.ctype(&value.ty);
        let v = self.expr(value);
        let t = self.fresh("m");
        self.line(d, "{");
        self.line(d + 1, &format!("{vt} {t} = {v};"));
        for (i, arm) in arms.iter().enumerate() {
            let cond = match &arm.pat.kind {
                PatKind::Wild => "1".to_string(),
                PatKind::Int(n) => format!("{t} == INT64_C({n})"),
                PatKind::Str(s) => format!("strcmp({t}, {}) == 0", c_string(s)),
                PatKind::Bool(true) => t.clone(),
                PatKind::Bool(false) => format!("!{t}"),
                PatKind::Variant { index, .. } => format!("{t}.tag == {index}"),
            };
            let kw = if i == 0 { "if" } else { "} else if" };
            self.line(d + 1, &format!("{kw} ({cond}) {{"));
            if let (PatKind::Variant { binds, index, .. }, Type::Enum(key)) = (&arm.pat.kind, &value.ty) {
                let fields: Vec<(String, Type)> = self.enums[key.as_str()].variants[*index].fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
                for ((b, _), (fname, fty)) in binds.iter().zip(fields) {
                    if b != "_" {
                        let ft = self.ctype(&fty);
                        self.line(d + 2, &format!("{ft} v_{b} = {t}.as.v{index}.f_{fname};"));
                    }
                }
            }
            self.block(&arm.body, d + 2);
        }
        if !arms.is_empty() {
            self.line(d + 1, "}");
        }
        self.line(d, "}");
    }

    fn op(&mut self, e: &Expr) -> Op {
        let code = self.expr(e);
        Op { code, cty: self.ctype(&e.ty), call: has_call(e) }
    }

    /// Volt wertet Operanden von links nach rechts aus, C legt die Reihenfolge nicht fest.
    /// Haben mehrere Operanden Nebenwirkungen, landen alle der Reihe nach in Hilfsvariablen
    /// (GNU-Statement-Expression `({ … })`, die tcc, gcc und clang verstehen).
    fn seq(&mut self, ops: Vec<Op>, build: impl FnOnce(&[String]) -> String) -> String {
        if ops.iter().filter(|o| o.call).count() < 2 {
            return build(&ops.into_iter().map(|o| o.code).collect::<Vec<_>>());
        }
        let mut out = String::from("({ ");
        let mut names = Vec::new();
        for o in ops {
            let name = self.fresh("t");
            out += &format!("{} {name} = {}; ", o.cty, o.code);
            names.push(name);
        }
        out + &build(&names) + "; })"
    }

    fn ops(&mut self, args: &[Expr]) -> Vec<Op> {
        args.iter().map(|a| self.op(a)).collect()
    }

    fn expr(&mut self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(n) => format!("INT64_C({n})"),
            ExprKind::Float(x) => format!("{x:?}"),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::None => "0".into(),
            ExprKind::Ident(name) if name == "self" && self.mut_self => "(*v_self)".into(),
            ExprKind::Ident(name) => format!("v_{name}"),
            ExprKind::Str(segs) => {
                if let [StrSeg::Lit(text)] = segs.as_slice() {
                    return c_string(text);
                }
                let inserts: Vec<&Expr> = segs.iter().filter_map(|s| if let StrSeg::Expr(e) = s { Some(e) } else { None }).collect();
                let ops = inserts.iter().map(|e| self.op(e)).collect();
                self.seq(ops, |values| {
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
            ExprKind::Binary(op, l, r) => {
                let ops = vec![self.op(l), self.op(r)];
                let (lt, line, ty) = (l.ty.clone(), e.span.line, e.ty.clone());
                self.seq(ops, |v| match op {
                    BinOp::Div if ty == Type::Int => format!("volt_div({}, {}, {line})", v[0], v[1]),
                    BinOp::Mod => format!("volt_mod({}, {}, {line})", v[0], v[1]),
                    BinOp::Eq | BinOp::Ne if lt == Type::Str => format!("(strcmp({}, {}) {} 0)", v[0], v[1], op.sym()),
                    BinOp::Eq | BinOp::Ne if matches!(lt, Type::Enum(_)) => format!("(({}).tag {} ({}).tag)", v[0], op.sym(), v[1]),
                    _ => format!("({} {} {})", v[0], op.sym(), v[1]),
                })
            }
            ExprKind::List(items) => {
                let et = self.ctype(elem(&e.ty));
                let l = self.fresh("l");
                let mut out = format!("({{ volt_list* {l} = volt_list_new(); ");
                for item in items {
                    let v = self.expr(item);
                    out += &format!("*({et}*)volt_list_push({l}, sizeof({et})) = {v}; ");
                }
                out + &format!("{l}; }})")
            }
            ExprKind::NewColl(_) => match &e.ty {
                Type::Map(k, v) => {
                    let vt = self.ctype(v);
                    format!("volt_map_new({}, sizeof({vt}))", **k == Type::Str)
                }
                _ => "volt_list_new()".into(),
            },
            ExprKind::Field(x, name) => format!("({}).f_{name}", self.expr(x)),
            ExprKind::Index(x, i) => {
                let line = e.span.line;
                let ops = vec![self.op(x), self.op(i)];
                match &x.ty {
                    Type::Map(kt, _) => {
                        let (kt, vt) = ((**kt).clone(), self.ctype(&e.ty));
                        let ptr = self.seq(ops, |v| format!("volt_map_at({}, {}, {line})", v[0], map_key(&v[1], &kt)));
                        format!("(*({vt}*){ptr})")
                    }
                    _ => {
                        let et = self.ctype(&e.ty);
                        let ptr = self.seq(ops, |v| format!("volt_at({}, {}, sizeof({et}), {line})", v[0], v[1]));
                        format!("(*({et}*){ptr})")
                    }
                }
            }
            ExprKind::Try(x) => {
                let xt = self.ctype(&x.ty);
                let rt = self.ctype(&self.ret.clone());
                let code = self.expr(x);
                let t = self.fresh("t");
                let fail = if matches!(x.ty, Type::Res(_)) { format!("({rt}){{.ok = false, .err = {t}.err}}") } else { format!("({rt}){{0}}") };
                let val = if *elem(&x.ty) == Type::Void { String::new() } else { format!(" {t}.val;") };
                format!("({{ {xt} {t} = {code}; if (!{t}.ok) return {fail};{val} }})")
            }
            ExprKind::Wrap(x) => {
                let wt = self.ctype(&e.ty);
                match (&e.ty, &x.ty) {
                    (Type::Opt(_), Type::None) => format!("(({wt}){{0}})"),
                    (Type::Res(_), Type::Error) => format!("(({wt}){{.ok = false, .err = {}}})", self.expr(x)),
                    (Type::Res(_), _) => format!("(({wt}){{.ok = true, .val = {}}})", self.expr(x)),
                    _ => format!("(({wt}){{true, {}}})", self.expr(x)),
                }
            }
            ExprKind::Variant { key, index, args } => {
                let et = self.ctype(&Type::Enum(key.clone()));
                if args.is_empty() {
                    return format!("(({et}){{.tag = {index}}})");
                }
                let ops = self.ops(args);
                let index = *index;
                self.seq(ops, |v| format!("(({et}){{.tag = {index}, .as.v{index} = {{{}}}}})", v.join(", ")))
            }
            ExprKind::Invoke { target, args } => self.invoke(e, target, args),
            ExprKind::Call { .. } => unreachable!("der Typchecker löst alle Aufrufe auf"),
        }
    }

    fn invoke(&mut self, e: &Expr, target: &Target, args: &[Expr]) -> String {
        let line = e.span.line;
        match target {
            Target::Fn(key) => {
                let ops = self.ops(args);
                let f = fname(key);
                self.seq(ops, |v| format!("{f}({})", v.join(", ")))
            }
            Target::Method { key, this } => {
                let mut ops = Vec::new();
                if *this == SelfKind::Mut {
                    let recv = &args[0];
                    let code = self.expr(recv);
                    let cty = self.ctype(&recv.ty);
                    ops.push(Op { code: format!("&({code})"), cty: format!("{cty}*"), call: has_call(recv) });
                } else {
                    ops.push(self.op(&args[0]));
                }
                ops.extend(self.ops(&args[1..]));
                let f = fname(key);
                self.seq(ops, |v| format!("{f}({})", v.join(", ")))
            }
            Target::New(key) => {
                let st = self.ctype(&Type::Struct(key.clone()));
                if args.is_empty() {
                    return format!("(({st}){{0}})");
                }
                let ops = self.ops(args);
                self.seq(ops, |v| format!("(({st}){{{}}})", v.join(", ")))
            }
            Target::Builtin(name) => self.builtin(e, name, args),
            Target::BuiltinMethod(id) => self.builtin_method(e, id, args, line),
        }
    }

    fn builtin(&mut self, e: &Expr, name: &str, args: &[Expr]) -> String {
        match name {
            "print" => {
                let kind = match args[0].ty {
                    Type::Int => "int",
                    Type::F64 => "f64",
                    Type::Bool => "bool",
                    _ => "str",
                };
                format!("volt_print_{kind}({})", self.expr(&args[0]))
            }
            "input" => {
                let ot = self.ctype(&e.ty);
                let prompt = if args.is_empty() { "\"\"".to_string() } else { self.expr(&args[0]) };
                let o = self.fresh("o");
                format!("({{ {ot} {o} = {{0}}; {o}.ok = volt_input({prompt}, &{o}.val); {o}; }})")
            }
            "error" => self.expr(&args[0]),
            "int" => format!("((int64_t)({}))", self.expr(&args[0])),
            "float" => format!("((double)({}))", self.expr(&args[0])),
            "sqrt" => format!("sqrt({})", self.expr(&args[0])),
            "exit" => format!("exit((int)({}))", self.expr(&args[0])),
            _ /* abs */ => {
                let v = self.expr(&args[0]);
                if e.ty == Type::F64 {
                    format!("fabs({v})")
                } else {
                    let t = self.fresh("t");
                    format!("({{ int64_t {t} = {v}; {t} < 0 ? -{t} : {t}; }})")
                }
            }
        }
    }

    fn builtin_method(&mut self, e: &Expr, id: &str, args: &[Expr], line: u32) -> String {
        let recv_ty = args[0].ty.clone();
        // `.or(d)`: der Ersatzwert wird nur ausgerechnet, wenn er gebraucht wird.
        if id == "opt.or" {
            let ot = self.ctype(&recv_ty);
            let (r, dflt) = (self.expr(&args[0]), self.expr(&args[1]));
            let t = self.fresh("t");
            return format!("({{ {ot} {t} = {r}; {t}.ok ? {t}.val : ({dflt}); }})");
        }
        let et = match &recv_ty {
            Type::List(t) => self.ctype(t),
            Type::Map(_, v) => self.ctype(v),
            _ => String::new(),
        };
        let kt = if let Type::Map(k, _) = &recv_ty { (**k).clone() } else { Type::Int };
        let rt = self.ctype(&e.ty);
        let (t1, t2, t3) = (self.fresh("t"), self.fresh("t"), self.fresh("t"));
        let ops = self.ops(args);
        let elem_eq = |a: &str, b: &str| if *elem(&recv_ty) == Type::Str { format!("strcmp({a}, {b}) == 0") } else { format!("{a} == {b}") };
        self.seq(ops, |v| match id {
            "str.len" => format!("((int64_t)strlen({}))", v[0]),
            "str.upper" => format!("volt_str_upper({})", v[0]),
            "str.lower" => format!("volt_str_lower({})", v[0]),
            "str.trim" => format!("volt_str_trim({})", v[0]),
            "str.contains" => format!("(strstr({}, {}) != NULL)", v[0], v[1]),
            "str.starts_with" => format!("volt_str_starts({}, {})", v[0], v[1]),
            "str.ends_with" => format!("volt_str_ends({}, {})", v[0], v[1]),
            "str.split" => format!("volt_str_split({}, {})", v[0], v[1]),
            "str.replace" => format!("volt_str_replace({}, {}, {})", v[0], v[1], v[2]),
            "str.to_int" => format!("({{ {rt} {t1} = {{0}}; {t1}.ok = volt_str_to_int({}, &{t1}.val); {t1}; }})", v[0]),
            "str.to_f64" => format!("({{ {rt} {t1} = {{0}}; {t1}.ok = volt_str_to_f64({}, &{t1}.val); {t1}; }})", v[0]),
            "list.len" => format!("(({})->len)", v[0]),
            // Wert zuerst ausrechnen: er darf die Liste noch lesen (`xs.push(xs.len())`), bevor sie wächst.
            "list.push" => format!("({{ volt_list* {t1} = {}; {et} {t2} = {}; *({et}*)volt_list_push({t1}, sizeof({et})) = {t2}; }})", v[0], v[1]),
            "list.pop" => format!(
                "({{ volt_list* {t1} = {}; {rt} {t2} = {{0}}; if ({t1}->len) {{ {t2}.ok = true; {t2}.val = (({et}*){t1}->data)[{t1}->len - 1]; {t1}->len--; }} {t2}; }})",
                v[0]
            ),
            "list.remove" => format!("({{ {et} {t1}; volt_list_remove({}, {}, sizeof({et}), {line}, &{t1}); {t1}; }})", v[0], v[1]),
            "list.insert" => format!("({{ {et} {t1} = {}; volt_list_insert({}, {}, sizeof({et}), {line}, &{t1}); }})", v[2], v[0], v[1]),
            "list.clear" => format!("((void)(({})->len = 0))", v[0]),
            "list.contains" => format!(
                "({{ volt_list* {t1} = {}; {et} {t2} = {}; bool {t3} = false; for (int64_t i = 0; i < {t1}->len; i++) if ({}) {{ {t3} = true; break; }} {t3}; }})",
                v[0],
                v[1],
                elem_eq(&format!("(({et}*){t1}->data)[i]"), &t2)
            ),
            "list.join" => format!("volt_list_join({}, {})", v[0], v[1]),
            "map.len" => format!("volt_map_len({})", v[0]),
            "map.has" => format!("(volt_map_find({}, {}) != NULL)", v[0], map_key(&v[1], &kt)),
            "map.get" => format!(
                "({{ {et}* {t1} = volt_map_find({}, {}); {rt} {t2} = {{0}}; if ({t1}) {{ {t2}.ok = true; {t2}.val = *{t1}; }} {t2}; }})",
                v[0],
                map_key(&v[1], &kt)
            ),
            "map.remove" => format!("volt_map_remove({}, {})", v[0], map_key(&v[1], &kt)),
            "map.keys" => format!("volt_map_keys({})", v[0]),
            other => unreachable!("unbekannte eingebaute Methode {other}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(src: &str) -> Module {
        let (mut m, diags) = volt_parser::parse(volt_lexer::lex(src).unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        let diags = volt_types::check(&mut m);
        assert!(diags.is_empty(), "{diags:?}");
        m
    }

    /// C-Code für den Rumpf von `fn main()`, ohne Runtime davor.
    fn c(body: &str) -> String {
        let src = format!("fn f(a: int) -> int\n    return a\n\nfn main()\n{}", body.lines().map(|l| format!("    {l}\n")).collect::<String>());
        emit(&checked(&src))[RUNTIME.len()..].to_string()
    }

    #[test]
    fn strings_werden_sicher_nach_c_geschrieben() {
        assert_eq!(c_string("a\"b\\c\n"), r#""a\"b\\c\012""#);
        assert_eq!(c_string("ä7"), r#""\303\2447""#);
        assert_eq!(c_string("??="), r#""\?\?=""#);
    }

    #[test]
    fn namen_bekommen_ein_praefix() {
        let out = c("let double = 1\nprint(double)");
        assert!(out.contains("int64_t v_double = INT64_C(1);"), "{out}");
        assert!(out.contains("static void v_main(void)") && out.contains("v_main();"));
        assert_eq!(fname("Player.hit"), "v_6Player3hit");
        assert_ne!(mangle("a.b_c"), mangle("a_b.c"));
    }

    #[test]
    fn mehrere_aufrufe_werden_der_reihe_nach_ausgewertet() {
        let out = c("print(f(1) - f(2))");
        assert!(out.contains("int64_t volt_t1 = v_f(INT64_C(1)); int64_t volt_t2 = v_f(INT64_C(2)); (volt_t1 - volt_t2);"), "{out}");
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

    #[test]
    fn typen_und_aufloesung() {
        let src = "\
struct P
    x: int
    fn bump(mut self)
        self.x += 1

enum S
    A(n: int)
    B

fn half(n: int) -> int?
    if n % 2 == 1
        return none
    return n / 2

fn main()
    var p = P(1)
    p.bump()
    let s = S.A(2)
    match s
        A(n) => print(n)
        B => print(0)
    let h = half(4)?
";
        // `?` in main (gibt nichts zurück) wäre ein Typfehler – nur bis vor die letzte Zeile prüfen.
        let src = src.lines().take_while(|l| !l.contains("half(4)?")).map(|l| format!("{l}\n")).collect::<String>();
        let out = emit(&checked(&src))[RUNTIME.len()..].to_string();
        assert!(out.contains("typedef struct vt_P { int64_t f_x; } vt_P;"), "{out}");
        assert!(out.contains("typedef struct vt_S { int32_t tag; union { struct { int64_t f_n; } v0; } as; } vt_S;"), "{out}");
        assert!(out.contains("typedef struct { bool ok; int64_t val; } volt_opt_int;"), "{out}");
        assert!(out.contains("static void v_1P4bump(vt_P* v_self)") && out.contains("v_1P4bump(&(v_p))"), "{out}");
        assert!(out.contains("((vt_S){.tag = 0, .as.v0 = {INT64_C(2)}})") && out.contains(".tag == 0") && out.contains("int64_t v_n = volt_m"), "{out}");
        assert!(out.contains("return ((volt_opt_int){0});"), "{out}");
    }
}
