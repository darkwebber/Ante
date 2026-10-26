/*
 * "Ante" runtime shim (spec Appendix E, M7): the few things compiled
 * programs need from the operating system — buffered output, traps
 * (§20.3), memory, text helpers, a stable sort, files and time.
 *
 * Generated code passes aggregates by pointer, so this file never depends
 * on how a C compiler passes structs. Text and vectors share one layout:
 * { pointer, length, capacity }; capacity 0 marks static (literal) data.
 */

#include <errno.h>
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#include <windows.h>
#else
#include <pthread.h>
#include <signal.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

typedef struct {
    uint8_t *ptr;
    int64_t len;
    int64_t cap;
} ante_str;

typedef ante_str ante_vec;

static int ante_argc;
static char **ante_argv;
static ante_str out_buf;

#if defined(__GNUC__) || defined(__clang__)
#define NORETURN __attribute__((noreturn))
#else
#define NORETURN
#endif

/* ── output ───────────────────────────────────────────────────────────── */

void ante_flush(void) {
    if (out_buf.len > 0) {
        fwrite(out_buf.ptr, 1, (size_t)out_buf.len, stdout);
        out_buf.len = 0;
    }
    fflush(stdout);
}

/* T09 (§20.3). The runtime doesn't know the source position here. */
static NORETURN void out_of_memory(void) {
    ante_flush();
    fputs("kompa munigindhi! memory ayipoyindhi\n", stderr);
    exit(101);
}

/* `a * b` for sizes; a negative or overflowing size is T09, never a short
   allocation. */
static int64_t size_mul(int64_t a, int64_t b) {
    if (a < 0 || b < 0 || (b != 0 && a > INT64_MAX / b)) out_of_memory();
    return a * b;
}

static int64_t size_add(int64_t a, int64_t b) {
    if (a < 0 || b < 0 || a > INT64_MAX - b) out_of_memory();
    return a + b;
}

/* The next capacity of at least `need`, doubling from `cap`. */
static int64_t grow_cap(int64_t cap, int64_t need, int64_t first) {
    if (cap < first) cap = first;
    while (cap < need) cap = size_mul(cap, 2);
    return cap;
}

static void buf_grow(ante_str *b, int64_t extra) {
    int64_t need = size_add(b->len, extra);
    if (need <= b->cap) return;
    int64_t cap = grow_cap(b->cap, need, 64);
    uint8_t *p = (uint8_t *)malloc((size_t)cap);
    if (!p) out_of_memory();
    if (b->len) memcpy(p, b->ptr, (size_t)b->len);
    if (b->cap) free(b->ptr);
    b->ptr = p;
    b->cap = cap;
}

void ante_buf_push(ante_str *b, const uint8_t *p, int64_t n) {
    if (n <= 0) return;
    buf_grow(b, n);
    memcpy(b->ptr + b->len, p, (size_t)n);
    b->len += n;
}

static void push_c(ante_str *b, const char *s) { ante_buf_push(b, (const uint8_t *)s, (int64_t)strlen(s)); }

/* Writes text to stdout (fd 1, buffered) or stderr (fd 2). */
void ante_write(int32_t fd, const ante_str *s, int32_t newline) {
    if (fd == 2) {
        ante_flush();
        fwrite(s->ptr, 1, (size_t)s->len, stderr);
        if (newline) fputc('\n', stderr);
        return;
    }
    ante_buf_push(&out_buf, s->ptr, s->len);
    if (newline) ante_buf_push(&out_buf, (const uint8_t *)"\n", 1);
    if (out_buf.len > (1 << 16)) ante_flush();
}

void ante_init(int32_t argc, char **argv) {
    ante_argc = argc;
    ante_argv = argv;
#ifdef _WIN32
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stderr), _O_BINARY);
#endif
}

NORETURN void ante_exit(int32_t code) {
    ante_flush();
    exit(code);
}

/* ── program start and T08 ────────────────────────────────────────────── */

