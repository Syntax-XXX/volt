// Dateien, Zeit, Zufall und Hashes für Volt (std/datei.vt). Nur einfache Typen (Zahlen, Text), damit der C-Import sie lesen kann.
// Blockierende oder lange Arbeit gibt die gemeinsame Sperre frei, damit andere Fäden weiterlaufen. Nicht für WebAssembly.
#ifndef VOLT_FS_H
#define VOLT_FS_H
#include "volt_thread.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <ctype.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <dirent.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <time.h>
#include <stdint.h>

#define VOLT_FS_BLOCK_BEGIN volt_thr* volt_fs_me = volt_me(); char volt_fs_marker; volt_block_begin(volt_fs_me, &volt_fs_marker)
#define VOLT_FS_BLOCK_END volt_block_end(volt_fs_me)

static const char* volt_fs_text(const char* s, size_t n) {
    char* out = volt_str_alloc(n);
    memcpy(out, s, n);
    return out;
}

// 0 = gibt es nicht, 1 = Datei, 2 = Ordner
static long long volt_fs_art(const char* p) {
    struct stat st;
    if (stat(p, &st) != 0) return 0;
    return S_ISDIR(st.st_mode) ? 2 : 1;
}
static long long volt_fs_groesse(const char* p) { struct stat st; return stat(p, &st) == 0 ? (long long)st.st_size : -1; }
static long long volt_fs_geaendert(const char* p) { struct stat st; return stat(p, &st) == 0 ? (long long)st.st_mtime : -1; }
static long long volt_fs_jetzt(void) { return (long long)time(NULL); }
static int volt_fs_setze_zeit(const char* p, long long t) { struct timespec ts[2] = {{(time_t)t, 0}, {(time_t)t, 0}}; return utimensat(AT_FDCWD, p, ts, 0); }

// Ordner samt fehlender Eltern anlegen; 0 = ok (auch wenn schon da)
static int volt_fs_ordner(const char* p) {
    char buf[4096];
    size_t n = strlen(p);
    if (n == 0 || n >= sizeof buf) return -1;
    memcpy(buf, p, n + 1);
    for (size_t i = 1; i < n; i++) {
        if (buf[i] == '/') { buf[i] = 0; if (mkdir(buf, 0755) != 0 && errno != EEXIST) return -1; buf[i] = '/'; }
    }
    if (mkdir(buf, 0755) != 0 && errno != EEXIST) return -1;
    return 0;
}

static int volt_fs_loesche_rek(const char* p) {
    struct stat st;
    if (lstat(p, &st) != 0) return errno == ENOENT ? 0 : -1;
    if (!S_ISDIR(st.st_mode)) return unlink(p);
    DIR* d = opendir(p);
    if (!d) return -1;
    struct dirent* e;
    int rc = 0;
    while ((e = readdir(d))) {
        if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")) continue;
        char sub[4096];
        if (snprintf(sub, sizeof sub, "%s/%s", p, e->d_name) >= (int)sizeof sub) { rc = -1; continue; }
        if (volt_fs_loesche_rek(sub) != 0) rc = -1;
    }
    closedir(d);
    return rc == 0 ? rmdir(p) : rc;
}
static int volt_fs_loesche(const char* p) { VOLT_FS_BLOCK_BEGIN; int rc = volt_fs_loesche_rek(p); VOLT_FS_BLOCK_END; return rc; }

static int volt_fs_umbenennen(const char* a, const char* b) { return rename(a, b); }

