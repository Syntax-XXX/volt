// Volt-Runtime (Phase 1): Ausgabe, String-Bau und sichere Division.
// Der Compiler setzt diese Datei vor jedes erzeugte C-Programm.
#include <inttypes.h>
// WebAssembly: kein setjmp, und Variablen liegen nicht im Speicher – ein Stack-Scan wäre unzuverlässig → Müllsammler aus
// (shortcut: Web-Seiten laufen kurz; Upgrade mit Ownership/RC im Compiler).
#ifdef __wasm__
#define VOLT_NO_GC 1
#else
#include <setjmp.h>
#include <pthread.h>
#include <unistd.h>
#endif
#include <math.h>
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

// ---- Speicher: konservativer Mark-and-Sweep-Müllsammler ----
// shortcut: Phase 3 räumt per GC auf statt per Ownership/Ref-Counting im Compiler. Der Sammler sucht Zeiger auf dem Stack,
// in den Registern und in allen lebenden Blöcken (jedes Wort, das in einen Block zeigt, hält ihn am Leben, auch Zeiger ins Innere).
// Läuft nur beim Reservieren (kein Hintergrund-Thread), Pausen ~1 ms je 8 MB neuer Daten. Upgrade: Compiler-seitige Besitzanalyse.
typedef struct { size_t n; size_t mark; } volt_hdr;  // steht vor jedem Block (16 Bytes, Daten bleiben 16-ausgerichtet); mark: Bit 0 = erreicht, Bit 1 = ohne Zeiger (Text)
static volt_hdr** volt_blks;
static size_t volt_nblk, volt_cblk, volt_since, volt_limit = (size_t)8 << 20;
static char* volt_stack_base;

#ifndef VOLT_NO_GC
// Fäden (std/faden.vt): Volt-Code läuft immer nur in einem Faden – wer läuft, hält `volt_gil`. Gewechselt wird nur an
// Blockierstellen (schlafen, warten, `pause`), dort legt der Faden vorher seinen Stack-Bereich und seine Register für den Sammler ab.
typedef struct { pthread_t id; char* base; char* sp; jmp_buf regs; int live; } volt_thr;
static pthread_mutex_t volt_gil = PTHREAD_MUTEX_INITIALIZER;
static volt_thr* volt_thrs[512];
static int volt_nthrs;
static volt_thr* volt_me(void) {
    pthread_t me = pthread_self();
    for (int i = 0; i < volt_nthrs; i++)
        if (volt_thrs[i] && volt_thrs[i]->live && pthread_equal(volt_thrs[i]->id, me)) return volt_thrs[i];
    return 0;
}
#endif

static void volt_gc_init(void* marker) {
    volt_stack_base = (char*)marker + 64;
#ifndef VOLT_NO_GC
    volt_thr* t = calloc(1, sizeof *t);
    t->id = pthread_self();
    t->base = volt_stack_base;
    t->live = 1;
    volt_thrs[volt_nthrs++] = t;
    pthread_mutex_lock(&volt_gil);
#endif
}

static int volt_cmp_blk(const void* a, const void* b) {
    const char *x = (const char*)*(volt_hdr* const*)a, *y = (const char*)*(volt_hdr* const*)b;
    return x < y ? -1 : x > y;
}

static volt_hdr** volt_mstack;
static size_t volt_nmark, volt_cmark;

// Zeigt `w` in einen Block? Dann markieren und zum Durchsuchen vormerken.
static void volt_mark_word(uintptr_t w) {
    size_t lo = 0, hi = volt_nblk;
    while (lo < hi) {
        size_t mid = (lo + hi) / 2;
        uintptr_t start = (uintptr_t)volt_blks[mid];
        if (w < start) hi = mid;
        else if (w >= start + sizeof(volt_hdr) + volt_blks[mid]->n) lo = mid + 1;
        else {
            if (!(volt_blks[mid]->mark & 1)) {
                volt_blks[mid]->mark |= 1;
                if (volt_blks[mid]->mark & 2) return;  // Text enthält keine Zeiger
                if (volt_nmark == volt_cmark) {
                    volt_cmark = volt_cmark ? volt_cmark * 2 : 1024;
                    volt_mstack = realloc(volt_mstack, volt_cmark * sizeof *volt_mstack);
                    if (!volt_mstack) { fprintf(stderr, "panic: kein Speicher mehr frei\n"); exit(101); }
                }
                volt_mstack[volt_nmark++] = volt_blks[mid];
            }
            return;
        }
    }
}

