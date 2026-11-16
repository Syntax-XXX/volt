# Volt – Sprachstand Phase 1

## Dateien
- `.vt`: Programm. Auf oberster Ebene stehen nur Funktionen, der Start ist `fn main()` ohne Parameter und Rückgabe.
- `.vts`: Volt Script. Funktionen und Anweisungen dürfen gemischt oben stehen, die Anweisungen laufen der Reihe nach. `fn main()` ist dort verboten. Eine erste Zeile `#!…` ist ein Kommentar.
- Andere Endungen lehnt `volt` ab.

## Lexik
- Blöcke durch Einrückung mit genau 4 Leerzeichen; Tabs sind ein Fehler.
- Kommentare mit `#` bis zum Zeilenende. In Klammern sind Zeilenumbrüche egal.
- Zahlen `42`, `3.14`; Strings `"…"` mit Escapes `\n \t \\ \" \{ \}` und Einfügungen `"x = {x + 1}"`.

## Typen
`int` (64 Bit, läuft bei Überlauf über), `f64`, `bool`, `str`. Ein `int` wird automatisch zu `f64`, aber nie umgekehrt.

## Anweisungen
```
let x = 1            # unveränderlich
var y: f64 = 0       # veränderlich, Typ optional
y = y + 1            # auch += -= *= /= %=
if a … else if b … else …
while cond …
for i in 0..n …      # n exklusiv, n wird einmal berechnet
return [wert]
```
Namen dürfen nicht neu vergeben werden (kein Shadowing).

## Ausdrücke (schwächste Bindung zuerst)
`or` · `and` · `not` / `== != < <= > >=` · `+ -` · `* / %` · `-x`
- `/` auf zwei `int` ist Ganzzahl-Division; `/` und `%` durch 0 brechen mit Exit-Code 101 ab.
- `%` nur für `int`. `+` verbindet keine Strings, dafür gibt es Einfügungen.
- Operanden werden von links nach rechts ausgewertet, `and` und `or` werten rechts nur bei Bedarf aus.

## Eingebaut
`print(x)` gibt einen Wert eines beliebigen Typs mit Zeilenumbruch aus.
