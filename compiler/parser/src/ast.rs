//! Der Syntaxbaum. Die `ty`-Felder füllt erst der Typchecker aus; er ersetzt dabei auch
//! `Call`, `Field` und `Ident` durch aufgelöste Formen (`Invoke`, `Variant`).

use volt_lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    F64,
    Bool,
    Str,
    /// Funktion ohne Rückgabewert.
    Void,
    /// Noch nicht geprüft oder fehlerhaft – löst keine Folgefehler aus.
    Unknown,
    /// Ein `struct`, Schlüssel wie `Player` oder `modul.Player`.
    Struct(String),
    Enum(String),
    List(Box<Type>),
    Map(Box<Type>, Box<Type>),
    /// `T?`: ein Wert oder `none`.
    Opt(Box<Type>),
    /// `T!`: ein Wert oder ein Fehler (Text). `!` allein ist `Res(Void)`.
    Res(Box<Type>),
    /// Der Typ von `none`, passt in jedes `T?`.
    None,
    /// Der Typ von `error(…)`, passt in jedes `T!`.
    Error,
}

impl Type {
    pub fn name(&self) -> String {
        match self {
            Type::Int => "int".into(),
            Type::F64 => "f64".into(),
            Type::Bool => "bool".into(),
            Type::Str => "str".into(),
            Type::Void => "nichts".into(),
            Type::Unknown => "?".into(),
            Type::Struct(k) | Type::Enum(k) => k.clone(),
            Type::List(t) => format!("List[{}]", t.name()),
            Type::Map(k, v) => format!("Map[{}, {}]", k.name(), v.name()),
            Type::Opt(t) => format!("{}?", t.name()),
            Type::Res(t) if **t == Type::Void => "!".into(),
            Type::Res(t) => format!("{}!", t.name()),
            Type::None => "none".into(),
            Type::Error => "error".into(),
        }
    }

    pub fn is_num(&self) -> bool {
        matches!(self, Type::Int | Type::F64)
    }

    /// Lässt sich direkt ausgeben und in Strings einfügen.
    pub fn is_prim(&self) -> bool {
        matches!(self, Type::Int | Type::F64 | Type::Bool | Type::Str)
    }
}

