# Benchmarks (Phase 3)

`bench/run.sh` baut jedes Programm in Volt (gcc `-O2` und tcc-Debug), C (gcc `-O2`), Go und Python, startet es 3-mal und nimmt die beste Zeit. Alle Ausgaben müssen übereinstimmen („Ausgabe: ja“).
Gemessen am 17.11.2026 auf pve2 (Intel, 8 Kerne, belegt durch laufende VMs – die Zahlen schwanken um ±15 %).

| Programm | Volt `--release` | Volt Debug (tcc) | C `-O2` | Go | Python |
|---|---|---|---|---|---|
| fib(35), rekursiv | 30 ms | 120 ms | 28 ms | 58 ms | 1 995 ms |
| Sieb 5 Mio. (`List[bool]`) | 51 ms | 210 ms | 47 ms | 57 ms | 1 712 ms |
| N-Body, 200 000 Schritte (`List[Body]`) | 22 ms | 272 ms | 37 ms | – | 1 741 ms |
| 500 000 Strings bauen | 154 ms | 192 ms | 74 ms | 138 ms | 216 ms |

**Meilenstein „N-Body höchstens 1,5× langsamer als C“: erreicht** (hier sogar schneller als das handgeschriebene C, weil gcc die lokalen Struct-Kopien in Registern hält; das ist kein allgemeines Versprechen).

## Was die Zahlen zeigen
- **Zahlen und Schleifen:** Volt `--release` liegt auf C-Niveau, weil es C ist. Debug mit tcc ist 4–12× langsamer, übersetzt dafür in ~15 ms.
- **Strings sind die Schwachstelle:** 154 ms gegen 74 ms C und die Spitze beim Speicher: 40 MB bei 500 000 Strings (C: 1,3 MB, Go: 6,8 MB), weil Strings nie freigegeben werden. Bei 5 Mio. Strings wächst das auf ~400 MB. Das ist der Grund für die Speicherverwaltung als nächsten Schritt.
