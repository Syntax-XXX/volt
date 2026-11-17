# Volt – Sprachstand (Phase 2)

## Dateien
- `.vt`: Programm. Auf oberster Ebene stehen `fn`, `struct`, `enum` und `use`; Start ist `fn main()` (oder `fn main() -> !`).
- `.vts`: Volt Script. Zusätzlich dürfen Anweisungen oben stehen, sie laufen der Reihe nach. `fn main()` ist dort verboten. Eine erste Zeile `#!…` ist ein Kommentar.
- `use name` lädt `name.vt` aus demselben Ordner. Dessen Inhalt heißt dann `name.f(…)`, `name.Typ`, `name.Typ.Fall`.

## Lexik
- Blöcke durch Einrückung (4 Leerzeichen), keine Tabs. Kommentare mit `#`. In `( )` und `[ ]` sind Zeilenumbrüche egal.
- Zahlen `42`, `3.14`, `10_000`; Strings `"…"` mit `\n \t \\ \" \{ \}` und Einfügungen `"x = {x + 1}"`.

## Typen
| Typ | Bedeutung |
|---|---|
| `int`, `f64`, `bool`, `str` | 64-Bit-Ganzzahl (läuft über), Kommazahl, Wahrheitswert, Text |
| `List[T]`, `Map[K, V]` | Referenzen (Kopieren teilt den Inhalt); Schlüssel `int` oder `str` |
| `T?` | Wert oder `none` |
| `T!` | Wert oder Fehler (Text aus `error("…")`); `!` allein = nichts oder Fehler |
| `struct`, `enum` | eigene Typen; Structs sind Werte (Kopie) |

`int` wird automatisch zu `f64`, ein `T` automatisch zu `T?` und `T!`.

## Typen definieren
```
struct Player
    name: str
    hp: int = 100            # Standardwert
    fn hit(mut self, dmg: int)   # mut self darf Felder ändern
        self.hp -= dmg
    fn alive(self) -> bool
        return self.hp > 0

enum Shape
    Circle(radius: f64)
    Rect(w: f64, h: f64)
    Empty

var p = Player("Ana")                 # Felder der Reihe nach …
let q = Player(name: "Bo", hp: 5)     # … oder mit Namen
let s = Shape.Circle(2.0)
```

## Anweisungen
```
let x = 1 / var y: f64 = 0            # unveränderlich / veränderlich, kein Shadowing
y = 2; y += 1; p.hp -= 3; xs[0] = 1; m["a"] = 2
if a … else if b … else …
if let v = optional … else …          # nur wenn ein Wert da ist
let v = f() else err                  # sonst: Block, der return/break/continue macht; err = Fehlertext bei T!
while cond … / for i in 0..n … / for x in liste …
break / continue / return [wert]
defer aufruf(…)                       # läuft beim Verlassen des Blocks – auch bei return, break, continue und `?`;
                                      # mehrere: zuletzt angemeldet zuerst; Werte werden erst dann ausgewertet
match wert
    Circle(r) => print(r)              # Enum-Fall mit Bindung
    3 => …   "ja" => …   true => …     # Literale
    _ => …                             # Rest
```
`match` muss alle Fälle abdecken. Steht es am Ende einer Funktion mit Rückgabewert, sind `Fall => ausdruck` Rückgaben.

## Ausdrücke
`or` · `and` · `not` / Vergleiche · `+ -` · `* / %` · `-x` · Anhängsel `.feld .methode() (…) [i] ?`
- `wert?` gibt `none` bzw. den Fehler an den Aufrufer weiter (die Funktion muss selbst `T?`/`T!` liefern).
- `/` und `%` durch 0, Index außerhalb, fehlender Map-Schlüssel → Abbruch mit Exit 101 und Zeile.
- Ein `T!`-Ergebnis darf nicht still verworfen werden (`f()?` oder `let _ = f() else err`).

## Eingebaut
- Funktionen: `print(x)`, `input(prompt) -> str?`, `error(text)`, `int(f64)`, `float(int)`, `sqrt`, `abs`, `exit(code)`.
- `str`: `len upper lower trim contains starts_with ends_with split replace to_int to_f64`
- `List[T]`: `len push pop remove insert clear contains join`; Literal `[1, 2]`, leer `List[int]()`
- `Map[K, V]`: `len has get remove keys`; `m[k]` lesen/schreiben; leer `Map[str, int]()`
- `T?`/`T!`: `.or(ersatz)`
