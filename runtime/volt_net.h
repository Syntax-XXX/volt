// Netz für Volt (std/http.vt): einfache TCP-Sockets. Blockierende Aufrufe geben die gemeinsame Sperre frei (siehe volt_rt.h),
// damit andere Fäden weiterlaufen. Im Browser (WebAssembly) gibt es kein Netz: alles scheitert mit -1 / leerem Text.
#ifndef VOLT_NET_H
#define VOLT_NET_H
#ifdef __wasm__
static int volt_net_listen(int port, int nur_lokal) { (void)port; (void)nur_lokal; return -1; }
static int volt_net_accept(int fd) { (void)fd; return -1; }
static const char* volt_net_read_head(int fd) { (void)fd; return ""; }
static const char* volt_net_read_body(int fd, int n) { (void)fd; (void)n; return ""; }
static void volt_net_write(int fd, const char* text) { (void)fd; (void)text; }
static void volt_net_close(int fd) { (void)fd; }
static long long volt_net_body_to_file(int fd, long long n, const char* p) { (void)fd; (void)n; (void)p; return -1; }
static int volt_net_send_file(int fd, const char* p, long long a, long long b) { (void)fd; (void)p; (void)a; (void)b; return -1; }
static const char* volt_net_peer(int fd) { (void)fd; return ""; }
static long long volt_net_zip_laenge(const char* b, const char* l) { (void)b; (void)l; return -1; }
static int volt_net_zip_senden(int fd, const char* b, const char* l) { (void)fd; (void)b; (void)l; return -1; }
#else
#include "volt_thread.h"
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <errno.h>
#include <signal.h>
#include <fcntl.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <stdint.h>
#include <time.h>

static int volt_net_listen(int port, int nur_lokal) {
    signal(SIGPIPE, SIG_IGN);  // ein abgebrochener Client darf den Server nicht beenden
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return -1;
    int one = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    struct sockaddr_in addr;
    memset(&addr, 0, sizeof addr);
    addr.sin_family = AF_INET;
    addr.sin_port = htons((unsigned short)port);
    addr.sin_addr.s_addr = htonl(nur_lokal ? INADDR_LOOPBACK : INADDR_ANY);
    if (bind(fd, (struct sockaddr*)&addr, sizeof addr) < 0 || listen(fd, 1024) < 0) { close(fd); return -1; }
    return fd;
}

