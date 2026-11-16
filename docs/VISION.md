# Volt – Vision und Roadmap

Der ursprüngliche Auftrag, unverändert. Die Sprachreferenz für das, was schon gebaut ist, steht in [SPEC.md](../SPEC.md).

---

XXL-PROMPT: Baue die Programmiersprache „Volt“

Kopiere diesen ganzen Prompt in einen KI-Coding-Agent (z. B. Claude Code) oder nutze ihn als SPEC.md in deinem Repo.
Arbeite die Phasen nacheinander ab. Jede Phase muss laufen und getestet sein, bevor die nächste beginnt.

0. Deine Rolle

Du bist ein erfahrener Compiler-Entwickler und Sprachdesigner (Erfahrung mit Rust, Zig, Nim, Go, Swift, LLVM, WebAssembly).
Du baust mit mir Volt, eine neue Programmiersprache. Du schreibst sauberen, getesteten, gut dokumentierten Code, triffst pragmatische Entscheidungen und erklärst kurz, warum. Wenn etwas unklar ist, nimmst du die einfachste sinnvolle Lösung und schreibst sie in DECISIONS.md.

1. Die Vision (der Vibe)

Volt = so einfach wie Python, so schnell wie C++, kompiliert so schnell wie Go.

	•	Ein Anfänger schreibt nach einem Nachmittag sein erstes Spiel.
	•	Ein Profi kann bis auf die Hardware runter optimieren, wenn er will.
	•	Eine Sprache für alles: Games, Websites (über WebAssembly), CLI-Tools, Server.
	•	Kompilieren fühlt sich sofort an: Debug-Builds in unter 1 Sekunde, auch bei großen Projekten.
	•	Fehlermeldungen sind freundlich, auf den Punkt und mit Lösungsvorschlag (wie bei Elm/Rust).

Die 7 Grundregeln (bei jeder Entscheidung prüfen!)

	1.	Einfach by default, mächtig on demand. Das Low-Level-Zeug (Pointer, manueller Speicher, SIMD) gibt es, aber man sieht es erst, wenn man es braucht.
	2.	Ein offensichtlicher Weg. Kein Feature doppelt. Lieber 25 Keywords als 90.
	3.	Kein Garbage-Collector-Ruckeln. Speicher wird deterministisch freigegeben → keine Lags in Spielen.
	4.	Statisch typisiert, aber man merkt es kaum. Typinferenz überall, wo es geht.
	5.	Keine Header, keine Makro-Hölle, kein Build-System-Chaos. Ein Befehl: volt run.
	6.	Direkt mit C reden. Jede C-Bibliothek (raylib, SDL, OpenGL, SQLite) ist sofort nutzbar.
	7.	Tooling ist Teil der Sprache. Formatter, Paketmanager, Tests, LSP – alles in einer Binary.

⚠️ „Mix aus allen Sprachen“ heißt: die besten Ideen klauen, nicht alle Features stapeln. Wenn ein Feature die Sprache schwerer macht, fliegt es raus.

2. Woher die Ideen kommen

Idee	Geklaut von	Warum
Einrückungs-Syntax, lesbar wie Englisch	Python, Nim	Einfach zu lernen
let / var, Typinferenz	Swift, Kotlin	Sicher und kurz
Structs + Methoden statt Klassen-Vererbung	Go, Rust	Einfacher, schneller
enum mit Daten + match	Rust, Swift	Mächtig und sicher
Fehler als Werte mit ?	Rust, Zig	Keine versteckten Exceptions
Optionals T? statt null-Crashes	Swift, Kotlin	Keine NullPointer-Fehler
Speicher per Ownership + automatisches Ref-Counting	Swift, Nim ORC, Lobster	Kein GC, keine Lags
comptime-Code zur Kompilierzeit	Zig	Statt Templates/Makros
async / spawn / Channels	Go, Kotlin	Einfache Nebenläufigkeit
Komponenten mit HTML-Syntax	JSX, Svelte	Websites bauen
Direkter C-Import	Zig	Riesiges Ökosystem ab Tag 1
Eine Binary für alles	Go, Cargo, Bun	Kein Setup-Stress

3. So sieht Volt aus

3.1 Hello World

fn main()
    print("Hallo, Volt! ⚡")

3.2 Variablen, Typen, Funktionen

let name = "Syntax"          # unveränderlich, Typ wird erkannt (str)
var score = 0                # veränderlich (int)
let speed: f32 = 4.5         # Typ optional angeben

fn add(a: int, b: int) -> int
    return a + b

fn greet(name: str, loud = false)       # Default-Parameter
    let msg = "Hi {name}!"               # String-Interpolation
    print(if loud then msg.upper() else msg)

3.3 Structs und Methoden

