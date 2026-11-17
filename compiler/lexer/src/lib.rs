//! Lexer: Quelltext → Tokens. Einrückung wird zu INDENT/DEDENT, `{…}` in Strings zu eigenen Token-Listen.

use std::fmt;

/// Stelle im Quelltext: Zeile und Spalte ab 1, Länge in Zeichen.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
    pub len: u32,
}

/// Eine Fehlermeldung mit Ort und optionalem Tipp.
#[derive(Debug, Clone, PartialEq)]
pub struct Diag {
    pub span: Span,
    pub msg: String,
    pub tip: Option<String>,
}

impl Diag {
    pub fn new(span: Span, msg: impl Into<String>) -> Self {
        Diag { span, msg: msg.into(), tip: None }
    }

    pub fn tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    /// Meldung mit Datei:Zeile:Spalte, markierter Quellzeile und Tipp.
    pub fn render(&self, file: &str, src: &str) -> String {
        let Span { line, col, len } = self.span;
        let text = src.lines().nth((line as usize).saturating_sub(1)).unwrap_or("");
        let pad = " ".repeat((col as usize).saturating_sub(1));
        let marks = "^".repeat(len.max(1) as usize);
        let mut out = format!("error: {}\n  --> {file}:{line}:{col}\n     |\n{line:>4} | {text}\n     | {pad}{marks}\n", self.msg);
        if let Some(tip) = &self.tip {
            out += &format!("     = tipp: {tip}\n");
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(Vec<StrPart>),
    Ident(String),
    Fn,
    Let,
    Var,
    If,
    Else,
    While,
    For,
    In,
    Return,
    Struct,
    Enum,
    Match,
    None,
    Use,
    Mut,
    Break,
    Continue,
    Defer,
    True,
    False,
    And,
    Or,
    Not,
    LParen,
    RParen,
    Comma,
    Colon,
    Arrow,
    Assign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    DotDot,
    Dot,
    LBracket,
    RBracket,
    FatArrow,
    Question,
    Bang,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    Newline,
    Indent,
    Dedent,
    Eof,
}

/// Ein Stück eines Strings: fester Text oder die Tokens einer Einfügung `{…}`.
#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Lit(String),
    Code(Vec<Token>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

/// Schlüsselwörter und Zeichen mit festem Text – eine Tabelle für Lexer und Fehlermeldungen.
static FIXED: &[(&str, Tok)] = &[
    ("fn", Tok::Fn),
    ("let", Tok::Let),
    ("var", Tok::Var),
    ("if", Tok::If),
    ("else", Tok::Else),
    ("while", Tok::While),
    ("for", Tok::For),
    ("in", Tok::In),
    ("return", Tok::Return),
    ("struct", Tok::Struct),
    ("enum", Tok::Enum),
    ("match", Tok::Match),
    ("none", Tok::None),
    ("use", Tok::Use),
    ("mut", Tok::Mut),
    ("break", Tok::Break),
    ("continue", Tok::Continue),
    ("defer", Tok::Defer),
    ("true", Tok::True),
    ("false", Tok::False),
    ("and", Tok::And),
    ("or", Tok::Or),
    ("not", Tok::Not),
    ("(", Tok::LParen),
    (")", Tok::RParen),
    (",", Tok::Comma),
    (":", Tok::Colon),
    ("->", Tok::Arrow),
    ("=", Tok::Assign),
    ("==", Tok::Eq),
    ("!=", Tok::Ne),
    ("<", Tok::Lt),
    ("<=", Tok::Le),
    (">", Tok::Gt),
    (">=", Tok::Ge),
    ("+", Tok::Plus),
    ("-", Tok::Minus),
    ("*", Tok::Star),
    ("/", Tok::Slash),
    ("%", Tok::Percent),
    ("..", Tok::DotDot),
    (".", Tok::Dot),
    ("[", Tok::LBracket),
    ("]", Tok::RBracket),
    ("=>", Tok::FatArrow),
    ("?", Tok::Question),
    ("!", Tok::Bang),
    ("+=", Tok::PlusEq),
    ("-=", Tok::MinusEq),
    ("*=", Tok::StarEq),
    ("/=", Tok::SlashEq),
    ("%=", Tok::PercentEq),
];

/// So steht ein Token in Fehlermeldungen („erwartet `)`, gefunden das Zeilenende“).
impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Tok::Int(n) => write!(f, "`{n}`"),
            Tok::Float(x) => write!(f, "`{x}`"),
            Tok::Ident(name) => write!(f, "`{name}`"),
            Tok::Str(_) => f.write_str("ein String"),
            Tok::Newline => f.write_str("das Zeilenende"),
            Tok::Indent => f.write_str("eine Einrückung"),
            Tok::Dedent => f.write_str("das Ende des Blocks"),
            Tok::Eof => f.write_str("das Dateiende"),
            fixed => match FIXED.iter().find(|(_, t)| t == fixed) {
                Some((text, _)) => write!(f, "`{text}`"),
                None => f.write_str("?"),
            },
        }
    }
}