static int volt_fs_kopiere_rek(const char* a, const char* b) {
    struct stat st;
    if (stat(a, &st) != 0) return -1;
    if (S_ISDIR(st.st_mode)) {
        if (mkdir(b, 0755) != 0 && errno != EEXIST) return -1;
        DIR* d = opendir(a);
        if (!d) return -1;
        struct dirent* e;
        int rc = 0;
        while ((e = readdir(d))) {
            if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")) continue;
            char sa[4096], sb[4096];
            if (snprintf(sa, sizeof sa, "%s/%s", a, e->d_name) >= (int)sizeof sa || snprintf(sb, sizeof sb, "%s/%s", b, e->d_name) >= (int)sizeof sb) { rc = -1; continue; }
            if (volt_fs_kopiere_rek(sa, sb) != 0) rc = -1;
        }
        closedir(d);
        return rc;
    }
    int in = open(a, O_RDONLY);
    if (in < 0) return -1;
    char tmp[4100];
    snprintf(tmp, sizeof tmp, "%s.part", b);
    int out = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (out < 0) { close(in); return -1; }
    char buf[65536];
    ssize_t r;
    int rc = 0;
    while ((r = read(in, buf, sizeof buf)) > 0) {
        ssize_t off = 0;
        while (off < r) { ssize_t w = write(out, buf + off, (size_t)(r - off)); if (w <= 0) { rc = -1; break; } off += w; }
        if (rc) break;
    }
    if (r < 0) rc = -1;
    close(in);
    if (close(out) != 0) rc = -1;
    if (rc == 0) rc = rename(tmp, b); else unlink(tmp);
    return rc;
}
static int volt_fs_kopiere(const char* a, const char* b) { VOLT_FS_BLOCK_BEGIN; int rc = volt_fs_kopiere_rek(a, b); VOLT_FS_BLOCK_END; return rc; }

// Harter Link (kostet nichts, auch bei riesigen Dateien); geht das nicht (anderes Dateisystem), wird kopiert.
static int volt_fs_verlinke(const char* a, const char* b) { if (link(a, b) == 0) return 0; return volt_fs_kopiere(a, b); }

static long long volt_fs_ordnergroesse_rek(const char* p) {
    struct stat st;
    if (lstat(p, &st) != 0) return 0;
    if (!S_ISDIR(st.st_mode)) return (long long)st.st_size;
    DIR* d = opendir(p);
    if (!d) return 0;
    long long sum = 0;
    struct dirent* e;
    while ((e = readdir(d))) {
        if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")) continue;
        char sub[4096];
        if (snprintf(sub, sizeof sub, "%s/%s", p, e->d_name) < (int)sizeof sub) sum += volt_fs_ordnergroesse_rek(sub);
    }
    closedir(d);
    return sum;
}
static long long volt_fs_ordnergroesse(const char* p) { VOLT_FS_BLOCK_BEGIN; long long s = volt_fs_ordnergroesse_rek(p); VOLT_FS_BLOCK_END; return s; }

typedef struct { char* name; int art; long long groesse, zeit; } volt_fs_eintrag;
static int volt_fs_vergleiche(const void* x, const void* y) {
    const volt_fs_eintrag *a = x, *b = y;
    if (a->art != b->art) return a->art == 2 ? -1 : 1;  // Ordner zuerst
    return strcasecmp(a->name, b->name);
}
// Inhalt eines Ordners, eine Zeile je Eintrag: "Name\tArt\tGröße\tZeit\n" (Art 1 = Datei, 2 = Ordner); leer, wenn es ihn nicht gibt.
static const char* volt_fs_liste(const char* p) {
    DIR* d = opendir(p);
    if (!d) return "";
    size_t cap = 64, n = 0;
    volt_fs_eintrag* v = malloc(cap * sizeof *v);
    struct dirent* e;
    while ((e = readdir(d))) {
        if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")) continue;
        if (strpbrk(e->d_name, "\t\n\r")) continue;  // würde das Zeilenformat zerstören
        char sub[4096];
        if (snprintf(sub, sizeof sub, "%s/%s", p, e->d_name) >= (int)sizeof sub) continue;
        struct stat st;
        if (stat(sub, &st) != 0) continue;
        if (n == cap) { cap *= 2; v = realloc(v, cap * sizeof *v); }
        v[n].name = strdup(e->d_name);
        v[n].art = S_ISDIR(st.st_mode) ? 2 : 1;
        v[n].groesse = (long long)st.st_size;
        v[n].zeit = (long long)st.st_mtime;
        n++;
    }
    closedir(d);
    qsort(v, n, sizeof *v, volt_fs_vergleiche);
    size_t total = 1;
    for (size_t i = 0; i < n; i++) total += strlen(v[i].name) + 48;
    char* buf = malloc(total);
    size_t len = 0;
    for (size_t i = 0; i < n; i++) {
        len += (size_t)snprintf(buf + len, total - len, "%s\t%d\t%lld\t%lld\n", v[i].name, v[i].art, v[i].groesse, v[i].zeit);
        free(v[i].name);
    }
    const char* out = volt_fs_text(buf, len);
    free(buf);
    free(v);
    return out;
}