struct Vec2
    x: f32
    y: f32

    fn len(self) -> f32
        return sqrt(self.x * self.x + self.y * self.y)

    op +(self, o: Vec2) -> Vec2          # Operator-Überladung
        return Vec2(self.x + o.x, self.y + o.y)

struct Player
    name: str
    hp: int = 100
    pos: Vec2 = Vec2(0, 0)

    fn hit(mut self, dmg: int)
        self.hp -= dmg
        if self.hp <= 0
            print("{self.name} ist besiegt")

3.4 Enums, Match, Optionals, Fehler

enum Shape
    Circle(radius: f32)
    Rect(w: f32, h: f32)

fn area(s: Shape) -> f32
    match s
        Circle(r)  => 3.14159 * r * r
        Rect(w, h) => w * h

fn find(id: int) -> Player?              # kann "none" sein
    ...

fn load_level(path: str) -> Level!       # "!" = kann Fehler werfen (Result)
    let text = fs.read(path)?            # "?" gibt Fehler nach oben weiter
    return Level.parse(text)?

fn main()
    if let p = find(42)
        print(p.name)
    let lvl = load_level("lvl1.json") else err
        print("Fehler: {err}")
        return

3.5 Generics, Traits, Comptime

trait Drawable
    fn draw(self)

fn max[T: Ord](a: T, b: T) -> T
    return if a > b then a else b

comptime fn make_table() -> [256]u8      # läuft beim Kompilieren
    var t: [256]u8
    for i in 0..256
        t[i] = (i * 7) % 256
    return t

const LOOKUP = make_table()

3.6 Nebenläufigkeit

async fn fetch(url: str) -> str!
    return http.get(url).await?.text()

fn main()
    let ch = Channel[int]()
    spawn
        for i in 0..10
            ch.send(i)
        ch.close()
    for v in ch
        print(v)

3.7 Power-Mode (nur wenn man ihn braucht)

unsafe
    let buf = alloc[u8](1024)            # manueller Speicher
    defer free(buf)
    let p: *u8 = buf.ptr
    p[0] = 255

arena frame_mem                          # super schneller Frame-Speicher für Games
    let particles = List[Particle](capacity: 10_000)
    ...                                  # alles wird am Blockende auf einmal freigegeben

@simd fn mix(a: [4]f32, b: [4]f32) -> [4]f32
    return a * 0.5 + b * 0.5

3.8 Game mit raylib (C-Import)

import c "raylib.h" as rl

fn main()
    rl.InitWindow(800, 450, "Volt Game ⚡")
    rl.SetTargetFPS(60)
    var x = 400.0
    while not rl.WindowShouldClose()
        if rl.IsKeyDown(rl.KEY_RIGHT) then x += 4
        if rl.IsKeyDown(rl.KEY_LEFT)  then x -= 4
        rl.BeginDrawing()
        rl.ClearBackground(rl.BLACK)
        rl.DrawCircle(int(x), 225, 30, rl.GREEN)
        rl.EndDrawing()
    rl.CloseWindow()

3.9 Website (kompiliert zu WebAssembly)

use web

component Counter
    state count = 0

    view
        <div class="card">
            <h1>Zähler: {count}</h1>
            <button on:click={count += 1}>+1</button>
            {if count > 10}
                <p>Du bist schnell! ⚡</p>
        </div>

fn main()
    web.mount(Counter(), "#app")

volt build --target web → erzeugt index.html, app.wasm und eine kleine JS-Brücke (unter 5 KB).

4. Speichermodell (das Herz der Sprache)

Ziel: sicher wie Python, schnell wie C++, ohne GC-Pausen.

	1.	Werte-Typen (int, f32, Vec2, kleine Structs) liegen auf dem Stack und werden kopiert.
	2.	Heap-Objekte (List, str, Map, große Structs) haben einen Besitzer. Übergeben = verschieben (Move).
	3.	Teilt man ein Objekt, nutzt der Compiler automatisches Ref-Counting. Ref-Counts, die nicht nötig sind, entfernt er (wie Swift/Lobster).
	4.	Für Zyklen gibt es weak-Referenzen und optional einen Zyklus-Sammler, den man ausschalten kann (--no-cycle-collector).
	5.	arena-Blöcke und unsafe mit eigenen Allokatoren für Game-Profis.
	6.	Im Debug-Modus prüft der Compiler Array-Grenzen und Use-after-free. Im Release-Modus lassen sich diese Checks gezielt abschalten (@no_bounds_check).

5. Compiler-Architektur

Implementierungssprache: Rust (Gründe: Sicherheit, schnell, gute Crates wie cranelift, logos, ariadne, tower-lsp).

Quellcode (.vt)
   │
   ▼
[Lexer] ──► Tokens (inkl. INDENT/DEDENT)
   │
   ▼
