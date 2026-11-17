# Volt ⚡

Eine eigene Programmiersprache: lesbar wie Python, schnell wie C. Der Compiler ist in Rust geschrieben und erzeugt C, das tcc, gcc oder clang zu einem nativen Programm machen.

| Endung | Was | Start |
|---|---|---|
| `.vt`  | Volt-Programm, beginnt in `fn main()` | `volt run prog.vt` |
| `.vts` | **Volt Script**: Code steht direkt oben und läuft von oben nach unten, wie JavaScript oder Python | `volt skript.vts` oder `./skript.vts` mit `#!/usr/bin/env volt` |

```volt
#!/usr/bin/env volt
fn quadrat(n: int) -> int
    return n * n

var summe = 0
for i in 1..101
    summe += quadrat(i)
print("Summe = {summe}")
```

## Was geht schon (Phase 2)

`struct` mit Methoden, `enum` mit Daten und `match`, Optionals `T?`, Fehler `T!` mit `?`, `List`, `Map`, String-Funktionen, Module mit `use`. Meilenstein: [examples/abenteuer.vt](examples/abenteuer.vt), ein Textadventure komplett in Volt (`volt run examples/abenteuer.vt`).

## Web (Phase 5, Stand)

`volt build x.vt --target web` baut aus einem Volt-Programm eine fertige Seite (WebAssembly + `index.html`), `volt serve x.vt` liefert sie aus und lädt bei jeder Änderung neu. `use web` verbindet das Programm mit der Seite (`examples/web_zaehler.vt`, `examples/web_todo.vt` – beide im echten Browser getestet). Braucht clang mit WebAssembly-Unterstützung. Noch offen: Komponenten-Syntax mit HTML-Vorlagen (braucht Funktionswerte).

## Editor

`editors/vscode/`: VS-Code-Addon (Syntaxfarben, Fehler beim Tippen, Vervollständigung, Hover, Snippets, Formatieren, Befehle zum Ausführen/Testen). Bauen: `cd editors/vscode && npm install && python3 package-vsix.py`, installieren: `code --install-extension volt-lang-0.1.0.vsix`.

## Werkzeuge

`volt lsp` ist ein Language-Server (Fehler beim Tippen, Vervollständigung, Hover) für Editoren.

`volt run` (mit `--watch`), `build [--release]`, `check` (nur prüfen), `test` (test-Blöcke mit `assert`), `fmt` (Leerraum aufräumen), `new <name> --template cli|game|lib`, `parse --ast`.

## Spiele und C (Phase 4, Stand)

`import c "math.h" as m` macht Funktionen und Zahlenkonstanten jedes C-Headers nutzbar (`examples/c_mathe.vt`), Operator-Überladung (`op +`) gibt es für eigene Typen (`examples/vektoren.vt`), `volt run --watch` baut bei jeder Änderung neu, und `examples/snake.vt` ist ein Snake im Terminal, das sich selbst spielt (läuft im Terminal mit Animation). Noch offen: Structs aus Headern (nötig für raylib), `std.math` als Bibliothek, `volt new --template game`.

## Speicher und Tempo (Phase 3, Stand)

Ein Müllsammler in der Runtime gibt Strings, Listen und Maps frei (5 Mio. Strings: 16 MB statt 392 MB), ein Build-Cache übersetzt unveränderte Programme nicht neu (5 ms statt 22 ms), und `bench/run.sh` vergleicht Volt mit C, Go und Python – Ergebnisse in [docs/BENCHMARKS.md](docs/BENCHMARKS.md). Noch offen: `defer`, Ownership statt GC, parallele Übersetzung.

## Bauen und testen

```sh
cargo test                              # Lexer, Parser, Typen, C-Backend + alle Programme in examples/ und tests/
cargo install --path cli                # `volt` nach ~/.cargo/bin
volt run examples/fib.vt
volt examples/hello.vts
volt parse examples/fizzbuzz.vt --ast   # Syntaxbaum
volt build examples/fib.vt --release -o fib
```

Gebraucht wird ein C-Compiler: tcc (am schnellsten zu bauen), gcc oder clang. Mit `VOLT_CC` legst du ihn selbst fest.

## Wie schnell?

`fib(35)` rekursiv, gemessen auf pve2 inklusive Kompilieren und Start:

| | Zeit |
|---|---|
| Volt Script (`volt fib.vts`, tcc) | 135 ms |
| Volt `--release` (gcc -O2, nur Laufzeit) | 40 ms |
| Node.js | 285 ms |
| Python (hochgerechnet aus fib(30)) | ≈ 2 200 ms |

Ein kleines Skript ist in rund 15 ms übersetzt und gestartet.

Mehr: [docs/SPEC.md](docs/SPEC.md) (Sprache), [docs/DECISIONS.md](docs/DECISIONS.md) (warum so), [docs/VISION.md](docs/VISION.md) (das große Ziel).