/* `modhalu` runs on its own thread with the stack size from `[build] stack`
   (§7.1). Below that stack sits a guard region; touching it is a stack
   overflow, reported as T08 instead of a bare crash. The position isn't
   known there, so the message has none (§20.3). */

typedef void (*ante_entry_fn)(void);
static ante_entry_fn entry_fn;
static const char overflow_msg[] = "kompa munigindhi! stack nindipoyindhi (chaala lothu recursion)\n";

#ifdef _WIN32
static LONG WINAPI on_exception(EXCEPTION_POINTERS *e) {
    if (e->ExceptionRecord->ExceptionCode == EXCEPTION_STACK_OVERFLOW) {
        DWORD n;
        if (out_buf.len > 0) WriteFile(GetStdHandle(STD_OUTPUT_HANDLE), out_buf.ptr, (DWORD)out_buf.len, &n, NULL);
        WriteFile(GetStdHandle(STD_ERROR_HANDLE), overflow_msg, (DWORD)(sizeof overflow_msg - 1), &n, NULL);
        ExitProcess(101);
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

static DWORD WINAPI program_thread(LPVOID arg) {
    (void)arg;
    ULONG guarantee = 64 * 1024; /* room for the handler itself */
    SetThreadStackGuarantee(&guarantee);
    entry_fn();
    return 0;
}

NORETURN void ante_start(int32_t argc, char **argv, ante_entry_fn entry, int64_t stack) {
    ante_init(argc, argv);
    entry_fn = entry;
    AddVectoredExceptionHandler(1, on_exception);
    HANDLE t = CreateThread(NULL, (SIZE_T)stack, program_thread, NULL, STACK_SIZE_PARAM_IS_A_RESERVATION, NULL);
    if (t)
        WaitForSingleObject(t, INFINITE);
    else
        entry();
    ante_exit(0);
}
#else
static uint8_t *guard_lo, *guard_hi;

static void on_fault(int sig, siginfo_t *info, void *ctx) {
    (void)ctx;
    uint8_t *addr = (uint8_t *)info->si_addr;
    if (addr >= guard_lo && addr < guard_hi) {
        if (out_buf.len > 0 && write(1, out_buf.ptr, (size_t)out_buf.len) < 0) _exit(101);
        if (write(2, overflow_msg, sizeof overflow_msg - 1) < 0) _exit(101);
        _exit(101);
    }
    /* Some other crash: let it happen as usual. */
    signal(sig, SIG_DFL);
}

static void *program_thread(void *arg) {
    (void)arg;
    static uint8_t alt[64 * 1024]; /* the handler runs here, off the full stack */
    stack_t ss;
    memset(&ss, 0, sizeof ss);
    ss.ss_sp = alt;
    ss.ss_size = sizeof alt;
    sigaltstack(&ss, NULL);
    entry_fn();
    return NULL;
}

NORETURN void ante_start(int32_t argc, char **argv, ante_entry_fn entry, int64_t stack) {
    ante_init(argc, argv);
    entry_fn = entry;
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    size_t guard = ((size_t)1 << 20) / page * page;
    size_t size = ((size_t)(stack < (1 << 16) ? (1 << 16) : stack) + page - 1) / page * page;
    uint8_t *mem = (uint8_t *)mmap(NULL, guard + size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
    pthread_attr_t attr;
    pthread_t t;
    if (mem != MAP_FAILED && mprotect(mem, guard, PROT_NONE) == 0 && pthread_attr_init(&attr) == 0 &&
        pthread_attr_setstack(&attr, mem + guard, size) == 0) {
        guard_lo = mem;
        guard_hi = mem + guard;
        struct sigaction sa;
        memset(&sa, 0, sizeof sa);
        sa.sa_sigaction = on_fault;
        sa.sa_flags = SA_SIGINFO | SA_ONSTACK;
        sigemptyset(&sa.sa_mask);
        sigaction(SIGSEGV, &sa, NULL);
        sigaction(SIGBUS, &sa, NULL);
        if (pthread_create(&t, &attr, program_thread, NULL) == 0) {
            pthread_join(t, NULL);
            ante_exit(0);
        }
    }
    entry(); /* couldn't make the thread: run on the main stack */
    ante_exit(0);
}
#endif

/* ── traps (§20.3) ────────────────────────────────────────────────────── */

typedef struct {
    const uint8_t *file;
    int64_t file_len;
    int32_t line;
    int32_t col;
} ante_loc;

static NORETURN void trap_text(const char *msg, const ante_loc *loc) {
    ante_flush();
    fprintf(stderr, "kompa munigindhi! %s (%.*s:%d:%d)\n", msg, (int)loc->file_len, (const char *)loc->file,
            loc->line, loc->col);
    exit(101);
}

NORETURN void ante_trap(const ante_str *msg, const ante_loc *loc) {
    char text[1024];
    int n = msg->len < 1000 ? (int)msg->len : 1000;
    memcpy(text, msg->ptr, (size_t)n);
    text[n] = 0;
    trap_text(text, loc);
}

NORETURN void ante_trap_overflow(const char *ty, const char *op, int64_t a, int64_t b, int32_t unary,
                                 int32_t is_unsigned, const ante_loc *loc) {
    char text[256];
    if (unary)
        snprintf(text, sizeof text, "`%s` lo -(%lld) pattadhu", ty, (long long)a);
    else if (is_unsigned)
        snprintf(text, sizeof text, "`%s` lo %llu %s %llu pattadhu", ty, (unsigned long long)a, op,
                 (unsigned long long)b);
    else
        snprintf(text, sizeof text, "`%s` lo %lld %s %lld pattadhu", ty, (long long)a, op, (long long)b);
    trap_text(text, loc);
}

NORETURN void ante_trap_index(int64_t i, int64_t len, const ante_loc *loc) {
    char text[128];
    snprintf(text, sizeof text, "index %lld bayatiki vellindhi (length %lld)", (long long)i, (long long)len);
    trap_text(text, loc);
}

NORETURN void ante_trap_slice(int64_t lo, int64_t hi, int64_t len, const ante_loc *loc) {
    char text[128];
    snprintf(text, sizeof text, "mukka haddulu tappu: %lld nunchi %lld (length %lld)", (long long)lo, (long long)hi,
             (long long)len);
    trap_text(text, loc);
}

NORETURN void ante_trap_shift(int64_t count, int32_t bits, const ante_loc *loc) {
    char text[128];
    snprintf(text, sizeof text, "shift count %lld tappu (0..%d undali)", (long long)count, bits);
    trap_text(text, loc);
}

NORETURN void ante_trap_code(int32_t code, const ante_loc *loc) {
    switch (code) {
    case 2: trap_text("sunna (0) tho bhaagham cheyyalem", loc);
    case 6: trap_text("`ledhu` meeda kachithanga() pilicharu", loc);
    case 7: trap_text("nijame() fail ayyindhi", loc);
    default: trap_text("antec bug: unreachable code ki vacchindhi", loc);
    }
}

/* ── memory ───────────────────────────────────────────────────────────── */

void *ante_alloc(int64_t size) {
    if (size < 0) out_of_memory();
    void *p = malloc(size > 0 ? (size_t)size : 1);
    if (!p) out_of_memory();
    return p;
}

/* Frees a `padham` or `varusa` buffer. `cap == 0` means it owns nothing
   (an empty value, or a literal in static memory). */
void ante_buf_free(ante_str *b) {
    if (b->cap) free(b->ptr);
    b->ptr = 0;
    b->len = 0;
    b->cap = 0;
}

/* A closure's environment starts with a reference count and a function
   that drops the captured values (spec §10.7.4). A named function used as
   a `pani` has no environment (NULL). Counts are atomic so tasks can share
   closures. */
typedef struct {
    int64_t rc;
    void (*drop)(void *env);
} ante_closure_env;

void ante_pani_inc(void *env) {
    if (env) __atomic_fetch_add(&((ante_closure_env *)env)->rc, 1, __ATOMIC_RELAXED);
}

void ante_pani_dec(void *env) {
    if (!env) return;
    ante_closure_env *e = (ante_closure_env *)env;
    if (__atomic_fetch_sub(&e->rc, 1, __ATOMIC_ACQ_REL) == 1) {
        e->drop(env);
        free(env);
    }
}

/* Appends one element of `size` bytes to a vector. */
void ante_vec_push(ante_vec *v, const void *elem, int64_t size) {
    if (v->len == v->cap) {
        int64_t cap = grow_cap(v->cap, size_add(v->len, 1), 4);
        uint8_t *p = (uint8_t *)ante_alloc(size_mul(cap, size));
        if (v->len) memcpy(p, v->ptr, (size_t)size_mul(v->len, size));
        if (v->cap) free(v->ptr);
        v->ptr = p;
        v->cap = cap;
    }
    memcpy(v->ptr + v->len * size, elem, (size_t)size);
    v->len += 1;
}

/* Appends `n` elements. */
void ante_vec_extend(ante_vec *v, const void *elems, int64_t n, int64_t size) {
    for (int64_t i = 0; i < n; i++) ante_vec_push(v, (const uint8_t *)elems + i * size, size);
}

typedef int32_t (*ante_cmp_fn)(const void *, const void *, void *);

static void merge_sort(uint8_t *a, uint8_t *tmp, int64_t n, int64_t size, ante_cmp_fn cmp, void *ctx) {
    if (n < 2) return;
    int64_t mid = n / 2;
    merge_sort(a, tmp, mid, size, cmp, ctx);
    merge_sort(a + mid * size, tmp, n - mid, size, cmp, ctx);
    int64_t i = 0, j = mid, k = 0;
    while (i < mid && j < n) {
        if (cmp(a + j * size, a + i * size, ctx) < 0)
            memcpy(tmp + (k++) * size, a + (j++) * size, (size_t)size);
        else
            memcpy(tmp + (k++) * size, a + (i++) * size, (size_t)size);
    }
    while (i < mid) memcpy(tmp + (k++) * size, a + (i++) * size, (size_t)size);
    while (j < n) memcpy(tmp + (k++) * size, a + (j++) * size, (size_t)size);
    memcpy(a, tmp, (size_t)(n * size));
}

/* Stable sort (§21.2: all sorts are stable). */
void ante_sort(void *base, int64_t n, int64_t size, ante_cmp_fn cmp, void *ctx) {
    if (n < 2) return;
    uint8_t *tmp = (uint8_t *)ante_alloc(size_mul(n, size));
    merge_sort((uint8_t *)base, tmp, n, size, cmp, ctx);
    free(tmp);
}

/* ── formatting ───────────────────────────────────────────────────────── */

void ante_buf_i64(ante_str *b, int64_t v) {
    char t[32];
    snprintf(t, sizeof t, "%lld", (long long)v);
    push_c(b, t);
}

void ante_buf_u64(ante_str *b, uint64_t v) {
    char t[32];
    snprintf(t, sizeof t, "%llu", (unsigned long long)v);
    push_c(b, t);
}

void ante_buf_hex(ante_str *b, uint64_t v) {
    char t[32];
    snprintf(t, sizeof t, "%llx", (unsigned long long)v);
    push_c(b, t);
}

void ante_buf_char(ante_str *b, uint32_t c) {
    uint8_t t[4];
    int n;
    if (c < 0x80) {
        t[0] = (uint8_t)c;
        n = 1;
    } else if (c < 0x800) {
        t[0] = (uint8_t)(0xC0 | (c >> 6));
        t[1] = (uint8_t)(0x80 | (c & 0x3F));
        n = 2;
    } else if (c < 0x10000) {
        t[0] = (uint8_t)(0xE0 | (c >> 12));
        t[1] = (uint8_t)(0x80 | ((c >> 6) & 0x3F));
        t[2] = (uint8_t)(0x80 | (c & 0x3F));
        n = 3;
    } else {
        t[0] = (uint8_t)(0xF0 | (c >> 18));
        t[1] = (uint8_t)(0x80 | ((c >> 12) & 0x3F));
        t[2] = (uint8_t)(0x80 | ((c >> 6) & 0x3F));
        t[3] = (uint8_t)(0x80 | (c & 0x3F));
        n = 4;
    }
    ante_buf_push(b, t, n);
}

/*
 * Floats in the shortest form that reads back exactly, positional (no
 * exponent); whole numbers get ".0", however big (spec §13.5).
 */
static void fmt_float(ante_str *b, double v, int is32) {
    if (isnan(v)) { push_c(b, "NaN"); return; }
    if (isinf(v)) { push_c(b, v > 0 ? "inf" : "-inf"); return; }
    char e[64];
    int prec;
    for (prec = 1; prec <= 17; prec++) {
        snprintf(e, sizeof e, "%.*e", prec - 1, v);
        double back = strtod(e, NULL);
        if (is32 ? (float)back == (float)v : back == v) break;
    }
    /* e = [-]d[.ddd]e[+-]XX */
    char digits[40];
    int nd = 0, neg = 0;
    const char *p = e;
    if (*p == '-') { neg = 1; p++; }
    while (*p && *p != 'e') {
        if (*p >= '0' && *p <= '9') digits[nd++] = *p;
        p++;
    }
    int exp10 = atoi(p + 1);
    while (nd > 1 && digits[nd - 1] == '0') nd--;
    char out[400];
    int k = 0;
    if (neg) out[k++] = '-';
    int point = exp10 + 1; /* digits before the decimal point */
    if (point <= 0) {
        out[k++] = '0';
        out[k++] = '.';
        for (int i = 0; i < -point; i++) out[k++] = '0';
        for (int i = 0; i < nd; i++) out[k++] = digits[i];
    } else {
        for (int i = 0; i < point; i++) out[k++] = i < nd ? digits[i] : '0';
        if (nd > point) {
            out[k++] = '.';
            for (int i = point; i < nd; i++) out[k++] = digits[i];
        } else {
            out[k++] = '.';
            out[k++] = '0';
        }
    }
    ante_buf_push(b, (const uint8_t *)out, k);
}

void ante_buf_f64(ante_str *b, double v) { fmt_float(b, v, 0); }
void ante_buf_f32(ante_str *b, float v) { fmt_float(b, (double)v, 1); }

/* ── text (padham) ────────────────────────────────────────────────────── */

void ante_str_concat(ante_str *out, const ante_str *a, const ante_str *b) {
    ante_str r = {0, 0, 0};
    buf_grow(&r, size_add(size_add(a->len, b->len), 1));
    ante_buf_push(&r, a->ptr, a->len);
    ante_buf_push(&r, b->ptr, b->len);
    *out = r;
}

/* `s += t`: appends in place. `t` may be `s` itself or point into it. */
void ante_str_append(ante_str *s, const ante_str *t) {
    int64_t n = t->len;
    if (n <= 0) return;
    const uint8_t *src = t->ptr;
    int inside = s->len > 0 && src >= s->ptr && src < s->ptr + s->len;
    int64_t off = inside ? (int64_t)(src - s->ptr) : 0;
    buf_grow(s, n);
    if (inside) src = s->ptr + off;
    memcpy(s->ptr + s->len, src, (size_t)n);
    s->len += n;
}

void ante_str_copy(ante_str *out, const ante_str *s) {
    ante_str r = {0, 0, 0};
    buf_grow(&r, s->len + 1);
    ante_buf_push(&r, s->ptr, s->len);
    *out = r;
}

int32_t ante_str_cmp(const ante_str *a, const ante_str *b) {
    int64_t n = a->len < b->len ? a->len : b->len;
    int c = n ? memcmp(a->ptr, b->ptr, (size_t)n) : 0;
    if (c) return c < 0 ? -1 : 1;
    return a->len < b->len ? -1 : (a->len > b->len ? 1 : 0);
}

/* Is `c` in a sorted table of inclusive ranges? (Unicode properties.) */
bool ante_char_in(int32_t c, const int32_t *ranges, int64_t n) {
    int64_t lo = 0, hi = n;
    while (lo < hi) {
        int64_t mid = lo + (hi - lo) / 2;
        if (c < ranges[2 * mid]) hi = mid;
        else if (c > ranges[2 * mid + 1]) lo = mid + 1;
        else return true;
    }
    return false;
}

int64_t ante_str_chars(const ante_str *s) {
    int64_t n = 0;
    for (int64_t i = 0; i < s->len; i++)
        if ((s->ptr[i] & 0xC0) != 0x80) n++;
    return n;
}

/* Decodes the character at byte `*pos`, advancing it; returns 0 at the end. */
int32_t ante_str_next_char(const ante_str *s, int64_t *pos, uint32_t *c) {
    int64_t i = *pos;
    if (i >= s->len) return 0;
    uint8_t b0 = s->ptr[i];
    uint32_t v;
    int n;
    if (b0 < 0x80) { v = b0; n = 1; }
    else if ((b0 >> 5) == 6) { v = b0 & 0x1F; n = 2; }
    else if ((b0 >> 4) == 14) { v = b0 & 0x0F; n = 3; }
    else { v = b0 & 0x07; n = 4; }
    for (int k = 1; k < n && i + k < s->len; k++) v = (v << 6) | (s->ptr[i + k] & 0x3F);
    *pos = i + n;
    *c = v;
    return 1;
}

static int is_space(uint8_t c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v'; }

void ante_str_trim(ante_str *out, const ante_str *s) {
    int64_t a = 0, b = s->len;
    while (a < b && is_space(s->ptr[a])) a++;
    while (b > a && is_space(s->ptr[b - 1])) b--;
    ante_str r = {s->ptr + a, b - a, 0};
    ante_str_copy(out, &r);
}

int32_t ante_str_starts(const ante_str *s, const ante_str *p) {
    return p->len <= s->len && memcmp(s->ptr, p->ptr, (size_t)p->len) == 0;
}

int32_t ante_str_contains(const ante_str *s, const ante_str *p) {
    if (p->len == 0) return 1;
    for (int64_t i = 0; i + p->len <= s->len; i++)
        if (memcmp(s->ptr + i, p->ptr, (size_t)p->len) == 0) return 1;
    return 0;
}

/* Splits `s` by `sep` into a vector of text. */
void ante_str_split(ante_vec *out, const ante_str *s, const ante_str *sep) {
    ante_vec v = {0, 0, 0};
    if (sep->len == 0) {
        int64_t pos = 0, start;
        uint32_t c;
        while ((start = pos), ante_str_next_char(s, &pos, &c)) {
            ante_str part = {s->ptr + start, pos - start, 0}, copy;
            ante_str_copy(&copy, &part);
            ante_vec_push(&v, &copy, sizeof copy);
        }
        *out = v;
        return;
    }
    int64_t start = 0;
    for (int64_t i = 0; i + sep->len <= s->len;) {
        if (memcmp(s->ptr + i, sep->ptr, (size_t)sep->len) == 0) {
            ante_str part = {s->ptr + start, i - start, 0}, copy;
            ante_str_copy(&copy, &part);
            ante_vec_push(&v, &copy, sizeof copy);
            i += sep->len;
            start = i;
        } else {
            i++;
        }
    }
    ante_str part = {s->ptr + start, s->len - start, 0}, copy;
    ante_str_copy(&copy, &part);
    ante_vec_push(&v, &copy, sizeof copy);
    *out = v;
}

/* Parses an integer in `base`; 1 on success. Accepts a leading sign. */
int32_t ante_str_parse_int(const ante_str *s, int32_t base, int32_t is_unsigned, int64_t *out) {
    if (s->len == 0 || s->len > 70 || base < 2 || base > 36) return 0;
    int64_t i = 0;
    int neg = 0;
    if (s->ptr[0] == '+' || s->ptr[0] == '-') {
        neg = s->ptr[0] == '-';
        i = 1;
        if (s->len == 1) return 0;
    }
    unsigned __int128 v = 0;
    for (; i < s->len; i++) {
        int c = s->ptr[i], d;
        if (c >= '0' && c <= '9') d = c - '0';
        else if (c >= 'a' && c <= 'z') d = c - 'a' + 10;
        else if (c >= 'A' && c <= 'Z') d = c - 'A' + 10;
        else return 0;
        if (d >= base) return 0;
        v = v * (unsigned)base + (unsigned)d;
        if (v > ((unsigned __int128)1 << 64)) return 0;
    }
    if (is_unsigned) {
        if (neg && v != 0) return 0;
        if (v > UINT64_MAX) return 0;
        *out = (int64_t)(uint64_t)v;
        return 1;
    }
    if (neg) {
        if (v > (unsigned __int128)INT64_MAX + 1) return 0;
        *out = (int64_t)(0 - (uint64_t)v);
    } else {
        if (v > INT64_MAX) return 0;
        *out = (int64_t)v;
    }
    return 1;
}

int32_t ante_str_parse_float(const ante_str *s, double *out) {
    char t[128];
    if (s->len == 0 || s->len >= (int64_t)sizeof t) return 0;
    memcpy(t, s->ptr, (size_t)s->len);
    t[s->len] = 0;
    char *end;
    errno = 0;
    double v = strtod(t, &end);
    while (*end && is_space((uint8_t)*end)) end++;
    if (*end || end == t) return 0;
    *out = v;
    return 1;
}

/* ── files, system, time ──────────────────────────────────────────────── */

static char *cstr(const ante_str *s) {
    char *p = (char *)ante_alloc(size_add(s->len, 1));
    memcpy(p, s->ptr, (size_t)s->len);
    p[s->len] = 0;
    return p;
}

static void set_err(ante_str *err, const char *what) {
    ante_str r = {0, 0, 0};
    push_c(&r, what);
    *err = r;
}

/* The text of an I/O error: the same words as the interpreter's `io_text`. */
static void io_error(ante_str *err, int code) {
    switch (code) {
    case ENOENT: set_err(err, "aa file ledhu"); return;
    case EACCES:
    case EPERM: set_err(err, "permission ledhu"); return;
    case EISDIR: set_err(err, "adhi file kaadhu, folder"); return;
    case ENOTDIR: set_err(err, "path lo oka bhaagam folder kaadhu"); return;
    case EEXIST: set_err(err, "adhi mundhe undhi"); return;
    case ENOSPC: set_err(err, "disk nindipoyindhi"); return;
    default: set_err(err, strerror(code));
    }
}

/* Opening a folder as a file: POSIX says EISDIR, Windows says "access
   denied". Both read as a folder error, the same as the interpreter. */
static int is_folder(const char *p) {
#ifdef _WIN32
    DWORD a = GetFileAttributesA(p);
    return a != INVALID_FILE_ATTRIBUTES && (a & FILE_ATTRIBUTE_DIRECTORY);
#else
    struct stat st;
    return stat(p, &st) == 0 && S_ISDIR(st.st_mode);
#endif
}

static void open_error(ante_str *err, int code, const char *p) {
    io_error(err, (code == EACCES || code == EPERM) && is_folder(p) ? EISDIR : code);
}

/* Is this valid UTF-8 (no overlongs, surrogates or values past U+10FFFF)? */
static int utf8_valid(const uint8_t *s, int64_t n) {
    int64_t i = 0;
    while (i < n) {
        uint8_t c = s[i];
        int len;
        uint32_t min, cp;
        if (c < 0x80) { i++; continue; }
        else if ((c & 0xE0) == 0xC0) { len = 2; min = 0x80; cp = c & 0x1F; }
        else if ((c & 0xF0) == 0xE0) { len = 3; min = 0x800; cp = c & 0x0F; }
        else if ((c & 0xF8) == 0xF0) { len = 4; min = 0x10000; cp = c & 0x07; }
        else return 0;
        if (i + len > n) return 0;
        for (int k = 1; k < len; k++) {
            if ((s[i + k] & 0xC0) != 0x80) return 0;
            cp = (cp << 6) | (s[i + k] & 0x3F);
        }
        if (cp < min || cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF)) return 0;
        i += len;
    }
    return 1;
}

int32_t ante_file_read(const ante_str *path, ante_str *out, ante_str *err) {
    char *p = cstr(path);
    FILE *f = fopen(p, "rb");
    if (!f) {
        open_error(err, errno, p);
        free(p);
        return 0;
    }
    free(p);
    ante_str r = {0, 0, 0};
    uint8_t chunk[8192];
    size_t n;
    while ((n = fread(chunk, 1, sizeof chunk, f)) > 0) ante_buf_push(&r, chunk, (int64_t)n);
    int bad = ferror(f) ? errno : 0;
    fclose(f);
    if (bad) {
        io_error(err, bad);
        return 0;
    }
    /* `padham` is always valid UTF-8 (§9.1). */
    if (!utf8_valid(r.ptr, r.len)) {
        set_err(err, "file lo text valid UTF-8 kaadhu");
        return 0;
    }
    *out = r;
    return 1;
}

int32_t ante_file_write(const ante_str *path, const ante_str *text, ante_str *err) {
    char *p = cstr(path);
    FILE *f = fopen(p, "wb");
    if (!f) {
        open_error(err, errno, p);
        free(p);
        return 0;
    }
    free(p);
    size_t wrote = fwrite(text->ptr, 1, (size_t)text->len, f);
    int bad = wrote != (size_t)text->len ? errno : 0;
    if (fclose(f) != 0 && !bad) bad = errno;
    if (bad) {
        io_error(err, bad);
        return 0;
    }
    return 1;
}

/* Files and folders both count, readable or not (like the interpreter). */
int32_t ante_file_exists(const ante_str *path) {
    char *p = cstr(path);
#ifdef _WIN32
    int found = GetFileAttributesA(p) != INVALID_FILE_ATTRIBUTES;
#else
    struct stat st;
    int found = stat(p, &st) == 0;
#endif
    free(p);
    return found;
}

void ante_args(ante_vec *out) {
    ante_vec v = {0, 0, 0};
    for (int i = 1; i < ante_argc; i++) {
        ante_str s = {0, 0, 0};
        push_c(&s, ante_argv[i]);
        ante_vec_push(&v, &s, sizeof s);
    }
    *out = v;
}

int32_t ante_env(const ante_str *k, ante_str *out) {
    char *p = cstr(k);
    const char *v = getenv(p);
    free(p);
    if (!v) return 0;
    ante_str r = {0, 0, 0};
    push_c(&r, v);
    *out = r;
    return 1;
}

int32_t ante_read_line(ante_str *out) {
    ante_flush();
    ante_str r = {0, 0, 0};
    int c, any = 0;
    while ((c = fgetc(stdin)) != EOF) {
        any = 1;
        if (c == '\n') break;
        uint8_t ch = (uint8_t)c;
        ante_buf_push(&r, &ch, 1);
    }
    if (!any) return 0;
    if (r.len > 0 && r.ptr[r.len - 1] == '\r') r.len--;
    *out = r;
    return 1;
}

void ante_os(ante_str *out) {
    ante_str r = {0, 0, 0};
#if defined(_WIN32)
    push_c(&r, "windows");
#elif defined(__APPLE__)
    push_c(&r, "macos");
#else
    push_c(&r, "linux");
#endif
    *out = r;
}

void ante_arch(ante_str *out) {
    ante_str r = {0, 0, 0};
#if defined(__aarch64__) || defined(_M_ARM64)
    push_c(&r, "aarch64");
#else
    push_c(&r, "x86_64");
#endif
    *out = r;
}

int64_t ante_now_ns(void) {
#ifdef _WIN32
    LARGE_INTEGER f, c;
    QueryPerformanceFrequency(&f);
    QueryPerformanceCounter(&c);
    return (int64_t)((double)c.QuadPart * 1e9 / (double)f.QuadPart);
#else
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (int64_t)t.tv_sec * 1000000000 + t.tv_nsec;
#endif
}