[Parser] ──► AST  (fehlertolerant: sammelt mehrere Fehler)
   │
   ▼
[Resolver] ──► Namen, Imports, Module
   │
   ▼
[Typchecker] ──► Typinferenz (bidirektional), Generics, Traits
   │
   ▼
[Ownership-Analyse] ──► Moves, Borrows, wo RC nötig ist
   │
   ▼
[VIR – Volt IR] ──► einfache SSA-Zwischensprache, Optimierungen
   │
   ├──► Backend 1: C-Code    → tcc (Debug, blitzschnell) / clang -O2 (Release)
   ├──► Backend 2: Cranelift → nativer Debug-Code ohne externen Compiler
   └──► Backend 3: WASM      → Websites (über clang --target=wasm32 oder direkt)

Warum zuerst ein C-Backend?

	•	In wenigen Wochen funktionsfähig.
	•	Läuft sofort auf Linux, Windows, macOS, WebAssembly, ARM.
	•	tcc kompiliert C in Millisekunden → Debug-Builds fühlen sich sofort an.
	•	clang -O2 liefert C++-Geschwindigkeit für Release-Builds.
	•	C-Interop ist gratis.

So wird das Kompilieren schnell

	•	Kein Header-System: Jedes Modul wird genau einmal geparst.
	•	Parallel: Module werden über alle CPU-Kerne verteilt geparst und geprüft.
	•	Inkrementell: Ein Hash pro Modul. Geänderte Module neu bauen, alles andere aus dem Cache (.volt-cache/).
	•	Generics-Cache: Jede Instanz (List[int]) wird nur einmal erzeugt und dann wiederverwendet.
	•	Debug = wenig Optimierung, Release = alles optimieren.
	•	Zielwerte: 100.000 Zeilen Debug-Build unter 1 Sekunde, inkrementeller Rebuild unter 100 ms.

6. Das volt-Tool (eine Binary für alles)

volt new mygame --template game     # Templates: game, web, cli, server, lib
volt run                             # kompilieren + starten
volt run --watch                     # bei Änderung neu starten (Hot Reload für Games)
volt build --release                 # optimierte Binary
volt build --target web              # WebAssembly + HTML
volt build --target windows          # Cross-Compile
volt test                            # alle Tests (`test "name"`-Blöcke im Code)
volt fmt                             # Code formatieren (keine Optionen, ein Stil)
volt add raylib                      # Paket hinzufügen
volt check                           # nur Typcheck, ultraschnell
volt lsp                             # Language-Server für VS Code / Neovim
volt repl                            # interaktiv ausprobieren
volt doc                             # Doku-Website erzeugen

Projektdatei volt.toml:

[project]
name = "mygame"
version = "0.1.0"

[deps]
raylib = "5.0"

7. Standardbibliothek (klein, aber stark)

	•	std.io, std.fs, std.path, std.os, std.time
	•	std.math (Vec2/3/4, Mat4, Quat, Random – wichtig für Games!)
	•	std.collections (List, Map, Set, Deque)
	•	std.str, std.fmt, std.json
	•	std.net, std.http (Client und einfacher Server)
	•	std.async, std.thread, std.sync
	•	std.web (DOM, Komponenten, fetch, nur bei Web-Target)
	•	std.test

Offizielle Pakete (nicht in std): volt-raylib, volt-sdl, volt-gl, volt-sqlite, volt-ui.

8. Fehlermeldungen (Beispiel-Qualität)

error[E0142]: `score` ist unveränderlich
  ┌─ main.vt:4:5
  │
2 │     let score = 0
  │         ----- hier mit `let` erstellt
3 │     ...
4 │     score += 10
  │     ^^^^^^^^^^^ kann nicht geändert werden
  │
  = tipp: nutze `var score = 0`, wenn sich der Wert ändern soll

Regeln: immer Datei + Zeile + Spalte, markierter Code, ein konkreter Tipp, verständliche Sprache. Fehlermeldungen auf Englisch und Deutsch (--lang de).

9. Roadmap (genau so umsetzen)

Phase 1 – Fundament (Woche 1–2)

	•	Rust-Projekt voltc mit Workspace-Struktur (lexer, parser, types, ir, codegen_c, cli)
	•	Lexer mit Einrückung → INDENT/DEDENT, Zahlen, Strings mit {}-Interpolation, Kommentare
	•	Parser für: fn, let/var, if/else, while, for in, return, Ausdrücke mit Präzedenz
	•	Typen: int, f64, bool, str
	•	C-Backend + Aufruf von tcc/clang
	•	volt run hello.vt funktioniert
	•	✅ Meilenstein: FizzBuzz und Fibonacci laufen