static void volt_mark_range(const char* from, const char* to) {
    from = (const char*)(((uintptr_t)from + 7) & ~(uintptr_t)7);
    for (; from + sizeof(uintptr_t) <= to; from += sizeof(uintptr_t)) volt_mark_word(*(const uintptr_t*)from);
}

// Globale Variablen des Programms: ihr Inhalt zählt als erreichbar (das Programm meldet sie in volt_init an).
static struct { const char* p; size_t n; } volt_roots[512];
static size_t volt_nroots;
static void volt_gc_root(void* p, size_t n) {
    if (volt_nroots < sizeof volt_roots / sizeof *volt_roots) { volt_roots[volt_nroots].p = (const char*)p; volt_roots[volt_nroots++].n = n; }
}

#if defined(__GNUC__) && !defined(__TINYC__)
__attribute__((noinline))
#endif
static void volt_gc_collect(void) {
#ifdef VOLT_NO_GC
    volt_limit = (size_t)-1;
    return;
#else
    jmp_buf regs;
    volatile char top = 0;
    setjmp(regs);  // Register in `regs` auf den Stack retten
#if defined(__GNUC__) && !defined(__TINYC__)
    __builtin_unwind_init();  // alle Callee-Saved-Register in diesen Stackrahmen
#endif
    if (!volt_nblk || !volt_stack_base) return;
    qsort(volt_blks, volt_nblk, sizeof *volt_blks, volt_cmp_blk);
    const char* sp = (const char*)&top < (const char*)&regs ? (const char*)&top : (const char*)&regs;
#ifndef VOLT_NO_GC
    volt_thr* me = volt_me();
    volt_mark_range(sp, me ? me->base : volt_stack_base);
    for (int i = 0; i < volt_nthrs; i++) {  // alle anderen Fäden: ihr abgelegter Stack und ihre Register
        volt_thr* t = volt_thrs[i];
        if (!t || t == me || !t->live || !t->base) continue;
        if (t->sp) volt_mark_range(t->sp, t->base);
        volt_mark_range((const char*)&t->regs, (const char*)&t->regs + sizeof t->regs);
    }
#else
    volt_mark_range(sp, volt_stack_base);
#endif
    for (size_t r = 0; r < volt_nroots; r++) volt_mark_range(volt_roots[r].p, volt_roots[r].p + volt_roots[r].n);
    while (volt_nmark) {
        volt_hdr* h = volt_mstack[--volt_nmark];
        volt_mark_range((const char*)(h + 1), (const char*)(h + 1) + h->n);
    }
    size_t live = 0, j = 0;
    for (size_t i = 0; i < volt_nblk; i++) {
        volt_hdr* h = volt_blks[i];
        if (h->mark & 1) { h->mark &= ~(size_t)1; live += h->n; volt_blks[j++] = h; }
        else free(h);
    }
    volt_nblk = j;
    volt_since = 0;
    volt_limit = live * 2 > ((size_t)8 << 20) ? live * 2 : ((size_t)8 << 20);
#endif
}

// Neuer Block; `old` (aus volt_realloc) wird hineinkopiert, der alte wird beim nächsten Lauf eingesammelt.
// `atomic`: Text ohne Zeiger – wird nicht genullt (der Aufrufer füllt ihn) und beim Sammeln nicht durchsucht.
static void* volt_alloc(void* old, size_t n, int atomic) {
    if (n == 0) n = 1;
    volt_since += n;
    if (volt_since > volt_limit) volt_gc_collect();
    volt_hdr* h = atomic ? malloc(sizeof(volt_hdr) + n) : calloc(1, sizeof(volt_hdr) + n);
    if (!h) { volt_gc_collect(); h = atomic ? malloc(sizeof(volt_hdr) + n) : calloc(1, sizeof(volt_hdr) + n); }
    if (!h) volt_panic("kein Speicher mehr frei", 0);
    h->n = n;
    h->mark = atomic ? 2 : 0;
    if (volt_nblk == volt_cblk) {
        volt_cblk = volt_cblk ? volt_cblk * 2 : 4096;
        volt_blks = realloc(volt_blks, volt_cblk * sizeof *volt_blks);
        if (!volt_blks) volt_panic("kein Speicher mehr frei", 0);
    }
    volt_blks[volt_nblk++] = h;
    if (old) { size_t on = ((volt_hdr*)old - 1)->n; memcpy(h + 1, old, on < n ? on : n); }
    return h + 1;
}
static void* volt_realloc(void* old, size_t n) { return volt_alloc(old, n, 0); }

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
// Ganzzahl → Text ohne snprintf; schreibt rückwärts ans Ende von buf[32] und gibt den Anfang zurück.
static char* volt_itoa(int64_t v, char buf[32]) {
    char* p = buf + 31;
    *p = 0;
    uint64_t u = v < 0 ? 0 - (uint64_t)v : (uint64_t)v;
    do { *--p = (char)('0' + u % 10); u /= 10; } while (u);
    if (v < 0) *--p = '-';
    return p;
}