// Ganze Datei als Text (höchstens 64 MB; Binärdaten werden am ersten NUL-Byte abgeschnitten). Leer, wenn es sie nicht gibt.
static const char* volt_fs_lies(const char* p) {
    int fd = open(p, O_RDONLY);
    if (fd < 0) return "";
    struct stat st;
    if (fstat(fd, &st) != 0 || !S_ISREG(st.st_mode) || st.st_size > (64 << 20)) { close(fd); return ""; }
    char* out = volt_str_alloc((size_t)st.st_size);
    size_t len = 0;
    while (len < (size_t)st.st_size) {
        ssize_t r = read(fd, out + len, (size_t)st.st_size - len);
        if (r <= 0) break;
        len += (size_t)r;
    }
    close(fd);
    out[len] = 0;
    return out;
}

// Schreibt über eine Zwischendatei und benennt um: ein Absturz hinterlässt nie eine halbe Datei. 0 = ok
static int volt_fs_schreibe(const char* p, const char* text) {
    char tmp[4100];
    if (snprintf(tmp, sizeof tmp, "%s.part", p) >= (int)sizeof tmp) return -1;
    int fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd < 0) return -1;
    size_t n = strlen(text), off = 0;
    while (off < n) { ssize_t w = write(fd, text + off, n - off); if (w <= 0) { close(fd); unlink(tmp); return -1; } off += (size_t)w; }
    if (fsync(fd) != 0 || close(fd) != 0) { unlink(tmp); return -1; }
    return rename(tmp, p);
}

static const char* volt_fs_umgebung(const char* name) { const char* v = getenv(name); return v ? v : ""; }

static const char* volt_fs_zufall(int bytes) {
    if (bytes < 1) bytes = 1;
    if (bytes > 1024) bytes = 1024;
    unsigned char raw[1024];
    int fd = open("/dev/urandom", O_RDONLY);
    size_t got = 0;
    if (fd >= 0) { while (got < (size_t)bytes) { ssize_t r = read(fd, raw + got, (size_t)bytes - got); if (r <= 0) break; got += (size_t)r; } close(fd); }
    if (got < (size_t)bytes) return "";  // lieber kein Token als ein vorhersagbares
    char* out = volt_str_alloc((size_t)bytes * 2);
    for (int i = 0; i < bytes; i++) snprintf(out + 2 * i, 3, "%02x", raw[i]);
    return out;
}

// ---- SHA-256, HMAC, PBKDF2 ----
typedef struct { uint32_t h[8]; uint64_t len; unsigned char buf[64]; size_t n; } volt_sha;
static const uint32_t volt_sha_k[64] = {
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2};
#define VOLT_ROR(x, n) (((x) >> (n)) | ((x) << (32 - (n))))
static void volt_sha_block(volt_sha* s, const unsigned char* p) {
    uint32_t w[64];
    for (int i = 0; i < 16; i++) w[i] = (uint32_t)p[4*i] << 24 | (uint32_t)p[4*i+1] << 16 | (uint32_t)p[4*i+2] << 8 | p[4*i+3];
    for (int i = 16; i < 64; i++) {
        uint32_t s0 = VOLT_ROR(w[i-15], 7) ^ VOLT_ROR(w[i-15], 18) ^ (w[i-15] >> 3), s1 = VOLT_ROR(w[i-2], 17) ^ VOLT_ROR(w[i-2], 19) ^ (w[i-2] >> 10);
        w[i] = w[i-16] + s0 + w[i-7] + s1;
    }
    uint32_t a = s->h[0], b = s->h[1], c = s->h[2], d = s->h[3], e = s->h[4], f = s->h[5], g = s->h[6], h = s->h[7];
    for (int i = 0; i < 64; i++) {
        uint32_t t1 = h + (VOLT_ROR(e, 6) ^ VOLT_ROR(e, 11) ^ VOLT_ROR(e, 25)) + ((e & f) ^ (~e & g)) + volt_sha_k[i] + w[i];
        uint32_t t2 = (VOLT_ROR(a, 2) ^ VOLT_ROR(a, 13) ^ VOLT_ROR(a, 22)) + ((a & b) ^ (a & c) ^ (b & c));
        h = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
    }
    s->h[0] += a; s->h[1] += b; s->h[2] += c; s->h[3] += d; s->h[4] += e; s->h[5] += f; s->h[6] += g; s->h[7] += h;
}
static void volt_sha_init(volt_sha* s) {
    static const uint32_t i[8] = {0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19};
    memcpy(s->h, i, sizeof i); s->len = 0; s->n = 0;
}
static void volt_sha_add(volt_sha* s, const unsigned char* p, size_t n) {
    s->len += n;
    while (n > 0) {
        size_t take = 64 - s->n < n ? 64 - s->n : n;
        memcpy(s->buf + s->n, p, take); s->n += take; p += take; n -= take;
        if (s->n == 64) { volt_sha_block(s, s->buf); s->n = 0; }
    }
}
static void volt_sha_end(volt_sha* s, unsigned char out[32]) {
    uint64_t bits = s->len * 8;
    unsigned char pad = 0x80;
    volt_sha_add(s, &pad, 1);
    pad = 0;
    while (s->n != 56) volt_sha_add(s, &pad, 1);
    unsigned char l[8];
    for (int i = 0; i < 8; i++) l[i] = (unsigned char)(bits >> (56 - 8 * i));
    volt_sha_add(s, l, 8);
    for (int i = 0; i < 8; i++) { out[4*i] = (unsigned char)(s->h[i] >> 24); out[4*i+1] = (unsigned char)(s->h[i] >> 16); out[4*i+2] = (unsigned char)(s->h[i] >> 8); out[4*i+3] = (unsigned char)s->h[i]; }
}
static void volt_sha256(const unsigned char* p, size_t n, unsigned char out[32]) { volt_sha s; volt_sha_init(&s); volt_sha_add(&s, p, n); volt_sha_end(&s, out); }
static void volt_hex(const unsigned char* p, size_t n, char* out) { for (size_t i = 0; i < n; i++) snprintf(out + 2 * i, 3, "%02x", p[i]); }

