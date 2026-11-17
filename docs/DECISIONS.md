# Entscheidungen

1. **C als erstes Backend.** C läuft überall, tcc übersetzt in Millisekunden, gcc und clang optimieren stark. LLVM und ein eigenes Backend kommen später.
2. **Eine Binary `volt`** für run, build, parse und Skripte.
3. **Eigene Endungen `.vt` und `.vts`.** Programme brauchen `main`, Skripte nicht. Beide nutzen denselben Compiler, Volt Script ist also kein Interpreter, sondern wird nativ übersetzt.
4. **Kein Shadowing, `let` ist unveränderlich.** Das verhindert Verwechslungen, und die Fehlermeldung schlägt `var` vor.
5. **`print` ist eingebaut**, bis es eine Standardbibliothek gibt (Phase 2).
6. **`int`-Überlauf läuft über** (`-fwrapv`, tcc macht das ohnehin) statt undefiniert zu sein. Division durch 0 bricht sauber ab.
7. **Strings aus Einfügungen werden nie freigegeben.** Das ist eine bewusste Abkürzung, Ownership und Ref-Counting kommen in Phase 3.
8. **Fehlermeldungen auf Deutsch**, mit Ort, Codeausschnitt und Tipp. Es werden mehrere auf einmal gemeldet.
9. **Debug baut mit tcc, `--release` mit clang/gcc `-O2`.**

## Phase 2
10. **Structs sind Werte, List und Map Referenzen.** Kleine Daten kopieren ist schnell und vorhersehbar; Sammlungen teilen wie in Python/JS. `let` verbietet nur das Neuzuweisen, nicht das Ändern einer Liste.
11. **Enum-Fälle werden mit Typnamen erzeugt** (`Shape.Circle(1.0)`), in Mustern reicht der Fallname – ein offensichtlicher Weg, keine Namenskollisionen zwischen Enums.
12. **Fehler sind Text.** `T!` trägt im Fehlerfall einen `str` (`error("…")`). Eigene Fehlertypen kommen, wenn es Generics gibt.
13. **Kein stilles Ignorieren:** ein `T!`-Aufruf als Anweisung ist ein Fehler. `?` oder `let … else` machen den Fall sichtbar.
14. **Module ohne `pub`:** `use name` lädt `name.vt`, alles darin ist über `name.` erreichbar. Sichtbarkeit kommt, wenn Pakete kommen.
15. **Map-Schlüssel nur `int` oder `str`** (Hash in der Runtime). Andere Schlüssel brauchen Hash/Gleichheit aus Traits (später).
16. **Eigene Fehlerausgabe statt ariadne:** gleiche Qualität (Ort, Markierung, Tipp, mehrere Fehler), keine Abhängigkeit.
17. **Noch nicht drin:** Generics/Traits, `f32`, Default-Parameter von Funktionen, `if … then … else` als Ausdruck – kommen später, wenn sie gebraucht werden.