/// Eine C-Funktion aus einem Header: nur Zahlen, Bool und Text kommen durch (`int`, `double`, `char*` …).
#[derive(Debug, Clone)]
pub struct CFunc {
    pub name: String,
    pub params: Vec<Type>,
    pub ret: Type,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CConst {
    Int(i64),
    Float(f64),
}

/// `import c "math.h" as m link "m"` – Funktionen und Konstanten füllt die CLI nach dem Parsen aus dem Header.
#[derive(Debug, Clone)]
pub struct CImport {
    pub header: String,
    pub alias: String,
    pub libs: Vec<String>,
    pub span: Span,
    pub funcs: Vec<CFunc>,
    pub consts: Vec<(String, CConst)>,
}

#[derive(Default)]
pub struct Module {
    pub c_imports: Vec<CImport>,
    pub uses: Vec<(String, Span)>,
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    pub fns: Vec<FnDecl>,
}

/// Ein Typ, wie er im Quelltext steht: `int`, `modul.Player`, `List[int]`, `Map[str, int]?`, `!`.
#[derive(Debug, Clone)]
pub struct TypeRef {
    /// Leer bei `!` allein.
    pub name: String,
    pub args: Vec<TypeRef>,
    /// `?` und `!` in Schreibreihenfolge.
    pub suffix: Vec<char>,
    pub span: Span,
}

impl TypeRef {
    pub fn text(&self) -> String {
        let mut s = self.name.clone();
        if !self.args.is_empty() {
            s += &format!("[{}]", self.args.iter().map(TypeRef::text).collect::<Vec<_>>().join(", "));
        }
        s.extend(&self.suffix);
        s
    }
}

pub struct Param {
    pub name: String,
    pub span: Span,
    pub ty_ref: TypeRef,
    pub ty: Type,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SelfKind {
    /// `fn f(self)`: bekommt eine Kopie.
    Value,
    /// `fn f(mut self)`: darf die Felder ändern.
    Mut,
}

pub struct FnDecl {
    pub name: String,
    pub span: Span,
    /// Nur bei Methoden.
    pub this: Option<SelfKind>,
    pub params: Vec<Param>,
    pub ret_ref: Option<TypeRef>,
    pub ret: Type,
    pub body: Vec<Stmt>,
    /// Voller Name, vom Typchecker gesetzt: `main`, `Player.hit`, `modul.f`.
    pub key: String,
}

pub struct Field {
    pub name: String,
    pub span: Span,
    pub ty_ref: TypeRef,
    pub ty: Type,
    pub default: Option<Expr>,
}

pub struct StructDecl {
    pub name: String,
    pub span: Span,
    pub fields: Vec<Field>,
    pub methods: Vec<FnDecl>,
    pub key: String,
}

pub struct Variant {
    pub name: String,
    pub span: Span,
    pub fields: Vec<Field>,
}

pub struct EnumDecl {
    pub name: String,
    pub span: Span,
    pub variants: Vec<Variant>,
    pub methods: Vec<FnDecl>,
    pub key: String,
}

#[derive(Clone)]
pub enum Stmt {
    Let { name: String, span: Span, mutable: bool, ty_ref: Option<TypeRef>, ty: Type, init: Expr },
    /// `let x = f() else err` + Block, der nicht weiterläuft. `err` bekommt bei `T!` den Fehlertext.
    LetElse { name: String, span: Span, init: Expr, err: Option<(String, Span)>, els: Vec<Stmt> },
    /// `ziel = wert` oder `ziel += wert` (dann ist `op` gesetzt).
    Assign { target: Expr, op: Option<BinOp>, value: Expr },
    /// `else if` ist ein `If` als einzige Anweisung in `els`.
    If { cond: Expr, then: Vec<Stmt>, els: Vec<Stmt> },
    /// `if let x = optional` – der Block läuft nur, wenn ein Wert da ist.
    IfLet { name: String, span: Span, value: Expr, then: Vec<Stmt>, els: Vec<Stmt> },
    While { cond: Expr, body: Vec<Stmt> },
    /// `for i in a..b` (dann ist `end` gesetzt) oder `for x in liste`.
    For { var: String, span: Span, start: Expr, end: Option<Expr>, body: Vec<Stmt> },
    Match { value: Expr, arms: Vec<Arm> },
    Return { span: Span, value: Option<Expr> },
    Break(Span),
    Continue(Span),
    /// `defer aufruf(…)`: läuft beim Verlassen des Blocks (auch bei return/break/continue/`?`), zuletzt Angemeldetes zuerst.
    Defer(Expr),
    Expr(Expr),
}

#[derive(Clone)]
pub struct Arm {
    pub pat: Pattern,
    pub body: Vec<Stmt>,
}

#[derive(Clone)]
pub struct Pattern {
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Clone)]
pub enum PatKind {
    /// `_`
    Wild,
    Int(i64),
    Str(String),
    Bool(bool),
    /// `Circle(r)` – `index` setzt der Typchecker.
    Variant { name: String, binds: Vec<(String, Span)>, index: usize },
}

#[derive(Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    pub ty: Type,
}

#[derive(Clone)]
pub struct Arg {
    /// `name: wert` bei Konstruktoren.
    pub name: Option<(String, Span)>,
    pub value: Expr,
}

#[derive(Clone)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(Vec<StrSeg>),
    None,
    Ident(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `[a, b, c]`
    List(Vec<Expr>),
    /// `List[int]()` oder `Map[str, int]()`
    NewColl(TypeRef),
    /// Aufruf, wie er im Quelltext steht. Der Typchecker macht daraus `Invoke` oder `Variant`.
    Call { callee: Box<Expr>, args: Vec<Arg> },
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    /// `wert?`
    Try(Box<Expr>),
    // --- ab hier nur vom Typchecker erzeugt ---
    Invoke { target: Target, args: Vec<Expr> },
    /// Enum-Wert: `Shape.Circle(1.0)` oder `Dir.North`.
    Variant { key: String, index: usize, args: Vec<Expr> },
    /// Ein Wert, der in `T?` oder `T!` verpackt wird (der Zieltyp steht in `ty`).
    Wrap(Box<Expr>),
}

