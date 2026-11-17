//! Typprüfung: trägt die Typen in den AST ein, löst Namen auf und sammelt alle Fehler.
//! Aufrufe, Feldzugriffe und Enum-Fälle werden dabei in aufgelöste Formen umgeschrieben
//! (`ExprKind::Invoke`, `ExprKind::Variant`), Werte für `T?`/`T!` in `ExprKind::Wrap` verpackt.

use std::collections::{HashMap, HashSet};
use volt_lexer::{Diag, Span};
use volt_parser::ast::*;

/// Eine Quelldatei. Das Hauptprogramm hat den leeren Namen, Module (`use name`) ihren Dateinamen.
pub struct Unit {
    pub name: String,
    pub module: Module,
}

#[derive(Clone)]
struct Sig {
    this: Option<SelfKind>,
    params: Vec<Type>,
    ret: Type,
}

#[derive(Clone)]
struct FieldInfo {
    name: String,
    ty: Type,
    default: Option<Expr>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Struct,
    Enum,
}

#[derive(Clone, Copy, PartialEq)]
enum VarKind {
    Let,
    Var,
    Param,
    Loop,
    SelfVal,
    SelfMut,
}

#[derive(Clone)]
struct Var {
    ty: Type,
    kind: VarKind,
}

/// Eingebaute Funktionen – diese Namen sind für eigene Funktionen tabu.
const BUILTINS: &[&str] = &["print", "input", "error", "int", "float", "sqrt", "abs", "exit", "sin", "cos", "tan", "exp", "log", "floor", "ceil", "pow", "atan2", "min", "max", "assert"];
const TYPE_NAMES: &[&str] = &["int", "f64", "bool", "str", "List", "Map"];

/// Wozu ein Name wie `modul.Typ.Fall` gehört.
enum PathRes {
    Module,
    Type(String, Kind),
    Fn(String),
    Variant(String, usize),
    /// Methode über den Typnamen: `Player.new`.
    Static(String),
    Missing,
}

struct Inst {
    unit: usize,
    subst: HashMap<String, Type>,
    decl: Option<FnDecl>,
}

struct SInst {
    unit: usize,
    subst: HashMap<String, Type>,
    decl: Option<StructDecl>,
}

struct Checker {
    /// Name und `use`-Liste je Datei.
    units: Vec<(String, HashSet<String>)>,
    types: HashMap<String, Kind>,
    structs: HashMap<String, Vec<FieldInfo>>,
    variants: HashMap<String, Vec<(String, Vec<FieldInfo>)>>,
    /// Freie Funktionen (`f`, `modul.f`) und Methoden (`Typ.m`).
    fns: HashMap<String, Sig>,
    /// Konstanten aus C-Headern (`m.M_PI`), als Literal eingesetzt.
    cconsts: HashMap<String, CConst>,
    /// Traits: Name → Methoden-Köpfe (Name, Anzahl Parameter).
    traits: HashMap<String, Vec<(String, usize)>>,
    /// Generische Funktionen (Vorlagen): Schlüssel → (Datei, Deklaration).
    generics: HashMap<String, (usize, FnDecl)>,
    /// Aktuell eingesetzte Typ-Parameter beim Prüfen einer Kopie (`T` → `int`).
    subst: HashMap<String, Type>,
    /// Erzeugte Kopien generischer Funktionen, Rümpfe werden nach allen anderen geprüft.
    insts: Vec<Inst>,
    /// Generische Structs (Vorlagen) und ihre erzeugten Kopien.
    generic_structs: HashMap<String, (usize, StructDecl)>,
    sinsts: Vec<SInst>,
    /// Kopie → (Vorlage, Typ-Argumente), damit `Paar[T, T]` gegen `Paar<int,int>` abgeglichen werden kann.
    inst_args: HashMap<String, (String, Vec<Type>)>,
    unit: usize,
    /// Sichtbare Variablen, innerster Block zuletzt.
    scopes: Vec<HashMap<String, Var>>,
    /// Variablen auf oberster Ebene je Datei (stehen in jeder Funktion der Datei ganz unten im Gültigkeitsbereich).
    globals: Vec<HashMap<String, Var>>,
    /// Rückgabetyp der Funktion, die gerade geprüft wird.
    ret: Type,
    loops: u32,
    diags: Vec<(usize, Diag)>,
}

/// Prüft ein einzelnes Programm ohne Module.
pub fn check(m: &mut Module) -> Vec<Diag> {
    let mut units = vec![Unit { name: String::new(), module: std::mem::take(m) }];
    let diags = check_program(&mut units);
    *m = units.pop().expect("eine Datei").module;
    diags.into_iter().map(|(_, d)| d).collect()
}

/// Prüft das Hauptprogramm (`units[0]`) samt Modulen. Jeder Fehler nennt den Index seiner Datei.
pub fn check_program(units: &mut [Unit]) -> Vec<(usize, Diag)> {
    let mut c = Checker {
        units: units.iter().map(|u| (u.name.clone(), u.module.uses.iter().map(|(n, _)| n.clone()).collect())).collect(),
        types: HashMap::new(),
        structs: HashMap::new(),
        variants: HashMap::new(),
        fns: HashMap::new(),
        cconsts: HashMap::new(),
        traits: HashMap::new(),
        generics: HashMap::new(),
        subst: HashMap::new(),
        insts: Vec::new(),
        generic_structs: HashMap::new(),
        sinsts: Vec::new(),
        inst_args: HashMap::new(),
        unit: 0,
        scopes: Vec::new(),
        globals: vec![HashMap::new(); units.len()],
        ret: Type::Void,
        loops: 0,
        diags: Vec::new(),
    };
    let names: HashSet<String> = units.iter().map(|u| u.name.clone()).collect();
    // Traits sammeln (programmweit, Name muss eindeutig sein)
    for (ui, u) in units.iter().enumerate() {
        c.unit = ui;
        for t in &u.module.traits {
            if c.traits.insert(t.name.clone(), t.methods.iter().map(|m| (m.name.clone(), m.params.len())).collect()).is_some() {
                c.err(Diag::new(t.span, format!("das Trait `{}` gibt es schon", t.name)));
            }
        }
    }
    // 0. C-Importe: Funktionen als `alias.name`, Konstanten als `alias.NAME`; der Alias zählt wie ein Modul.
    for (ui, u) in units.iter().enumerate() {
        for ci in &u.module.c_imports {
            c.units[ui].1.insert(ci.alias.clone());
            for f in &ci.funcs {
                c.fns.insert(format!("{}.{}", ci.alias, f.name), Sig { this: None, params: f.params.clone(), ret: f.ret.clone() });
            }
            for (n, v) in &ci.consts {
                c.cconsts.insert(format!("{}.{n}", ci.alias), *v);
            }
        }
    }
    // 1. Typnamen, damit Felder und Signaturen sich auf alle Typen beziehen können.
    for (ui, u) in units.iter_mut().enumerate() {
        c.unit = ui;
        for (name, span) in &u.module.uses {
            if !names.contains(name) {
                c.err(Diag::new(*span, format!("das Modul `{name}` gibt es nicht")).tip(format!("lege `{name}.vt` neben diese Datei")));
            }
        }
        for s in &mut u.module.structs {
            s.key = c.qual(&s.name);
            if s.generics.is_empty() {
                c.add_type(&s.name, &s.key, Kind::Struct, s.span);
            } else {
                c.generic_structs.insert(s.key.clone(), (ui, s.clone()));
            }
        }
        for e in &mut u.module.enums {
            e.key = c.qual(&e.name);
            c.add_type(&e.name, &e.key, Kind::Enum, e.span);
        }
    }
    // 2. Felder, Fälle und Signaturen.
    for (ui, u) in units.iter_mut().enumerate() {
        c.unit = ui;
        for s in &mut u.module.structs {
            if !s.generics.is_empty() {
                continue;
            }
            let fields = c.fields(&mut s.fields, &s.name);
            c.structs.insert(s.key.clone(), fields);
            for m in &mut s.methods {
                c.signature(m, Some(&s.key));
            }
        }
        for e in &mut u.module.enums {
            let mut variants = Vec::new();
            for v in &mut e.variants {
                if variants.iter().any(|(n, _)| *n == v.name) {
                    c.err(Diag::new(v.span, format!("den Fall `{}` gibt es schon", v.name)));
                }
                let fields = c.fields(&mut v.fields, &v.name);
                variants.push((v.name.clone(), fields));
            }
            c.variants.insert(e.key.clone(), variants);
            for m in &mut e.methods {
                c.signature(m, Some(&e.key));
            }
        }
        for f in &mut u.module.fns {
            if f.generics.is_empty() {
                c.signature(f, None);
            } else {
                // Vorlage: erst beim Aufruf mit konkreten Typen prüfen (wie C++-Templates)
                f.key = c.qual(&f.name);
                c.fns.insert(f.key.clone(), Sig { this: None, params: Vec::new(), ret: Type::Unknown });
                c.generics.insert(f.key.clone(), (ui, f.clone()));
            }
        }
    }
    // 3. Ein Typ darf sich nicht selbst (direkt oder über andere) als Wert enthalten.
    for (ui, u) in units.iter().enumerate() {
        c.unit = ui;
        for (key, span) in u.module.structs.iter().filter(|s| s.generics.is_empty()).map(|s| (&s.key, s.span)).chain(u.module.enums.iter().map(|e| (&e.key, e.span))) {
            if c.contains_itself(key) {
                c.err(Diag::new(span, format!("`{key}` enthält sich selbst")).tip(format!("ein Wert wäre unendlich groß – nutze `List[{key}]`")));
            }
        }
    }
    // 4. Standardwerte der Felder.
    for (ui, u) in units.iter_mut().enumerate() {
        c.unit = ui;
        for s in &mut u.module.structs {
            if s.generics.is_empty() {
                c.defaults(&mut s.fields, &s.key, None);
            }
        }
        for e in &mut u.module.enums {
            for (i, v) in e.variants.iter_mut().enumerate() {
                c.defaults(&mut v.fields, &e.key, Some(i));
            }
        }
    }
    // 4b. Variablen auf oberster Ebene: Startwerte der Reihe nach prüfen; Namen gelten programmweit nur einmal.
    let mut global_owner: HashMap<String, usize> = HashMap::new();
    for (ui, u) in units.iter_mut().enumerate() {
        c.unit = ui;
        c.scopes = vec![HashMap::new()];
        for g in &mut u.module.globals {
            if let Stmt::Let { name, span, .. } = g {
                if global_owner.insert(name.clone(), ui).is_some_and(|other| other != ui) {
                    c.err(Diag::new(*span, format!("die Variable `{name}` gibt es schon in einer anderen Datei")).tip("globale Namen müssen im ganzen Programm eindeutig sein"));
                }
            }
            c.stmt(g);
        }
        c.globals[ui] = c.scopes.pop().unwrap_or_default();
        c.scopes.clear();
    }
    // 5. `main` im Hauptprogramm.
    c.unit = 0;
    match units[0].module.fns.iter().find(|f| f.name == "main") {
        None => c.err(Diag::new(Span { line: 1, col: 1, len: 1 }, "keine `fn main()` gefunden").tip("jedes Programm startet in `fn main()`")),
        Some(f) if !f.params.is_empty() || !matches!(&f.ret, Type::Void | Type::Unknown) && f.ret != Type::Res(Box::new(Type::Void)) => {
            c.err(Diag::new(f.span, "`main` hat keine Parameter und keinen Rückgabewert").tip("schreibe `fn main()` – oder `fn main() -> !`, wenn Fehler herauskommen dürfen"));
        }
        Some(_) => {}
    }
    // 6. Die Rümpfe.
    for (ui, u) in units.iter_mut().enumerate() {
        c.unit = ui;
        for s in &mut u.module.structs {
            if s.generics.is_empty() {
                for m in &mut s.methods {
                    c.function(m, Some(&s.key));
                }
            }
        }
        for e in &mut u.module.enums {
            for m in &mut e.methods {
                c.function(m, Some(&e.key));
            }
        }
        for f in &mut u.module.fns {
            if f.generics.is_empty() {
                c.function(f, None);
            }
        }
    }
    // Kopien generischer Funktionen und Structs (jede kann weitere auslösen)
    let (mut i, mut j) = (0, 0);
    while i < c.insts.len() || j < c.sinsts.len() {
        if i < c.insts.len() {
            let mut decl = c.insts[i].decl.take().expect("Kopie wartet auf Prüfung");
            c.unit = c.insts[i].unit;
            c.subst = c.insts[i].subst.clone();
            c.function(&mut decl, None);
            c.subst.clear();
            c.insts[i].decl = Some(decl);
            i += 1;
        } else {
            let mut decl = c.sinsts[j].decl.take().expect("Kopie wartet auf Prüfung");
            c.unit = c.sinsts[j].unit;
            c.subst = c.sinsts[j].subst.clone();
            let key = decl.key.clone();
            for m in &mut decl.methods {
                c.function(m, Some(&key));
            }
            c.subst.clear();
            c.sinsts[j].decl = Some(decl);
            j += 1;
        }
    }
    for inst in std::mem::take(&mut c.insts) {
        units[inst.unit].module.fns.push(inst.decl.expect("geprüft"));
    }
    for inst in std::mem::take(&mut c.sinsts) {
        units[inst.unit].module.structs.push(inst.decl.expect("geprüft"));
    }
    for u in units.iter_mut() {
        u.module.fns.retain(|f| f.generics.is_empty());
        u.module.structs.retain(|s| s.generics.is_empty());
    }
    // `x += 1` mit unbekanntem `x` meldet sonst denselben Fehler zweimal.
    c.diags.dedup_by(|a, b| a.0 == b.0 && a.1.span == b.1.span && a.1.msg == b.1.msg);
    c.diags
}

