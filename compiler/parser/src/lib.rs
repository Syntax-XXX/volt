//! Parser: Tokens → AST. Eine fehlerhafte Anweisung wird übersprungen, damit mehrere Fehler auf einmal gemeldet werden.

pub mod ast;

use ast::*;
use volt_lexer::{Diag, Span, StrPart, Tok, Token};

type R<T> = Result<T, Diag>;

/// Erwartet die Token-Liste des Lexers (endet mit `Eof`).
pub fn parse(tokens: Vec<Token>) -> (Module, Vec<Diag>) {
    parse_mode(tokens, false)
}

/// Volt Script (`.vts`): Anweisungen dürfen auf oberster Ebene stehen und laufen von oben nach unten –
/// der Parser packt sie in ein unsichtbares `fn main()`.
pub fn parse_script(tokens: Vec<Token>) -> (Module, Vec<Diag>) {
    parse_mode(tokens, true)
}

fn parse_mode(tokens: Vec<Token>, script: bool) -> (Module, Vec<Diag>) {
    let mut p = Parser { toks: tokens, i: 0, diags: Vec::new() };
    let mut m = Module::default();
    let mut top = Vec::new();
    while !p.at(&Tok::Eof) {
        let before = p.i;
        let item = match p.peek() {
            Tok::Fn => p.fn_decl(false).map(|f| m.fns.push(f)),
            Tok::Struct => p.struct_decl().map(|s| m.structs.push(s)),
            Tok::Enum => p.enum_decl().map(|e| m.enums.push(e)),
            Tok::Use => p.use_decl().map(|u| m.uses.push(u)),
            Tok::Ident(n) if n == "import" => p.import_decl().map(|c| m.c_imports.push(c)),
            Tok::Dedent => Ok(()), // bleibt nur nach einem schon gemeldeten Fehler übrig
            Tok::Indent => Err(Diag::new(p.span(), "unerwartete Einrückung").tip("Funktionen beginnen ganz links, ohne Leerzeichen davor")),
            _ if script => p.stmt().map(|s| top.push(s)),
            _ => {
                let d = Diag::new(p.span(), "auf oberster Ebene sind nur Funktionen, Typen und `use` erlaubt").tip("packe den Code in `fn main()` – oder schreibe ein Skript (.vts)");
                while !matches!(p.peek(), Tok::Fn | Tok::Struct | Tok::Enum | Tok::Use | Tok::Eof) && !matches!(p.peek(), Tok::Ident(n) if n == "import") {
                    p.i += 1;
                }
                Err(d)
            }
        };
        if let Err(d) = item {
            p.diags.push(d);
            if !p.at(&Tok::Eof) && !matches!(p.peek(), Tok::Fn | Tok::Struct | Tok::Enum | Tok::Use) || p.i == before {
                p.sync();
            }
        }
        if p.i == before {
            p.i += 1;
        }
    }
    if script {
        if let Some(f) = m.fns.iter().find(|f| f.name == "main") {
            p.diags.push(Diag::new(f.span, "ein Skript braucht kein `fn main()`").tip("schreibe den Code direkt auf die oberste Ebene"));
        }
        let span = Span { line: 1, col: 1, len: 1 };
        m.fns.push(FnDecl { name: "main".into(), span, this: None, params: Vec::new(), ret_ref: None, ret: Type::Unknown, body: top, key: String::new() });
    }
    (m, p.diags)
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
    diags: Vec<Diag>,
}

fn node(kind: ExprKind, span: Span) -> Expr {
    Expr { kind, span, ty: Type::Unknown }
}

/// Bereich vom Anfang von `a` bis zum Ende von `b` (nur innerhalb einer Zeile).
fn join(a: Span, b: Span) -> Span {
    if a.line == b.line && b.col + b.len >= a.col {
        Span { len: b.col + b.len - a.col, ..a }
    } else {
        a
    }
}

/// Zweistellige Operatoren je Stufe, von schwacher (0) zu starker Bindung (4).
fn binop(level: u8, t: &Tok) -> Option<BinOp> {
    Some(match (level, t) {
        (0, Tok::Or) => BinOp::Or,
        (1, Tok::And) => BinOp::And,
        (2, Tok::Eq) => BinOp::Eq,
        (2, Tok::Ne) => BinOp::Ne,
        (2, Tok::Lt) => BinOp::Lt,
        (2, Tok::Le) => BinOp::Le,
        (2, Tok::Gt) => BinOp::Gt,
        (2, Tok::Ge) => BinOp::Ge,
        (3, Tok::Plus) => BinOp::Add,
        (3, Tok::Minus) => BinOp::Sub,
        (4, Tok::Star) => BinOp::Mul,
        (4, Tok::Slash) => BinOp::Div,
        (4, Tok::Percent) => BinOp::Mod,
        _ => return None,
    })
}