/// Wohin ein aufgelöster Aufruf geht.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Eigene Funktion (Schlüssel).
    Fn(String),
    /// Methode: `args[0]` ist der Empfänger.
    Method { key: String, this: SelfKind },
    /// Konstruktor eines Structs, Argumente in Feld-Reihenfolge.
    New(String),
    /// Eingebaute Funktion wie `print`, `input`, `error`.
    Builtin(&'static str),
    /// Eingebaute Methode auf str/List/Map/Optional, `args[0]` ist der Empfänger.
    BuiltinMethod(&'static str),
}

/// Ein Stück eines Strings: fester Text oder eine Einfügung `{ausdruck}`.
#[derive(Clone)]
pub enum StrSeg {
    Lit(String),
    Expr(Expr),
}

#[derive(Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    /// Der Operator, wie man ihn in Volt schreibt.
    pub fn sym(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }
}

/// Der Baum als lesbarer Text (`volt parse datei.vt --ast`).
pub fn dump(m: &Module) -> String {
    let mut out = String::new();
    for (name, _) in &m.uses {
        out += &format!("use {name}\n");
    }
    for c in &m.c_imports {
        out += &format!("import c \"{}\" as {}{}\n", c.header, c.alias, c.libs.iter().map(|l| format!(" link \"{l}\"")).collect::<String>());
    }
    for s in &m.structs {
        out += &format!("struct {}\n", s.name);
        dump_fields(&mut out, &s.fields);
        for f in &s.methods {
            dump_fn(&mut out, f, 1);
        }
    }
    for e in &m.enums {
        out += &format!("enum {}\n", e.name);
        for v in &e.variants {
            let fields: Vec<String> = v.fields.iter().map(|f| format!("{}: {}", f.name, f.ty_ref.text())).collect();
            out += &format!("  {}({})\n", v.name, fields.join(", "));
        }
        for f in &e.methods {
            dump_fn(&mut out, f, 1);
        }
    }
    for f in &m.fns {
        dump_fn(&mut out, f, 0);
    }
    out
}

fn dump_fields(out: &mut String, fields: &[Field]) {
    for f in fields {
        let default = f.default.as_ref().map(|d| format!(" = {}", dump_expr(d))).unwrap_or_default();
        *out += &format!("  {}: {}{default}\n", f.name, f.ty_ref.text());
    }
}

fn dump_fn(out: &mut String, f: &FnDecl, depth: usize) {
    let mut params: Vec<String> = Vec::new();
    match f.this {
        Some(SelfKind::Value) => params.push("self".into()),
        Some(SelfKind::Mut) => params.push("mut self".into()),
        None => {}
    }
    params.extend(f.params.iter().map(|p| format!("{}: {}", p.name, p.ty_ref.text())));
    *out += &format!("{}fn {}({})", "  ".repeat(depth), f.name, params.join(", "));
    if let Some(ret) = &f.ret_ref {
        *out += &format!(" -> {}", ret.text());
    }
    out.push('\n');
    dump_block(out, &f.body, depth + 1);
}