/// Verlässt dieser Block auf jedem Weg die Funktion (oder mit `loops` auch die Schleifenrunde)?
fn exits(stmts: &[Stmt], loops: bool) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::Return { .. } => true,
        Stmt::Break(_) | Stmt::Continue(_) => loops,
        Stmt::If { then, els, .. } | Stmt::IfLet { then, els, .. } => exits(then, loops) && exits(els, loops),
        Stmt::Match { arms, .. } => !arms.is_empty() && arms.iter().all(|a| exits(&a.body, loops)),
        Stmt::Expr(Expr { kind: ExprKind::Invoke { target: Target::Builtin("exit"), .. }, .. }) => true,
        _ => false,
    })
}

/// Kurzer Text für ein Zuweisungsziel in Meldungen: `x`, `p.hp`, `xs[…]`.
fn text(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Ident(n) => n.clone(),
        ExprKind::Field(x, n) => format!("{}.{n}", text(x)),
        ExprKind::Index(x, _) => format!("{}[…]", text(x)),
        _ => "der Wert".into(),
    }
}

fn take(e: &mut Expr) -> ExprKind {
    std::mem::replace(&mut e.kind, ExprKind::Bool(false))
}

/// Eingebaute Methoden: Name der Methode → (Kennung fürs Backend, Parameter, Rückgabe).
fn builtin_method(recv: &Type, name: &str) -> Option<(&'static str, Vec<Type>, Type)> {
    use Type::*;
    let opt = |t: &Type| Opt(Box::new(t.clone()));
    Some(match (recv, name) {
        (Str, "len") => ("str.len", vec![], Int),
        (Str, "upper") => ("str.upper", vec![], Str),
        (Str, "lower") => ("str.lower", vec![], Str),
        (Str, "trim") => ("str.trim", vec![], Str),
        (Str, "contains") => ("str.contains", vec![Str], Bool),
        (Str, "starts_with") => ("str.starts_with", vec![Str], Bool),
        (Str, "ends_with") => ("str.ends_with", vec![Str], Bool),
        (Str, "split") => ("str.split", vec![Str], List(Box::new(Str))),
        (Str, "replace") => ("str.replace", vec![Str, Str], Str),
        (Str, "to_int") => ("str.to_int", vec![], opt(&Int)),
        (Str, "to_f64") => ("str.to_f64", vec![], opt(&F64)),
        (List(_), "len") => ("list.len", vec![], Int),
        (List(t), "push") => ("list.push", vec![(**t).clone()], Void),
        (List(t), "pop") => ("list.pop", vec![], opt(t)),
        (List(t), "remove") => ("list.remove", vec![Int], (**t).clone()),
        (List(t), "insert") => ("list.insert", vec![Int, (**t).clone()], Void),
        (List(_), "clear") => ("list.clear", vec![], Void),
        (List(t), "contains") if t.is_prim() => ("list.contains", vec![(**t).clone()], Bool),
        (List(t), "join") if **t == Str => ("list.join", vec![Str], Str),
        (Map(..), "len") => ("map.len", vec![], Int),
        (Map(k, _), "has") => ("map.has", vec![(**k).clone()], Bool),
        (Map(k, v), "get") => ("map.get", vec![(**k).clone()], opt(v)),
        (Map(k, _), "remove") => ("map.remove", vec![(**k).clone()], Void),
        (Map(k, _), "keys") => ("map.keys", vec![], List(k.clone())),
        (Opt(t) | Res(t), "or") if **t != Void => ("opt.or", vec![(**t).clone()], (**t).clone()),
        _ => return Option::None,
    })
}

fn methods_of(recv: &Type) -> &'static str {
    match recv {
        Type::Str => "len, upper, lower, trim, contains, starts_with, ends_with, split, replace, to_int, to_f64",
        Type::List(_) => "len, push, pop, remove, insert, clear, contains, join",
        Type::Map(..) => "len, has, get, remove, keys",
        Type::Opt(_) | Type::Res(_) => "or",
        _ => "",
    }
}

impl Checker {
    fn err(&mut self, d: Diag) {
        self.diags.push((self.unit, d));
    }

    /// Voller Name eines Eintrags der aktuellen Datei.
    fn qual(&self, name: &str) -> String {
        let unit = &self.units[self.unit].0;
        if unit.is_empty() { name.to_string() } else { format!("{unit}.{name}") }
    }

    fn uses(&self, module: &str) -> bool {
        self.units[self.unit].1.contains(module)
    }

    fn add_type(&mut self, name: &str, key: &str, kind: Kind, span: Span) {
        if TYPE_NAMES.contains(&name) {
            self.err(Diag::new(span, format!("`{name}` ist schon eingebaut")).tip("gib deinem Typ einen anderen Namen"));
        } else if self.uses(name) {
            self.err(Diag::new(span, format!("`{name}` ist schon ein Modul")));
        } else if self.types.insert(key.to_string(), kind).is_some() {
            self.err(Diag::new(span, format!("den Typ `{name}` gibt es schon")));
        }
    }

    fn fields(&mut self, fields: &mut [Field], owner: &str) -> Vec<FieldInfo> {
        let mut out: Vec<FieldInfo> = Vec::new();
        for f in fields {
            f.ty = self.resolve(&f.ty_ref);
            if out.iter().any(|o| o.name == f.name) {
                self.err(Diag::new(f.span, format!("`{owner}` hat schon ein Feld `{}`", f.name)));
            }
            out.push(FieldInfo { name: f.name.clone(), ty: f.ty.clone(), default: None });
        }
        out
    }

    fn defaults(&mut self, fields: &mut [Field], key: &str, variant: Option<usize>) {
        for (i, f) in fields.iter_mut().enumerate() {
            let Some(d) = &mut f.default else { continue };
            self.scopes.push(HashMap::new());
            self.value(d);
            self.scopes.pop();
            let name = f.name.clone();
            self.coerce(d, &f.ty, |w, g| format!("der Standardwert von `{name}` soll {w} sein, ist aber {g}"));
            let slot = match variant {
                None => self.structs.get_mut(key).map(|fs| &mut fs[i]),
                Some(v) => self.variants.get_mut(key).map(|vs| &mut vs[v].1[i]),
            };
            if let Some(slot) = slot {
                slot.default = Some(d.clone());
            }
        }
    }

    fn signature(&mut self, f: &mut FnDecl, owner: Option<&str>) {
        for p in &mut f.params {
            p.ty = self.resolve(&p.ty_ref);
        }
        f.ret = f.ret_ref.as_ref().map_or(Type::Void, |r| self.resolve(r));
        f.key = match owner {
            Some(o) => format!("{o}.{}", f.name),
            None => self.qual(&f.name),
        };
        let sig = Sig { this: f.this, params: f.params.iter().map(|p| p.ty.clone()).collect(), ret: f.ret.clone() };
        if owner.is_none() && BUILTINS.contains(&f.name.as_str()) {
            self.err(Diag::new(f.span, format!("`{}` ist schon eingebaut", f.name)).tip("gib deiner Funktion einen anderen Namen"));
        } else if owner.is_none() && self.types.contains_key(&f.key) {
            self.err(Diag::new(f.span, format!("`{}` ist schon ein Typ", f.name)));
        } else if self.fns.insert(f.key.clone(), sig).is_some() {
            let what = if owner.is_some() { "Methode" } else { "Funktion" };
            self.err(Diag::new(f.span, format!("die {what} `{}` gibt es schon", f.name)));
        }
    }

    /// Schlüssel eines generischen Structs zu einem Namen wie `Paar` oder `modul.Paar`.
    fn generic_key(&self, name: &str) -> Option<String> {
        let key = match name.split_once('.') {
            Some((module, _)) if self.uses(module) => name.to_string(),
            Some(_) => return None,
            None => self.qual(name),
        };
        self.generic_structs.contains_key(&key).then_some(key)
    }

