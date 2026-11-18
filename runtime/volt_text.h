// Teilstrings für std/text.vt. Positionen zählen Bytes (bei Umlauten und Emojis sind das mehrere je Zeichen).
#ifndef VOLT_TEXT_H
#define VOLT_TEXT_H
// `n` Bytes ab `von`; außerhalb des Textes wird auf den Rand gekürzt.
static const char* volt_text_teil(const char* s, long long von, long long n) {
    long long len = (long long)strlen(s);
    if (von < 0) von = 0;
    if (von > len) von = len;
    if (n < 0) n = 0;
    if (n > len - von) n = len - von;
    char* out = volt_str_alloc((size_t)n);
    memcpy(out, s + von, (size_t)n);
    return out;
}
// Erste Fundstelle von `teil` ab Position `ab`, sonst -1. Leerer Teil: -1.
static long long volt_text_suche(const char* s, const char* teil, long long ab) {
    long long len = (long long)strlen(s);
    if (ab < 0) ab = 0;
    if (ab > len || !*teil) return -1;
    const char* hit = strstr(s + ab, teil);
    return hit ? (long long)(hit - s) : -1;
}
// Byte an Position i (0-255), -1 außerhalb.
static long long volt_text_byte(const char* s, long long i) {
    if (i < 0 || i >= (long long)strlen(s)) return -1;
    return (unsigned char)s[i];
}
// 1, wenn der Text nicht leer ist und nur Zeichen aus `erlaubt` enthält.
static int volt_text_nur(const char* s, const char* erlaubt) {
    if (!*s) return 0;
    for (; *s; s++) if (!strchr(erlaubt, *s)) return 0;
    return 1;
}
#endif