static const char* volt_fs_sha256(const char* text) {
    unsigned char d[32];
    volt_sha256((const unsigned char*)text, strlen(text), d);
    char* out = volt_str_alloc(64);
    volt_hex(d, 32, out);
    return out;
}

// SHA-256 des Inhalts einer Datei (Hex), leer bei Fehler. Liest in Stücken und gibt dabei die Sperre frei.
static const char* volt_fs_sha256_datei(const char* p) {
    int fd = open(p, O_RDONLY);
    if (fd < 0) return "";
    volt_sha s; volt_sha_init(&s);
    unsigned char* buf = malloc(65536);
    int fehler = buf == NULL;
    while (!fehler) {
        ssize_t r;
        { VOLT_FS_BLOCK_BEGIN; r = read(fd, buf, 65536); VOLT_FS_BLOCK_END; }
        if (r == 0) break;
        if (r < 0) { fehler = 1; break; }
        volt_sha_add(&s, buf, (size_t)r);
    }
    close(fd); free(buf);
    if (fehler) return "";
    unsigned char d[32]; volt_sha_end(&s, d);
    char* out = volt_str_alloc(64); volt_hex(d, 32, out);
    return out;
}

static void volt_hmac(const unsigned char* key, size_t kn, const unsigned char* msg, size_t mn, const unsigned char* msg2, size_t mn2, unsigned char out[32]) {
    unsigned char k[64] = {0}, ip[64], op[64], inner[32];
    if (kn > 64) volt_sha256(key, kn, k); else memcpy(k, key, kn);
    for (int i = 0; i < 64; i++) { ip[i] = k[i] ^ 0x36; op[i] = k[i] ^ 0x5c; }
    volt_sha s; volt_sha_init(&s); volt_sha_add(&s, ip, 64); volt_sha_add(&s, msg, mn); if (mn2) volt_sha_add(&s, msg2, mn2); volt_sha_end(&s, inner);
    volt_sha_init(&s); volt_sha_add(&s, op, 64); volt_sha_add(&s, inner, 32); volt_sha_end(&s, out);
}