    /// Kopie eines generischen Structs für konkrete Typen (einmal je Kombination): Felder, Methoden-Signaturen und
    /// Standardwerte werden mit den echten Typen aufgelöst; die Methoden-Rümpfe prüft `check_program` danach.
    fn instantiate_struct(&mut self, gkey: &str, args: Vec<Type>) -> Type {
        let (ui, decl) = self.generic_structs[gkey].clone();
        let key = format!("{gkey}<{}>", args.iter().map(|t| t.name()).collect::<Vec<_>>().join(","));
        if !self.types.contains_key(&key) {
            self.inst_args.insert(key.clone(), (gkey.to_string(), args.clone()));
            let subst: HashMap<String, Type> = decl.generics.iter().cloned().zip(args).collect();
            let (saved_unit, saved_subst) = (self.unit, std::mem::replace(&mut self.subst, subst.clone()));
            self.unit = ui;
            // vorab eintragen: ein Feld darf den eigenen Typ (z. B. in einer Liste) nennen
            self.types.insert(key.clone(), Kind::Struct);
            self.structs.insert(key.clone(), Vec::new());
            let mut d = decl.clone();
            d.generics.clear();
            d.key = key.clone();
            let fields = self.fields(&mut d.fields, &d.name);
            self.structs.insert(key.clone(), fields);
            for m in &mut d.methods {
                self.signature(m, Some(&key));
            }
            self.defaults(&mut d.fields, &key, None);
            self.unit = saved_unit;
            self.subst = saved_subst;
            self.sinsts.push(SInst { unit: ui, subst, decl: Some(d) });
        }
        Type::Struct(key)
    }

    /// `Paar(1, "a")` ohne Typ-Argumente: sie ergeben sich aus den Werten der Felder.
    fn generic_ctor(&mut self, gkey: String, mut args: Vec<Arg>, span: Span) -> (ExprKind, Type) {
        let decl = self.generic_structs[&gkey].1.clone();
        let types: Vec<Type> = args.iter_mut().map(|a| self.value(&mut a.value)).collect();
        let (mut bound, mut pos) = (HashMap::new(), 0);
        for (a, t) in args.iter().zip(&types) {
            let field = match &a.name {
                None => {
                    pos += 1;
                    decl.fields.get(pos - 1)
                }
                Some((n, _)) => decl.fields.iter().find(|f| f.name == *n),
            };
            if let Some(f) = field {
                self.unify(&f.ty_ref, t, &decl.generics, &mut bound);
            }
        }
        for g in &decl.generics {
            if !bound.contains_key(g) {
                self.err(Diag::new(span, format!("bei `{}(…)` lässt sich der Typ `{g}` aus den Werten nicht erkennen", decl.name)).tip(format!("gib ihn an: `{}[{}](…)`", decl.name, decl.generics.iter().map(|_| "int").collect::<Vec<_>>().join(", "))));
                return (ExprKind::Int(0), Type::Unknown);
            }
        }
        let targs: Vec<Type> = decl.generics.iter().map(|g| bound[g].clone()).collect();
        if targs.iter().any(|t| matches!(t, Type::Unknown | Type::None | Type::Error)) {
            return (ExprKind::Int(0), Type::Unknown);
        }
        let ty = self.instantiate_struct(&gkey, targs);
        let Type::Struct(key) = &ty else { unreachable!() };
        let fields = self.structs[key].clone();
        let args = self.ctor_args(&decl.name, &fields, args, span);
        (ExprKind::Invoke { target: Target::New(key.clone()), args }, ty)
    }

    /// Typ zu einem Namen wie `Player` oder `modul.Player`.
    fn type_key(&self, name: &str) -> Option<(String, Kind)> {
        let key = match name.split_once('.') {
            Some((module, _)) if self.uses(module) => name.to_string(),
            Some(_) => return None,
            None => self.qual(name),
        };
        self.types.get(&key).map(|k| (key, *k))
    }

    fn resolve(&mut self, t: &TypeRef) -> Type {
        let mut ty = match (t.name.as_str(), t.args.len()) {
            (name, 0) if self.subst.contains_key(name) => self.subst[name].clone(),
            ("fn", _) => {
                let ps: Vec<Type> = t.args.iter().map(|a| self.resolve(a)).collect();
                let ret = t.ret.as_ref().map_or(Type::Void, |r| self.resolve(r));
                Type::Fn(ps, Box::new(ret))
            }
            ("", _) => Type::Void,
            ("int", 0) => Type::Int,
            ("f64", 0) => Type::F64,
            ("bool", 0) => Type::Bool,
            ("str", 0) => Type::Str,
            ("List", 1) => Type::List(Box::new(self.resolve(&t.args[0]))),
            ("Map", 2) => {
                let k = self.resolve(&t.args[0]);
                if !matches!(k, Type::Int | Type::Str | Type::Unknown) {
                    self.err(Diag::new(t.args[0].span, format!("Schlüssel einer Map sind int oder str, nicht {}", k.name())));
                }
                Type::Map(Box::new(k), Box::new(self.resolve(&t.args[1])))
            }
            ("List", _) => {
                self.err(Diag::new(t.span, "`List` braucht genau einen Typ").tip("z. B. `List[int]`"));
                Type::Unknown
            }
            ("Map", _) => {
                self.err(Diag::new(t.span, "`Map` braucht zwei Typen").tip("z. B. `Map[str, int]`"));
                Type::Unknown
            }
            (name, n) if self.generic_key(name).is_some() => {
                let gkey = self.generic_key(name).expect("geprüft");
                let want = self.generic_structs[&gkey].1.generics.clone();
                if n != want.len() {
                    self.err(Diag::new(t.span, format!("`{name}` braucht {} Typ-Argument(e), hier stehen {n}", want.len())).tip(format!("z. B. `{name}[{}]`", want.iter().map(|_| "int").collect::<Vec<_>>().join(", "))));
                    Type::Unknown
                } else {
                    let args: Vec<Type> = t.args.iter().map(|a| self.resolve(a)).collect();
                    if args.contains(&Type::Unknown) { Type::Unknown } else { self.instantiate_struct(&gkey, args) }
                }
            }
            (name, n) => match self.type_key(name) {
                Some(_) if n > 0 => {
                    self.err(Diag::new(t.span, format!("`{name}` hat keine Typ-Parameter")));
                    Type::Unknown
                }
                Some((key, Kind::Struct)) => Type::Struct(key),
                Some((key, Kind::Enum)) => Type::Enum(key),
                None => {
                    self.err(Diag::new(t.span, format!("den Typ `{name}` gibt es nicht")).tip("Volt kennt int, f64, bool, str, List[…], Map[…, …] und eigene `struct`s und `enum`s"));
                    Type::Unknown
                }
            },
        };
        for c in &t.suffix {
            if ty == Type::Void && *c == '?' {
                self.err(Diag::new(t.span, "`?` braucht einen Typ davor").tip("z. B. `int?`"));
                return Type::Unknown;
            }
            ty = if *c == '?' { Type::Opt(Box::new(ty)) } else { Type::Res(Box::new(ty)) };
        }
        ty
    }

    /// Enthält `key` sich selbst als Wert (nicht über List oder Map)?
    fn contains_itself(&self, key: &str) -> bool {
        fn walk(c: &Checker, t: &Type, target: &str, seen: &mut HashSet<String>) -> bool {
            match t {
                Type::Opt(t) | Type::Res(t) => walk(c, t, target, seen),
                Type::Struct(k) | Type::Enum(k) => {
                    if k == target {
                        return true;
                    }
                    if !seen.insert(k.clone()) {
                        return false;
                    }
                    let fields: Vec<&FieldInfo> = match c.structs.get(k) {
                        Some(fs) => fs.iter().collect(),
                        None => c.variants.get(k).map(|vs| vs.iter().flat_map(|v| &v.1).collect()).unwrap_or_default(),
                    };
                    fields.iter().any(|f| walk(c, &f.ty, target, seen))
                }
                _ => false,
            }
        }
        let fields: Vec<&FieldInfo> = match self.structs.get(key) {
            Some(fs) => fs.iter().collect(),
            None => self.variants.get(key).map(|vs| vs.iter().flat_map(|v| &v.1).collect()).unwrap_or_default(),
        };
        let mut seen = HashSet::new();
        fields.iter().any(|f| walk(self, &f.ty, key, &mut seen))
    }

    fn lookup(&self, name: &str) -> Option<Var> {
        self.scopes.iter().rev().find_map(|s| s.get(name)).cloned()
    }

    fn declare(&mut self, name: &str, span: Span, ty: Type, kind: VarKind) {
        if name == "_" {
            return;
        }
        if self.lookup(name).is_some() {
            self.err(Diag::new(span, format!("`{name}` gibt es hier schon")).tip("nimm einen anderen Namen – in Volt steht ein Name immer für genau eine Variable"));
        } else if BUILTINS.contains(&name) || self.fns.contains_key(&self.qual(name)) {
            self.err(Diag::new(span, format!("`{name}` ist schon eine Funktion")).tip("nimm für die Variable einen anderen Namen"));
        } else if self.type_key(name).is_some() || self.uses(name) {
            self.err(Diag::new(span, format!("`{name}` ist schon ein Typ oder Modul")).tip("nimm für die Variable einen anderen Namen"));
        }
        self.scopes.last_mut().expect("Variablen gibt es nur in Funktionen").insert(name.to_string(), Var { ty, kind });
    }

    /// Passt `got` dorthin, wo `want` erwartet wird? int wird automatisch zu f64, Werte zu `T?`/`T!`.
    fn fits(want: &Type, got: &Type) -> bool {
        match (want, got) {
            (Type::Unknown, _) | (_, Type::Unknown) => true,
            (Type::F64, Type::Int) => true,
            (Type::Opt(_), Type::None) | (Type::Res(_), Type::Error) => true,
            (Type::Opt(t), g) | (Type::Res(t), g) if want != got => **t != Type::Void && Self::fits(t, g),
            _ => want == got,
        }
    }

    /// Prüft `e` gegen `want` und verpackt den Wert, wenn er in ein `T?` oder `T!` wandert.
    fn coerce(&mut self, e: &mut Expr, want: &Type, msg: impl FnOnce(&str, &str) -> String) -> bool {
        if !Self::fits(want, &e.ty) {
            let mut d = Diag::new(e.span, msg(&want.name(), &e.ty.name()));
            if *want == Type::Int && e.ty == Type::F64 {
                d = d.tip("f64 wird nicht automatisch zu int – nutze `int(x)`, dann fallen die Nachkommastellen weg");
            } else if matches!((want, &e.ty), (Type::Opt(t) | Type::Res(t), g) if **t == *g) {
            } else if let (_, Type::Opt(t) | Type::Res(t)) = (want, &e.ty) {
                if Self::fits(want, t) {
                    d = d.tip("der Wert kann fehlen – pack ihn mit `if let`, `let … else`, `?` oder `.or(…)` aus");
                }
            }
            self.err(d);
            return false;
        }
        if matches!(want, Type::Opt(_) | Type::Res(_)) && *want != e.ty && e.ty != Type::Unknown {
            let span = e.span;
            let inner = std::mem::replace(e, Expr { kind: ExprKind::Bool(false), span, ty: Type::Unknown });
            *e = Expr { kind: ExprKind::Wrap(Box::new(inner)), span, ty: want.clone() };
        }
        true
    }

