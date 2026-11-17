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
#else
#include "volt_thread.h"
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <errno.h>
#include <signal.h>

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

// Liest bis zur Leerzeile hinter den Kopfzeilen (höchstens 16 KB). Gibt den Text einschließlich evtl. schon gelesener Daten zurück.
static const char* volt_net_read_head(int fd) {
    enum { MAXHEAD = 16384 };
    char buf[MAXHEAD + 1];
    size_t len = 0;
    while (len < MAXHEAD) {
        long got = volt_net_recv(fd, buf + len, MAXHEAD - len);
        if (got <= 0) break;
        len += (size_t)got;
        buf[len] = 0;
        if (strstr(buf, "\r\n\r\n")) break;
    }
    buf[len] = 0;
    char* out = volt_str_alloc(len);
    memcpy(out, buf, len);
    return out;
}

static const char* volt_net_read_body(int fd, int n) {
    if (n < 0) n = 0;
    if (n > (1 << 24)) n = 1 << 24;  // höchstens 16 MB
    char* out = volt_str_alloc((size_t)n);
    size_t len = 0;
    while (len < (size_t)n) {
        long got = volt_net_recv(fd, out + len, (size_t)n - len);
        if (got <= 0) break;
        len += (size_t)got;
    }
    out[len] = 0;
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

static void volt_net_close(int fd) { close(fd); }
#endif
#endif
