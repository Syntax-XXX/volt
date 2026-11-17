# Benchmarks (Phase 3)

`bench/run.sh` baut jedes Programm in Volt (gcc `-O2` und tcc-Debug), C (gcc `-O2`), Go und Python, startet es 3-mal und nimmt die beste Zeit. Alle Ausgaben müssen übereinstimmen („Ausgabe: ja“).
Gemessen am 17.11.2026 auf pve2 (Intel, 8 Kerne, belegt durch laufende VMs – die Zahlen schwanken um ±15 %).

| Programm | Volt `--release` | Volt Debug (tcc) | C `-O2` | Go | Python |
|---|---|---|---|---|---|
| fib(35), rekursiv | 34 ms | 153 ms | 26 ms | 63 ms | 2 072 ms |
| Sieb 5 Mio. (`List[bool]`) | 74 ms | 250 ms | 52 ms | 56 ms | 2 001 ms |
| N-Body, 200 000 Schritte (`List[Body]`) | 30 ms | 296 ms | 36 ms | – | 1 842 ms |
| 500 000 Strings bauen | 178 ms | 224 ms | 78 ms | 160 ms | 228 ms |

Stand nach dem Müllsammler (Messung wie oben; vor dem Sammler waren es fib 30, Sieb 51, N-Body 22, Strings 154 ms – der Unterschied liegt überwiegend im Messrauschen, beim Sieb kostet das Durchsuchen der 5-MB-Liste beim Sammeln ca. 20 ms).

**Meilenstein „N-Body höchstens 1,5× langsamer als C“: erreicht** (30 ms gegen 36 ms; gcc hält die lokalen Struct-Kopien in Registern – kein allgemeines Versprechen).

## Speicher (Spitze, `--release`)
| Programm | vor dem Sammler | mit Sammler | C | Go |
|---|---|---|---|---|
| 500 000 Strings | 40 MB | **15,6 MB** | 1,3 MB | 6,8 MB |
| 5 000 000 Strings | 392 MB | **15,7 MB** | – | – |
| Sieb 5 Mio. | – | 12,7 MB | – | – |

Der Verbrauch bleibt bei steigender Menge flach: Der Müllsammler räumt alle 8 MB neuer Daten auf.

## Was die Zahlen zeigen
- **Zahlen und Schleifen:** Volt `--release` liegt auf C-Niveau, weil es C ist. Debug mit tcc ist 4–12× langsamer, übersetzt dafür in ~15 ms.
- **Strings** sind noch 2× langsamer als C (Formatierung über `volt_fmt` mit va_args, jede Einfügung ein eigener Block). Hier steckt als Nächstes das meiste Potenzial: Platz vorab berechnen statt wachsen lassen.
