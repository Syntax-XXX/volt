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

## Bauen und testen

```sh
cargo test                              # Lexer, Parser, Typen, C-Backend, Beispiele
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
