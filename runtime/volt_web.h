// Browser-Anbindung für Volt (Web-Ziel). Im Browser sind die volt_*-Funktionen JavaScript (volt.js); beim normalen
// Start (`volt run`) gibt es Ersatz-Funktionen, die ihre Aufrufe ausgeben – so lassen sich Web-Programme auch testen.
#ifndef VOLT_WEB_H
#define VOLT_WEB_H
#include <stdio.h>
#ifdef __wasm__
#define VOLT_JS(name) __attribute__((import_module("volt"), import_name(name)))
VOLT_JS("set_text") void volt_set_text(const char* selector, const char* text);
VOLT_JS("set_html") void volt_set_html(const char* selector, const char* html);
VOLT_JS("append_html") void volt_append_html(const char* selector, const char* html);
VOLT_JS("get_value") const char* volt_get_value(const char* selector);
VOLT_JS("set_value") void volt_set_value(const char* selector, const char* value);
VOLT_JS("listen") void volt_listen(const char* selector, const char* event, int id);
VOLT_JS("every") void volt_every(int ms, int id);
VOLT_JS("log") void volt_log(const char* text);
// Der Browser legt Texte im Speicher des Programms ab (z. B. den Wert eines Eingabefelds).
__attribute__((export_name("volt_alloc"))) void* volt_js_alloc(int n) { return volt_alloc(0, (size_t)n, 1); }
#else
static void volt_set_text(const char* selector, const char* text) { printf("[web] setze_text %s = %s\n", selector, text); }
static void volt_set_html(const char* selector, const char* html) { printf("[web] setze_html %s = %s\n", selector, html); }
static void volt_append_html(const char* selector, const char* html) { printf("[web] haenge_an %s += %s\n", selector, html); }
static const char* volt_get_value(const char* selector) { (void)selector; return ""; }
static void volt_set_value(const char* selector, const char* value) { printf("[web] setze_wert %s = %s\n", selector, value); }
static void volt_listen(const char* selector, const char* event, int id) { printf("[web] lausche %s %s -> %d\n", selector, event, id); }
static void volt_every(int ms, int id) { printf("[web] alle %d ms -> %d\n", ms, id); }
static void volt_log(const char* text) { printf("[web] log %s\n", text); }
#endif
#endif
