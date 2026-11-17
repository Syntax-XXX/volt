# Lerne Volt in einer Stunde

Volt liest sich wie Python und läuft wie C. Alle Beispiele hier sind vollständige Programme: kopieren, in `datei.vt` speichern, mit `volt run datei.vt` starten.

## 1. Hallo, Volt
```volt
fn main()
    print("Hallo, Volt! ⚡")
```
Ein Programm beginnt in `fn main()`. Blöcke werden durch Einrücken (4 Leerzeichen) gebildet, Kommentare beginnen mit `#`.

## 2. Variablen und Typen
```volt
fn main()
    let name = "Ana"          # unveränderlich
    var punkte = 0            # veränderlich
    punkte += 10
    let preis: f64 = 4.5      # Typ ist optional
    let aktiv = true
    print("{name} hat {punkte} Punkte, Preis {preis}, aktiv: {aktiv}")
```
Typen: `int`, `f64`, `bool`, `str`. In Strings steht `{ausdruck}` für einen eingefügten Wert. Ein Name bedeutet immer genau eine Variable (kein Überdecken).

## 3. Funktionen
```volt
fn quadrat(n: int) -> int
    return n * n

fn begruesse(name: str)
    print("Hallo, {name}!")

fn main()
    begruesse("Welt")
    print(quadrat(12))
```

## 4. Bedingungen und Schleifen
```volt
fn main()
    for i in 1..16
        if i % 15 == 0
            print("FizzBuzz")
        else if i % 3 == 0
            print("Fizz")
        else if i % 5 == 0
            print("Buzz")
        else
            print(i)
    var n = 1
    while n < 100
        n *= 2
    print(n)
```
`a..b` läuft von a bis b-1. `break` und `continue` gibt es auch.

## 5. Listen, Maps und Texte
```volt
fn main()
    let zahlen = [3, 1, 2]
    zahlen.push(10)
    print(zahlen.len())
    for z in zahlen
        print(z)

    let alter = Map[str, int]()
    alter["ana"] = 30
    print(alter["ana"])
    print(alter.get("bo").or(0))

    let satz = "  Volt ist schnell  ".trim()
    print(satz.upper())
    for wort in satz.split(" ")
        print(wort)
```
Listen und Maps zeigen auf ihre Daten: wer eine Liste weitergibt, teilt sie.

## 6. Eigene Typen: struct
```volt
struct Spieler
    name: str
    leben: int = 100

    fn treffer(mut self, schaden: int)
        self.leben -= schaden

    fn lebt(self) -> bool
        return self.leben > 0

fn main()
    var s = Spieler("Ana")
    s.treffer(30)
    print("{s.name}: {s.leben}, lebt: {s.lebt()}")
```
`mut self` darf Felder ändern, `self` nur lesen. Structs werden beim Zuweisen kopiert.

## 7. Auswahl: enum und match
```volt
enum Form
    Kreis(radius: f64)
    Rechteck(b: f64, h: f64)
    Punkt

fn flaeche(f: Form) -> f64
    match f
        Kreis(r) => 3.14159 * r * r
        Rechteck(b, h) => b * h
        Punkt => 0.0

fn main()
    print(flaeche(Form.Kreis(2.0)))
    print(flaeche(Form.Rechteck(3.0, 4.0)))
```
`match` muss alle Fälle abdecken – der Compiler sagt dir, welcher fehlt.

## 8. Fehlendes und Fehler: `T?` und `T!`
```volt
fn finde(zahlen: List[int], gesucht: int) -> int?
    for i in 0..zahlen.len()
        if zahlen[i] == gesucht
            return i
    return none

fn teile(a: int, b: int) -> int!
    if b == 0
        return error("durch null")
    return a / b

fn rechne() -> int!
    let x = teile(10, 2)?
    return x + teile(9, 3)?

fn main()
    if let i = finde([5, 6, 7], 7)
        print("gefunden bei {i}")
    let r = rechne() else fehler
        print(fehler)
        return
    print(r)
```
`T?` ist ein Wert oder `none`, `T!` ein Wert oder ein Fehlertext. `?` gibt Fehler nach oben weiter, `let x = … else` fängt sie ab, `.or(ersatz)` gibt einen Ersatzwert.

## 9. Module und Standardbibliothek
```volt
use liste
use text
use mathe

fn main()
    let xs = [5, 2, 9, 1]
    liste.sortiere(xs)
    print(xs[0])
    print(text.fuelle_links("7", 3))
    print(mathe.ggt(12, 18))
```
Eingebaut: `liste`, `text`, `mathe`, `zeit`, `web`. Eigene Module: `use geo` lädt `geo.vt` neben deinem Programm.

## 10. Funktionen als Werte und Closures
```volt
use liste

fn mal(faktor: int) -> fn(int) -> int
    return fn(x: int) -> int => x * faktor

fn main()
    let verdopple = mal(2)
    print(verdopple(21))
    let zahlen = [1, 2, 3, 4]
    print(liste.abbilden(zahlen, fn(x: int) => x * x).len())
    print(liste.falte(zahlen, 0, fn(summe: int, x: int) => summe + x))
```
`fn(x: int) => ausdruck` ist eine Funktion ohne Namen; sie nimmt benutzte Variablen als Kopie mit.

## 11. Generics und Traits
```volt
trait Flaeche
    fn flaeche(self) -> f64

struct Quadrat
    seite: f64

    fn flaeche(self) -> f64
        return self.seite * self.seite

fn groesser[T](a: T, b: T) -> T
    if a < b
        return b
    return a

fn zeige[T: Flaeche](x: T)
    print(x.flaeche())

struct Kiste[T]
    inhalt: T

fn main()
    print(groesser(3, 9))
    print(groesser("a", "b"))
    zeige(Quadrat(3.0))
    let k = Kiste("Überraschung")
    print(k.inhalt)
```

## 12. Aufräumen und Testen
```volt
fn quadrat(n: int) -> int
    return n * n

fn main()
    defer print("fertig")
    print(quadrat(7))

test "quadrat"
    assert(quadrat(3) == 9)
```
`defer` läuft beim Verlassen des Blocks. `volt test datei.vt` führt alle `test`-Blöcke aus.

## 13. C-Bibliotheken
```volt
import c "math.h" as m

fn main()
    print(m.hypot(3.0, 4.0))
    print(m.M_PI)
```
Funktionen und Zahlenkonstanten aus C-Headern stehen direkt zur Verfügung.

## 14. Im Browser
```volt
use web

var zaehler = 0

fn erhoehen()
    zaehler += 1
    web.setze_text("#zahl", "{zaehler}")

fn main()
    web.setze_html("#app", "<p id='zahl'>0</p><button id='knopf'>+1</button>")
    web.klick("#knopf", erhoehen)
```
```sh
volt serve zaehler.vt        # öffnet http://localhost:8000, lädt bei jeder Änderung neu
volt build zaehler.vt --target web
```

## Weiter
Die ganze Sprache steht in [SPEC.md](SPEC.md), die Entscheidungen dahinter in [DECISIONS.md](DECISIONS.md), Beispiele in `examples/`. Für Editoren gibt es `volt lsp` und das VS-Code-Addon in `editors/vscode/`.
