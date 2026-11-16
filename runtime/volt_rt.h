// Volt-Runtime (Phase 1): Ausgabe, String-Bau und sichere Division.
// Der Compiler setzt diese Datei vor jedes erzeugte C-Programm.
#include <inttypes.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef const char* volt_str;

static void volt_panic(const char* msg, int line) {
    fflush(stdout);
    fprintf(stderr, "panic: %s (Zeile %d)\n", msg, line);
    exit(101);
}

// Kürzeste Schreibweise, die beim Einlesen wieder genau denselben Wert ergibt (wie Python):
// 0.1 statt 0.10000000000000001, aber immer als Kommazahl erkennbar (4.0, nicht 4).
static void volt_f64_text(double v, char buf[32]) {
    if (v != v || v - v != 0) {  // nan, inf
        snprintf(buf, 32, "%g", v);
        return;
    }
    int digits = 0;  // Nachkommastellen in der Schreibweise 1.5e+06
    do snprintf(buf, 32, "%.*e", digits, v);
    while (strtod(buf, NULL) != v && ++digits < 17);
    int exp = atoi(strchr(buf, 'e') + 1);
    if (exp >= -4 && exp < 16) snprintf(buf, 32, "%.*f", digits - exp > 1 ? digits - exp : 1, v);
}

static void volt_print_int(int64_t v) { printf("%" PRId64 "\n", v); }
static void volt_print_bool(bool v) { puts(v ? "true" : "false"); }
static void volt_print_str(volt_str s) { puts(s); }
static void volt_print_f64(double v) {
    char buf[32];
    volt_f64_text(v, buf);
    puts(buf);
}

// Baut einen String aus Teilen. `tags` nennt je Argument den Typ: s = str, i = int, f = f64, b = bool.
// shortcut: der Speicher wird nie freigegeben, Phase 3 (Ownership + Ref-Counting) räumt das auf.
static volt_str volt_fmt(const char* tags, ...) {
    size_t len = 0, cap = 64;
    char* out = malloc(cap);
    va_list ap;
    va_start(ap, tags);
    for (; out && *tags; tags++) {
        char num[32];
        const char* part = num;
        switch (*tags) {
            case 's': part = va_arg(ap, volt_str); break;
            case 'i': snprintf(num, sizeof num, "%" PRId64, va_arg(ap, int64_t)); break;
            case 'f': volt_f64_text(va_arg(ap, double), num); break;
            default: part = va_arg(ap, int) ? "true" : "false";
        }
        size_t n = strlen(part);
        if (len + n + 1 > cap) {
            cap = (len + n + 1) * 2;
            out = realloc(out, cap);
            if (!out) break;
        }
        memcpy(out + len, part, n);
        len += n;
    }
    va_end(ap);
    if (!out) volt_panic("kein Speicher mehr frei", 0);
    out[len] = 0;
    return out;
}

// Ganzzahl-Division: durch null ist ein sauberer Abbruch statt eines Absturzes;
// INT64_MIN / -1 wäre in C undefiniert und läuft hier wie jede andere Rechnung über.
static int64_t volt_div(int64_t a, int64_t b, int line) {
    if (b == 0) volt_panic("Division durch null", line);
    return b == -1 ? (int64_t)(0 - (uint64_t)a) : a / b;
}

static int64_t volt_mod(int64_t a, int64_t b, int line) {
    if (b == 0) volt_panic("Division durch null", line);
    return b == -1 ? 0 : a % b;
}
