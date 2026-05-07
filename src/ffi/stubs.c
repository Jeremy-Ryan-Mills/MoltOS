/*
 * src/doom/stubs.c
 *
 * C-side stubs for every external symbol doomgeneric needs.
 *
 */

#include <stddef.h>
#include <stdint.h>
#include <stdarg.h>

/* Forward declarations for Rust functions  */

extern void  rust_serial_write(const char *s, size_t len);
extern void *rust_alloc(size_t size, size_t align);
extern void *rust_realloc(void *ptr, size_t old_size, size_t align, size_t new_size);
extern void  rust_dealloc(void *ptr, size_t size, size_t align);
extern void  rust_hlt(void);
extern const unsigned char *rust_fs_open(const char *name, size_t *out_size);

/* compiler-builtins provides these declare so we can call them below */
extern void *memcpy(void *dst, const void *src, size_t n);
extern void *memset(void *dst, int c, size_t n);
extern size_t strlen(const char *s);


/* TODO: size classes for alignment if doom allocates over-aligned types.
 * For now assume 16-byte alignment is always sufficient. */

void *malloc(size_t size) {
    if (size == 0) return (void *)1; /* non-NULL for zero-size */
    void* raw = rust_alloc(size + sizeof(size_t), 16);
    if (!raw) { return NULL; }
    *(size_t *)raw = size;
    return (char *)raw + sizeof(size_t);
}

void free(void *ptr) {
    if (!ptr) return;
    char *raw = (char *)ptr - sizeof(size_t);
    size_t size = *(size_t *)raw;
    rust_free(raw, size + sizeof(size_t), 16);
}

void *calloc(size_t nmemb, size_t size) {
    size_t total = nmemb * size;
    void *p = malloc(total);
    if (p) memset(p, 0, total);
    return p;
}

void *realloc(void *ptr, size_t new_size) {
    if (!ptr) return malloc(new_size);
    if (!new_size) { free(ptr); return NULL; }
    size_t old_size = *((size_t *)((char *)ptr - sizeof(size_t)));
    void *n = malloc(new_size);
    if (n) memcpy(n, ptr, old_size < new_size ? old_size : new_size);
    free(ptr);
    return n;
}

/* DONE: just call through to the real functions, ignore the size check.   */

void *__memcpy_chk(void *dst, const void *src, size_t len, size_t dstlen) {
    (void)dstlen;
    return memcpy(dst, src, len);
}

void *__memset_chk(void *dst, int c, size_t len, size_t dstlen) {
    (void)dstlen;
    return memset(dst, c, len);
}

/* TODO: implement or pull in a no_std string library (e.g. compiler-rt). */
/* The simple ones below are correct but not optimised. */

size_t strlen(const char *s) {
    size_t n = 0;
    while (s[n]) n++;
    return n;
}

int strcmp(const char *a, const char *b) {
    while (*a && *a == *b) { a++; b++; }
    return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n) {
    while (n-- && *a && *a == *b) { a++; b++; }
    return n == (size_t)-1 ? 0 : (unsigned char)*a - (unsigned char)*b;
}

static int to_lower(int c) { return (c >= 'A' && c <= 'Z') ? c + 32 : c; }

int strcasecmp(const char *a, const char *b) {
    while (*a && to_lower(*a) == to_lower(*b)) { a++; b++; }
    return to_lower((unsigned char)*a) - to_lower((unsigned char)*b);
}

int strncasecmp(const char *a, const char *b, size_t n) {
    while (n-- && *a && to_lower(*a) == to_lower(*b)) { a++; b++; }
    return n == (size_t)-1 ? 0 : to_lower((unsigned char)*a) - to_lower((unsigned char)*b);
}

char *strncpy(char *dst, const char *src, size_t n) {
    size_t i;
    for (i = 0; i < n && src[i]; i++) dst[i] = src[i];
    for (; i < n; i++) dst[i] = 0;
    return dst;
}

char *__strncpy_chk(char *dst, const char *src, size_t n, size_t dstlen) {
    (void)dstlen;
    return strncpy(dst, src, n);
}

char *strchr(const char *s, int c) {
    for (; *s; s++) if (*s == (char)c) return (char *)s;
    return c == 0 ? (char *)s : NULL;
}

char *strrchr(const char *s, int c) {
    const char *last = NULL;
    for (; *s; s++) if (*s == (char)c) last = s;
    return (char *)last;
}

char *strstr(const char *haystack, const char *needle) {
    size_t nlen = strlen(needle);
    if (!nlen) return (char *)haystack;
    for (; *haystack; haystack++)
        if (strncmp(haystack, needle, nlen) == 0) return (char *)haystack;
    return NULL;
}

