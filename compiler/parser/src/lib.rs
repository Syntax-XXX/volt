//! Parser: Tokens → AST. Eine fehlerhafte Anweisung wird übersprungen, damit mehrere Fehler auf einmal gemeldet werden.

pub mod ast;

use ast::*;
use volt_lexer::{Diag, Span, StrPart, Tok, Token};

type R<T> = Result<T, Diag>;

/// Erwartet die Token-Liste des Lexers (endet mit `Eof`).
pub fn parse(tokens: Vec<Token>) -> (Module, Vec<Diag>) {
    let mut p = Parser { toks: tokens, i: 0, diags: Vec::new() };
    let mut fns = Vec::new();
    while !p.at(&Tok::Eof) {
        let before = p.i;
        if p.at(&Tok::Fn) {
            match p.fn_decl() {
                Ok(f) => fns.push(f),
                Err(d) => {
                    p.diags.push(d);
                    p.sync();
                }
            }
        } else if !p.at(&Tok::Dedent) {
            // Dedent bleibt nur nach einem schon gemeldeten Fehler übrig.
            p.diags.push(if p.at(&Tok::Indent) {
                Diag::new(p.span(), "unerwartete Einrückung").tip("Funktionen beginnen ganz links, ohne Leerzeichen davor")
            } else {
                Diag::new(p.span(), "auf oberster Ebene sind nur Funktionen erlaubt").tip("packe den Code in `fn main()`")
            });
            while !p.at(&Tok::Fn) && !p.at(&Tok::Eof) {
                p.i += 1;
            }
        }
        if p.i == before {
            p.i += 1;
        }
    }
    (Module { fns }, p.diags)
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
    if a.line == b.line {
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

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.i.min(self.toks.len() - 1)].tok
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

    fn fn_decl(&mut self) -> R<FnDecl> {
        self.expect(Tok::Fn)?;
        let (name, span) = self.ident("einen Funktionsnamen")?;
        self.expect(Tok::LParen)?;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen) {
            let (name, span) = self.ident("einen Parameternamen")?;
            if !self.eat(&Tok::Colon) {
                return Err(Diag::new(span, format!("der Parameter `{name}` braucht einen Typ")).tip(format!("schreibe z. B. `{name}: int`")));
            }
            params.push(Param { name, span, ty_ref: self.type_ref()?, ty: Type::Unknown });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen)?;
        let ret_ref = if self.eat(&Tok::Arrow) { Some(self.type_ref()?) } else { None };
        let body = self.block()?;
        Ok(FnDecl { name, span, params, ret_ref, ret: Type::Unknown, body })
    }

    fn type_ref(&mut self) -> R<TypeRef> {
        let (name, span) = self.ident("einen Typ")?;
        Ok(TypeRef { name, span })
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
                Stmt::Let { name, span, mutable, ty_ref, ty: Type::Unknown, init: self.expr()? }
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
                if !self.eat(&Tok::DotDot) {
                    return Err(Diag::new(start.span, "`for` braucht einen Bereich").tip("schreibe z. B. `for i in 0..10`"));
                }
                let end = self.expr()?;
                return Ok(Stmt::For { var, span, start, end, body: self.block()? });
            }
            Tok::Return => {
                self.i += 1;
                Stmt::Return { span, value: if self.at(&Tok::Newline) { None } else { Some(self.expr()?) } }
            }
            Tok::Indent => {
                return Err(Diag::new(span, "unerwartete Einrückung").tip("diese Zeile gehört zu keinem Block – rücke sie wie die Zeile darüber ein"));
            }
            Tok::Ident(name) if self.assign_op().is_some() => {
                let op = self.assign_op().flatten();
                self.i += 2;
                let rhs = self.expr()?;
                let value = match op {
                    Some(op) => node(ExprKind::Binary(op, Box::new(node(ExprKind::Ident(name.clone()), span)), Box::new(rhs)), span),
                    None => rhs,
                };
                Stmt::Assign { name, span, value }
            }
            _ => Stmt::Expr(self.expr()?),
        };
        self.expect(Tok::Newline)?;
        Ok(stmt)
    }

    /// Folgt auf den Namen eine Zuweisung? `Some(None)` ist `=`, `Some(Some(op))` ist `+=` und Co.
    fn assign_op(&self) -> Option<Option<BinOp>> {
        Some(match &self.toks.get(self.i + 1)?.tok {
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
        let cond = self.cond()?;
        let then = self.block()?;
        let els = match self.eat(&Tok::Else) {
            true if self.at(&Tok::If) => vec![self.if_stmt()?],
            true => self.block()?,
            false => Vec::new(),
        };
        Ok(Stmt::If { cond, then, els })
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
        self.primary()
    }

    fn primary(&mut self) -> R<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(n) => ExprKind::Int(n),
            Tok::Float(x) => ExprKind::Float(x),
            Tok::True => ExprKind::Bool(true),
            Tok::False => ExprKind::Bool(false),
            Tok::Str(parts) => ExprKind::Str(
                parts
                    .into_iter()
                    .map(|p| match p {
                        StrPart::Lit(text) => Ok(StrSeg::Lit(text)),
                        StrPart::Code(toks) => interpolation(toks).map(StrSeg::Expr),
                    })
                    .collect::<R<_>>()?,
            ),
            Tok::Ident(name) => {
                self.i += 1;
                if !self.eat(&Tok::LParen) {
                    return Ok(node(ExprKind::Ident(name), span));
                }
                let mut args = Vec::new();
                while !self.at(&Tok::RParen) {
                    args.push(self.expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                let end = self.expect(Tok::RParen)?;
                return Ok(node(ExprKind::Call { name, args }, join(span, end)));
            }
            Tok::LParen => {
                self.i += 1;
                let inner = self.expr()?;
                self.expect(Tok::RParen)?;
                return Ok(inner);
            }
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
    s = (+ s i)
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
        assert!(messages("fn main()\n    for i in 5\n        print(i)\n")[0].contains("Bereich"));
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
}