fn dump_block(out: &mut String, stmts: &[Stmt], depth: usize) {
    let pad = "  ".repeat(depth);
    for s in stmts {
        match s {
            Stmt::Let { name, mutable, ty_ref, init, .. } => {
                let kw = if *mutable { "var" } else { "let" };
                let ty = ty_ref.as_ref().map(|t| format!(": {}", t.text())).unwrap_or_default();
                *out += &format!("{pad}{kw} {name}{ty} = {}\n", dump_expr(init));
            }
            Stmt::LetElse { name, init, err, els, .. } => {
                let err = err.as_ref().map(|(e, _)| format!(" {e}")).unwrap_or_default();
                *out += &format!("{pad}let {name} = {} else{err}\n", dump_expr(init));
                dump_block(out, els, depth + 1);
            }
            Stmt::Assign { target, op, value } => {
                let op = op.map(|o| o.sym()).unwrap_or("");
                *out += &format!("{pad}{} {op}= {}\n", dump_expr(target), dump_expr(value));
            }
            Stmt::If { cond, then, els } => {
                *out += &format!("{pad}if {}\n", dump_expr(cond));
                dump_block(out, then, depth + 1);
                if !els.is_empty() {
                    *out += &format!("{pad}else\n");
                    dump_block(out, els, depth + 1);
                }
            }
            Stmt::IfLet { name, value, then, els, .. } => {
                *out += &format!("{pad}if let {name} = {}\n", dump_expr(value));
                dump_block(out, then, depth + 1);
                if !els.is_empty() {
                    *out += &format!("{pad}else\n");
                    dump_block(out, els, depth + 1);
                }
            }
            Stmt::While { cond, body } => {
                *out += &format!("{pad}while {}\n", dump_expr(cond));
                dump_block(out, body, depth + 1);
            }
            Stmt::For { var, start, end, body, .. } => {
                match end {
                    Some(end) => *out += &format!("{pad}for {var} in {} .. {}\n", dump_expr(start), dump_expr(end)),
                    None => *out += &format!("{pad}for {var} in {}\n", dump_expr(start)),
                }
                dump_block(out, body, depth + 1);
            }
            Stmt::Match { value, arms } => {
                *out += &format!("{pad}match {}\n", dump_expr(value));
                for arm in arms {
                    let pat = match &arm.pat.kind {
                        PatKind::Wild => "_".to_string(),
                        PatKind::Int(n) => n.to_string(),
                        PatKind::Str(s) => format!("{s:?}"),
                        PatKind::Bool(b) => b.to_string(),
                        PatKind::Variant { name, binds, .. } if binds.is_empty() => name.clone(),
                        PatKind::Variant { name, binds, .. } => {
                            format!("{name}({})", binds.iter().map(|b| b.0.as_str()).collect::<Vec<_>>().join(", "))
                        }
                    };
                    *out += &format!("{pad}  {pat} =>\n");
                    dump_block(out, &arm.body, depth + 2);
                }
            }
            Stmt::Return { value: Some(v), .. } => *out += &format!("{pad}return {}\n", dump_expr(v)),
            Stmt::Return { value: None, .. } => *out += &format!("{pad}return\n"),
            Stmt::Break(_) => *out += &format!("{pad}break\n"),
            Stmt::Continue(_) => *out += &format!("{pad}continue\n"),
            Stmt::Defer(e) => *out += &format!("{pad}defer {}\n", dump_expr(e)),
            Stmt::Expr(e) => *out += &format!("{pad}{}\n", dump_expr(e)),
        }
    }
}

fn list(items: impl Iterator<Item = String>) -> String {
    items.map(|s| format!(" {s}")).collect()
}

/// Ausdruck in Klammer-Schreibweise: `1 + 2 * 3` → `(+ 1 (* 2 3))`.
pub fn dump_expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(x) => format!("{x:?}"),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::None => "none".into(),
        ExprKind::Str(segs) => match segs.as_slice() {
            [StrSeg::Lit(text)] => format!("{text:?}"),
            _ => {
                let parts = segs.iter().map(|s| match s {
                    StrSeg::Lit(text) => format!("{text:?}"),
                    StrSeg::Expr(e) => dump_expr(e),
                });
                format!("(fmt{})", list(parts))
            }
        },
        ExprKind::Ident(name) => name.clone(),
        ExprKind::Unary(op, x) => format!("({} {})", if *op == UnOp::Neg { "-" } else { "not" }, dump_expr(x)),
        ExprKind::Binary(op, l, r) => format!("({} {} {})", op.sym(), dump_expr(l), dump_expr(r)),
        ExprKind::List(items) => format!("(list{})", list(items.iter().map(dump_expr))),
        ExprKind::NewColl(t) => format!("(new {})", t.text()),
        ExprKind::Call { callee, args } => {
            let args = args.iter().map(|a| match &a.name {
                Some((n, _)) => format!("{n}: {}", dump_expr(&a.value)),
                None => dump_expr(&a.value),
            });
            format!("(call {}{})", dump_expr(callee), list(args))
        }
        ExprKind::Field(x, name) => format!("{}.{name}", dump_expr(x)),
        ExprKind::Index(x, i) => format!("(index {} {})", dump_expr(x), dump_expr(i)),
        ExprKind::Try(x) => format!("(try {})", dump_expr(x)),
        ExprKind::Invoke { target, args } => format!("(invoke {target:?}{})", list(args.iter().map(dump_expr))),
        ExprKind::Variant { key, index, args } => format!("(variant {key} {index}{})", list(args.iter().map(dump_expr))),
        ExprKind::Wrap(x) => format!("(wrap {})", dump_expr(x)),
    }
}