char *strdup(const char *s) {
    size_t n = strlen(s) + 1;
    char *p = malloc(n);
    if (p) memcpy(p, s, n);
    return p;
}

long strtol(const char *s, char **end, int base) {
    /* TODO: minimal implementation — only handles base 10 and 16. */
    while (*s == ' ' || *s == '\t') s++;
    int neg = 0;
    if (*s == '-') { neg = 1; s++; } else if (*s == '+') s++;
    if (base == 0) {
        if (s[0] == '0' && (s[1] == 'x' || s[1] == 'X')) { base = 16; s += 2; }
        else if (s[0] == '0') base = 8;
        else base = 10;
    }
    long val = 0;
    while (*s) {
        int d;
        if (*s >= '0' && *s <= '9') d = *s - '0';
        else if (*s >= 'a' && *s <= 'f') d = *s - 'a' + 10;
        else if (*s >= 'A' && *s <= 'F') d = *s - 'A' + 10;
        else break;
        if (d >= base) break;
        val = val * base + d;
        s++;
    }
    if (end) *end = (char *)s;
    return neg ? -val : val;
}

double strtod(const char *s, char **end) {
    /* TODO: doom uses this for config file parsing; good enough for integers. */
    long i = strtol(s, end, 10);
    return (double)i;
}

int abs(int x) { return x < 0 ? -x : x; }

double fabs(double x) { return x < 0 ? -x : x; }

/* Minimal vsnprintf: handles %s %d %i %u %x %X %c %% with basic width/zero-pad. */
static int fmt_vsnprintf(char *buf, size_t maxlen, const char *fmt, va_list ap) {
    size_t pos = 0;
#define OUT(c) do { if (pos + 1 < maxlen) buf[pos++] = (c); } while(0)

    while (*fmt) {
        if (*fmt != '%') { OUT(*fmt++); continue; }
        fmt++;
        if (*fmt == '\0') break;
        if (*fmt == '%') { OUT('%'); fmt++; continue; }

        /* flags */
        int zero_pad = 0;
        if (*fmt == '-') { fmt++; } /* ignore left-align flag */
        if (*fmt == '0') { zero_pad = 1; fmt++; }

        /* width */
        int width = 0;
        while (*fmt >= '0' && *fmt <= '9') { width = width * 10 + (*fmt++ - '0'); }

        /* precision: .N for integers means minimum N digits (treat as zero-padded width) */
        if (*fmt == '.') {
            fmt++;
            int prec = 0;
            while (*fmt >= '0' && *fmt <= '9') { prec = prec * 10 + (*fmt++ - '0'); }
            if (prec > width) { width = prec; zero_pad = 1; }
        }

        /* long modifier */
        int is_long = 0;
        if (*fmt == 'l') { is_long = 1; fmt++; if (*fmt == 'l') fmt++; }

        char spec = *fmt++;
        if (spec == 'c') {
            OUT((char)va_arg(ap, int));
        } else if (spec == 's') {
            const char *s = va_arg(ap, const char *);
            if (!s) s = "(null)";
            while (*s) OUT(*s++);
        } else if (spec == 'd' || spec == 'i' || spec == 'u' ||
                   spec == 'x' || spec == 'X') {
            unsigned long long val;
            int neg = 0;
            if (spec == 'd' || spec == 'i') {
                long long sv = is_long ? (long long)va_arg(ap, long) : (long long)va_arg(ap, int);
                if (sv < 0) { neg = 1; val = (unsigned long long)-sv; }
                else val = (unsigned long long)sv;
            } else {
                val = is_long ? (unsigned long long)va_arg(ap, unsigned long) : (unsigned long long)va_arg(ap, unsigned int);
            }
            int base = (spec == 'x' || spec == 'X') ? 16 : 10;
            const char *digits = (spec == 'X') ? "0123456789ABCDEF" : "0123456789abcdef";
            char tmp[32]; int tlen = 0;
            if (val == 0) tmp[tlen++] = '0';
            else while (val) { tmp[tlen++] = digits[val % base]; val /= base; }
            int total = tlen + (neg ? 1 : 0);
            char pad = zero_pad ? '0' : ' ';
            if (!zero_pad) while (total < width) { OUT(pad); total++; }
            if (neg) OUT('-');
            if (zero_pad) while (tlen + (neg?1:0) < width) { OUT('0'); width--; }
            for (int k = tlen - 1; k >= 0; k--) OUT(tmp[k]);
        }
    }
    if (pos < maxlen) buf[pos] = '\0';
    else if (maxlen > 0) buf[maxlen-1] = '\0';
    return (int)pos;
#undef OUT
}

static void serial_puts(const char *s) {
    rust_serial_write(s, strlen(s));
}