static volt_str volt_fmt(const char* tags, ...) {
    size_t len = 0, cap = 64;
    char* out = volt_alloc(NULL, cap, 1);
    va_list ap;
    va_start(ap, tags);
    for (; out && *tags; tags++) {
        char num[32];
        const char* part = num;
        switch (*tags) {
            case 's': part = va_arg(ap, volt_str); break;
            case 'i': part = volt_itoa(va_arg(ap, int64_t), num); break;
            case 'f': volt_f64_text(va_arg(ap, double), num); break;
            default: part = va_arg(ap, int) ? "true" : "false";
        }
        size_t n = strlen(part);
        if (len + n + 1 > cap) {
            cap = (len + n + 1) * 2;
            out = volt_alloc(out, cap, 1);
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

// ---- Phase 2: Listen, Maps, String-Funktionen, Eingabe ----


static char* volt_str_alloc(size_t n) {  // n Bytes plus Endnull
    char* s = volt_alloc(NULL, n + 1, 1);
    s[n] = 0;
    return s;
}

// Liste: Elemente liegen am Stück, die Elementgröße gibt der Aufrufer mit.
typedef struct { int64_t len, cap; char* data; } volt_list;

static volt_list* volt_list_new(void) {
    volt_list* l = volt_realloc(NULL, sizeof *l);
    l->len = l->cap = 0;
    l->data = NULL;
    return l;
}

static void* volt_list_push(volt_list* l, size_t size) {
    if (l->len == l->cap) {
        l->cap = l->cap ? l->cap * 2 : 4;
        l->data = volt_realloc(l->data, (size_t)l->cap * size);
    }
    return l->data + (size_t)l->len++ * size;
}

static void* volt_at(volt_list* l, int64_t i, size_t size, int line) {
    if (i < 0 || i >= l->len) volt_panic("Index außerhalb der Liste", line);
    return l->data + (size_t)i * size;
}

static void volt_list_remove(volt_list* l, int64_t i, size_t size, int line, void* out) {
    char* p = volt_at(l, i, size, line);
    memcpy(out, p, size);
    memmove(p, p + size, (size_t)(l->len - i - 1) * size);
    l->len--;
}

static void volt_list_insert(volt_list* l, int64_t i, size_t size, int line, const void* value) {
    if (i < 0 || i > l->len) volt_panic("Index außerhalb der Liste", line);
    volt_list_push(l, size);
    char* p = l->data + (size_t)i * size;
    memmove(p + size, p, (size_t)(l->len - 1 - i) * size);
    memcpy(p, value, size);
}

static volt_str volt_list_join(volt_list* l, volt_str sep) {
    volt_str* it = (volt_str*)l->data;
    size_t n = 0, ns = strlen(sep);
    for (int64_t i = 0; i < l->len; i++) n += strlen(it[i]) + (i ? ns : 0);
    char* out = volt_str_alloc(n);
    char* w = out;
    for (int64_t i = 0; i < l->len; i++) {
        if (i) { memcpy(w, sep, ns); w += ns; }
        size_t k = strlen(it[i]);
        memcpy(w, it[i], k);
        w += k;
    }
    return out;
}

static volt_str volt_str_sub(const char* s, size_t n) {
    char* out = volt_str_alloc(n);
    memcpy(out, s, n);
    return out;
}

// Wie Python str.split(sep): "a,,b" ergibt drei Teile, auch leere.
static volt_list* volt_str_split(volt_str s, volt_str sep) {
    size_t ns = strlen(sep);
    if (!ns) volt_panic("split braucht einen Trenner", 0);
    volt_list* l = volt_list_new();
    for (const char* hit; (hit = strstr(s, sep)); s = hit + ns)
        *(volt_str*)volt_list_push(l, sizeof(volt_str)) = volt_str_sub(s, (size_t)(hit - s));
    *(volt_str*)volt_list_push(l, sizeof(volt_str)) = volt_str_sub(s, strlen(s));
    return l;
}

// Map: Einträge in Einfügereihenfolge (keys/vals), dazu eine Hash-Tabelle mit offener Adressierung.
// Bei str_keys ist der Schlüssel ein volt_str (als Zahl gespeichert), sonst ein int64.
typedef struct volt_map {
    bool str_keys;
    size_t vsize;
    int64_t len, cap, icap;  // icap: Größe der Indextabelle, Zweierpotenz
    uint64_t* keys;
    char* vals;
    int64_t* idx;  // Eintragsnummer oder -1 = leer
} volt_map;

static uint64_t volt_map_hash(volt_map* m, uint64_t key) {
    if (m->str_keys) {  // FNV-1a
        uint64_t h = 14695981039346656037ULL;
        for (const unsigned char* p = (const unsigned char*)(uintptr_t)key; *p; p++) h = (h ^ *p) * 1099511628211ULL;
        return h;
    }
    key *= 0x9E3779B97F4A7C15ULL;
    return key ^ (key >> 32);
}

static bool volt_map_eq(volt_map* m, uint64_t a, uint64_t b) {
    return m->str_keys ? strcmp((volt_str)(uintptr_t)a, (volt_str)(uintptr_t)b) == 0 : a == b;
}

// Position in der Indextabelle: dort steht der Schlüssel oder -1 (freier Platz).
static int64_t volt_map_probe(volt_map* m, uint64_t key) {
    int64_t mask = m->icap - 1, p = (int64_t)(volt_map_hash(m, key) & (uint64_t)mask);
    while (m->idx[p] >= 0 && !volt_map_eq(m, m->keys[m->idx[p]], key)) p = (p + 1) & mask;
    return p;
}

static void volt_map_reindex(volt_map* m, int64_t icap) {
    m->icap = icap;
    m->idx = volt_realloc(m->idx, (size_t)icap * sizeof(int64_t));
    memset(m->idx, 0xff, (size_t)icap * sizeof(int64_t));  // alles -1
    for (int64_t i = 0; i < m->len; i++) m->idx[volt_map_probe(m, m->keys[i])] = i;
}

static volt_map* volt_map_new(bool str_keys, size_t vsize) {
    volt_map* m = volt_realloc(NULL, sizeof *m);
    memset(m, 0, sizeof *m);
    m->str_keys = str_keys;
    m->vsize = vsize;
    volt_map_reindex(m, 8);
    return m;
}

static int64_t volt_map_len(volt_map* m) { return m->len; }

static void* volt_map_find(volt_map* m, uint64_t key) {
    int64_t i = m->idx[volt_map_probe(m, key)];
    return i < 0 ? NULL : m->vals + (size_t)i * m->vsize;
}

static void* volt_map_slot(volt_map* m, uint64_t key) {
    void* v = volt_map_find(m, key);
    if (v) return v;
    if ((m->len + 1) * 3 > m->icap * 2) volt_map_reindex(m, m->icap * 2);  // höchstens 2/3 voll
    if (m->len == m->cap) {
        m->cap = m->cap ? m->cap * 2 : 4;
        m->keys = volt_realloc(m->keys, (size_t)m->cap * sizeof(uint64_t));
        m->vals = volt_realloc(m->vals, (size_t)m->cap * m->vsize);
    }
    m->idx[volt_map_probe(m, key)] = m->len;
    m->keys[m->len] = key;
    v = m->vals + (size_t)m->len++ * m->vsize;
    memset(v, 0, m->vsize);
    return v;
}

static void* volt_map_at(volt_map* m, uint64_t key, int line) {
    void* v = volt_map_find(m, key);
    if (!v) volt_panic("Schlüssel fehlt in der Map", line);
    return v;
}

// shortcut: Entfernen baut den ganzen Index neu auf (O(n)), bei Bedarf auf Tombstones umstellen.
static void volt_map_remove(volt_map* m, uint64_t key) {
    int64_t i = m->idx[volt_map_probe(m, key)];
    if (i < 0) return;
    m->len--;  // letzten Eintrag in die Lücke ziehen
    m->keys[i] = m->keys[m->len];
    memmove(m->vals + (size_t)i * m->vsize, m->vals + (size_t)m->len * m->vsize, m->vsize);
    volt_map_reindex(m, m->icap);
}

static volt_list* volt_map_keys(volt_map* m) {
    volt_list* l = volt_list_new();
    for (int64_t i = 0; i < m->len; i++) *(uint64_t*)volt_list_push(l, sizeof(uint64_t)) = m->keys[i];
    return l;
}

// String-Funktionen geben neue Strings zurück; upper/lower ändern nur ASCII-Buchstaben.
static volt_str volt_str_upper(volt_str s) {
    char* out = (char*)volt_str_sub(s, strlen(s));
    for (char* p = out; *p; p++) if (*p >= 'a' && *p <= 'z') *p -= 32;
    return out;
}

static volt_str volt_str_lower(volt_str s) {
    char* out = (char*)volt_str_sub(s, strlen(s));
    for (char* p = out; *p; p++) if (*p >= 'A' && *p <= 'Z') *p += 32;
    return out;
}

static bool volt_is_space(char c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r'; }

static volt_str volt_str_trim(volt_str s) {
    size_t n = strlen(s);
    while (n && volt_is_space(s[n - 1])) n--;
    while (n && volt_is_space(*s)) s++, n--;
    return volt_str_sub(s, n);
}

static bool volt_str_starts(volt_str s, volt_str p) { return strncmp(s, p, strlen(p)) == 0; }

static bool volt_str_ends(volt_str s, volt_str p) {
    size_t n = strlen(s), k = strlen(p);
    return k <= n && memcmp(s + n - k, p, k) == 0;
}

static volt_str volt_str_replace(volt_str s, volt_str from, volt_str to) {
    size_t nf = strlen(from), nt = strlen(to), n = strlen(s), hits = 0;
    if (!nf) return s;
    for (const char* p = s; (p = strstr(p, from)); p += nf) hits++;
    char* out = volt_str_alloc(n - hits * nf + hits * nt);
    char* w = out;
    for (const char* hit; (hit = strstr(s, from)); s = hit + nf) {
        memcpy(w, s, (size_t)(hit - s));
        w += hit - s;
        memcpy(w, to, nt);
        w += nt;
    }
    strcpy(w, s);
    return out;
}

static bool volt_str_to_int(volt_str s, int64_t* out) {
    const char* p = volt_str_trim(s);
    bool neg = *p == '-';
    if (*p == '-' || *p == '+') p++;
    if (!*p) return false;
    uint64_t v = 0, max = neg ? (uint64_t)INT64_MAX + 1 : (uint64_t)INT64_MAX;
    for (; *p; p++) {
        if (*p < '0' || *p > '9') return false;
        unsigned d = (unsigned)(*p - '0');
        if (v > (max - d) / 10) return false;  // Überlauf
        v = v * 10 + d;
    }
    *out = neg ? (int64_t)(0 - v) : (int64_t)v;
    return true;
}

static bool volt_str_to_f64(volt_str s, double* out) {
    const char* p = volt_str_trim(s);
    char* end;
    double v = strtod(p, &end);
    if (!*p || *end) return false;
    *out = v;
    return true;
}

// Liest eine Zeile beliebiger Länge; false bei Dateiende ohne gelesene Zeichen.
static bool volt_input(volt_str prompt, volt_str* out) {
    fputs(prompt, stdout);
    fflush(stdout);
    size_t len = 0, cap = 64;
    char* buf = volt_alloc(NULL, cap, 1);
    int c;
    while ((c = getchar()) != EOF && c != '\n') {
        if (len + 1 >= cap) buf = volt_alloc(buf, cap *= 2, 1);
        buf[len++] = (char)c;
    }
    if (c == EOF && !len) return false;
    while (len && buf[len - 1] == '\r') len--;
    buf[len] = 0;
    *out = buf;
    return true;
}
