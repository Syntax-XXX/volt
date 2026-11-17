// Fäden für Volt (std/faden.vt). Volt-Code läuft in einem Faden zur Zeit (gemeinsame Sperre `volt_gil`, siehe volt_rt.h) und
// wechselt nur beim Schlafen, Warten und bei `pause` – zwischen diesen Stellen wird er nie unterbrochen. Im Browser (WebAssembly)
// läuft ein Faden sofort bis zum Ende, `schlafe` ist wirkungslos.
#ifndef VOLT_THREAD_H
#define VOLT_THREAD_H
static void volt_thread_entry(int64_t idx);  // das Programm definiert es (ruft `faden.__lauf`)
#ifdef __wasm__
static int volt_thread_spawn(int idx) { volt_thread_entry(idx); return 0; }
static void volt_thread_spawn_detached(int idx) { volt_thread_entry(idx); }
static void volt_thread_join(int handle) { (void)handle; }
static void volt_thread_sleep(int ms) { (void)ms; }
static void volt_thread_pause(void) {}
#else
static void volt_block_begin(volt_thr* me, char* marker) {
    if (me) { setjmp(me->regs); me->sp = marker; }
    pthread_mutex_unlock(&volt_gil);
}
static void volt_block_end(volt_thr* me) {
    pthread_mutex_lock(&volt_gil);
    if (me) me->sp = 0;
}
typedef struct { volt_thr* rec; int64_t idx; } volt_start_arg;
static void* volt_thread_main(void* p) {
    volt_start_arg a = *(volt_start_arg*)p;
    free(p);
    char marker;
    pthread_mutex_lock(&volt_gil);
    a.rec->base = &marker + 64;
    volt_thread_entry(a.idx);
    a.rec->live = 0;
    a.rec->base = 0;
    if (a.rec->detached) {  // niemand wartet auf diesen Faden: Eintrag sofort freigeben
        for (int i = 0; i < volt_nthrs; i++) if (volt_thrs[i] == a.rec) volt_thrs[i] = 0;
        free(a.rec);
    }
    pthread_mutex_unlock(&volt_gil);
    return 0;
}
static int volt_thread_spawn_ex(int idx, int detached) {
    volt_thr* rec = calloc(1, sizeof *rec);
    volt_start_arg* a = malloc(sizeof *a);
    if (!rec || !a) volt_panic("kein Speicher mehr frei", 0);
    a->rec = rec;
    a->idx = idx;
    rec->live = 1;
    int slot = -1;
    for (int i = 0; i < volt_nthrs; i++) if (!volt_thrs[i]) { slot = i; break; }
    if (slot < 0 && volt_nthrs < 16384) slot = volt_nthrs++;
    if (slot < 0) volt_panic("zu viele Fäden (höchstens 16384 gleichzeitig)", 0);
    rec->detached = detached;
    volt_thrs[slot] = rec;
    pthread_attr_t attr;
    pthread_attr_init(&attr);
    pthread_attr_setstacksize(&attr, (size_t)1 << 20);  // 1 MB reichen für Volt-Funktionen; viele Fäden brauchen wenig Platz
    if (detached) pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED);
    if (pthread_create(&rec->id, &attr, volt_thread_main, a)) volt_panic("Faden lässt sich nicht starten", 0);
    pthread_attr_destroy(&attr);
    return slot;
}
static int volt_thread_spawn(int idx) { return volt_thread_spawn_ex(idx, 0); }
static void volt_thread_spawn_detached(int idx) { volt_thread_spawn_ex(idx, 1); }
static void volt_thread_join(int handle) {
    if (handle < 0 || handle >= volt_nthrs || !volt_thrs[handle]) return;
    volt_thr* rec = volt_thrs[handle];
    volt_thr* me = volt_me();
    char marker;
    volt_block_begin(me, &marker);
    pthread_join(rec->id, 0);
    volt_block_end(me);
    volt_thrs[handle] = 0;
    free(rec);
}
static void volt_thread_sleep(int ms) {
    volt_thr* me = volt_me();
    char marker;
    volt_block_begin(me, &marker);
    usleep((useconds_t)(ms > 0 ? ms : 0) * 1000);
    volt_block_end(me);
}
static void volt_thread_pause(void) {
    volt_thr* me = volt_me();
    char marker;
    volt_block_begin(me, &marker);
    usleep(50);
    volt_block_end(me);
}
#endif
#endif