// PBKDF2-HMAC-SHA256 (ein Block, 32 Byte) als Hex-Text. Für Passwörter: viele Runden machen Raten teuer.
static const char* volt_fs_passwort(const char* pw, const char* salt, int runden) {
    if (runden < 1) runden = 1;
    VOLT_FS_BLOCK_BEGIN;
    unsigned char one[4] = {0, 0, 0, 1}, u[32], t[32], msg[1024];
    size_t sn = strlen(salt), pn = strlen(pw);
    if (sn > sizeof msg - 4) sn = sizeof msg - 4;
    memcpy(msg, salt, sn);
    memcpy(msg + sn, one, 4);
    volt_hmac((const unsigned char*)pw, pn, msg, sn + 4, NULL, 0, u);
    memcpy(t, u, 32);
    for (int i = 1; i < runden; i++) {
        volt_hmac((const unsigned char*)pw, pn, u, 32, NULL, 0, u);
        for (int j = 0; j < 32; j++) t[j] ^= u[j];
    }
    VOLT_FS_BLOCK_END;
    char* out = volt_str_alloc(64);
    volt_hex(t, 32, out);
    return out;
}

// Vergleich in gleichbleibender Zeit (Passwörter, Tokens): 1 = gleich
static int volt_fs_gleich(const char* a, const char* b) {
    size_t na = strlen(a), nb = strlen(b);
    unsigned char diff = (unsigned char)(na != nb);
    for (size_t i = 0; i < na; i++) diff |= (unsigned char)(a[i] ^ b[i % (nb ? nb : 1)]);
    return diff == 0;
}

// ---- Base64 (Anmeldung per Basic-Auth) und URL-Kodierung (Pfade) ----
static const char* volt_fs_base64_dekodiere(const char* s) {
    size_t n = strlen(s);
    char* out = volt_str_alloc(n);
    size_t len = 0;
    uint32_t acc = 0;
    int bits = 0;
    for (size_t i = 0; i < n; i++) {
        char c = s[i];
        int v;
        if (c >= 'A' && c <= 'Z') v = c - 'A'; else if (c >= 'a' && c <= 'z') v = c - 'a' + 26; else if (c >= '0' && c <= '9') v = c - '0' + 52;
        else if (c == '+' || c == '-') v = 62; else if (c == '/' || c == '_') v = 63; else if (c == '=') break; else continue;
        acc = acc << 6 | (uint32_t)v; bits += 6;
        if (bits >= 8) { bits -= 8; char b = (char)((acc >> bits) & 0xff); if (b == 0) break; out[len++] = b; }
    }
    out[len] = 0;
    return out;
}

static const char* volt_fs_url_dekodiere(const char* s) {
    size_t n = strlen(s);
    char* out = volt_str_alloc(n);
    size_t len = 0;
    for (size_t i = 0; i < n; i++) {
        if (s[i] == '%' && i + 2 < n && isxdigit((unsigned char)s[i+1]) && isxdigit((unsigned char)s[i+2])) {
            char h[3] = {s[i+1], s[i+2], 0};
            char b = (char)strtol(h, NULL, 16);
            if (b == 0) { i += 2; continue; }  // %00 würde den Text abschneiden
            out[len++] = b; i += 2;
        } else out[len++] = s[i];
    }
    out[len] = 0;
    return out;
}

// Kodiert alles außer Buchstaben, Ziffern und -_.~/ (Schrägstriche bleiben, damit Pfade lesbar sind)
static const char* volt_fs_url_kodiere(const char* s) {
    size_t n = strlen(s);
    char* out = volt_str_alloc(n * 3);
    size_t len = 0;
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (isalnum(c) || c == '-' || c == '_' || c == '.' || c == '~' || c == '/') out[len++] = (char)c;
        else len += (size_t)snprintf(out + len, 4, "%%%02X", c);
    }
    out[len] = 0;
    return out;
}

// Zeit als HTTP-Datum ("Tue, 18 Nov 2026 09:00:00 GMT") und als ISO-Text
static const char* volt_fs_http_datum(long long t) {
    time_t tt = (time_t)t; struct tm tm; gmtime_r(&tt, &tm);
    char buf[64]; size_t n = strftime(buf, sizeof buf, "%a, %d %b %Y %H:%M:%S GMT", &tm);
    return volt_fs_text(buf, n);
}
static const char* volt_fs_iso_datum(long long t) {
    time_t tt = (time_t)t; struct tm tm; gmtime_r(&tt, &tm);
    char buf[64]; size_t n = strftime(buf, sizeof buf, "%Y-%m-%dT%H:%M:%SZ", &tm);
    return volt_fs_text(buf, n);
}
#endif