    fn function(&mut self, f: &mut FnDecl, owner: Option<&str>) {
        self.ret = f.ret.clone();
        self.loops = 0;
        self.scopes.clear();
        self.scopes.push(self.globals.get(self.unit).cloned().unwrap_or_default());
        self.scopes.push(HashMap::new());
        if let (Some(this), Some(owner)) = (f.this, owner) {
            let ty = match self.types.get(owner) {
                Some(Kind::Struct) => Type::Struct(owner.to_string()),
                _ => Type::Enum(owner.to_string()),
            };
            let kind = if this == SelfKind::Mut { VarKind::SelfMut } else { VarKind::SelfVal };
            self.scopes.last_mut().unwrap().insert("self".into(), Var { ty, kind });
        }
        for p in &f.params {
            self.declare(&p.name, p.span, p.ty.clone(), VarKind::Param);
        }
        self.block(&mut f.body);
        self.scopes.pop();
        let void = matches!(&f.ret, Type::Void | Type::Unknown) || f.ret == Type::Res(Box::new(Type::Void));
        if !void && !exits(&f.body, false) {
            self.err(
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
                        self.coerce(init, &want, |w, g| format!("`{name}` soll {w} sein, der Wert ist aber {g}"));
                        want
                    }
                    None if matches!(got, Type::None | Type::Error) => {
                        self.err(Diag::new(init.span, format!("bei `{}` ist unklar, welcher Typ gemeint ist", got.name())).tip(format!("gib den Typ an, z. B. `let {name}: int? = none`")));
                        Type::Unknown
                    }
                    None => got,
                };
                self.declare(name, *span, ty.clone(), if *mutable { VarKind::Var } else { VarKind::Let });
            }
            Stmt::LetElse { name, span, init, err, els } => {
                let got = self.value(init);
                let ty = match &got {
                    Type::Opt(t) => {
                        if let Some((e, at)) = err {
                            self.err(Diag::new(*at, format!("`none` hat keinen Fehlertext – `{e}` gibt es hier nicht")).tip("schreibe nur `else`"));
                        }
                        (**t).clone()
                    }
                    Type::Res(t) => (**t).clone(),
                    Type::Unknown => Type::Unknown,
                    other => {
                        self.err(Diag::new(init.span, format!("`let … else` braucht einen Wert, der fehlen kann (T? oder T!), das hier ist {}", other.name())).tip("dann reicht ein normales `let`"));
                        Type::Unknown
                    }
                };
                if ty == Type::Void && name != "_" {
                    self.err(Diag::new(init.span, "hier kommt kein Wert heraus, nur vielleicht ein Fehler").tip(format!("schreibe `let _ = … else {}`", err.as_ref().map_or("err", |e| e.0.as_str()))));
                }
                self.scopes.push(HashMap::new());
                if let (Some((e, at)), Type::Res(_)) = (err, &got) {
                    self.declare(e, *at, Type::Str, VarKind::Let);
                }
                self.block(els);
                self.scopes.pop();
                if !exits(els, self.loops > 0) {
                    self.err(Diag::new(*span, "der `else`-Block muss hier aufhören").tip("beende ihn mit `return`, `break` oder `continue` – danach gäbe es sonst keinen Wert"));
                }
                self.declare(name, *span, ty, VarKind::Let);
            }
            Stmt::Assign { target, op, value } => {
                let target_ty = self.expr(target);
                self.check_mut(target, "zuweisen");
                let got = self.value(value);
                let what = text(target);
                match op {
                    Some(op) => {
                        let span = value.span;
                        let result = self.binary(*op, &target_ty, &got, span);
                        if !Self::fits(&target_ty, &result) {
                            self.err(Diag::new(span, format!("`{what}` ist {}, der neue Wert ist aber {}", target_ty.name(), result.name())));
                        }
                    }
                    None => {
                        self.coerce(value, &target_ty, |w, g| format!("`{what}` ist {w}, der neue Wert ist aber {g}"));
                    }
                }
            }
            Stmt::If { cond, then, els } => {
                self.cond(cond);
                self.block(then);
                self.block(els);
            }
            Stmt::IfLet { name, span, value, then, els } => {
                let got = self.value(value);
                let ty = match got {
                    Type::Opt(t) | Type::Res(t) if *t != Type::Void => *t,
                    Type::Unknown => Type::Unknown,
                    other => {
                        self.err(Diag::new(value.span, format!("`if let` braucht einen Wert, der fehlen kann (T? oder T!), das hier ist {}", other.name())));
                        Type::Unknown
                    }
                };
                self.scopes.push(HashMap::new());
                self.declare(name, *span, ty, VarKind::Let);
                self.block(then);
                self.scopes.pop();
                self.block(els);
            }
            Stmt::While { cond, body } => {
                self.cond(cond);
                self.loop_body(body, None);
            }
            Stmt::For { var, span, start, end, body } => {
                let ty = match end {
                    Some(end) => {
                        for bound in [&mut *start, end] {
                            let got = self.value(bound);
                            if !matches!(got, Type::Int | Type::Unknown) {
                                self.err(Diag::new(bound.span, format!("der Bereich braucht int-Werte, das hier ist {}", got.name())));
                            }
                        }
                        Type::Int
                    }
                    None => match self.value(start) {
                        Type::List(t) => *t,
                        Type::Unknown => Type::Unknown,
                        Type::Map(..) => {
                            self.err(Diag::new(start.span, "über eine Map läuft `for` mit `.keys()`").tip("z. B. `for k in m.keys()`"));
                            Type::Unknown
                        }
                        other => {
                            self.err(Diag::new(start.span, format!("`for` braucht einen Bereich `a..b` oder eine Liste, das hier ist {}", other.name())).tip("z. B. `for i in 0..10`"));
                            Type::Unknown
                        }
                    },
                };
                self.loop_body(body, Some((var.as_str(), *span, ty)));
            }
            Stmt::Match { value, arms } => self.match_stmt(value, arms),
            Stmt::Return { span, value } => match value {
                Some(v) => {
                    let got = self.value(v);
                    let ret = self.ret.clone();
                    if matches!(ret, Type::Void) {
                        let mut d = Diag::new(v.span, "diese Funktion gibt nichts zurück");
                        if got != Type::Unknown {
                            d = d.tip(format!("schreibe `-> {}` hinter die Parameter, wenn sie einen Wert liefern soll", got.name()));
                        }
                        self.err(d);
                    } else {
                        self.coerce(v, &ret, |w, g| format!("die Funktion gibt {w} zurück, das hier ist aber {g}"));
                    }
                }
                None => {
                    if !matches!(self.ret, Type::Void | Type::Unknown) && self.ret != Type::Res(Box::new(Type::Void)) {
                        self.err(Diag::new(*span, format!("hier fehlt der Rückgabewert ({})", self.ret.name())));
                    }
                }
            },
            Stmt::Break(span) | Stmt::Continue(span) => {
                if self.loops == 0 {
                    self.err(Diag::new(*span, "`break` und `continue` gibt es nur in Schleifen"));
                }
            }
            Stmt::Defer(e) => {
                let ty = self.expr(e);
                if !matches!(e.kind, ExprKind::Invoke { .. }) {
                    if e.ty != Type::Unknown {
                        self.err(Diag::new(e.span, "`defer` braucht einen Aufruf").tip("z. B. `defer print(\"fertig\")` oder `defer liste.clear()`"));
                    }
                } else if matches!(ty, Type::Res(_)) {
                    self.err(Diag::new(e.span, "hier kann ein Fehler herauskommen, der nicht beachtet wird").tip("ein `defer` kann den Fehler nicht weitergeben – fange ihn im Aufruf selbst ab"));
                }
            }
            Stmt::Expr(e) => {
                let ty = self.expr(e);
                let call = match &e.kind {
                    ExprKind::Invoke { .. } => true,
                    ExprKind::Try(inner) => matches!(inner.kind, ExprKind::Invoke { .. }),
                    _ => false,
                };
                if !call && ty != Type::Unknown {
                    let mut d = Diag::new(e.span, "dieser Wert wird nicht benutzt");
                    if matches!(e.kind, ExprKind::Binary(BinOp::Eq, ..)) {
                        d = d.tip("zum Zuweisen nutze `=` statt `==`");
                    }
                    self.err(d);
                } else if matches!(ty, Type::Res(_)) {
                    self.err(Diag::new(e.span, "hier kann ein Fehler herauskommen, der nicht beachtet wird").tip("`…?` gibt ihn weiter, `let _ = … else err` fängt ihn ab"));
                }
            }
        }
    }

    fn loop_body(&mut self, body: &mut [Stmt], var: Option<(&str, Span, Type)>) {
        self.scopes.push(HashMap::new());
        if let Some((name, span, ty)) = var {
            self.declare(name, span, ty, VarKind::Loop);
        }
        self.loops += 1;
        self.block(body);
        self.loops -= 1;
        self.scopes.pop();
    }

    fn match_stmt(&mut self, value: &mut Expr, arms: &mut [Arm]) {
        let ty = self.value(value);
        let variants = match &ty {
            Type::Enum(k) => self.variants.get(k).cloned(),
            _ => None,
        };
        let mut covered = HashSet::new();
        let mut wild = false;
        for arm in arms.iter_mut() {
            self.scopes.push(HashMap::new());
            let span = arm.pat.span;
            match (&mut arm.pat.kind, &ty) {
                (PatKind::Wild, _) => wild = true,
                (_, Type::Unknown) => {}
                (PatKind::Variant { name, binds, index }, Type::Enum(key)) => {
                    let vs = variants.as_ref().expect("Enum ist registriert");
                    match vs.iter().position(|(n, _)| n == name) {
                        Some(i) => {
                            *index = i;
                            covered.insert(i);
                            let fields = &vs[i].1;
                            if !binds.is_empty() && binds.len() != fields.len() {
                                let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
                                self.err(Diag::new(span, format!("`{name}` hat {} Feld(er), hier stehen {} Namen", fields.len(), binds.len())).tip(format!("schreibe `{name}({})`", names.join(", "))));
                            }
                            for ((b, at), f) in binds.iter().zip(fields) {
                                self.declare(b, *at, f.ty.clone(), VarKind::Let);
                            }
                        }
                        None => {
                            let names: Vec<&str> = vs.iter().map(|v| v.0.as_str()).collect();
                            self.err(Diag::new(span, format!("`{key}` hat keinen Fall `{name}`")).tip(format!("die Fälle sind: {}", names.join(", "))));
                        }
                    }
                }
                (PatKind::Int(_), Type::Int) | (PatKind::Str(_), Type::Str) => {}
                (PatKind::Bool(b), Type::Bool) => {
                    covered.insert(*b as usize);
                }
                (_, other) => {
                    self.err(Diag::new(span, format!("dieses Muster passt nicht zu {}", other.name())));
                }
            }
            self.block(&mut arm.body);
            self.scopes.pop();
        }
        if wild || ty == Type::Unknown {
            return;
        }
        match (&ty, variants) {
            (Type::Enum(_), Some(vs)) => {
                let missing: Vec<&str> = vs.iter().enumerate().filter(|(i, _)| !covered.contains(i)).map(|(_, v)| v.0.as_str()).collect();
                if !missing.is_empty() {
                    self.err(Diag::new(value.span, format!("`match` deckt nicht alle Fälle ab, es fehlt: {}", missing.join(", "))).tip("ergänze die Fälle oder fange den Rest mit `_ => …` ab"));
                }
            }
            (Type::Bool, _) if covered.len() == 2 => {}
            (Type::Int | Type::Str | Type::Bool, _) => {
                self.err(Diag::new(value.span, "`match` deckt nicht alle Werte ab").tip("fange den Rest mit `_ => …` ab"));
            }
            (other, _) => {
                self.err(Diag::new(value.span, format!("`match` geht mit Enums, int, str und bool, nicht mit {}", other.name())).tip("für T? und T! nimm `if let`"));
            }
        }
    }

    fn cond(&mut self, e: &mut Expr) {
        let got = self.value(e);
        if !matches!(got, Type::Bool | Type::Unknown) {
            let mut d = Diag::new(e.span, format!("die Bedingung muss bool sein, ist aber {}", got.name()));
            if got.is_num() {
                d = d.tip("vergleiche die Zahl, z. B. mit `!= 0`");
            } else if matches!(got, Type::Opt(_) | Type::Res(_)) {
                d = d.tip("prüfe mit `if let x = …`, ob ein Wert da ist");
            }
            self.err(d);
        }
    }

    /// Darf man `e` ändern (Zuweisung oder `mut self`-Methode)? Meldet sonst einen Fehler.
    fn check_mut(&mut self, e: &Expr, what: &str) {
        match &e.kind {
            ExprKind::Ident(name) => {
                let Some(var) = self.lookup(name) else { return };
                let tip = match var.kind {
                    VarKind::Var | VarKind::SelfMut => return,
                    VarKind::Param => format!("Parameter sind fest – kopiere den Wert in eine eigene Variable, z. B. `var wert = {name}`"),
                    VarKind::Loop => "die Schleifenvariable setzt Volt selbst – nutze dafür eine eigene `var`".to_string(),
                    VarKind::SelfVal => "schreibe `mut self`, damit die Methode ihre Felder ändern darf".to_string(),
                    VarKind::Let => format!("nutze `var {name} = …`, wenn sich der Wert ändern soll"),
                };
                self.err(Diag::new(e.span, format!("`{name}` ist unveränderlich")).tip(tip));
            }
            ExprKind::Field(x, _) => self.check_mut(x, what),
            // Listen und Maps sind Referenzen: ihr Inhalt lässt sich immer ändern.
            ExprKind::Index(..) => {}
            _ => {
                if e.ty != Type::Unknown {
                    self.err(Diag::new(e.span, format!("hier kann man nichts {what}")).tip("speichere den Wert zuerst in einer `var`"));
                }
            }
        }
    }

    /// Wie `expr`, aber der Ausdruck muss einen Wert liefern.
    fn value(&mut self, e: &mut Expr) -> Type {
        if self.expr(e) == Type::Void {
            self.err(Diag::new(e.span, "dieser Aufruf gibt keinen Wert zurück"));
            e.ty = Type::Unknown;
        }
        e.ty.clone()
    }

    /// `a`, `a.b`, `a.b.c` – wenn `a` keine Variable ist, ein Name aus Modulen und Typen.
    fn path(&self, e: &Expr) -> Option<Vec<String>> {
        match &e.kind {
            ExprKind::Ident(n) if self.lookup(n).is_none() => Some(vec![n.clone()]),
            ExprKind::Field(x, n) => {
                let mut p = self.path(x)?;
                p.push(n.clone());
                Some(p)
            }
            _ => None,
        }
    }

    fn resolve_path(&self, segs: &[String]) -> PathRes {
        let mut res = if self.uses(&segs[0]) {
            PathRes::Module
        } else if let Some((key, kind)) = self.type_key(&segs[0]) {
            PathRes::Type(key, kind)
        } else if self.fns.contains_key(&self.qual(&segs[0])) {
            PathRes::Fn(self.qual(&segs[0]))
        } else {
            return PathRes::Missing;
        };
        for (i, seg) in segs.iter().enumerate().skip(1) {
            res = match res {
                PathRes::Module => {
                    let key = format!("{}.{seg}", segs[i - 1]);
                    match self.types.get(&key) {
                        Some(kind) => PathRes::Type(key, *kind),
                        None if self.fns.contains_key(&key) => PathRes::Fn(key),
                        None => return PathRes::Missing,
                    }
                }
                PathRes::Type(key, kind) => {
                    let variant = if kind == Kind::Enum { self.variants.get(&key).and_then(|vs| vs.iter().position(|v| v.0 == *seg)) } else { None };
                    match variant {
                        Some(v) => PathRes::Variant(key, v),
                        None if self.fns.contains_key(&format!("{key}.{seg}")) => PathRes::Static(format!("{key}.{seg}")),
                        None => return PathRes::Missing,
                    }
                }
                _ => return PathRes::Missing,
            };
        }
        res
    }

    fn field_type(&mut self, recv: &Type, name: &str, span: Span) -> Type {
        match recv {
            Type::Struct(k) => match self.structs.get(k).and_then(|fs| fs.iter().find(|f| f.name == name)) {
                Some(f) => f.ty.clone(),
                None => {
                    let mut d = Diag::new(span, format!("`{k}` hat kein Feld `{name}`"));
                    if self.fns.contains_key(&format!("{k}.{name}")) {
                        d = d.tip(format!("`{name}` ist eine Methode – rufe sie mit `.{name}(…)` auf"));
                    }
                    self.err(d);
                    Type::Unknown
                }
            },
            Type::Unknown => Type::Unknown,
            other => {
                let mut d = Diag::new(span, format!("{} hat kein Feld `{name}`", other.name()));
                if builtin_method(other, name).is_some() {
                    d = d.tip(format!("`{name}` ist eine Methode – rufe sie mit `.{name}()` auf"));
                }
                self.err(d);
                Type::Unknown
            }
        }
    }

    /// Positions- und benannte Argumente den Feldern zuordnen, fehlende mit Standardwerten füllen.
    fn ctor_args(&mut self, owner: &str, fields: &[FieldInfo], args: Vec<Arg>, span: Span) -> Vec<Expr> {
        let mut slots: Vec<Option<Expr>> = vec![None; fields.len()];
        let mut pos = 0;
        for mut a in args {
            self.value(&mut a.value);
            let i = match &a.name {
                None if pos < fields.len() => {
                    pos += 1;
                    pos - 1
                }
                None => {
                    self.err(Diag::new(a.value.span, format!("`{owner}` hat nur {} Feld(er)", fields.len())));
                    continue;
                }
                Some((n, at)) => match fields.iter().position(|f| f.name == *n) {
                    Some(i) if slots[i].is_none() => i,
                    Some(_) => {
                        self.err(Diag::new(*at, format!("`{n}` ist doppelt angegeben")));
                        continue;
                    }
                    None => {
                        self.err(Diag::new(*at, format!("`{owner}` hat kein Feld `{n}`")));
                        continue;
                    }
                },
            };
            let fname = fields[i].name.clone();
            self.coerce(&mut a.value, &fields[i].ty, |w, g| format!("`{owner}.{fname}` soll {w} sein, ist aber {g}"));
            slots[i] = Some(a.value);
        }
        let mut missing = Vec::new();
        let out = slots
            .into_iter()
            .zip(fields)
            .map(|(slot, f)| {
                slot.or_else(|| f.default.clone()).unwrap_or_else(|| {
                    missing.push(f.name.as_str());
                    Expr { kind: ExprKind::Int(0), span, ty: Type::Unknown }
                })
            })
            .collect();
        if !missing.is_empty() {
            self.err(Diag::new(span, format!("`{owner}(…)` braucht noch: {}", missing.join(", "))).tip(format!("z. B. `{owner}({}: …)`", missing[0])));
        }
        out
    }

    /// Argumente eines normalen Aufrufs prüfen.
    fn call_args(&mut self, name: &str, params: &[Type], args: Vec<Arg>, span: Span) -> Vec<Expr> {
        let mut out = Vec::new();
        for mut a in args {
            if let Some((_, at)) = &a.name {
                self.err(Diag::new(*at, "benannte Argumente gibt es nur beim Erzeugen von Structs und Enum-Fällen"));
            }
            self.value(&mut a.value);
            out.push(a.value);
        }
        if params.len() != out.len() {
            self.err(Diag::new(span, format!("`{name}` erwartet {} Argument(e), bekommen hat es {}", params.len(), out.len())));
        }
        for (i, (want, a)) in params.iter().zip(&mut out).enumerate() {
            self.coerce(a, want, |w, g| format!("Argument {} von `{name}` soll {w} sein, ist aber {g}", i + 1));
        }
        out
    }

    fn builtin_call(&mut self, name: &'static str, args: Vec<Arg>, span: Span) -> (Vec<Expr>, Type) {
        use Type::*;
        let n = args.len();
        let (params, ret): (Vec<Type>, Type) = match name {
            "print" => {
                let mut out = self.call_args(name, &vec![Unknown; n], args, span);
                if n != 1 {
                    self.err(Diag::new(span, format!("`print` gibt genau einen Wert aus, bekommen hat es {n}")).tip("mehrere Werte passen in einen String: `print(\"{a} {b}\")`"));
                } else if !out[0].ty.is_prim() && out[0].ty != Unknown {
                    let d = Diag::new(out[0].span, format!("`print` gibt int, f64, bool und str aus, das hier ist {}", out[0].ty.name()));
                    self.err(d.tip("gib einzelne Felder aus, z. B. `print(\"{p.name}\")`"));
                    out[0].ty = Unknown;
                }
                return (out, Void);
            }
            "input" if n == 0 => (vec![], Opt(Box::new(Str))),
            "input" => (vec![Str], Opt(Box::new(Str))),
            "error" => (vec![Str], Error),
            "int" => (vec![F64], Int),
            "float" => (vec![Int], F64),
            "sqrt" => (vec![F64], F64),
            "exit" => (vec![Int], Void),
            "assert" => (vec![Bool], Void),
            "sin" | "cos" | "tan" | "exp" | "log" | "floor" | "ceil" => (vec![F64], F64),
            "pow" | "atan2" => (vec![F64, F64], F64),
            "min" | "max" => {
                let out = self.call_args(name, &[Unknown, Unknown], args, span);
                let ty = match (out.first().map(|a| a.ty.clone()), out.get(1).map(|a| a.ty.clone())) {
                    (Some(Int), Some(Int)) => Int,
                    (Some(a), Some(b)) if a.is_num() && b.is_num() => F64,
                    (Some(Unknown), _) | (_, Some(Unknown)) => Unknown,
                    _ => { self.err(Diag::new(span, format!("`{name}` braucht zwei Zahlen"))); Unknown }
                };
                return (out, ty);
            }
            _ /* abs */ => {
                let out = self.call_args(name, &[Unknown], args, span);
                let ty = out.first().map(|a| a.ty.clone()).unwrap_or(Unknown);
                if !ty.is_num() && ty != Unknown {
                    self.err(Diag::new(span, format!("`abs` braucht eine Zahl, das hier ist {}", ty.name())));
                    return (out, Unknown);
                }
                return (out, ty);
            }
        };
        (self.call_args(name, &params, args, span), ret)
    }

    /// Typ-Parameter aus einem Argumenttyp ableiten: `List[T]` gegen `List[int]` ergibt `T = int`.
    fn unify(&self, r: &TypeRef, t: &Type, gens: &[String], out: &mut HashMap<String, Type>) {
        let mut t = t.clone();
        for c in r.suffix.iter().rev() {
            t = match (c, t) {
                ('?', Type::Opt(x)) | ('!', Type::Res(x)) => *x,
                (_, other) => other,
            };
        }
        if r.name == "fn" {
            if let Type::Fn(ps, ret) = &t {
                for (ra, ta) in r.args.iter().zip(ps) {
                    self.unify(ra, ta, gens, out);
                }
                if let Some(rr) = &r.ret {
                    self.unify(rr, ret, gens, out);
                }
            }
        } else if r.args.is_empty() && gens.contains(&r.name) {
            match out.get(&r.name) {
                Some(Type::Int) if t == Type::F64 => {
                    out.insert(r.name.clone(), Type::F64); // int und f64 gemischt → f64
                }
                Some(_) => {}
                None => {
                    out.insert(r.name.clone(), t);
                }
            }
        } else if let Type::Struct(k) = &t {
            // generischer Typ: `Paar[T, T]` gegen die Kopie `Paar<int,int>`
            if let Some((gkey, targs)) = self.inst_args.get(k) {
                let same = |a: &str, b: &str| a == b || a.rsplit('.').next() == b.rsplit('.').next();
                if same(gkey, &r.name) && r.args.len() == targs.len() {
                    for (ra, ta) in r.args.iter().zip(targs) {
                        self.unify(ra, ta, gens, out);
                    }
                }
            }
        } else if r.name == "List" && r.args.len() == 1 {
            if let Type::List(x) = &t {
                self.unify(&r.args[0], x, gens, out);
            }
        } else if r.name == "Map" && r.args.len() == 2 {
            if let Type::Map(k, v) = &t {
                self.unify(&r.args[0], k, gens, out);
                self.unify(&r.args[1], v, gens, out);
            }
        }
    }

    /// Aufruf einer generischen Funktion: Typen aus den Argumenten ableiten, Kopie für genau diese Typen anlegen
    /// (einmal je Kombination) und wie einen normalen Aufruf behandeln.
    fn generic_call(&mut self, key: String, mut args: Vec<Arg>, span: Span) -> (ExprKind, Type) {
        let (ui, decl) = self.generics[&key].clone();
        let types: Vec<Type> = args.iter_mut().map(|a| self.value(&mut a.value)).collect();
        if types.len() != decl.params.len() {
            self.err(Diag::new(span, format!("`{}` erwartet {} Argument(e), bekommen hat es {}", decl.name, decl.params.len(), types.len())));
            return (ExprKind::Int(0), Type::Unknown);
        }
        let mut bound = HashMap::new();
        for (p, t) in decl.params.iter().zip(&types) {
            self.unify(&p.ty_ref, t, &decl.generics, &mut bound);
        }
        for g in &decl.generics {
            if !bound.contains_key(g) {
                self.err(Diag::new(span, format!("bei `{}` lässt sich der Typ `{g}` aus den Argumenten nicht erkennen", decl.name)).tip(format!("`{g}` muss in einem Parameter vorkommen, z. B. `x: {g}`")));
                return (ExprKind::Int(0), Type::Unknown);
            }
        }
        if bound.values().any(|t| matches!(t, Type::Unknown | Type::None | Type::Error)) {
            return (ExprKind::Int(0), Type::Unknown);
        }
        for (g, tname, at) in &decl.bounds {
            let Some(methods) = self.traits.get(tname).cloned() else {
                self.err(Diag::new(*at, format!("das Trait `{tname}` gibt es nicht")));
                return (ExprKind::Int(0), Type::Unknown);
            };
            let owner = match &bound[g] {
                Type::Struct(k) | Type::Enum(k) => Some(k.clone()),
                _ => None,
            };
            let missing: Vec<String> = methods
                .iter()
                .filter(|(m, n)| !owner.as_ref().is_some_and(|k| self.fns.get(&format!("{k}.{m}")).is_some_and(|sg| sg.this.is_some() && sg.params.len() == *n)))
                .map(|(m, _)| m.clone())
                .collect();
            if !missing.is_empty() {
                self.err(Diag::new(span, format!("`{}` erfüllt das Trait `{tname}` nicht (für `{g}` bei `{}`)", bound[g].name(), decl.name)).tip(format!("es fehlt: {}", missing.iter().map(|m| format!("fn {m}(self, …)")).collect::<Vec<_>>().join(", "))));
                return (ExprKind::Int(0), Type::Unknown);
            }
        }
        let inst_key = format!("{key}<{}>", decl.generics.iter().map(|g| bound[g].name()).collect::<Vec<_>>().join(","));
        if !self.fns.contains_key(&inst_key) {
            let (saved_unit, saved_subst) = (self.unit, std::mem::replace(&mut self.subst, bound.clone()));
            self.unit = ui;
            let mut d = decl.clone();
            d.generics.clear();
            for p in &mut d.params {
                p.ty = self.resolve(&p.ty_ref);
            }
            d.ret = d.ret_ref.as_ref().map_or(Type::Void, |r| self.resolve(r));
            d.key = inst_key.clone();
            self.fns.insert(inst_key.clone(), Sig { this: None, params: d.params.iter().map(|p| p.ty.clone()).collect(), ret: d.ret.clone() });
            self.unit = saved_unit;
            self.subst = saved_subst;
            self.insts.push(Inst { unit: ui, subst: bound, decl: Some(d) });
        }
        let sig = self.fns[&inst_key].clone();
        let args = self.call_args(&decl.name, &sig.params, args, span);
        (ExprKind::Invoke { target: Target::Fn(inst_key), args }, sig.ret)
    }

    fn call(&mut self, callee: Expr, args: Vec<Arg>, span: Span) -> (ExprKind, Type) {
        // Methode auf einem Wert: `p.hit(5)`, `name.upper()`
        if self.path(&callee).is_none() {
            if let ExprKind::Field(mut recv, name) = callee.kind {
                let recv_ty = self.value(&mut recv);
                return match &recv_ty {
                    Type::Struct(k) | Type::Enum(k) => {
                        let key = format!("{k}.{name}");
                        match self.fns.get(&key).cloned() {
                            Some(Sig { this: Some(this), params, ret }) => {
                                if this == SelfKind::Mut {
                                    self.check_mut(&recv, "ändern");
                                }
                                let mut all = vec![*recv];
                                all.extend(self.call_args(&name, &params, args, span));
                                (ExprKind::Invoke { target: Target::Method { key, this }, args: all }, ret)
                            }
                            Some(_) => {
                                self.err(Diag::new(span, format!("`{key}` gehört zum Typ, nicht zu einem Wert")).tip(format!("rufe sie mit `{key}(…)` auf")));
                                (ExprKind::Int(0), Type::Unknown)
                            }
                            None if self.structs.get(k).and_then(|fs| fs.iter().find(|f| f.name == name)).is_some_and(|f| matches!(f.ty, Type::Fn(..))) => {
                                let Some(Type::Fn(ps, ret)) = self.structs.get(k).and_then(|fs| fs.iter().find(|f| f.name == name)).map(|f| f.ty.clone()) else { unreachable!() };
                                let field = Expr { kind: ExprKind::Field(recv, name.clone()), span, ty: Type::Fn(ps.clone(), ret.clone()) };
                                let mut all = vec![field];
                                all.extend(self.call_args(&name, &ps, args, span));
                                (ExprKind::Invoke { target: Target::Indirect, args: all }, *ret)
                            }
                            None => {
                                let mut d = Diag::new(span, format!("`{k}` hat keine Methode `{name}`"));
                                if self.structs.get(k).is_some_and(|fs| fs.iter().any(|f| f.name == name)) {
                                    d = d.tip(format!("`{name}` ist ein Feld – lies es ohne Klammern: `.{name}`"));
                                }
                                self.err(d);
                                (ExprKind::Int(0), Type::Unknown)
                            }
                        }
                    }
                    Type::Unknown => {
                        self.call_args(&name, &[], Vec::new(), span);
                        (ExprKind::Int(0), Type::Unknown)
                    }
                    other => match builtin_method(other, &name) {
                        Some((id, params, ret)) => {
                            let mut all = vec![*recv];
                            all.extend(self.call_args(&name, &params, args, span));
                            (ExprKind::Invoke { target: Target::BuiltinMethod(id), args: all }, ret)
                        }
                        None => {
                            let mut d = Diag::new(span, format!("{} hat keine Methode `{name}`", other.name()));
                            if !methods_of(other).is_empty() {
                                d = d.tip(format!("es gibt: {}", methods_of(other)));
                            }
                            self.err(d);
                            (ExprKind::Int(0), Type::Unknown)
                        }
                    },
                };
            }
            // Aufruf einer Variablen mit Funktionstyp: `f(1, 2)`
            {
                let mut callee = callee;
                if let Type::Fn(ps, ret) = self.expr(&mut callee) {
                    let name = text(&callee);
                    let mut all = vec![callee];
                    all.extend(self.call_args(&name, &ps, args, span));
                    return (ExprKind::Invoke { target: Target::Indirect, args: all }, *ret);
                }
                // keine Funktion: unten die übliche Meldung
                let name = text(&callee);
                self.err(Diag::new(span, format!("die Funktion `{name}` gibt es nicht")).tip(format!("`{name}` ist eine Variable, keine Funktion")));
                for mut a in args {
                    self.value(&mut a.value);
                }
                return (ExprKind::Int(0), Type::Unknown);
            }
        }
        let segs = self.path(&callee).expect("oben geprüft");
        let shown = segs.join(".");
        let fail = |c: &mut Self, d: Diag, args: Vec<Arg>| {
            c.err(d);
            for mut a in args {
                c.value(&mut a.value);
            }
            (ExprKind::Int(0), Type::Unknown)
        };
        if let Some(gkey) = self.generic_key(&shown) {
            return self.generic_ctor(gkey, args, span);
        }
        match self.resolve_path(&segs) {
            PathRes::Fn(key) if self.generics.contains_key(&key) => self.generic_call(key, args, span),
            PathRes::Fn(key) | PathRes::Static(key) if self.fns[&key].this.is_none() => {
                let sig = self.fns[&key].clone();
                let args = self.call_args(segs.last().unwrap(), &sig.params, args, span);
                (ExprKind::Invoke { target: Target::Fn(key), args }, sig.ret)
            }
            PathRes::Static(key) => {
                let name = segs.last().unwrap();
                fail(self, Diag::new(span, format!("`{key}` braucht einen Wert")).tip(format!("rufe sie auf einem Wert auf: `wert.{name}(…)`")), args)
            }
            PathRes::Type(key, Kind::Struct) => {
                let fields = self.structs.get(&key).cloned().unwrap_or_default();
                let args = self.ctor_args(&shown, &fields, args, span);
                (ExprKind::Invoke { target: Target::New(key.clone()), args }, Type::Struct(key))
            }
            PathRes::Type(key, _) => {
                let first = self.variants.get(&key).and_then(|vs| vs.first()).map(|v| v.0.clone()).unwrap_or_default();
                fail(self, Diag::new(span, format!("`{shown}` ist ein Enum – wähle einen Fall")).tip(format!("z. B. `{shown}.{first}(…)`")), args)
            }
            PathRes::Variant(key, index) => {
                let (vname, fields) = self.variants[&key][index].clone();
                let args = self.ctor_args(&format!("{shown}"), &fields, args, span);
                let _ = vname;
                (ExprKind::Variant { key: key.clone(), index, args }, Type::Enum(key))
            }
            PathRes::Module => fail(self, Diag::new(span, format!("`{shown}` ist ein Modul")).tip(format!("rufe eine Funktion darin auf: `{shown}.name(…)`")), args),
            PathRes::Fn(_) => unreachable!("freie Funktionen haben kein self"),
            PathRes::Missing => match BUILTINS.iter().find(|b| segs.len() == 1 && **b == segs[0]) {
                Some(b) => {
                    let (args, ret) = self.builtin_call(b, args, span);
                    (ExprKind::Invoke { target: Target::Builtin(b), args }, ret)
                }
                None => {
                    let d = if segs.len() > 1 && !matches!(self.resolve_path(&segs[..1]), PathRes::Missing) {
                        Diag::new(span, format!("`{shown}` gibt es nicht"))
                    } else {
                        Diag::new(span, format!("die Funktion `{shown}` gibt es nicht"))
                    };
                    fail(self, d, args)
                }
            },
        }
    }

    fn expr(&mut self, e: &mut Expr) -> Type {
        let span = e.span;
        let path = if matches!(e.kind, ExprKind::Field(..)) { self.path(e) } else { None };
        let mut rewrite: Option<ExprKind> = None;
        e.ty = match &mut e.kind {
            ExprKind::Int(_) => Type::Int,
            ExprKind::Float(_) => Type::F64,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::None => Type::None,
            ExprKind::Str(segs) => {
                for seg in segs {
                    if let StrSeg::Expr(inner) = seg {
                        let t = self.value(inner);
                        if !t.is_prim() && t != Type::Unknown {
                            let mut d = Diag::new(inner.span, format!("in `{{…}}` gehen int, f64, bool und str, das hier ist {}", t.name()));
                            if matches!(t, Type::Opt(_) | Type::Res(_)) {
                                d = d.tip("pack den Wert erst aus, z. B. mit `if let` oder `.or(…)`");
                            }
                            self.err(d);
                            inner.ty = Type::Unknown;
                        }
                    }
                }
                Type::Str
            }
            ExprKind::Ident(name) => match self.lookup(name) {
                Some(var) => var.ty,
                None => {
                    let mut d = Diag::new(span, format!("`{name}` gibt es hier nicht"));
                    match self.resolve_path(&[name.clone()]) {
                        PathRes::Fn(key) if !self.generics.contains_key(&key) && self.fns.get(&key).is_some_and(|sg| sg.this.is_none()) => {
                            let sg = self.fns[&key].clone();
                            e.kind = ExprKind::FnRef(key);
                            e.ty = Type::Fn(sg.params, Box::new(sg.ret));
                            return e.ty.clone();
                        }
                        PathRes::Fn(_) => d = d.tip(format!("`{name}` ist eine Funktion – rufe sie mit `{name}(…)` auf")),
                        PathRes::Type(..) => d = d.tip(format!("`{name}` ist ein Typ – einen Wert erzeugst du mit `{name}(…)`")),
                        PathRes::Module => d = d.tip(format!("`{name}` ist ein Modul – nutze `{name}.name`")),
                        _ if BUILTINS.contains(&name.as_str()) => d = d.tip(format!("`{name}` ist eine Funktion – rufe sie mit `{name}(…)` auf")),
                        _ => {}
                    }
                    self.err(d);
                    Type::Unknown
                }
            },
            ExprKind::Field(..) if path.is_some() => {
                let segs = path.expect("geprüft");
                let shown = segs.join(".");
                if let Some(c) = (segs.len() == 2).then(|| self.cconsts.get(&shown)).flatten() {
                    let c = *c;
                    e.kind = match c { CConst::Int(n) => ExprKind::Int(n), CConst::Float(x) => ExprKind::Float(x) };
                    e.ty = if matches!(c, CConst::Int(_)) { Type::Int } else { Type::F64 };
                    return e.ty.clone();
                }
                match self.resolve_path(&segs) {
                    PathRes::Fn(key) if !self.generics.contains_key(&key) && self.fns.get(&key).is_some_and(|sg| sg.this.is_none()) => {
                        let sg = self.fns[&key].clone();
                        e.kind = ExprKind::FnRef(key);
                        e.ty = Type::Fn(sg.params, Box::new(sg.ret));
                        return e.ty.clone();
                    }
                    PathRes::Variant(key, index) if self.variants[&key][index].1.is_empty() => {
                        e.kind = ExprKind::Variant { key: key.clone(), index, args: Vec::new() };
                        Type::Enum(key)
                    }
                    PathRes::Variant(key, index) => {
                        let names: Vec<String> = self.variants[&key][index].1.iter().map(|f| f.name.clone()).collect();
                        self.err(Diag::new(span, format!("`{shown}` braucht Werte")).tip(format!("schreibe `{shown}({})`", names.join(", "))));
                        Type::Unknown
                    }
                    PathRes::Missing if matches!(self.resolve_path(&segs[..1]), PathRes::Missing) => {
                        self.err(Diag::new(span, format!("`{}` gibt es hier nicht", segs[0])));
                        Type::Unknown
                    }
                    PathRes::Missing => {
                        self.err(Diag::new(span, format!("`{shown}` gibt es nicht")));
                        Type::Unknown
                    }
                    _ => {
                        self.err(Diag::new(span, format!("`{shown}` ist kein Wert")).tip(format!("rufe es auf: `{shown}(…)`")));
                        Type::Unknown
                    }
                }
            }
            ExprKind::Field(x, name) => {
                let recv = self.value(x);
                let name = name.clone();
                self.field_type(&recv, &name, span)
            }
            ExprKind::Index(x, i) => {
                let xt = self.value(x);
                self.value(i);
                match xt {
                    Type::List(t) => {
                        if !matches!(i.ty, Type::Int | Type::Unknown) {
                            self.err(Diag::new(i.span, format!("der Index einer Liste ist int, das hier ist {}", i.ty.name())));
                        }
                        *t
                    }
                    Type::Map(k, v) => {
                        self.coerce(i, &k, |w, g| format!("die Schlüssel dieser Map sind {w}, das hier ist {g}"));
                        *v
                    }
                    Type::Unknown => Type::Unknown,
                    other => {
                        self.err(Diag::new(span, format!("{} lässt sich nicht mit `[…]` lesen", other.name())));
                        Type::Unknown
                    }
                }
            }
            ExprKind::Try(x) => {
                let t = self.value(x);
                match (t, self.ret.clone()) {
                    (Type::Opt(inner), Type::Opt(_)) | (Type::Res(inner), Type::Res(_)) => *inner,
                    (Type::Unknown, _) | (_, Type::Unknown) => Type::Unknown,
                    (Type::Opt(_), _) => {
                        self.err(Diag::new(span, "`?` gibt `none` weiter – dafür muss die Funktion selbst `T?` zurückgeben").tip("oder pack den Wert mit `if let` / `let … else` aus"));
                        Type::Unknown
                    }
                    (Type::Res(_), _) => {
                        self.err(Diag::new(span, "`?` gibt den Fehler weiter – dafür muss die Funktion selbst `T!` zurückgeben").tip("oder fang ihn mit `let x = … else err` ab"));
                        Type::Unknown
                    }
                    (other, _) => {
                        self.err(Diag::new(span, format!("`?` geht nur bei T? und T!, das hier ist {}", other.name())));
                        Type::Unknown
                    }
                }
            }
            ExprKind::List(items) => {
                let mut ty = Type::Unknown;
                for (i, item) in items.iter_mut().enumerate() {
                    let got = self.value(item);
                    if i == 0 {
                        ty = got;
                        if matches!(ty, Type::None | Type::Error) {
                            self.err(Diag::new(item.span, "der Typ der Liste ist unklar").tip("schreibe `List[int?]()` und füge die Werte mit `.push(…)` hinzu"));
                            ty = Type::Unknown;
                        }
                    } else {
                        let first = ty.clone();
                        self.coerce(item, &first, |w, g| format!("die Liste enthält {w}, das hier ist aber {g}"));
                    }
                }
                Type::List(Box::new(ty))
            }
            ExprKind::NewColl(t) => {
                let t = t.clone();
                match self.resolve(&t) {
                    ty @ (Type::List(_) | Type::Map(..) | Type::Unknown) => ty,
                    other => {
                        self.err(Diag::new(span, format!("{} erzeugt man nicht mit `()`", other.name())));
                        Type::Unknown
                    }
                }
            }
            ExprKind::Unary(op, operand) => {
                let got = self.value(operand);
                let (sym, want, ok) = match op {
                    UnOp::Neg => ("-", "eine Zahl", got.is_num()),
                    UnOp::Not => ("not", "bool", got == Type::Bool),
                };
                if ok {
                    got
                } else {
                    if got != Type::Unknown {
                        self.err(Diag::new(span, format!("`{sym}` braucht {want}, bekommt aber {}", got.name())));
                    }
                    Type::Unknown
                }
            }
            ExprKind::Binary(op, l, r) => {
                let (lt, rt) = (self.value(l), self.value(r));
                let op = *op;
                // `a + b` auf einem Struct: Methode `op+` des Typs (Operator-Überladung)
                let user = match &lt {
                    Type::Struct(k) | Type::Enum(k) if !matches!(op, BinOp::And | BinOp::Or) => {
                        self.fns.get(&format!("{k}.op{}", op.sym())).filter(|sg| sg.this.is_some() && sg.params.len() == 1 && Self::fits(&sg.params[0], &rt)).map(|sg| (format!("{k}.op{}", op.sym()), sg.params[0].clone(), sg.ret.clone()))
                    }
                    _ => None,
                };
                if let Some((key, param, ret)) = user {
                    let hole = |e: &mut Expr| std::mem::replace(e, Expr { kind: ExprKind::Bool(false), span, ty: Type::Unknown });
                    let (a, mut b) = (hole(l), hole(r));
                    self.coerce(&mut b, &param, |_, _| String::new());
                    rewrite = Some(ExprKind::Invoke { target: Target::Method { key, this: SelfKind::Value }, args: vec![a, b] });
                    ret
                } else {
                    self.binary(op, &lt, &rt, span)
                }
            }
            ExprKind::Call { .. } => {
                let ExprKind::Call { callee, args } = take(e) else { unreachable!() };
                let (kind, ty) = self.call(*callee, args, span);
                e.kind = kind;
                ty
            }
            ExprKind::GenericNew { .. } => {
                let ExprKind::GenericNew { ty, args } = take(e) else { unreachable!() };
                let t = self.resolve(&ty);
                match &t {
                    Type::Struct(key) => {
                        let fields = self.structs[key].clone();
                        let args = self.ctor_args(&ty.name, &fields, args, span);
                        e.kind = ExprKind::Invoke { target: Target::New(key.clone()), args };
                        t
                    }
                    Type::Unknown => {
                        e.kind = ExprKind::Int(0);
                        Type::Unknown
                    }
                    other => {
                        self.err(Diag::new(span, format!("{} erzeugt man nicht mit `(…)`", other.name())));
                        e.kind = ExprKind::Int(0);
                        Type::Unknown
                    }
                }
            }
            // Schon aufgelöst (z. B. Standardwerte, die zweimal geprüft würden).
            ExprKind::Invoke { .. } | ExprKind::Variant { .. } | ExprKind::Wrap(_) | ExprKind::FnRef(_) => e.ty.clone(),
        };
        if let Some(k) = rewrite {
            e.kind = k;
        }
        e.ty.clone()
    }

    fn binary(&mut self, op: BinOp, l: &Type, r: &Type, span: Span) -> Type {
        use {BinOp::*, Type::*};
        let arithmetic = matches!(op, Add | Sub | Mul | Div | Mod);
        let fieldless = |t: &Type| match t {
            Enum(k) => self.variants.get(k).is_some_and(|vs| vs.iter().all(|v| v.1.is_empty())),
            _ => false,
        };
        let ty = match op {
            _ if *l == Unknown || *r == Unknown => Some(if arithmetic { Unknown } else { Bool }),
            Add | Sub | Mul | Div if l.is_num() && r.is_num() => Some(if *l == F64 || *r == F64 { F64 } else { Int }),
            Mod if *l == Int && *r == Int => Some(Int),
            Lt | Le | Gt | Ge if (l.is_num() && r.is_num()) || (*l == Str && *r == Str) => Some(Bool),
            Eq | Ne if (l == r && (l.is_prim() || fieldless(l))) || (l.is_num() && r.is_num()) => Some(Bool),
            And | Or if *l == Bool && *r == Bool => Some(Bool),
            _ => Option::None,
        };
        ty.unwrap_or_else(|| {
            let mut d = Diag::new(span, format!("`{}` geht nicht mit {} und {}", op.sym(), l.name(), r.name()));
            if op == Add && (*l == Str || *r == Str) {
                d = d.tip("Strings setzt man mit Einfügungen zusammen: `\"{a}{b}\"`");
            } else if op == Mod && l.is_num() && r.is_num() {
                d = d.tip("`%` gibt es nur für int");
            } else if matches!(op, And | Or) && (l.is_num() || r.is_num()) {
                d = d.tip("`and` und `or` verknüpfen Bedingungen, z. B. `a > 0 and b > 0`");
            } else if matches!(op, Eq | Ne) && (matches!(l, Opt(_)) || matches!(r, None)) {
                d = d.tip("ob ein Wert da ist, prüfst du mit `if let x = …`");
            } else if matches!(op, Eq | Ne) && l == r && matches!(l, Enum(_)) {
                d = d.tip("Fälle mit Daten vergleichst du mit `match`");
            }
            self.err(d);
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
                Stmt::Let { ty, .. } => Some(ty.clone()),
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
            let src = format!("fn add(a: int, b: f64) -> f64\n    return a + b\n\nfn zeige(x: int)\n    print(x)\n\nfn main()\n    {call}\n");
            run(&src).1.into_iter().map(|d| d.msg).collect()
        };
        assert_eq!(with("print(add(1, 2))"), Vec::<String>::new());
        assert_eq!(with("print(add(1))"), vec!["`add` erwartet 2 Argument(e), bekommen hat es 1"]);
        assert_eq!(with("print(add(1.5, 2))"), vec!["Argument 1 von `add` soll int sein, ist aber f64"]);
        assert_eq!(with("let x = zeige(1)"), vec!["dieser Aufruf gibt keinen Wert zurück"]);
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

    const TYPEN: &str = "\
struct Player
    name: str
    hp: int = 100

    fn hit(mut self, dmg: int)
        self.hp -= dmg

    fn alive(self) -> bool
        return self.hp > 0

enum Shape
    Circle(radius: f64)
    Rect(w: f64, h: f64)
    Empty

fn area(s: Shape) -> f64
    match s
        Circle(r) => 3.14 * r * r
        Rect(w, h) => w * h
        Empty => 0.0

fn find(xs: List[int], x: int) -> int?
    for i in 0..xs.len()
        if xs[i] == x
            return i
    return none

fn parse(s: str) -> int!
    let n = s.to_int() else
        return error(\"keine Zahl: {s}\")
    return n * 2

fn twice(s: str) -> int!
    return parse(s)? + parse(s)?
";

    fn with_types(body: &str) -> Vec<String> {
        let body: String = body.lines().map(|l| format!("    {l}\n")).collect();
        run(&format!("{TYPEN}\nfn main()\n{body}")).1.into_iter().map(|d| d.msg).collect()
    }

    #[test]
    fn phase2_gueltig() {
        let ok = "\
var p = Player(\"Ana\")
p.hit(5)
let q = Player(name: \"Bo\", hp: 3)
print(p.alive())
print(area(Shape.Circle(2.0)) + area(Shape.Empty))
let xs = [1, 2, 3]
xs.push(4)
if let i = find(xs, 3)
    print(i)
let m = Map[str, int]()
m[\"a\"] = 1
m[\"a\"] += 1
print(m.get(\"a\").or(0))
for k in m.keys()
    print(k.upper())
let words = \"a b\".split(\" \")
print(words.join(\",\"))
let n = twice(\"4\") else err
    print(err)
    return
print(n)
while true
    break
match n
    1 => print(\"eins\")
    _ => print(\"viel\")
";
        assert_eq!(with_types(ok), Vec::<String>::new());
    }

    #[test]
    fn phase2_fehler() {
        assert_eq!(with_types("let p = Player(\"A\")\np.hit(1)"), vec!["`p` ist unveränderlich"]);
        assert_eq!(with_types("let p = Player()"), vec!["`Player(…)` braucht noch: name"]);
        assert_eq!(with_types("let p = Player(\"A\", hpp: 1)"), vec!["`Player` hat kein Feld `hpp`"]);
        assert_eq!(with_types("let p = Player(1)"), vec!["`Player.name` soll str sein, ist aber int"]);
        assert_eq!(with_types("let s = Shape.Circle"), vec!["`Shape.Circle` braucht Werte"]);
        assert_eq!(with_types("let s = Shape.Kreis(1.0)"), vec!["`Shape.Kreis` gibt es nicht"]);
        assert_eq!(with_types("let x: int = find([1], 1)"), vec!["`x` soll int sein, der Wert ist aber int?"]);
        assert_eq!(with_types("let x = none"), vec!["bei `none` ist unklar, welcher Typ gemeint ist"]);
        assert_eq!(with_types("let x = parse(\"1\")?"), vec!["`?` gibt den Fehler weiter – dafür muss die Funktion selbst `T!` zurückgeben"]);
        assert_eq!(with_types("parse(\"1\")"), vec!["hier kann ein Fehler herauskommen, der nicht beachtet wird"]);
        assert_eq!(with_types("let n = parse(\"1\") else\n    print(1)"), vec!["der `else`-Block muss hier aufhören"]);
        assert_eq!(with_types("let s = Shape.Empty\nmatch s\n    Empty => print(1)"), vec!["`match` deckt nicht alle Fälle ab, es fehlt: Circle, Rect"]);
        assert_eq!(with_types("break"), vec!["`break` und `continue` gibt es nur in Schleifen"]);
        assert_eq!(with_types("let xs = [1]\nxs.push(\"a\")"), vec!["Argument 1 von `push` soll int sein, ist aber str"]);
        assert_eq!(with_types("let xs = [1]\nprint(xs.size())"), vec!["List[int] hat keine Methode `size`"]);
        assert_eq!(with_types("print(Player(\"a\"))"), vec!["`print` gibt int, f64, bool und str aus, das hier ist Player"]);
        assert_eq!(with_types("for i in 5\n    print(i)"), vec!["`for` braucht einen Bereich `a..b` oder eine Liste, das hier ist int"]);
        let msgs = |src: &str| -> Vec<String> { run(src).1.into_iter().map(|d| d.msg).collect() };
        assert_eq!(msgs("struct A\n    a: A\nfn main()\n    print(1)\n"), vec!["`A` enthält sich selbst"]);
        assert_eq!(msgs("use nix\nfn main()\n    print(1)\n"), vec!["das Modul `nix` gibt es nicht"]);
    }

    #[test]
    fn module() {
        let lib = volt_parser::parse(volt_lexer::lex("struct P\n    x: int\nfn neu(x: int) -> P\n    return P(x)\n").unwrap()).0;
        let main = volt_parser::parse(volt_lexer::lex("use geo\nfn main()\n    let p: geo.P = geo.neu(1)\n    print(p.x)\n    let q = geo.P(2)\n    print(q.x)\n").unwrap()).0;
        let mut units = vec![Unit { name: String::new(), module: main }, Unit { name: "geo".into(), module: lib }];
        let diags = check_program(&mut units);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(units[1].module.fns[0].key, "geo.neu");
    }
}