int puts(const char *s) {
    serial_puts(s);
    rust_serial_write("\n", 1);
    return 0;
}

int vsnprintf(char *buf, size_t maxlen, const char *fmt, va_list ap) {
    return fmt_vsnprintf(buf, maxlen, fmt, ap);
}

int snprintf(char *buf, size_t maxlen, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = fmt_vsnprintf(buf, maxlen, fmt, ap);
    va_end(ap);
    return r;
}

static int vprintf_impl(const char *fmt, va_list ap) {
    char buf[512];
    int r = fmt_vsnprintf(buf, sizeof(buf), fmt, ap);
    serial_puts(buf);
    return r;
}

int printf(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vprintf_impl(fmt, ap);
    va_end(ap);
    return r;
}

int fprintf(void *stream, const char *fmt, ...) {
    (void)stream;
    va_list ap;
    va_start(ap, fmt);
    int r = vprintf_impl(fmt, ap);
    va_end(ap);
    return r;
}

/* GCC emits __printf_chk(flag, fmt, ...) instead of printf when fortified. */
int __printf_chk(int flag, const char *fmt, ...) {
    (void)flag;
    char buf[512];
    va_list ap;
    va_start(ap, fmt);
    fmt_vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    serial_puts(buf);
    return 0;
}

int __fprintf_chk(void *stream, int flag, const char *fmt, ...) {
    (void)stream; (void)flag;
    char buf[512];
    va_list ap;
    va_start(ap, fmt);
    fmt_vsnprintf(buf, sizeof(buf), fmt, ap);
    va_end(ap);
    serial_puts(buf);
    return 0;
}

int toupper(int c) { return (c >= 'a' && c <= 'z') ? c - 32 : c; }
int tolower(int c) { return (c >= 'A' && c <= 'Z') ? c + 32 : c; }
int isdigit(int c) { return c >= '0' && c <= '9'; }
int isspace(int c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v'; }
int isprint(int c) { return c >= 0x20 && c < 0x7f; }
int isalpha(int c) { return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); }
int isalnum(int c) { return isalpha(c) || isdigit(c); }

int atoi(const char *s) { return (int)strtol(s, NULL, 10); }

int sprintf(char *buf, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = fmt_vsnprintf(buf, 4096, fmt, ap);
    va_end(ap);
    return r;
}

int vsprintf(char *buf, const char *fmt, va_list ap) {
    return fmt_vsnprintf(buf, 4096, fmt, ap);
}

int __snprintf_chk(char *buf, size_t maxlen, int flag, size_t buflen, const char *fmt, ...) {
    (void)flag; (void)buflen;
    va_list ap;
    va_start(ap, fmt);
    int r = fmt_vsnprintf(buf, maxlen, fmt, ap);
    va_end(ap);
    return r;
}

int __vsnprintf_chk(char *buf, size_t maxlen, int flag, size_t buflen, const char *fmt, va_list ap) {
    (void)flag; (void)buflen;
    return fmt_vsnprintf(buf, maxlen, fmt, ap);
}

int __isoc99_sscanf(const char *str, const char *fmt, ...) {
    /* TODO: doom uses sscanf to parse config values.
     * Minimal implementation needed for numbers. */
    (void)str; (void)fmt;
    return 0;
}

/* DONE: doom only uses these via strcasecmp internally; our strcasecmp
 * doesn't call them so these stubs just need to exist and not crash. */

static unsigned short ctype_table[384] = {0};
static const unsigned short *ctype_ptr = ctype_table + 128;

const unsigned short **__ctype_b_loc(void) {
    return &ctype_ptr;
}

static int toupper_table[384] = {0};
static const int *toupper_ptr = toupper_table + 128;

const int **__ctype_toupper_loc(void) {
    return &toupper_ptr;
}

static int errno_val = 0;

int *__errno_location(void) {
    return &errno_val;
}

/* TODO: back fopen with rust_fs_open so doom can load doom1.wad from CXFS. */

typedef struct {
    const unsigned char *data;
    size_t size;
    size_t pos;
} FILE;

/* stdout/stderr just need to be non-NULL valid pointers; fprintf ignores them
 * since our __fprintf_chk drops the stream argument. */
static FILE _stdout = {NULL, 0, 0};
static FILE _stderr = {NULL, 0, 0};
FILE *stdout = &_stdout;
FILE *stderr = &_stderr;

FILE *fopen(const char *path, const char *mode) {
    (void)mode;
    size_t size = 0;
    const unsigned char *data = rust_fs_open(path, &size);
    if (!data) return NULL;
    FILE *f = malloc(sizeof(FILE));
    if (!f) return NULL;
    f->data = data;
    f->size = size;
    f->pos  = 0;
    return f;
}