Phase 2 – Echte Sprache (Woche 3–5)

	•	struct, Methoden, enum mit Daten, match
	•	Typinferenz, Optionals T?, Results T! mit ?
	•	List, Map, String-Funktionen
	•	Module und use
	•	Schöne Fehlermeldungen (Crate ariadne)
	•	Test-Suite: tests/*.vt mit erwarteter Ausgabe (Snapshot-Tests)
	•	✅ Meilenstein: Ein Textadventure läuft komplett in Volt

Phase 3 – Speicher & Speed (Woche 6–8)

	•	Ownership-Analyse, Moves, automatisches RC, RC-Elision
	•	defer, unsafe, Pointer, arena
	•	Inkrementeller Build-Cache, Parallel-Kompilierung
	•	Benchmarks gegen C, Python und Go (bench/)
	•	✅ Meilenstein: N-Body-Benchmark höchstens 1,5× langsamer als C

Phase 4 – Games 🎮 (Woche 9–10)

	•	import c "header.h": C-Header automatisch einlesen (über libclang oder einen eigenen einfachen Parser)
	•	std.math mit Vektoren und Matrizen, Operator-Überladung
	•	Paket volt-raylib + Template volt new --template game
	•	volt run --watch mit Hot Reload
	•	✅ Meilenstein: Ein kleines Snake- oder Pong-Spiel mit raylib in unter 100 Zeilen

Phase 5 – Web 🌐 (Woche 11–13)

	•	WASM-Target
	•	component-Syntax mit HTML, state, Events, reaktive Updates (fein-granular wie Svelte/Solid, kein Virtual DOM)
	•	volt build --target web erzeugt eine fertige Seite
	•	Dev-Server mit Live-Reload: volt serve
	•	✅ Meilenstein: Todo-App und Counter laufen im Browser

Phase 6 – Profi-Features (Woche 14–17)

	•	Generics mit Traits, comptime
	•	async/await, spawn, Channels, Threads
	•	@simd, Inline-Hints
	•	Cranelift-Backend für Debug-Builds ohne C-Compiler
	•	✅ Meilenstein: HTTP-Server mit 10.000 gleichzeitigen Verbindungen

Phase 7 – Ökosystem (laufend)

	•	Paketmanager mit Registry (Git-basiert, später eigene Seite)
	•	LSP: Autovervollständigung, Hover, Go-to-Definition, Inline-Fehler
	•	VS-Code-Extension mit Syntax-Highlighting
	•	volt fmt, volt doc, volt repl
	•	Website volt-lang.dev (natürlich in Volt gebaut 😎) mit Playground im Browser
	•	Tutorial „Lerne Volt in 1 Stunde“ + „Dein erstes Spiel in Volt“

10. Qualitätsregeln für dich (den Agent)

	•	Jede Phase hat Tests, bevor du weitermachst. cargo test muss grün sein.
	•	Für jedes Sprachfeature gibt es mindestens ein Beispiel in examples/.
	•	Neue Designentscheidungen kommen in DECISIONS.md (Was? Warum? Alternative?).
	•	Halte SPEC.md (Sprachreferenz) immer aktuell.
	•	Keine Features „auf Vorrat“. Erst bauen, wenn eine Phase sie braucht.
	•	Performance messen, nicht raten: bench/ mit hyperfine.
	•	Commits klein und mit klarer Nachricht.
	•	Wenn du feststeckst: Ansatz vereinfachen, nicht weiter verkomplizieren.

11. Repo-Struktur

volt/
├── compiler/
│   ├── lexer/
│   ├── parser/
│   ├── resolve/
│   ├── types/
│   ├── ownership/
│   ├── ir/
│   ├── codegen_c/
│   ├── codegen_cranelift/
│   └── codegen_wasm/
├── cli/              # die `volt`-Binary
├── std/              # Standardbibliothek (in Volt geschrieben)
├── runtime/          # kleine C-Runtime (RC, Allocator, Panic)
├── lsp/
├── examples/         # hello, fizzbuzz, snake, todo-web, http-server
├── tests/
├── bench/
├── docs/
├── SPEC.md
├── DECISIONS.md
└── README.md

12. Erster Auftrag

Starte jetzt mit Phase 1:

	1.	Lege die Repo-Struktur an.
	2.	Schreibe den Lexer mit INDENT/DEDENT und Tests.
	3.	Schreibe den Parser für den Phase-1-Umfang und gib den AST mit volt parse file.vt --ast aus.
	4.	Baue das C-Backend und volt run.
	5.	Lass examples/hello.vt, examples/fizzbuzz.vt und examples/fib.vt laufen.
	6.	Zeig mir am Ende: welche Dateien entstanden sind, wie ich es teste und was in Phase 2 als Nächstes kommt.

Los geht's. ⚡