struct Lexer {
    chars: Vec<char>,
    i: usize,
    line: u32,
    col: u32,
    /// Tiefe offener Klammern: darin zählen Zeilenumbrüche und Einrückung nicht.
    parens: u32,
}

/// Zerlegt den Quelltext. Die Liste endet immer mit `Eof`; jede Anweisungszeile endet mit `Newline`.
pub fn lex(src: &str) -> Result<Vec<Token>, Diag> {
    let mut lx = Lexer { chars: src.chars().collect(), i: 0, line: 1, col: 1, parens: 0 };
    let mut out: Vec<Token> = Vec::new();
    let mut indents = vec![0u32];
    while lx.peek().is_some() {
        // Zeilenanfang: Einrückung zählen. Leere Zeilen und reine Kommentarzeilen zählen nicht.
        let mut width = 0;
        while lx.eat(' ') {
            width += 1;
        }
        match lx.peek() {
            Some('\t') => {
                return Err(Diag::new(lx.here(), "Tab in der Einrückung").tip("rücke mit Leerzeichen ein (üblich sind 4)"));
            }
            None | Some('\n' | '\r' | '#') => {}
            Some(_) => {
                let at = Span { line: lx.line, col: 1, len: width };
                let level = |indents: &Vec<u32>| *indents.last().unwrap();
                if width > level(&indents) {
                    indents.push(width);
                    out.push(Token { tok: Tok::Indent, span: at });
                }
                while width < level(&indents) {
                    indents.pop();
                    out.push(Token { tok: Tok::Dedent, span: at });
                }
                if width != level(&indents) {
                    return Err(Diag::new(at, "die Einrückung passt zu keinem Block darüber")
                        .tip(format!("nutze {} Leerzeichen wie der umgebende Block", level(&indents))));
                }
            }
        }
        // Tokens bis zum Zeilenende; innerhalb von Klammern geht die Zeile über Umbrüche hinweg weiter.
        loop {
            match lx.peek() {
                None => break,
                Some(' ' | '\t' | '\r') => {
                    lx.bump();
                }
                Some('#') => {
                    while !matches!(lx.peek(), None | Some('\n')) {
                        lx.bump();
                    }
                }
                Some('\n') => {
                    let span = lx.here();
                    lx.bump();
                    if lx.parens == 0 {
                        end_line(&mut out, span);
                        break;
                    }
                }
                Some(_) => out.push(lx.token()?),
            }
        }
    }
    let end = lx.here();
    end_line(&mut out, end);
    for _ in 1..indents.len() {
        out.push(Token { tok: Tok::Dedent, span: end });
    }
    out.push(Token { tok: Tok::Eof, span: end });
    Ok(out)
}

/// `Newline` nur nach einer Zeile mit Inhalt – leere Zeilen erzeugen keine Tokens.
fn end_line(out: &mut Vec<Token>, span: Span) {
    if !matches!(out.last(), None | Some(Token { tok: Tok::Newline, .. })) {
        out.push(Token { tok: Tok::Newline, span });
    }
}