/// Endet eine Funktion mit Rückgabewert auf `match`, sind Arme wie `Circle(r) => 3.14 * r * r` Rückgaben.
fn tail_match_returns(body: &mut [Stmt]) {
    if let Some(Stmt::Match { arms, .. }) = body.last_mut() {
        for arm in arms {
            if let [Stmt::Expr(e)] = arm.body.as_mut_slice() {
                let value = e.clone();
                arm.body = vec![Stmt::Return { span: value.span, value: Some(value) }];
            } else {
                tail_match_returns(&mut arm.body);
            }
        }
    }
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i.min(self.toks.len() - 1)].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.i + n).min(self.toks.len() - 1)].tok
    }

    fn span(&self) -> Span {
        self.toks[self.i.min(self.toks.len() - 1)].span
    }

    fn at(&self, t: &Tok) -> bool {
        self.peek() == t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        let hit = self.at(t);
        if hit {
            self.i += 1;
        }
        hit
    }
    fn unexpected(&self, wanted: &str) -> Diag {
        Diag::new(self.span(), format!("erwartet {wanted}, gefunden {}", self.peek()))
    }

    fn expect(&mut self, t: Tok) -> R<Span> {
        let span = self.span();
        if self.eat(&t) {
            Ok(span)
        } else {
            Err(self.unexpected(&t.to_string()))
        }
    }

    fn ident(&mut self, what: &str) -> R<(String, Span)> {
        let span = self.span();
        if let Tok::Ident(name) = self.peek().clone() {
            self.i += 1;
            Ok((name, span))
        } else {
            Err(self.unexpected(what))
        }
    }

    /// Nach einem Fehler: Rest der Zeile überspringen, samt einem eingerückten Block, der dazugehört.
    fn sync(&mut self) {
        if !self.at(&Tok::Indent) {
            while !matches!(self.peek(), Tok::Newline | Tok::Dedent | Tok::Eof) {
                self.i += 1;
            }
            self.eat(&Tok::Newline);
        }
        if self.at(&Tok::Indent) {
            let mut depth = 0;
            loop {
                match self.peek() {
                    Tok::Indent => depth += 1,
                    Tok::Dedent => depth -= 1,
                    Tok::Eof => break,
                    _ => {}
                }
                self.i += 1;
                if depth == 0 {
                    break;
                }
            }
        }
    }

    /// `import c "math.h" as m` (+ `link "m"` für Bibliotheken)
    fn import_decl(&mut self) -> R<CImport> {
        let span = self.span();
        self.i += 1;
        let kind = self.ident("`c`")?;
        if kind.0 != "c" {
            return Err(Diag::new(kind.1, "es gibt bisher nur `import c \"header.h\" as name`").tip("Volt-Dateien lädt `use name`"));
        }
        let header = match self.peek().clone() {
            Tok::Str(parts) if matches!(parts.as_slice(), [StrPart::Lit(_)]) => {
                self.i += 1;
                let [StrPart::Lit(h)] = parts.as_slice() else { unreachable!() };
                h.clone()
            }
            _ => return Err(self.unexpected("den Header als String, z. B. \"math.h\"")),
        };
        let word = |p: &mut Self, w: &str| -> R<()> {
            match p.ident(&format!("`{w}`"))? {
                (n, _) if n == w => Ok(()),
                (_, at) => Err(Diag::new(at, format!("hier gehört `{w}` hin")).tip("z. B. `import c \"math.h\" as m`")),
            }
        };
        word(self, "as")?;
        let (alias, _) = self.ident("einen Namen für den Import")?;
        let mut libs = Vec::new();
        while matches!(self.peek(), Tok::Ident(n) if n == "link") {
            self.i += 1;
            match self.peek().clone() {
                Tok::Str(parts) if matches!(parts.as_slice(), [StrPart::Lit(_)]) => {
                    self.i += 1;
                    let [StrPart::Lit(l)] = parts.as_slice() else { unreachable!() };
                    libs.push(l.clone());
                }
                _ => return Err(self.unexpected("den Bibliotheksnamen als String, z. B. \"raylib\"")),
            }
        }
        self.expect(Tok::Newline)?;
        Ok(CImport { header, alias, libs, span, funcs: Vec::new(), consts: Vec::new() })
    }

    fn use_decl(&mut self) -> R<(String, Span)> {
        self.expect(Tok::Use)?;
        let name = self.ident("den Namen eines Moduls")?;
        self.expect(Tok::Newline)?;
        Ok(name)
    }

    fn fn_decl(&mut self, method: bool) -> R<FnDecl> {
        self.expect(Tok::Fn)?;
        let (name, span) = self.ident("einen Funktionsnamen")?;
        self.fn_rest(name, span, method)
    }

    /// `op +(self, o: Vec2) -> Vec2` – Operator-Methode, heißt intern `op+`.
    fn op_decl(&mut self) -> R<FnDecl> {
        let span = self.span();
        self.i += 1;
        let sym = match self.peek() {
            Tok::Plus => "+", Tok::Minus => "-", Tok::Star => "*", Tok::Slash => "/", Tok::Percent => "%",
            Tok::Eq => "==", Tok::Ne => "!=", Tok::Lt => "<", Tok::Le => "<=", Tok::Gt => ">", Tok::Ge => ">=",
            _ => return Err(self.unexpected("einen Operator (+ - * / % == != < <= > >=)")),
        };
        self.i += 1;
        self.fn_rest(format!("op{sym}"), span, true)
    }

    fn fn_rest(&mut self, name: String, span: Span, method: bool) -> R<FnDecl> {
        self.expect(Tok::LParen)?;
        let mut this = None;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen) {
            let at = self.span();
            let is_self = matches!(self.peek(), Tok::Ident(n) if n == "self") || (self.at(&Tok::Mut) && matches!(self.peek_at(1), Tok::Ident(n) if n == "self"));
            if is_self {
                let kind = if self.eat(&Tok::Mut) { SelfKind::Mut } else { SelfKind::Value };
                self.i += 1;
                if !method {
                    return Err(Diag::new(at, "`self` gibt es nur in Methoden").tip("schreibe die Funktion eingerückt in ein `struct` oder `enum`"));
                }
                if !params.is_empty() || this.is_some() {
                    return Err(Diag::new(at, "`self` muss der erste Parameter sein"));
                }
                this = Some(kind);
            } else {
                let (name, span) = self.ident("einen Parameternamen")?;
                if !self.eat(&Tok::Colon) {
                    return Err(Diag::new(span, format!("der Parameter `{name}` braucht einen Typ")).tip(format!("schreibe z. B. `{name}: int`")));
                }
                params.push(Param { name, span, ty_ref: self.type_ref()?, ty: Type::Unknown });
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen)?;
        let ret_ref = if self.eat(&Tok::Arrow) { Some(self.type_ref()?) } else { None };
        let mut body = self.block()?;
        if ret_ref.is_some() {
            tail_match_returns(&mut body);
        }
        Ok(FnDecl { name, span, this, params, ret_ref, ret: Type::Unknown, body, key: String::new() })
    }

    /// `name: Typ` mit optionalem `= standardwert`.
    fn field(&mut self) -> R<Field> {
        let (name, span) = self.ident("einen Feldnamen")?;
        if !self.eat(&Tok::Colon) {
            return Err(Diag::new(span, format!("das Feld `{name}` braucht einen Typ")).tip(format!("schreibe z. B. `{name}: int`")));
        }
        let ty_ref = self.type_ref()?;
        let default = if self.eat(&Tok::Assign) { Some(self.expr()?) } else { None };
        Ok(Field { name, span, ty_ref, ty: Type::Unknown, default })
    }

    /// Kopfzeile, dann eingerückt die Einträge. `entry` liest einen Eintrag ohne `fn`.
    fn type_body<T>(&mut self, methods: &mut Vec<FnDecl>, mut entry: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        self.expect(Tok::Newline)?;
        let mut items = Vec::new();
        if !self.eat(&Tok::Indent) {
            return Err(Diag::new(self.span(), "hier fehlt ein eingerückter Block").tip("rücke die Einträge mit 4 Leerzeichen ein"));
        }
        while !self.at(&Tok::Dedent) && !self.at(&Tok::Eof) {
            let is_op = matches!(self.peek(), Tok::Ident(n) if n == "op")
                && matches!(self.peek_at(1), Tok::Plus | Tok::Minus | Tok::Star | Tok::Slash | Tok::Percent | Tok::Eq | Tok::Ne | Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge);
            let result = if self.at(&Tok::Fn) {
                self.fn_decl(true).map(|f| methods.push(f))
            } else if is_op {
                self.op_decl().map(|f| methods.push(f))
            } else {
                entry(self).and_then(|item| {
                    items.push(item);
                    self.expect(Tok::Newline).map(|_| ())
                })
            };
            if let Err(d) = result {
                self.diags.push(d);
                self.sync();
            }
        }
        self.eat(&Tok::Dedent);
        Ok(items)
    }

    fn struct_decl(&mut self) -> R<StructDecl> {
        self.expect(Tok::Struct)?;
        let (name, span) = self.ident("einen Namen für das Struct")?;
        let mut methods = Vec::new();
        let fields = self.type_body(&mut methods, Self::field)?;
        Ok(StructDecl { name, span, fields, methods, key: String::new() })
    }

    fn enum_decl(&mut self) -> R<EnumDecl> {
        self.expect(Tok::Enum)?;
        let (name, span) = self.ident("einen Namen für das Enum")?;
        let mut methods = Vec::new();
        let variants = self.type_body(&mut methods, |p| {
            let (name, span) = p.ident("einen Fall (z. B. `Circle(radius: f64)`)")?;
            let mut fields = Vec::new();
            if p.eat(&Tok::LParen) {
                while !p.at(&Tok::RParen) {
                    fields.push(p.field()?);
                    if !p.eat(&Tok::Comma) {
                        break;
                    }
                }
                p.expect(Tok::RParen)?;
            }
            Ok(Variant { name, span, fields })
        })?;
        Ok(EnumDecl { name, span, variants, methods, key: String::new() })
    }

    /// `int`, `modul.Typ`, `List[int]`, `Map[str, int]`, jeweils mit `?`/`!` dahinter; `!` allein heißt „nichts oder Fehler“.
    fn type_ref(&mut self) -> R<TypeRef> {
        let span = self.span();
        if self.eat(&Tok::Bang) {
            return Ok(TypeRef { name: String::new(), args: Vec::new(), suffix: vec!['!'], span });
        }
        let (mut name, _) = self.ident("einen Typ")?;
        while self.eat(&Tok::Dot) {
            name = format!("{name}.{}", self.ident("einen Typ")?.0);
        }
        let mut args = Vec::new();
        if self.eat(&Tok::LBracket) {
            loop {
                args.push(self.type_ref()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(Tok::RBracket)?;
        }
        let mut suffix = Vec::new();
        loop {
            if self.eat(&Tok::Question) {
                suffix.push('?');
            } else if self.eat(&Tok::Bang) {
                suffix.push('!');
            } else {
                break;
            }
        }
        let end = self.toks[self.i - 1].span;
        Ok(TypeRef { name, args, suffix, span: join(span, end) })
    }

    /// Zeilenende, dann die eingerückten Anweisungen bis zum Ende des Blocks.
    fn block(&mut self) -> R<Vec<Stmt>> {
        self.expect(Tok::Newline)?;
        if !self.eat(&Tok::Indent) {
            return Err(Diag::new(self.span(), "hier fehlt ein eingerückter Block").tip("rücke die Zeilen des Blocks mit 4 Leerzeichen ein"));
        }
        let mut stmts = Vec::new();
        while !self.at(&Tok::Dedent) && !self.at(&Tok::Eof) {
            match self.stmt() {
                Ok(s) => stmts.push(s),
                Err(d) => {
                    self.diags.push(d);
                    self.sync();
                }
            }
        }
        self.eat(&Tok::Dedent);
        Ok(stmts)
    }

    fn stmt(&mut self) -> R<Stmt> {
        let span = self.span();
        let stmt = match self.peek().clone() {
            Tok::Let | Tok::Var => {
                let mutable = self.at(&Tok::Var);
                self.i += 1;
                let (name, span) = self.ident("einen Variablennamen")?;
                let ty_ref = if self.eat(&Tok::Colon) { Some(self.type_ref()?) } else { None };
                if !self.eat(&Tok::Assign) {
                    let kw = if mutable { "var" } else { "let" };
                    return Err(Diag::new(span, format!("`{name}` braucht einen Startwert")).tip(format!("schreibe z. B. `{kw} {name} = 0`")));
                }
                let init = self.expr()?;
                if self.eat(&Tok::Else) {
                    if mutable || ty_ref.is_some() {
                        return Err(Diag::new(span, "`… else` gibt es nur als `let name = wert else`").tip("der Typ ergibt sich von selbst"));
                    }
                    let err = if self.at(&Tok::Newline) { None } else { Some(self.ident("einen Namen für den Fehler")?) };
                    return Ok(Stmt::LetElse { name, span, init, err, els: self.block()? });
                }
                Stmt::Let { name, span, mutable, ty_ref, ty: Type::Unknown, init }
            }
            Tok::If => return self.if_stmt(),
            Tok::While => {
                self.i += 1;
                let cond = self.cond()?;
                return Ok(Stmt::While { cond, body: self.block()? });
            }
            Tok::For => {
                self.i += 1;
                let (var, span) = self.ident("einen Namen für die Schleifenvariable")?;
                self.expect(Tok::In)?;
                let start = self.expr()?;
                let end = if self.eat(&Tok::DotDot) { Some(self.expr()?) } else { None };
                return Ok(Stmt::For { var, span, start, end, body: self.block()? });
            }
            Tok::Match => return self.match_stmt(),
            Tok::Return => {
                self.i += 1;
                Stmt::Return { span, value: if self.at(&Tok::Newline) { None } else { Some(self.expr()?) } }
            }
            Tok::Break => {
                self.i += 1;
                Stmt::Break(span)
            }
            Tok::Continue => {
                self.i += 1;
                Stmt::Continue(span)
            }
            Tok::Defer => {
                self.i += 1;
                Stmt::Defer(self.expr()?)
            }
            Tok::Indent => {
                return Err(Diag::new(span, "unerwartete Einrückung").tip("diese Zeile gehört zu keinem Block – rücke sie wie die Zeile darüber ein"));
            }
            _ => {
                let target = self.expr()?;
                match self.assign_op() {
                    Some(op) => {
                        self.i += 1;
                        Stmt::Assign { target, op, value: self.expr()? }
                    }
                    None => Stmt::Expr(target),
                }
            }
        };
        self.expect(Tok::Newline)?;
        Ok(stmt)
    }

    /// Steht eine Zuweisung an? `Some(None)` ist `=`, `Some(Some(op))` ist `+=` und Co.
    fn assign_op(&self) -> Option<Option<BinOp>> {
        Some(match self.peek() {
            Tok::Assign => None,
            Tok::PlusEq => Some(BinOp::Add),
            Tok::MinusEq => Some(BinOp::Sub),
            Tok::StarEq => Some(BinOp::Mul),
            Tok::SlashEq => Some(BinOp::Div),
            Tok::PercentEq => Some(BinOp::Mod),
            _ => return None,
        })
    }

    fn if_stmt(&mut self) -> R<Stmt> {
        self.expect(Tok::If)?;
        if self.eat(&Tok::Let) {
            let (name, span) = self.ident("einen Namen für den Wert")?;
            self.expect(Tok::Assign)?;
            let value = self.expr()?;
            let then = self.block()?;
            let els = self.else_part()?;
            return Ok(Stmt::IfLet { name, span, value, then, els });
        }
        let cond = self.cond()?;
        let then = self.block()?;
        Ok(Stmt::If { cond, then, els: self.else_part()? })
    }

    fn else_part(&mut self) -> R<Vec<Stmt>> {
        Ok(match self.eat(&Tok::Else) {
            true if self.at(&Tok::If) => vec![self.if_stmt()?],
            true => self.block()?,
            false => Vec::new(),
        })
    }

    fn match_stmt(&mut self) -> R<Stmt> {
        self.expect(Tok::Match)?;
        let value = self.expr()?;
        self.expect(Tok::Newline)?;
        if !self.eat(&Tok::Indent) {
            return Err(Diag::new(self.span(), "`match` braucht eingerückte Fälle").tip("z. B. `    Circle(r) => print(r)`"));
        }
        let mut arms = Vec::new();
        while !self.at(&Tok::Dedent) && !self.at(&Tok::Eof) {
            let arm = self.pattern().and_then(|pat| {
                let body = if self.eat(&Tok::FatArrow) { vec![self.stmt()?] } else { self.block()? };
                Ok(Arm { pat, body })
            });
            match arm {
                Ok(arm) => arms.push(arm),
                Err(d) => {
                    self.diags.push(d);
                    self.sync();
                }
            }
        }
        self.eat(&Tok::Dedent);
        Ok(Stmt::Match { value, arms })
    }

    fn pattern(&mut self) -> R<Pattern> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Ident(n) if n == "_" => PatKind::Wild,
            Tok::Int(n) => PatKind::Int(n),
            Tok::Minus if matches!(self.peek_at(1), Tok::Int(_)) => {
                self.i += 1;
                let Tok::Int(n) = self.peek().clone() else { unreachable!() };
                PatKind::Int(-n)
            }
            Tok::True => PatKind::Bool(true),
            Tok::False => PatKind::Bool(false),
            Tok::Str(parts) => match parts.as_slice() {
                [StrPart::Lit(text)] => PatKind::Str(text.clone()),
                _ => return Err(Diag::new(span, "in einem Muster darf der String keine Einfügung `{…}` haben")),
            },
            Tok::Ident(_) => {
                // `Circle`, `Shape.Circle`, `Circle(r)` – zählt nur der letzte Name.
                let mut name = self.ident("einen Fall")?.0;
                while self.eat(&Tok::Dot) {
                    name = self.ident("einen Fall")?.0;
                }
                let mut binds = Vec::new();
                if self.eat(&Tok::LParen) {
                    while !self.at(&Tok::RParen) {
                        binds.push(self.ident("einen Namen")?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(Tok::RParen)?;
                }
                return Ok(Pattern { kind: PatKind::Variant { name, binds, index: 0 }, span: join(span, self.toks[self.i - 1].span) });
            }
            _ => return Err(self.unexpected("ein Muster (z. B. `Circle(r)`, `3`, `\"ja\"` oder `_`)")),
        };
        self.i += 1;
        Ok(Pattern { kind, span })
    }

    /// Bedingung von `if`/`while` – fängt den Klassiker `if x = 5` ab.
    fn cond(&mut self) -> R<Expr> {
        let cond = self.expr()?;
        if self.at(&Tok::Assign) {
            return Err(Diag::new(self.span(), "`=` weist einen Wert zu").tip("zum Vergleichen nutze `==`"));
        }
        Ok(cond)
    }

    fn expr(&mut self) -> R<Expr> {
        self.binary(0)
    }

    fn binary(&mut self, level: u8) -> R<Expr> {
        // `not` bindet schwächer als Vergleiche: `not a == b` heißt `not (a == b)`.
        if level == 2 && self.at(&Tok::Not) {
            let span = self.span();
            self.i += 1;
            let operand = self.binary(2)?;
            return Ok(node(ExprKind::Unary(UnOp::Not, Box::new(operand)), span));
        }
        if level == 5 {
            return self.unary();
        }
        let mut left = self.binary(level + 1)?;
        while let Some(op) = binop(level, self.peek()) {
            self.i += 1;
            let right = self.binary(level + 1)?;
            let span = join(left.span, right.span);
            left = node(ExprKind::Binary(op, Box::new(left), Box::new(right)), span);
        }
        Ok(left)
    }

    fn unary(&mut self) -> R<Expr> {
        let span = self.span();
        if self.eat(&Tok::Minus) {
            let operand = self.unary()?;
            return Ok(node(ExprKind::Unary(UnOp::Neg, Box::new(operand)), span));
        }
        let mut e = self.primary()?;
        // Anhängsel: `.feld`, `.methode(…)`, `(…)`, `[i]`, `?`
        loop {
            let end = self.span();
            if self.eat(&Tok::Dot) {
                let name = if self.eat(&Tok::Or) { "or".to_string() } else { self.ident("einen Feld- oder Methodennamen")?.0 };
                let span = join(e.span, self.toks[self.i - 1].span);
                e = node(ExprKind::Field(Box::new(e), name), span);
            } else if self.eat(&Tok::LParen) {
                let mut args = Vec::new();
                while !self.at(&Tok::RParen) {
                    let name = match (self.peek().clone(), self.peek_at(1)) {
                        (Tok::Ident(n), Tok::Colon) => {
                            let at = self.span();
                            self.i += 2;
                            Some((n, at))
                        }
                        _ => None,
                    };
                    args.push(Arg { name, value: self.expr()? });
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                let close = self.expect(Tok::RParen)?;
                let span = join(e.span, close);
                e = node(ExprKind::Call { callee: Box::new(e), args }, span);
            } else if self.eat(&Tok::LBracket) {
                let index = self.expr()?;
                let close = self.expect(Tok::RBracket)?;
                let span = join(e.span, close);
                e = node(ExprKind::Index(Box::new(e), Box::new(index)), span);
            } else if self.eat(&Tok::Question) {
                let span = join(e.span, end);
                e = node(ExprKind::Try(Box::new(e)), span);
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> R<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(n) => ExprKind::Int(n),
            Tok::Float(x) => ExprKind::Float(x),
            Tok::True => ExprKind::Bool(true),
            Tok::False => ExprKind::Bool(false),
            Tok::None => ExprKind::None,
            Tok::Str(parts) => ExprKind::Str(
                parts
                    .into_iter()
                    .map(|p| match p {
                        StrPart::Lit(text) => Ok(StrSeg::Lit(text)),
                        StrPart::Code(toks) => interpolation(toks).map(StrSeg::Expr),
                    })
                    .collect::<R<_>>()?,
            ),
            Tok::Ident(name) if (name == "List" || name == "Map") && self.peek_at(1) == &Tok::LBracket => {
                let ty = self.type_ref()?;
                self.expect(Tok::LParen)?;
                let close = self.expect(Tok::RParen)?;
                return Ok(node(ExprKind::NewColl(ty), join(span, close)));
            }
            Tok::Ident(name) => ExprKind::Ident(name),
            Tok::LBracket => {
                self.i += 1;
                let mut items = Vec::new();
                while !self.at(&Tok::RBracket) {
                    items.push(self.expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                let close = self.expect(Tok::RBracket)?;
                if items.is_empty() {
                    return Err(Diag::new(join(span, close), "eine leere Liste braucht einen Typ").tip("schreibe z. B. `List[int]()`"));
                }
                return Ok(node(ExprKind::List(items), join(span, close)));
            }
            Tok::LParen => {
                self.i += 1;
                let inner = self.expr()?;
                self.expect(Tok::RParen)?;
                return Ok(inner);
            }
            Tok::Bang => return Err(Diag::new(span, "unerwartetes Zeichen `!`").tip("„nicht“ schreibt man in Volt `not`")),
            _ => return Err(self.unexpected("einen Ausdruck")),
        };
        self.i += 1;
        Ok(node(kind, span))
    }
}

/// Der Ausdruck in einer String-Einfügung `{…}`.
fn interpolation(mut toks: Vec<Token>) -> R<Expr> {
    let end = toks.last().map(|t| t.span).unwrap_or_default();
    toks.push(Token { tok: Tok::Eof, span: end });
    let mut p = Parser { toks, i: 0, diags: Vec::new() };
    match p.expr() {
        Ok(e) if p.at(&Tok::Eof) => Ok(e),
        Ok(_) => Err(Diag::new(p.span(), "in `{…}` darf nur ein einzelner Ausdruck stehen")),
        Err(_) if p.at(&Tok::Eof) => Err(Diag::new(end, "die Einfügung `{…}` ist unvollständig")),
        Err(d) => Err(d),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use volt_lexer::lex;

    fn parse_src(src: &str) -> (Module, Vec<Diag>) {
        parse(lex(src).unwrap())
    }

    /// Ausdruck parsen und in Klammer-Schreibweise zurückgeben.
    fn expr(src: &str) -> String {
        let (m, diags) = parse_src(&format!("fn main()\n    x = {src}\n"));
        assert!(diags.is_empty(), "{diags:?}");
        match &m.fns[0].body[0] {
            Stmt::Assign { value, .. } => dump_expr(value),
            _ => panic!("keine Zuweisung"),
        }
    }

    fn messages(src: &str) -> Vec<String> {
        parse_src(src).1.into_iter().map(|d| d.msg).collect()
    }

    #[test]
    fn vorrang_der_operatoren() {
        assert_eq!(expr("1 + 2 * 3"), "(+ 1 (* 2 3))");
        assert_eq!(expr("(1 + 2) * 3"), "(* (+ 1 2) 3)");
        assert_eq!(expr("10 - 4 - 3"), "(- (- 10 4) 3)");
        assert_eq!(expr("-a * b"), "(* (- a) b)");
        assert_eq!(expr("a < b + 1"), "(< a (+ b 1))");
        assert_eq!(expr("not a == b and c or d"), "(or (and (not (== a b)) c) d)");
        assert_eq!(expr("a or b and not c"), "(or a (and b (not c)))");
        assert_eq!(expr("f(1, g(2), x) % 2"), "(% (call f 1 (call g 2) x) 2)");
        assert_eq!(expr("4.5 / -2.0"), "(/ 4.5 (- 2.0))");
    }

    #[test]
    fn strings() {
        assert_eq!(expr(r#""plain""#), r#""plain""#);
        assert_eq!(expr(r#""a{b + 1}c{f("x")}""#), r#"(fmt "a" (+ b 1) "c" (call f "x"))"#);
    }

    #[test]
    fn ganzes_programm() {
        let src = "\
fn fib(n: int) -> int
    if n < 2
        return n
    return fib(n - 1) + fib(n - 2)

fn main()
    var s: f64 = 0
    for i in 0..10
        s += i
    while s > 1
        s = s / 2
    if s == 0
        print(\"null\")
    else if s < 1
        print(\"klein\")
    else
        return
";
        let want = "\
fn fib(n: int) -> int
  if (< n 2)
    return n
  return (+ (call fib (- n 1)) (call fib (- n 2)))
fn main()
  var s: f64 = 0
  for i in 0 .. 10
    s += i
  while (> s 1)
    s = (/ s 2)
  if (== s 0)
    (call print \"null\")
  else
    if (< s 1)
      (call print \"klein\")
    else
      return
";
        let (m, diags) = parse_src(src);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(dump(&m), want);
    }

    #[test]
    fn mehrere_fehler_auf_einmal() {
        let (m, diags) = parse_src("fn main()\n    let = 5\n    print(1)\n    x = = 2\n    print(2)\n");
        assert_eq!(diags.len(), 2);
        assert_eq!((diags[0].span.line, diags[1].span.line), (2, 4));
        assert_eq!(m.fns[0].body.len(), 2);
    }

    #[test]
    fn fehlerhafte_zeile_nimmt_ihren_block_mit() {
        let (m, diags) = parse_src("fn main()\n    if x = 1\n        print(1)\n    print(2)\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].tip.as_deref(), Some("zum Vergleichen nutze `==`"));
        assert_eq!(m.fns[0].body.len(), 1);
    }

    #[test]
    fn verstaendliche_meldungen() {
        assert!(messages("fn main()\nprint(1)\n")[0].contains("eingerückter Block"));
        assert!(messages("fn f(a)\n    return\n")[0].contains("braucht einen Typ"));
        assert!(messages("fn main()\n    var x\n")[0].contains("Startwert"));
        assert!(messages("fn main()\n    print(\"{1 +}\")\n")[0].contains("unvollständig"));
        assert!(messages("fn main()\n    print(1) print(2)\n")[0].contains("erwartet das Zeilenende, gefunden `print`"));
        assert!(messages("fn main()\n    print(1\n")[0].contains("erwartet `)`"));
        assert!(messages("fn main()\n    let a = 1\n        print(a)\n")[0].contains("unerwartete Einrückung"));
    }

    #[test]
    fn code_ausserhalb_von_funktionen() {
        let (m, diags) = parse_src("print(1)\nprint(2)\nfn main()\n    print(3)\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].tip.as_deref().unwrap().contains("fn main()"));
        assert_eq!(m.fns.len(), 1);
    }

    #[test]
    fn ausdruck_merkt_sich_seinen_bereich() {
        let (m, _) = parse_src("fn main()\n    x = a + bb\n");
        let Stmt::Assign { value, .. } = &m.fns[0].body[0] else { panic!() };
        assert_eq!(value.span, Span { line: 2, col: 9, len: 6 });
    }

    #[test]
    fn skript_ohne_main() {
        let (m, diags) = parse_script(volt_lexer::lex("let x = 2\nfn sq(n: int) -> int\n    return n * n\nprint(sq(x))\n").unwrap());
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(m.fns.iter().map(|f| (f.name.as_str(), f.body.len())).collect::<Vec<_>>(), [("sq", 1), ("main", 2)]);
        let (_, diags) = parse_script(volt_lexer::lex("fn main()\n    print(1)\n").unwrap());
        assert!(diags[0].msg.contains("kein `fn main()`"));
    }

    #[test]
    fn structs_enums_und_match() {
        let src = "\
use mathe

struct Player
    name: str
    hp: int = 100

    fn hit(mut self, dmg: int)
        self.hp -= dmg

enum Shape
    Circle(radius: f64)
    Empty

fn area(s: Shape) -> f64
    match s
        Circle(r) => 3.14 * r * r
        Empty => 0.0

fn main()
    var p = Player(\"Ana\", hp: 50)
    p.hit(5)
    let xs = [1, 2]
    let m = Map[str, List[int]?]()
    if let x = xs.pop()
        print(x)
    let n = \"5\".to_int() else
        return
    for x in xs
        break
";
        let want = "\
use mathe
struct Player
  name: str
  hp: int = 100
  fn hit(mut self, dmg: int)
    self.hp -= dmg
enum Shape
  Circle(radius: f64)
  Empty()
fn area(s: Shape) -> f64
  match s
    Circle(r) =>
      return (* (* 3.14 r) r)
    Empty =>
      return 0.0
fn main()
  var p = (call Player \"Ana\" hp: 50)
  (call p.hit 5)
  let xs = (list 1 2)
  let m = (new Map[str, List[int]?])
  if let x = (call xs.pop)
    (call print x)
  let n = (call \"5\".to_int) else
    return
  for x in xs
    break
";
        let (m, diags) = parse_src(src);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(dump(&m), want);
    }

    #[test]
    fn typen_und_anhaengsel() {
        assert_eq!(expr("a.b[1].c(2)?"), "(try (call (index a.b 1).c 2))");
        let (m, diags) = parse_src("fn f() -> !\n    return\nfn g(x: int?) -> List[str]!\n    return x?\n");
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(m.fns[0].ret_ref.as_ref().unwrap().text(), "!");
        assert_eq!(m.fns[1].ret_ref.as_ref().unwrap().text(), "List[str]!");
        assert!(messages("fn main()\n    let x = []\n")[0].contains("leere Liste"));
        assert!(messages("fn f(self)\n    return\n")[0].contains("nur in Methoden"));
        assert!(messages("fn main()\n    print(!x)\n")[0].contains("`!`"));
    }
}