static int volt_net_accept(int fd) {
    volt_thr* me = volt_me();
    char marker;
    volt_block_begin(me, &marker);
    int c = accept(fd, 0, 0);
    volt_block_end(me);
    if (c >= 0) {  // Zeitlimit: ein Client, der nichts schickt, blockiert keinen Faden ewig
        struct timeval tv = {15, 0};
        setsockopt(c, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    }
    return c;
}

static long volt_net_recv(int fd, char* buf, size_t n) {
    volt_thr* me = volt_me();
    char marker;
    volt_block_begin(me, &marker);
    long got = recv(fd, buf, n, 0);
    volt_block_end(me);
    return got;
}

// Pro Verbindung: Bytes, die beim Kopf-Lesen schon hinter der Leerzeile angekommen sind (Anfang des Inhalts, binärsicher).
#define VOLT_NET_MAXFD 65536
static char* volt_net_rest[VOLT_NET_MAXFD];
static size_t volt_net_rest_len[VOLT_NET_MAXFD];

// Liest bis zur Leerzeile hinter den Kopfzeilen (höchstens 16 KB) und gibt nur den Kopf zurück (ohne die Leerzeile).
// Überschüssige Bytes bleiben für volt_net_read_body / volt_net_body_to_file erhalten. Leer bei Fehler oder zu langem Kopf.
static const char* volt_net_read_head(int fd) {
    enum { MAXHEAD = 16384 };
    char buf[MAXHEAD + 1];
    size_t len = 0;
    char* end = NULL;
    while (len < MAXHEAD) {
        long got = volt_net_recv(fd, buf + len, MAXHEAD - len);
        if (got <= 0) break;
        len += (size_t)got;
        buf[len] = 0;
        end = strstr(buf, "\r\n\r\n");
        if (end) break;
    }
    if (!end) return "";
    size_t head = (size_t)(end - buf), extra = len - head - 4;
    if (fd >= 0 && fd < VOLT_NET_MAXFD) {
        free(volt_net_rest[fd]);
        volt_net_rest[fd] = extra ? malloc(extra) : NULL;
        if (extra) memcpy(volt_net_rest[fd], end + 4, extra);
        volt_net_rest_len[fd] = extra;
    }
    char* out = volt_str_alloc(head);
    memcpy(out, buf, head);
    return out;
}

// Nimmt höchstens n Bytes aus dem Rest-Puffer; gibt die Anzahl zurück.
static size_t volt_net_take_rest(int fd, char* dst, size_t n) {
    if (fd < 0 || fd >= VOLT_NET_MAXFD || !volt_net_rest[fd]) return 0;
    size_t take = volt_net_rest_len[fd] < n ? volt_net_rest_len[fd] : n;
    memcpy(dst, volt_net_rest[fd], take);
    if (take == volt_net_rest_len[fd]) { free(volt_net_rest[fd]); volt_net_rest[fd] = NULL; volt_net_rest_len[fd] = 0; }
    else { memmove(volt_net_rest[fd], volt_net_rest[fd] + take, volt_net_rest_len[fd] - take); volt_net_rest_len[fd] -= take; }
    return take;
}

// Inhalt als Text (höchstens 16 MB; Binärdaten würden am ersten NUL-Byte abgeschnitten – dafür gibt es body_to_file).
static const char* volt_net_read_body(int fd, int n) {
    if (n < 0) n = 0;
    if (n > (1 << 24)) n = 1 << 24;
    char* out = volt_str_alloc((size_t)n);
    size_t len = volt_net_take_rest(fd, out, (size_t)n);
    while (len < (size_t)n) {
        long got = volt_net_recv(fd, out + len, (size_t)n - len);
        if (got <= 0) break;
        len += (size_t)got;
    }
    out[len] = 0;
    return out;
}

// Schreibt genau n Bytes des Inhalts binärsicher in eine Datei (erst nach pfad.part, am Ende umbenannt). Gibt n zurück, sonst -1
// (Client weg, Platte voll, Pfad nicht beschreibbar); dann bleibt keine halbe Datei zurück.
static long long volt_net_body_to_file(int fd, long long n, const char* path) {
    char tmp[4100];
    if (n < 0 || snprintf(tmp, sizeof tmp, "%s.part", path) >= (int)sizeof tmp) return -1;
    int out = open(tmp, O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (out < 0) return -1;
    static char dummy;
    char* buf = malloc(65536);
    long long done = 0;
    int ok = buf != NULL;
    while (ok && done < n) {
        size_t want = (size_t)(n - done < 65536 ? n - done : 65536);
        size_t got = volt_net_take_rest(fd, buf, want);
        if (got == 0) { long r = volt_net_recv(fd, buf, want); if (r <= 0) { ok = 0; break; } got = (size_t)r; }
        size_t off = 0;
        while (off < got) {
            volt_thr* me = volt_me(); char marker; volt_block_begin(me, &marker);
            ssize_t w = write(out, buf + off, got - off);
            volt_block_end(me);
            if (w <= 0) { ok = 0; break; }
            off += (size_t)w;
        }
        done += (long long)got;
    }
    (void)dummy;
    free(buf);
    if (close(out) != 0) ok = 0;
    if (ok && rename(tmp, path) == 0) return done;
    unlink(tmp);
    return -1;
}

// Schickt die Bytes [von, bis] (einschließlich) einer Datei auf den Socket; bis < 0 = bis zum Ende. 0 = ok, -1 = Fehler.
static int volt_net_send_file(int fd, const char* path, long long von, long long bis) {
    int in = open(path, O_RDONLY);
    if (in < 0) return -1;
    struct stat st;
    if (fstat(in, &st) != 0) { close(in); return -1; }
    long long ende = (bis < 0 || bis >= st.st_size) ? (long long)st.st_size - 1 : bis;
    long long pos = von < 0 ? 0 : von;
    if (lseek(in, pos, SEEK_SET) < 0) { close(in); return -1; }
    char* buf = malloc(65536);
    int rc = buf ? 0 : -1;
    while (rc == 0 && pos <= ende) {
        size_t want = (size_t)(ende - pos + 1 < 65536 ? ende - pos + 1 : 65536);
        volt_thr* me = volt_me(); char marker; volt_block_begin(me, &marker);
        ssize_t r = read(in, buf, want);
        size_t off = 0;
        while (r > 0 && off < (size_t)r) { ssize_t w = send(fd, buf + off, (size_t)r - off, MSG_NOSIGNAL); if (w <= 0) { r = -1; break; } off += (size_t)w; }
        volt_block_end(me);
        if (r <= 0) { rc = -1; break; }
        pos += r;
    }
    free(buf);
    close(in);
    return rc;
}

// Fremde Adresse der Verbindung als Text ("203.0.113.5"); leer bei Fehler.
static const char* volt_net_peer(int fd) {
    struct sockaddr_in a; socklen_t l = sizeof a;
    if (getpeername(fd, (struct sockaddr*)&a, &l) != 0) return "";
    char buf[64];
    if (!inet_ntop(AF_INET, &a.sin_addr, buf, sizeof buf)) return "";
    size_t n = strlen(buf);
    char* out = volt_str_alloc(n);
    memcpy(out, buf, n);
    return out;
}


// ---- ZIP-Archiv streamen (Methode "gespeichert", kein ZIP64: höchstens 4 GB und 65534 Einträge) ----
static uint32_t volt_crc_tab[256];
static void volt_crc_init(void) {
    if (volt_crc_tab[1]) return;
    for (uint32_t i = 0; i < 256; i++) { uint32_t c = i; for (int k = 0; k < 8; k++) c = c & 1 ? 0xEDB88320u ^ (c >> 1) : c >> 1; volt_crc_tab[i] = c; }
}
static uint32_t volt_crc_add(uint32_t crc, const unsigned char* p, size_t n) {
    crc = ~crc;
    while (n--) crc = volt_crc_tab[(crc ^ *p++) & 0xff] ^ (crc >> 8);
    return ~crc;
}
typedef struct { char* name; int ordner; unsigned long long size; time_t mtime; uint32_t crc, offset; } volt_zip_e;
// Liest die Zeilen von `liste` (Pfade relativ zu `basis`, Ordner mit Endschrägstrich). Gibt die Einträge zurück oder NULL bei Verstoß/Überlänge.
static volt_zip_e* volt_zip_scan(const char* basis, const char* liste, size_t* n_out, unsigned long long* gesamt) {
    size_t cap = 64, n = 0;
    volt_zip_e* v = malloc(cap * sizeof *v);
    unsigned long long total = 22;
    const char* p = liste;
    while (*p) {
        const char* e = strchr(p, '\n');
        size_t len = e ? (size_t)(e - p) : strlen(p);
        if (len > 0) {
            char* rel = malloc(len + 1);
            memcpy(rel, p, len); rel[len] = 0;
            int bad = rel[0] == '/' || strstr(rel, "..") || len > 1000;
            char full[4400]; struct stat st;
            snprintf(full, sizeof full, "%s/%s", basis, rel);
            if (bad || stat(full, &st) != 0 || n >= 65534) { free(rel); goto fail; }
            if (n == cap) { cap *= 2; v = realloc(v, cap * sizeof *v); }
            int dir = S_ISDIR(st.st_mode);
            if (dir && rel[len - 1] != '/') { rel = realloc(rel, len + 2); rel[len] = '/'; rel[len + 1] = 0; len++; }
            v[n].name = rel; v[n].ordner = dir; v[n].size = dir ? 0 : (unsigned long long)st.st_size; v[n].mtime = st.st_mtime; v[n].crc = 0; v[n].offset = 0;
            total += (30 + len) + (dir ? 0 : v[n].size + 16) + (46 + len);
            n++;
            if (total > 0xFFFFFFF0ull) goto fail2;
        }
        if (!e) break;
        p = e + 1;
    }
    *n_out = n; *gesamt = total;
    return v;
fail:
fail2:
    for (size_t i = 0; i < n; i++) free(v[i].name);
    free(v);
    return NULL;
}
static void volt_zip_free(volt_zip_e* v, size_t n) { for (size_t i = 0; i < n; i++) free(v[i].name); free(v); }
// Länge des fertigen Archivs in Bytes, -1 wenn es nicht möglich ist (zu groß, zu viele Einträge, ungültiger Pfad).
static long long volt_net_zip_laenge(const char* basis, const char* liste) {
    size_t n; unsigned long long total;
    volt_zip_e* v = volt_zip_scan(basis, liste, &n, &total);
    if (!v) return -1;
    volt_zip_free(v, n);
    return (long long)total;
}
static int volt_zip_raus(int fd, const void* buf, size_t n) {
    const char* p = buf; size_t off = 0;
    while (off < n) {
        volt_thr* me = volt_me(); char marker; volt_block_begin(me, &marker);
        ssize_t w = send(fd, p + off, n - off, MSG_NOSIGNAL);
        volt_block_end(me);
        if (w <= 0) return -1;
        off += (size_t)w;
    }
    return 0;
}
static void volt_zip_dos(time_t t, uint16_t* d, uint16_t* tm) {
    struct tm x; localtime_r(&t, &x);
    int y = x.tm_year + 1900; if (y < 1980) { y = 1980; x.tm_mon = 0; x.tm_mday = 1; x.tm_hour = x.tm_min = x.tm_sec = 0; }
    *d = (uint16_t)(((y - 1980) << 9) | ((x.tm_mon + 1) << 5) | x.tm_mday);
    *tm = (uint16_t)((x.tm_hour << 11) | (x.tm_min << 5) | (x.tm_sec / 2));
}
static void volt_p16(unsigned char* b, uint16_t v) { b[0] = (unsigned char)v; b[1] = (unsigned char)(v >> 8); }
static void volt_p32(unsigned char* b, uint32_t v) { for (int i = 0; i < 4; i++) b[i] = (unsigned char)(v >> (8 * i)); }
// Schickt das Archiv. Gibt 0 zurück, bei Fehler (Client weg, Datei verändert) -1: dann die Verbindung einfach schließen.
static int volt_net_zip_senden(int fd, const char* basis, const char* liste) {
    volt_crc_init();
    size_t n; unsigned long long total;
    volt_zip_e* v = volt_zip_scan(basis, liste, &n, &total);
    if (!v) return -1;
    int rc = 0; uint32_t pos = 0;
    unsigned char h[46]; char* buf = malloc(65536);
    for (size_t i = 0; i < n && rc == 0; i++) {
        uint16_t dd, dt; volt_zip_dos(v[i].mtime, &dd, &dt);
        size_t nl = strlen(v[i].name);
        v[i].offset = pos;
        memset(h, 0, 30);
        volt_p32(h, 0x04034b50); volt_p16(h + 4, 20); volt_p16(h + 6, v[i].ordner ? 0x0800 : 0x0808); volt_p16(h + 8, 0); volt_p16(h + 10, dt); volt_p16(h + 12, dd); volt_p16(h + 26, (uint16_t)nl);
        if (volt_zip_raus(fd, h, 30) || volt_zip_raus(fd, v[i].name, nl)) { rc = -1; break; }
        pos += 30 + (uint32_t)nl;
        if (v[i].ordner) continue;
        char full[4400]; snprintf(full, sizeof full, "%s/%s", basis, v[i].name);
        int in = open(full, O_RDONLY);
        if (in < 0) { rc = -1; break; }
        uint32_t crc = 0; unsigned long long done = 0;
        while (done < v[i].size) {
            size_t want = (size_t)(v[i].size - done < 65536 ? v[i].size - done : 65536);
            volt_thr* me = volt_me(); char marker; volt_block_begin(me, &marker);
            ssize_t r = read(in, buf, want);
            volt_block_end(me);
            if (r <= 0) { rc = -1; break; }  // Datei ist kürzer geworden: Länge stimmt nicht mehr
            crc = volt_crc_add(crc, (unsigned char*)buf, (size_t)r);
            if (volt_zip_raus(fd, buf, (size_t)r)) { rc = -1; break; }
            done += (unsigned long long)r;
        }
        close(in);
        if (rc) break;
        v[i].crc = crc; pos += (uint32_t)v[i].size;
        unsigned char dsc[16]; volt_p32(dsc, 0x08074b50); volt_p32(dsc + 4, crc); volt_p32(dsc + 8, (uint32_t)v[i].size); volt_p32(dsc + 12, (uint32_t)v[i].size);
        if (volt_zip_raus(fd, dsc, 16)) { rc = -1; break; }
        pos += 16;
    }
    uint32_t cd_start = pos, cd_size = 0;
    for (size_t i = 0; i < n && rc == 0; i++) {
        uint16_t dd, dt; volt_zip_dos(v[i].mtime, &dd, &dt);
        size_t nl = strlen(v[i].name);
        memset(h, 0, 46);
        volt_p32(h, 0x02014b50); volt_p16(h + 4, 20); volt_p16(h + 6, 20); volt_p16(h + 8, v[i].ordner ? 0x0800 : 0x0808); volt_p16(h + 10, 0); volt_p16(h + 12, dt); volt_p16(h + 14, dd);
        volt_p32(h + 16, v[i].crc); volt_p32(h + 20, (uint32_t)v[i].size); volt_p32(h + 24, (uint32_t)v[i].size); volt_p16(h + 28, (uint16_t)nl);
        volt_p32(h + 38, v[i].ordner ? 0x10u : 0u); volt_p32(h + 42, v[i].offset);
        if (volt_zip_raus(fd, h, 46) || volt_zip_raus(fd, v[i].name, nl)) { rc = -1; break; }
        cd_size += 46 + (uint32_t)nl;
    }
    if (rc == 0) {
        unsigned char e[22]; memset(e, 0, 22);
        volt_p32(e, 0x06054b50); volt_p16(e + 8, (uint16_t)n); volt_p16(e + 10, (uint16_t)n); volt_p32(e + 12, cd_size); volt_p32(e + 16, cd_start);
        rc = volt_zip_raus(fd, e, 22);
    }
    free(buf); volt_zip_free(v, n);
    return rc;
}

static void volt_net_write(int fd, const char* text) {
    size_t n = strlen(text), done = 0;
    while (done < n) {
        volt_thr* me = volt_me();
        char marker;
        volt_block_begin(me, &marker);
        long w = send(fd, text + done, n - done, MSG_NOSIGNAL);
        volt_block_end(me);
        if (w <= 0) return;
        done += (size_t)w;
    }
}

static void volt_net_close(int fd) {
    if (fd >= 0 && fd < VOLT_NET_MAXFD) { free(volt_net_rest[fd]); volt_net_rest[fd] = NULL; volt_net_rest_len[fd] = 0; }
    close(fd);
}
#endif
#endif
