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
