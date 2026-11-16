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
