# Entscheidungen

1. **C als erstes Backend.** C läuft überall, tcc übersetzt in Millisekunden, gcc und clang optimieren stark. LLVM und ein eigenes Backend kommen später.
2. **Eine Binary `volt`** für run, build, parse und Skripte.
3. **Eigene Endungen `.vt` und `.vts`.** Programme brauchen `main`, Skripte nicht. Beide nutzen denselben Compiler, Volt Script ist also kein Interpreter, sondern wird nativ übersetzt.
4. **Kein Shadowing, `let` ist unveränderlich.** Das verhindert Verwechslungen, und die Fehlermeldung schlägt `var` vor.
5. **`print` ist eingebaut**, bis es eine Standardbibliothek gibt (Phase 2).
6. **`int`-Überlauf läuft über** (`-fwrapv`, tcc macht das ohnehin) statt undefiniert zu sein. Division durch 0 bricht sauber ab.
7. **Speicher räumt ein konservativer Müllsammler auf** (Phase 3, siehe 18). Zuvor wurden Strings, Listen und Maps nie freigegeben.
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

## Phase 3
18. **Erst ein konservativer Mark-and-Sweep-GC, dann Besitzanalyse.** Ownership + Ref-Counting braucht Analyse im Compiler (Moves, Kopien von Structs mit Strings, Rückgaben, Temporäre) und viel Testfläche. Der Sammler (`runtime/volt_rt.h`) löst das Leck sofort: er sucht Zeiger auf dem Stack, in Registern und in allen lebenden Blöcken (auch Zeiger ins Innere), läuft nur beim Reservieren und braucht keine Änderung am erzeugten Code außer `volt_gc_init` in `main`. Preis: kurze Pausen (~1 ms je 8 MB neue Daten) und kein deterministisches Freigeben – das widerspricht Grundregel 3 und ist deshalb als `shortcut` markiert. Upgrade: Compiler-seitiges RC mit Elision, der Sammler bleibt nur für Zyklen.
19. **Build-Cache:** der erzeugte C-Code (enthält die Runtime) + Compiler-Einstellung + Ziel ergeben einen FNV-1a-Schlüssel in `.volt-cache/<name>.key`; passt er und die Datei existiert, wird nicht neu übersetzt. Der Schlüssel wird erst nach erfolgreichem Bauen geschrieben.
20. **`defer` ist blockweise (wie Zig), nicht funktionsweise (wie Go)** und wertet seinen Aufruf erst beim Verlassen aus. Dadurch passt es in Schleifen (`defer` je Runde) und sieht den aktuellen Wert der Variablen. Es muss ein Aufruf sein; ein `T!`-Ergebnis darf nicht verworfen werden. Umgesetzt im Backend: beim Rückgabewert zuerst auswerten, dann die offenen defers aller umgebenden Blöcke ausführen, dann `return`.
21. **Operator-Überladung als Methoden `op+` usw.** statt eigener Trait-Syntax: Traits gibt es noch nicht, und eine Methode mit festem Namen ist eine einzige, offensichtliche Schreibweise. Der Typchecker schreibt `a + b` zu einem Methodenaufruf um, das Backend merkt davon nichts. Operatoren, die in C-Namen nicht vorkommen dürfen, werden als `_xHH` geschrieben.
22. **`volt run --watch` startet neu, statt Zustand zu erhalten.** Echtes Hot-Reload braucht dynamisches Laden (Shared Library + Zustandsübergabe) und kommt mit dem Spiele-Meilenstein; der Neustart-Weg ist schon nützlich, Fehler stoppen die Schleife nicht.