int fclose(FILE *f) {
    free(f);
    return 0;
}

size_t fread(void *buf, size_t size, size_t nmemb, FILE *f) {
    if (!f || !f->data) return 0;
    size_t bytes = size * nmemb;
    size_t avail = f->size - f->pos;
    if (bytes > avail) bytes = avail;
    memcpy(buf, f->data + f->pos, bytes);
    f->pos += bytes;
    return bytes / size;
}

size_t fwrite(const void *buf, size_t size, size_t nmemb, FILE *f) {
    /* TODO: doom uses fwrite for save files. Needs a writable backing store. */
    (void)buf; (void)size; (void)nmemb; (void)f;
    return 0;
}

int fseek(FILE *f, long offset, int whence) {
    if (!f) return -1;
    size_t pos;
    if      (whence == 0) pos = (size_t)offset;              /* SEEK_SET */
    else if (whence == 1) pos = (size_t)(f->pos + offset);   /* SEEK_CUR */
    else                  pos = (size_t)(f->size + offset);   /* SEEK_END */
    if (pos > f->size) return -1;
    f->pos = pos;
    return 0;
}

long ftell(FILE *f) {
    return f ? (long)f->pos : -1;
}

int fflush(FILE *f) {
    (void)f;
    return 0;
}

/* DONE: doom uses these for save files and screenshots; return error for now. */

int mkdir(const char *path, unsigned int mode) { (void)path; (void)mode; return -1; }
int remove(const char *path) { (void)path; return -1; }
int rename(const char *old, const char *new) { (void)old; (void)new; return -1; }


void exit(int code) {
    (void)code;
    rust_hlt();
}


/* I_GetTime, I_GetTimeMS, I_Sleep, I_WaitVBL — defined in i_timer.c.
 * That file calls DG_GetTicksMs and DG_SleepMs, which are in src/doom/mod.rs. */

void I_Error(const char *error, ...) {
    char buf[512];
    va_list ap;
    va_start(ap, error);
    fmt_vsnprintf(buf, sizeof(buf), error, ap);
    va_end(ap);
    serial_puts("DOOM ERROR: ");
    serial_puts(buf);
    rust_serial_write("\n", 1);
    rust_hlt();
}

void I_Quit(void) {
    rust_hlt();
}

void I_AtExit(void (*func)(void), int run_on_error) {
    /* TODO: store in a small callback list and call from I_Quit/I_Error.
     * Doom registers a few cleanup functions here. */
    (void)func; (void)run_on_error;
}

void *I_ZoneBase(int *size) {
    *size = 4 * 1024 * 1024;
    return malloc((size_t)*size);
}

int I_ConsoleStdout(void)             { return 1; }
void I_Tactile(int on, int off, int total) { (void)on; (void)off; (void)total; }
void I_GetMemoryValue(unsigned int offset, void *value, int size) {
    (void)offset; (void)value; (void)size;
}
void I_PrintBanner(const char *msg)          { serial_puts(msg); }
void I_PrintDivider(void)                    { serial_puts("---\n"); }
void I_PrintStartupBanner(const char *msg)   { serial_puts(msg); }

/* DONE: doom runs without audio. */

int  snd_musicdevice = 0;

void I_InitSound(int full_init)          { (void)full_init; }
void I_ShutdownSound(void)               {}
int  I_GetSfxLumpNum(void *sfxinfo)      { (void)sfxinfo; return -1; }
void I_PrecacheSounds(void *sounds, int num) { (void)sounds; (void)num; }
int  I_StartSound(void *sfxinfo, int channel, int vol, int sep) {
    (void)sfxinfo; (void)channel; (void)vol; (void)sep; return -1;
}
void I_StopSound(int handle)             { (void)handle; }
int  I_SoundIsPlaying(int handle)        { (void)handle; return 0; }
void I_UpdateSoundParams(int handle, int vol, int sep) {
    (void)handle; (void)vol; (void)sep;
}
void I_UpdateSound(void)                 {}
void I_BindSoundVariables(void)          {}

void I_InitMusic(void)                   {}
void I_ShutdownMusic(void)               {}
void I_SetMusicVolume(int volume)        { (void)volume; }
void I_PauseSong(void)                   {}
void I_ResumeSong(void)                  {}
void *I_RegisterSong(void *data, int len) { (void)data; (void)len; return NULL; }
void I_PlaySong(void *handle, int looping) { (void)handle; (void)looping; }
void I_StopSong(void)                    {}
int  I_MusicIsPlaying(void)              { return 0; }
void I_UnRegisterSong(void *handle)      { (void)handle; }