impl Lexer {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn eat(&mut self, c: char) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.bump();
        }
        hit
    }

    fn here(&self) -> Span {
        Span { line: self.line, col: self.col, len: 1 }
    }

    fn token(&mut self) -> Result<Token, Diag> {
        let (line, col, start) = (self.line, self.col, self.i);
        let first = Span { line, col, len: 1 };
        let c = self.bump().expect("token() wird nur vor einem Zeichen aufgerufen");
        let tok = match c {
            '(' => {
                self.parens += 1;
                Tok::LParen
            }
            ')' => {
                self.parens = self.parens.saturating_sub(1);
                Tok::RParen
            }
            '[' => {
                self.parens += 1;
                Tok::LBracket
            }
            ']' => {
                self.parens = self.parens.saturating_sub(1);
                Tok::RBracket
            }
            '?' => Tok::Question,
            ',' => Tok::Comma,
            ':' => Tok::Colon,
            '+' if self.eat('=') => Tok::PlusEq,
            '+' => Tok::Plus,
            '-' if self.eat('>') => Tok::Arrow,
            '-' if self.eat('=') => Tok::MinusEq,
            '-' => Tok::Minus,
            '*' if self.eat('=') => Tok::StarEq,
            '*' => Tok::Star,
            '/' if self.eat('=') => Tok::SlashEq,
            '/' => Tok::Slash,
            '%' if self.eat('=') => Tok::PercentEq,
            '%' => Tok::Percent,
            '=' if self.eat('=') => Tok::Eq,
            '=' if self.eat('>') => Tok::FatArrow,
            '=' => Tok::Assign,
            '<' if self.eat('=') => Tok::Le,
            '<' => Tok::Lt,
            '>' if self.eat('=') => Tok::Ge,
            '>' => Tok::Gt,
            '!' if self.eat('=') => Tok::Ne,
            // Allein steht `!` nur hinter einem Typ (`int!`); als „nicht“ meldet es der Parser.
            '!' => Tok::Bang,
            '.' if self.eat('.') => Tok::DotDot,
            '.' => Tok::Dot,
            '"' => self.string(first)?,
            '0'..='9' => self.number(start, first)?,
            'a'..='z' | 'A'..='Z' | '_' => {
                while matches!(self.peek(), Some('a'..='z' | 'A'..='Z' | '0'..='9' | '_')) {
                    self.bump();
                }
                let word: String = self.chars[start..self.i].iter().collect();
                match FIXED.iter().find(|(text, _)| *text == word) {
                    Some((_, tok)) => tok.clone(),
                    None => Tok::Ident(word),
                }
            }
            other => {
                let diag = Diag::new(first, format!("unerwartetes Zeichen `{other}`"));
                return Err(match other {
                    '&' | '|' => diag.tip("nutze `and` und `or`"),
                    ';' => diag.tip("Volt braucht kein Semikolon – eine Anweisung pro Zeile"),
                    '{' | '}' => diag.tip("geschweifte Klammern gibt es nur in Strings: `\"Wert: {x}\"`"),
                    _ => diag,
                });
            }
        };
        Ok(Token { tok, span: Span { line, col, len: (self.i - start) as u32 } })
    }

    fn number(&mut self, start: usize, first: Span) -> Result<Tok, Diag> {
        let digits = |lx: &mut Self| {
            while matches!(lx.peek(), Some('0'..='9' | '_')) {
                lx.bump();
            }
        };
        digits(self);
        // `1..5` ist ein Bereich: nur `.` mit einer Ziffer dahinter macht eine Kommazahl.
        let float = self.peek() == Some('.') && matches!(self.chars.get(self.i + 1), Some('0'..='9'));
        if float {
            self.bump();
            digits(self);
        }
        let text: String = self.chars[start..self.i].iter().filter(|c| **c != '_').collect();
        let span = Span { len: (self.i - start) as u32, ..first };
        if float {
            match text.parse::<f64>() {
                Ok(x) if x.is_finite() => Ok(Tok::Float(x)),
                _ => Err(Diag::new(span, "die Zahl ist zu groß für f64")),
            }
        } else {
            text.parse().map(Tok::Int).map_err(|_| Diag::new(span, "die Zahl ist zu groß für int").tip("int reicht bis 9223372036854775807"))
        }
    }

    /// Liest einen String ab dem Zeichen nach dem öffnenden `"`.
    fn string(&mut self, open: Span) -> Result<Tok, Diag> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        loop {
            let at = self.here();
            match self.bump() {
                None | Some('\n') => {
                    return Err(Diag::new(open, "der String ist nicht geschlossen").tip("setze ein `\"` ans Ende"));
                }
                Some('"') => break,
                Some('\\') => lit.push(match self.bump() {
                    Some('n') => '\n',
                    Some('t') => '\t',
                    Some(c @ ('\\' | '"' | '{' | '}')) => c,
                    _ => {
                        return Err(Diag::new(Span { len: 2, ..at }, "dieses Escape gibt es nicht")
                            .tip("erlaubt sind \\n, \\t, \\\\, \\\", \\{ und \\}"));
                    }
                }),
                Some('{') => {
                    let missing = Diag::new(at, "zu `{` fehlt die schließende `}`").tip("für eine echte Klammer im Text schreibe `\\{`");
                    // Ohne `}` im Rest der Zeile würde sonst das schließende `"` als neuer String gelesen.
                    if !self.chars[self.i..].iter().take_while(|c| **c != '\n').any(|c| *c == '}') {
                        return Err(missing);
                    }
                    if !lit.is_empty() {
                        parts.push(StrPart::Lit(std::mem::take(&mut lit)));
                    }
                    let mut code = Vec::new();
                    loop {
                        match self.peek() {
                            Some(' ') => {
                                self.bump();
                            }
                            Some('}') => {
                                self.bump();
                                break;
                            }
                            None | Some('\n') => return Err(missing),
                            Some(_) => code.push(self.token()?),
                        }
                    }
                    if code.is_empty() {
                        return Err(Diag::new(Span { len: 2, ..at }, "die Einfügung `{}` ist leer")
                            .tip("schreibe einen Wert hinein, z. B. `{name}`"));
                    }
                    parts.push(StrPart::Code(code));
                }
                Some(c) => lit.push(c),
            }
        }
        if !lit.is_empty() || parts.is_empty() {
            parts.push(StrPart::Lit(lit));
        }
        Ok(Tok::Str(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokens als kurzer Text: `fn main ( ) NL INDENT …`
    fn show(src: &str) -> String {
        let toks = lex(src).unwrap();
        let words: Vec<String> = toks
            .iter()
            .map(|t| match &t.tok {
                Tok::Newline => "NL".into(),
                Tok::Indent => "INDENT".into(),
                Tok::Dedent => "DEDENT".into(),
                Tok::Eof => "EOF".into(),
                Tok::Str(_) => "STR".into(),
                other => other.to_string().replace('`', ""),
            })
            .collect();
        words.join(" ")
    }

    fn error(src: &str) -> Diag {
        lex(src).unwrap_err()
    }

    #[test]
    fn einrueckung_wird_zu_indent_und_dedent() {
        let src = "fn main()\n    if x\n        a\n    b\nfn f()\n    c";
        assert_eq!(
            show(src),
            "fn main ( ) NL INDENT if x NL INDENT a NL DEDENT b NL DEDENT fn f ( ) NL INDENT c NL DEDENT EOF"
        );
    }

    #[test]
    fn mehrere_bloecke_enden_gleichzeitig() {
        assert_eq!(show("a\n  b\n    c\nd\n"), "a NL INDENT b NL INDENT c NL DEDENT DEDENT d NL EOF");
    }

    #[test]
    fn leere_zeilen_und_kommentare_zaehlen_nicht() {
        let src = "# Kopf\n\na   # dahinter\n\n      \n    # eingerückter Kommentar\nb\r\n";
        assert_eq!(show(src), "a NL b NL EOF");
    }

    #[test]
    fn in_klammern_geht_die_zeile_weiter() {
        assert_eq!(show("f(1,\n      2)\nx\n"), "f ( 1 , 2 ) NL x NL EOF");
    }

    #[test]
    fn zahlen_und_bereiche() {
        assert_eq!(show("1..101"), "1 .. 101 NL EOF");
        assert_eq!(lex("4.5 10_000 0.25").unwrap()[..3].iter().map(|t| t.tok.clone()).collect::<Vec<_>>(), vec![
            Tok::Float(4.5),
            Tok::Int(10_000),
            Tok::Float(0.25)
        ]);
    }

    #[test]
    fn operatoren() {
        assert_eq!(show("a += 1 -> == != <= >= < > = %= .."), "a += 1 -> == != <= >= < > = %= .. NL EOF");
        assert_eq!(show("p.x[0] => int? str! none"), "p . x [ 0 ] => int ? str ! none NL EOF");
        assert_eq!(show("[1,\n  2]"), "[ 1 , 2 ] NL EOF");
    }

    #[test]
    fn strings_mit_escapes_und_einfuegungen() {
        let tok = |src: &str| lex(src).unwrap().remove(0).tok;
        assert_eq!(tok(r#""a\n\"\{x\}""#), Tok::Str(vec![StrPart::Lit("a\n\"{x}".into())]));
        assert_eq!(tok(r#""""#), Tok::Str(vec![StrPart::Lit(String::new())]));
        let Tok::Str(parts) = tok(r#""Hi {name}, {a + 1}!""#) else { panic!() };
        let code = |p: &StrPart| match p {
            StrPart::Code(toks) => toks.iter().map(|t| t.tok.clone()).collect::<Vec<_>>(),
            StrPart::Lit(s) => panic!("Text statt Einfügung: {s}"),
        };
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0], StrPart::Lit("Hi ".into()));
        assert_eq!(code(&parts[1]), vec![Tok::Ident("name".into())]);
        assert_eq!(code(&parts[3]), vec![Tok::Ident("a".into()), Tok::Plus, Tok::Int(1)]);
        assert_eq!(parts[4], StrPart::Lit("!".into()));
    }

    #[test]
    fn string_in_einer_einfuegung() {
        let Tok::Str(parts) = lex(r#""{f("x")} y""#).unwrap().remove(0).tok else { panic!() };
        let StrPart::Code(code) = &parts[0] else { panic!() };
        assert_eq!(code.len(), 4); // f ( "x" )
        assert_eq!(parts[1], StrPart::Lit(" y".into()));
    }

    #[test]
    fn positionen() {
        let toks = lex("ab\n  cd = 12").unwrap();
        let cd = toks.iter().find(|t| t.tok == Tok::Ident("cd".into())).unwrap();
        assert_eq!(cd.span, Span { line: 2, col: 3, len: 2 });
        let n = toks.iter().find(|t| t.tok == Tok::Int(12)).unwrap();
        assert_eq!(n.span, Span { line: 2, col: 8, len: 2 });
    }

    #[test]
    fn fehler_mit_ort_und_tipp() {
        assert_eq!(error("a\n\tb").span, Span { line: 2, col: 1, len: 1 });
        assert!(error("a\n\tb").tip.unwrap().contains("Leerzeichen"));
        assert!(error("a\n    b\n  c").msg.contains("Einrückung"));
        assert!(error("x = \"offen").msg.contains("nicht geschlossen"));
        assert!(error(r#""{}""#).msg.contains("leer"));
        assert!(error(r#""{a""#).msg.contains("schließende"));
        assert!(error(r#""\q""#).msg.contains("Escape"));
        assert!(error("99999999999999999999").msg.contains("zu groß"));
        assert!(error("a; b").tip.unwrap().contains("Semikolon"));
    }

    #[test]
    fn meldung_zeigt_zeile_und_markierung() {
        let text = Diag::new(Span { line: 2, col: 5, len: 5 }, "kaputt").tip("so geht's").render("main.vt", "fn main()\n    score += 1\n");
        assert_eq!(
            text,
            "error: kaputt\n  --> main.vt:2:5\n     |\n   2 |     score += 1\n     |     ^^^^^\n     = tipp: so geht's\n"
        );
    }
}
