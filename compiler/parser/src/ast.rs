//! Der Syntaxbaum. Die `ty`-Felder füllt erst der Typchecker aus.

use volt_lexer::Span;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Type {
    Int,
    F64,
    Bool,
    Str,
    /// Funktion ohne Rückgabewert.
    Void,
    /// Noch nicht geprüft oder fehlerhaft – löst keine Folgefehler aus.
    Unknown,
}

impl Type {
    pub fn name(self) -> &'static str {
        match self {
            Type::Int => "int",
            Type::F64 => "f64",
            Type::Bool => "bool",
            Type::Str => "str",
            Type::Void => "nichts",
            Type::Unknown => "?",
        }
    }
}

pub struct Module {
    pub fns: Vec<FnDecl>,
}

/// Ein Typname, wie er im Quelltext steht.
pub struct TypeRef {
    pub name: String,
    pub span: Span,
}

pub struct Param {
    pub name: String,
    pub span: Span,
    pub ty_ref: TypeRef,
    pub ty: Type,
}

pub struct FnDecl {
    pub name: String,
    pub span: Span,
    pub params: Vec<Param>,
    pub ret_ref: Option<TypeRef>,
    pub ret: Type,
    pub body: Vec<Stmt>,
}

pub enum Stmt {
    Let { name: String, span: Span, mutable: bool, ty_ref: Option<TypeRef>, ty: Type, init: Expr },
    /// `x = wert`; `x += 1` wird beim Parsen zu `x = x + 1`.
    Assign { name: String, span: Span, value: Expr },
    /// `else if` ist ein `If` als einzige Anweisung in `els`.
    If { cond: Expr, then: Vec<Stmt>, els: Vec<Stmt> },
    While { cond: Expr, body: Vec<Stmt> },
    For { var: String, span: Span, start: Expr, end: Expr, body: Vec<Stmt> },
    Return { span: Span, value: Option<Expr> },
    Expr(Expr),
}

pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    pub ty: Type,
}

pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(Vec<StrSeg>),
    Ident(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call { name: String, args: Vec<Expr> },
}

/// Ein Stück eines Strings: fester Text oder eine Einfügung `{ausdruck}`.
pub enum StrSeg {
    Lit(String),
    Expr(Expr),
}

#[derive(Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, PartialEq)]
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
    for f in &m.fns {
        let params: Vec<String> = f.params.iter().map(|p| format!("{}: {}", p.name, p.ty_ref.name)).collect();
        out += &format!("fn {}({})", f.name, params.join(", "));
        if let Some(ret) = &f.ret_ref {
            out += &format!(" -> {}", ret.name);
        }
        out.push('\n');
        dump_block(&mut out, &f.body, 1);
    }
    out
}

fn dump_block(out: &mut String, stmts: &[Stmt], depth: usize) {
    let pad = "  ".repeat(depth);
    for s in stmts {
        match s {
            Stmt::Let { name, mutable, ty_ref, init, .. } => {
                let kw = if *mutable { "var" } else { "let" };
                let ty = ty_ref.as_ref().map(|t| format!(": {}", t.name)).unwrap_or_default();
                *out += &format!("{pad}{kw} {name}{ty} = {}\n", dump_expr(init));
            }
            Stmt::Assign { name, value, .. } => *out += &format!("{pad}{name} = {}\n", dump_expr(value)),
            Stmt::If { cond, then, els } => {
                *out += &format!("{pad}if {}\n", dump_expr(cond));
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
                *out += &format!("{pad}for {var} in {} .. {}\n", dump_expr(start), dump_expr(end));
                dump_block(out, body, depth + 1);
            }
            Stmt::Return { value: Some(v), .. } => *out += &format!("{pad}return {}\n", dump_expr(v)),
            Stmt::Return { value: None, .. } => *out += &format!("{pad}return\n"),
            Stmt::Expr(e) => *out += &format!("{pad}{}\n", dump_expr(e)),
        }
    }
}

/// Ausdruck in Klammer-Schreibweise: `1 + 2 * 3` → `(+ 1 (* 2 3))`.
pub fn dump_expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(x) => format!("{x:?}"),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Str(segs) => match segs.as_slice() {
            [StrSeg::Lit(text)] => format!("{text:?}"),
            _ => {
                let parts: Vec<String> = segs
                    .iter()
                    .map(|s| match s {
                        StrSeg::Lit(text) => format!("{text:?}"),
                        StrSeg::Expr(e) => dump_expr(e),
                    })
                    .collect();
                format!("(fmt {})", parts.join(" "))
            }
        },
        ExprKind::Ident(name) => name.clone(),
        ExprKind::Unary(op, x) => format!("({} {})", if *op == UnOp::Neg { "-" } else { "not" }, dump_expr(x)),
        ExprKind::Binary(op, l, r) => format!("({} {} {})", op.sym(), dump_expr(l), dump_expr(r)),
        ExprKind::Call { name, args } => {
            let mut s = format!("(call {name}");
            for a in args {
                s.push(' ');
                s += &dump_expr(a);
            }
            s + ")"
        }
    }
}
