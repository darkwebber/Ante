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
#include <setjmp.h>
#ifndef _WIN32
#include <unwind.h>
#endif
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
#include <dirent.h>
#include <poll.h>
#include <spawn.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>
extern char **environ;
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

/* ── portable locks (tasks share stdout and the runtime's task state) ── */

#ifdef _WIN32
typedef SRWLOCK amtx;
typedef CONDITION_VARIABLE acnd;
#define AMTX_INIT SRWLOCK_INIT
#define ACND_INIT CONDITION_VARIABLE_INIT
static void m_lock(amtx *m) { AcquireSRWLockExclusive(m); }
static void m_unlock(amtx *m) { ReleaseSRWLockExclusive(m); }
static void c_wait(acnd *c, amtx *m) { SleepConditionVariableSRW(c, m, INFINITE, 0); }
static void c_wait_ms(acnd *c, amtx *m, int64_t ms) {
    SleepConditionVariableSRW(c, m, ms < 0 ? 0 : (ms > 0x7ffffff0 ? 0x7ffffff0 : (DWORD)ms), 0);
}
static void c_wake_all(acnd *c) { WakeAllConditionVariable(c); }
static void c_wake_one(acnd *c) { WakeConditionVariable(c); }
#else
typedef pthread_mutex_t amtx;
typedef pthread_cond_t acnd;
#define AMTX_INIT PTHREAD_MUTEX_INITIALIZER
#define ACND_INIT PTHREAD_COND_INITIALIZER
static void m_lock(amtx *m) { pthread_mutex_lock(m); }
static void m_unlock(amtx *m) { pthread_mutex_unlock(m); }
static void c_wait(acnd *c, amtx *m) { pthread_cond_wait(c, m); }
static void c_wait_ms(acnd *c, amtx *m, int64_t ms) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    if (ms < 0) ms = 0;
    ts.tv_sec += (time_t)(ms / 1000);
    ts.tv_nsec += (long)(ms % 1000) * 1000000L;
    if (ts.tv_nsec >= 1000000000L) {
        ts.tv_sec += 1;
        ts.tv_nsec -= 1000000000L;
    }
    pthread_cond_timedwait(c, m, &ts);
}
static void c_wake_all(acnd *c) { pthread_cond_broadcast(c); }
static void c_wake_one(acnd *c) { pthread_cond_signal(c); }
#endif

static amtx out_lock = AMTX_INIT;

#if defined(__GNUC__) || defined(__clang__)
#define NORETURN __attribute__((noreturn))
#else
#define NORETURN
#endif

/* ── output ───────────────────────────────────────────────────────────── */

static void flush_locked(void) {
    if (out_buf.len > 0) {
        fwrite(out_buf.ptr, 1, (size_t)out_buf.len, stdout);
        out_buf.len = 0;
    }
    fflush(stdout);
}

void ante_flush(void) {
    m_lock(&out_lock);
    flush_locked();
    m_unlock(&out_lock);
}

/* Ends the process at once: a trap or `system.vellipo` (§20.3). Live values
   aren't dropped, so nothing exit-time runs either (`_Exit`); a leak checker
   would otherwise count what the program still held as lost. */
static NORETURN void end_now(int32_t code) {
    ante_flush();
    fflush(stderr);
    _Exit(code);
}

/* T09 (§20.3). The runtime doesn't know the source position here. */
static NORETURN void out_of_memory(void) {
    flush_locked(); /* never wait for a lock here: the holder may be us */
    fputs("kompa munigindhi! memory ayipoyindhi\n", stderr);
    end_now(101);
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
    m_lock(&out_lock);
    if (fd == 2) {
        flush_locked();
        fwrite(s->ptr, 1, (size_t)s->len, stderr);
        if (newline) fputc('\n', stderr);
    } else {
        ante_buf_push(&out_buf, s->ptr, s->len);
        if (newline) ante_buf_push(&out_buf, (const uint8_t *)"\n", 1);
        if (out_buf.len > (1 << 16)) flush_locked();
    }
    m_unlock(&out_lock);
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

/* `system.vellipo`: leaves without dropping what's alive. */
NORETURN void ante_vellipo(int32_t code) {
    end_now(code);
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

/* ── tasks (§24) ──────────────────────────────────────────────────────── */

/* Tasks run on a pool of OS threads. A worker runs one task at a time, to
   completion, on its own stack. When a task waits (a channel, a lock, a
   token, `nidrapo`), its worker counts as blocked; if work is queued and
   nobody is free to take it, another worker starts. So waiting never
   stalls the rest of the program, and the number of threads stays near
   the number of CPUs plus the number of waiting tasks.

   All task state (teams, tokens, locks, channels) is guarded by one lock,
   `tk`, with one condition variable that every waiter re-checks. That is
   simple and can't lose a wakeup; values themselves never touch the lock. */

struct ante_team;
struct lock_block;

typedef void (*ante_run_fn)(void *pani, void *out);
typedef void (*ante_drop_fn)(void *value);

typedef struct ante_task {
    int64_t id;
    int64_t refs; /* the team's list, the token, the queue/worker */
    struct ante_team *team;
    ante_run_fn run;
    uint8_t pani[16]; /* the closure: { code, environment } */
    void *result;
    ante_drop_fn drop_result;
    int32_t state; /* 0 queued, 1 running, 2 finished, 3 trapped, 4 cancelled */
    int32_t cancel, saw_cancel, detached, collected;
    char code[8];
    char *msg, *where;
    jmp_buf jb;
    struct ante_task *next_queued, *next_in_team;
    struct lock_block **held;
    int32_t nheld, capheld;
    struct ante_team *teams; /* teams this task opened, innermost first */
    int32_t unwinding;       /* a trap is unwinding this task's frames */
    uintptr_t stack_mark;    /* `run_task`'s frame: unwinding stops above it */
#ifndef _WIN32
    struct _Unwind_Exception exc;
#endif
} ante_task;

typedef struct ante_team {
    int64_t active;
    ante_task *tasks;
    struct ante_team *outer;
} ante_team;

static amtx tk = AMTX_INIT;
static acnd tk_changed = ACND_INIT;
static acnd tk_work = ACND_INIT;
static ante_task *queue_head, *queue_tail;
static int64_t workers_idle, workers_running, next_task_id = 2;
static int64_t task_stack = 1 << 20;
static ante_task main_task = {.id = 1};
#if defined(_MSC_VER) && !defined(__clang__)
static __declspec(thread) ante_task *cur;
#else
static _Thread_local ante_task *cur;
#endif

static ante_task *current(void) { return cur ? cur : &main_task; }

int64_t ante_cpus(void) {
#ifdef _WIN32
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    return si.dwNumberOfProcessors > 0 ? (int64_t)si.dwNumberOfProcessors : 1;
#else
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    return n > 0 ? (int64_t)n : 1;
#endif
}

static void *tk_alloc(size_t n) {
    void *p = calloc(1, n ? n : 1);
    if (!p) out_of_memory();
    return p;
}

static void run_task(ante_task *t);

#ifdef _WIN32
static DWORD WINAPI worker_main(LPVOID arg)
#else
static void *worker_main(void *arg)
#endif
{
    (void)arg;
    m_lock(&tk);
    for (;;) {
        while (!queue_head) {
            workers_idle++;
            c_wait(&tk_work, &tk);
            workers_idle--;
        }
        ante_task *t = queue_head;
        queue_head = t->next_queued;
        if (!queue_head) queue_tail = NULL;
        workers_running++;
        m_unlock(&tk);
        run_task(t);
        m_lock(&tk);
        workers_running--;
    }
#ifdef _WIN32
    return 0;
#else
    return NULL;
#endif
}

/* Called with `tk` held. */
static void start_worker(void) {
#ifdef _WIN32
    HANDLE h = CreateThread(NULL, (SIZE_T)task_stack, worker_main, NULL, STACK_SIZE_PARAM_IS_A_RESERVATION, NULL);
    if (!h) out_of_memory();
    CloseHandle(h);
#else
    pthread_attr_t attr;
    pthread_t th;
    pthread_attr_init(&attr);
    pthread_attr_setstacksize(&attr, (size_t)task_stack);
    pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED);
    if (pthread_create(&th, &attr, worker_main, NULL) != 0) out_of_memory();
    pthread_attr_destroy(&attr);
#endif
}

/* Hands queued work to a worker, starting one if every worker is busy and
   fewer than one per CPU are running. Called with `tk` held. */
static void dispatch(void) {
    if (!queue_head) return;
    if (workers_idle > 0)
        c_wake_one(&tk_work);
    else if (workers_running < ante_cpus())
        start_worker();
}

/* Waits on `tk_changed` (with `tk` held) for at most `ms` (-1: no limit).
   While the task waits, its worker doesn't count as running. */
static void tk_wait(int64_t ms) {
    int worker = cur != NULL;
    if (worker) {
        workers_running--;
        dispatch();
    }
    if (ms < 0)
        c_wait(&tk_changed, &tk);
    else
        c_wait_ms(&tk_changed, &tk, ms);
    if (worker) workers_running++;
}

static void task_unref(ante_task *t) { /* `tk` held */
    if (--t->refs == 0) {
        free(t->result);
        free(t->msg);
        free(t->where);
        free(t->held);
        free(t);
    }
}

/* `ante.toml`'s `[build] task_stack`, set before any task starts. */
void ante_set_task_stack(int64_t bytes) {
    if (bytes >= (256 << 10)) task_stack = bytes;
}

/* `task.kalisi`: a team for the current task. */
void *ante_team_new(void) {
    ante_team *team = (ante_team *)tk_alloc(sizeof *team);
    m_lock(&tk);
    ante_task *me = current();
    team->outer = me->teams;
    me->teams = team;
    m_unlock(&tk);
    return team;
}

/* The end of a `kalisi`: waits for every task of the team, then lets go of it. */
void ante_team_end(void *h) {
    ante_team *team = (ante_team *)h;
    m_lock(&tk);
    while (team->active > 0) tk_wait(-1);
    ante_task *me = current();
    if (me->teams == team) me->teams = team->outer;
    ante_task *t = team->tasks;
    while (t) {
        ante_task *next = t->next_in_team;
        t->team = NULL;
        task_unref(t);
        t = next;
    }
    m_unlock(&tk);
    free(team);
}

/* `f ni j.pampu()`: queues a task. `pani` is the closure, moved in. */
void *ante_task_spawn(void *h, ante_run_fn run, const void *pani, int64_t result_size, ante_drop_fn drop_result) {
    ante_team *team = (ante_team *)h;
    ante_task *t = (ante_task *)tk_alloc(sizeof *t);
    t->result = tk_alloc((size_t)result_size);
    t->run = run;
    memcpy(t->pani, pani, sizeof t->pani);
    t->drop_result = drop_result;
    t->refs = 3;
    m_lock(&tk);
    t->id = next_task_id++;
    t->team = team;
    t->next_in_team = team->tasks;
    team->tasks = t;
    team->active++;
    if (queue_tail)
        queue_tail->next_queued = t;
    else
        queue_head = t;
    queue_tail = t;
    dispatch();
    m_unlock(&tk);
    return t;
}

void ante_pani_dec(void *env);

static void finish_task(ante_task *t, int32_t state) {
    ante_pani_dec(*(void **)(t->pani + 8)); /* the closure's environment */
    m_lock(&tk);
    int produced = state == 2;
    /* A task that noticed it was cancelled has no value to give (§24.6). */
    if (produced && t->saw_cancel) state = 4;
    t->state = state;
    /* A value nobody will collect is dropped now, outside the lock, since
       a destructor may use tasks itself. */
    int drop = produced && (state == 4 || t->detached) && !t->collected;
    if (drop) t->collected = 1;
    m_unlock(&tk);
    if (drop && t->drop_result) t->drop_result(t->result);
    m_lock(&tk);
    if (t->team) t->team->active--;
    c_wake_all(&tk_changed);
    task_unref(t);
    m_unlock(&tk);
}

static void run_task(ante_task *t) {
    cur = t;
    t->stack_mark = (uintptr_t)__builtin_frame_address(0);
    m_lock(&tk);
    int start = !t->cancel;
    t->state = start ? 1 : 4;
    m_unlock(&tk);
    if (!start) {
        finish_task(t, 4);
    } else if (setjmp(t->jb) == 0) {
        t->run(t->pani, t->result);
        finish_task(t, 2);
    } else {
        finish_task(t, 3);
    }
    cur = NULL;
}

/* `r.aagu()`: 0 with the value moved to `out`; 1 trapped, with the
   details; 2 cancelled. Consumes the token. */
int32_t ante_task_wait(void *h, void *out, int64_t size, ante_str *code, ante_str *msg, ante_str *where) {
    ante_task *t = (ante_task *)h;
    m_lock(&tk);
    while (t->state < 2) tk_wait(-1);
    int32_t r = t->state == 2 ? 0 : t->state == 3 ? 1 : 2;
    if (r == 0) {
        memcpy(out, t->result, (size_t)size);
        t->collected = 1;
    } else if (r == 1) {
        size_t n = strlen(t->code), m = t->msg ? strlen(t->msg) : 0, w = t->where ? strlen(t->where) : 0;
        code->ptr = (uint8_t *)tk_alloc(n);
        memcpy(code->ptr, t->code, n);
        code->len = code->cap = (int64_t)n;
        msg->ptr = (uint8_t *)tk_alloc(m);
        if (m) memcpy(msg->ptr, t->msg, m);
        msg->len = msg->cap = (int64_t)m;
        where->ptr = (uint8_t *)tk_alloc(w);
        if (w) memcpy(where->ptr, t->where, w);
        where->len = where->cap = (int64_t)w;
    }
    task_unref(t);
    m_unlock(&tk);
    return r;
}

/* Dropping a token without `aagu`: the value, if any, is dropped. */
void ante_task_drop(void *h) {
    ante_task *t = (ante_task *)h;
    m_lock(&tk);
    t->detached = 1;
    int drop = t->state == 2 && !t->collected;
    if (drop) t->collected = 1;
    m_unlock(&tk);
    if (drop && t->drop_result) t->drop_result(t->result);
    m_lock(&tk);
    task_unref(t);
    m_unlock(&tk);
}

void ante_task_cancel(void *h) {
    m_lock(&tk);
    ((ante_task *)h)->cancel = 1;
    c_wake_all(&tk_changed);
    m_unlock(&tk);
}

void ante_team_cancel(void *h) {
    m_lock(&tk);
    for (ante_task *t = ((ante_team *)h)->tasks; t; t = t->next_in_team)
        if (t->state < 2) t->cancel = 1;
    c_wake_all(&tk_changed);
    m_unlock(&tk);
}

/* With `tk` held: has the current task been cancelled? Noticing it at a
   cancellation point means the task gives no value (§24.6). */
static int cancelled_now(void) {
    ante_task *me = current();
    if (me->cancel) me->saw_cancel = 1;
    return me->cancel;
}

int32_t ante_task_cancelled(void) {
    m_lock(&tk);
    int32_t r = current()->cancel;
    m_unlock(&tk);
    return r;
}

static int64_t now_ms(void) {
#ifdef _WIN32
    return (int64_t)GetTickCount64();
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000 + ts.tv_nsec / 1000000;
#endif
}

/* `task.nidrapo`: sleeps, waking early if the task is cancelled. */
void ante_task_sleep(int64_t nanos) {
    int64_t until = now_ms() + (nanos + 999999) / 1000000;
    m_lock(&tk);
    for (;;) {
        if (cancelled_now()) break;
        int64_t left = until - now_ms();
        if (left <= 0) break;
        tk_wait(left);
    }
    m_unlock(&tk);
}

/* ── `Thaalam` (§24.4) ── */

/* A lock block: who holds the key, whether it's poisoned, then the value. */
typedef struct lock_block {
    int64_t holder;
    int32_t poisoned;
    int32_t pad;
} lock_block;

void *ante_lock_new(int64_t size) { return tk_alloc(sizeof(lock_block) + (size_t)size); }

void ante_lock_free(void *h) { free(h); }

static NORETURN void trap_at(const char *code, const char *msg, const ante_loc *loc);

/* `l.theruvu()` (`check` 0) or `l.theruvu_choosi()` (`check` 1, which
   returns 1 for a poisoned lock instead of trapping T11). */
int32_t ante_lock_open(void *h, int32_t check, const ante_loc *loc) {
    lock_block *b = (lock_block *)h;
    m_lock(&tk);
    ante_task *me = current();
    if (b->holder == me->id) {
        m_unlock(&tk);
        trap_at("T12", "ee Thaalam key already nee daggare undhi (rendosaari theruvu)", loc);
    }
    while (b->holder != 0) tk_wait(-1);
    if (b->poisoned && !check) {
        m_unlock(&tk);
        trap_at("T11", "Thaalam virigipoyindhi: daanni pattukunna task kompa munigindhi", loc);
    }
    b->holder = me->id;
    if (me->nheld == me->capheld) {
        int32_t cap = me->capheld ? me->capheld * 2 : 4;
        lock_block **p = (lock_block **)realloc(me->held, (size_t)cap * sizeof *p);
        if (!p) {
            m_unlock(&tk);
            out_of_memory();
        }
        me->held = p;
        me->capheld = cap;
    }
    me->held[me->nheld++] = b;
    int32_t poisoned = b->poisoned;
    m_unlock(&tk);
    return poisoned;
}

/* Dropping a `Key`. */
void ante_lock_close(void *h) {
    lock_block *b = (lock_block *)h;
    m_lock(&tk);
    ante_task *me = current();
    for (int32_t i = me->nheld - 1; i >= 0; i--) {
        if (me->held[i] == b) {
            me->held[i] = me->held[--me->nheld];
            break;
        }
    }
    b->holder = 0;
    c_wake_all(&tk_changed);
    m_unlock(&tk);
}

/* `Virigindhi.lopala()`: the value is the programmer's again. */
void ante_lock_heal(void *h) {
    m_lock(&tk);
    ((lock_block *)h)->poisoned = 0;
    m_unlock(&tk);
}

/* ── `Atomic` (§24.4): every value is kept widened to 64 bits ── */

void *ante_atomic_new(int64_t v) {
    int64_t *p = (int64_t *)tk_alloc(sizeof *p);
    __atomic_store_n(p, v, __ATOMIC_SEQ_CST);
    return p;
}

void ante_atomic_free(void *h) { free(h); }

int64_t ante_atomic_load(void *h) { return __atomic_load_n((int64_t *)h, __ATOMIC_SEQ_CST); }

void ante_atomic_store(void *h, int64_t v) { __atomic_store_n((int64_t *)h, v, __ATOMIC_SEQ_CST); }

int64_t ante_atomic_swap(void *h, int64_t v) { return __atomic_exchange_n((int64_t *)h, v, __ATOMIC_SEQ_CST); }

/* Wraps `v` to an integer of `bits` bits, sign- or zero-extended back to 64. */
static int64_t wrap_bits(uint64_t v, int32_t bits, int32_t is_signed) {
    if (bits >= 64) return (int64_t)v;
    uint64_t mask = ((uint64_t)1 << bits) - 1;
    v &= mask;
    if (is_signed && (v >> (bits - 1)) & 1) v |= ~mask;
    return (int64_t)v;
}

/* `n ni a.kalupu()`: adds, wrapping around at the type's width; returns the old value. */
int64_t ante_atomic_add(void *h, int64_t n, int32_t bits, int32_t is_signed) {
    int64_t *p = (int64_t *)h;
    int64_t old = __atomic_load_n(p, __ATOMIC_SEQ_CST);
    for (;;) {
        int64_t next = wrap_bits((uint64_t)old + (uint64_t)n, bits, is_signed);
        if (__atomic_compare_exchange_n(p, &old, next, 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST)) return old;
    }
}

/* Compare-and-swap: 1 if it was `old` (now `new`); else 0, with the actual value in `*seen`. */
int32_t ante_atomic_cas(void *h, int64_t old, int64_t next, int64_t *seen) {
    int64_t expect = old;
    if (__atomic_compare_exchange_n((int64_t *)h, &expect, next, 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST)) return 1;
    *seen = expect;
    return 0;
}

/* ── `Kaluva` (§24.4) ── */

typedef struct {
    int64_t cap; /* as asked; 0 is a rendezvous */
    int64_t size;
    uint8_t *buf;
    int64_t slots, head, count;
    int64_t senders;
    int32_t receiver;
    int64_t refs;
    int64_t sent, taken;
    ante_drop_fn drop;
} chan;

void *ante_chan_new(int64_t cap, int64_t size, ante_drop_fn drop) {
    chan *c = (chan *)tk_alloc(sizeof *c);
    c->cap = cap;
    c->size = size;
    c->slots = cap > 0 ? cap : 1;
    c->buf = (uint8_t *)tk_alloc((size_t)size_mul(c->slots, size > 0 ? size : 1));
    c->senders = 1;
    c->receiver = 1;
    c->refs = 2;
    c->drop = drop;
    return c;
}

/* With `tk` held: one less handle; the last one drops what's left. */
static void chan_unref(chan *c) {
    if (--c->refs > 0) return;
    m_unlock(&tk);
    if (c->drop)
        for (int64_t i = 0; i < c->count; i++) c->drop(c->buf + ((c->head + i) % c->slots) * c->size);
    free(c->buf);
    free(c);
    m_lock(&tk);
}

/* `x ni p.pampu()`: 0 sent; 1 every receiver is gone or the task was
   cancelled, and `x` stays with the caller. */
int32_t ante_chan_send(void *h, const void *v) {
    chan *c = (chan *)h;
    m_lock(&tk);
    for (;;) {
        if (cancelled_now() || !c->receiver) {
            m_unlock(&tk);
            return 1;
        }
        if (c->count < c->slots) break;
        tk_wait(-1);
    }
    memcpy(c->buf + ((c->head + c->count) % c->slots) * c->size, v, (size_t)c->size);
    c->count++;
    int64_t mine = ++c->sent;
    c_wake_all(&tk_changed);
    /* A rendezvous ends when the value is taken. */
    while (c->cap == 0 && c->taken < mine && c->receiver && !cancelled_now()) tk_wait(-1);
    m_unlock(&tk);
    return 0;
}

/* `a.andhuko()` (`wait` 1) or `a.ippude_andhuko()` (`wait` 0): 1 with a value in `out`. */
int32_t ante_chan_recv(void *h, void *out, int32_t wait) {
    chan *c = (chan *)h;
    m_lock(&tk);
    while (c->count == 0) {
        if (!wait || c->senders == 0 || cancelled_now()) {
            m_unlock(&tk);
            return 0;
        }
        tk_wait(-1);
    }
    memcpy(out, c->buf + c->head * c->size, (size_t)c->size);
    c->head = (c->head + 1) % c->slots;
    c->count--;
    c->taken++;
    c_wake_all(&tk_changed);
    m_unlock(&tk);
    return 1;
}

void ante_chan_sender_copy(void *h) {
    m_lock(&tk);
    ((chan *)h)->senders++;
    ((chan *)h)->refs++;
    m_unlock(&tk);
}

void ante_chan_sender_drop(void *h) {
    chan *c = (chan *)h;
    m_lock(&tk);
    c->senders--;
    c_wake_all(&tk_changed);
    chan_unref(c);
    m_unlock(&tk);
}

void ante_chan_receiver_drop(void *h) {
    chan *c = (chan *)h;
    m_lock(&tk);
    c->receiver = 0;
    c_wake_all(&tk_changed);
    chan_unref(c);
    m_unlock(&tk);
}

/* ── `edhi_mundhu` (§24.4): whichever is ready first ── */

/* An expectation keeps what it watches alive: it counts as a handle of the
   channel or the task, so dropping those first can't leave it dangling. */
typedef struct {
    int32_t kind; /* 1 a channel's value or end, 2 a timer, 3 a task's end, 4 cancellation */
    void *p;
    int64_t until;
} expect;

void *ante_expect_chan(void *h) {
    expect *e = (expect *)tk_alloc(sizeof *e);
    e->kind = 1;
    e->p = h;
    m_lock(&tk);
    ((chan *)h)->refs++;
    m_unlock(&tk);
    return e;
}

void *ante_expect_timer(int64_t nanos) {
    expect *e = (expect *)tk_alloc(sizeof *e);
    e->kind = 2;
    e->until = now_ms() + (nanos + 999999) / 1000000;
    return e;
}

void *ante_expect_task(void *h) {
    expect *e = (expect *)tk_alloc(sizeof *e);
    e->kind = 3;
    e->p = h;
    m_lock(&tk);
    ((ante_task *)h)->refs++;
    m_unlock(&tk);
    return e;
}

void *ante_expect_cancel(void) {
    expect *e = (expect *)tk_alloc(sizeof *e);
    e->kind = 4;
    return e;
}

void ante_expect_free(void *h) {
    expect *e = (expect *)h;
    m_lock(&tk);
    if (e->kind == 1) chan_unref((chan *)e->p);
    if (e->kind == 3) task_unref((ante_task *)e->p);
    m_unlock(&tk);
    free(e);
}

/* `vetiki ni task.edhi_mundhu()`: the lowest index that's ready. Only a
   `cancel_ready` expectation makes this a cancellation point. */
int64_t ante_select(void *const *es, int64_t n, const ante_loc *loc) {
    if (n <= 0) trap_at("T05", "`edhi_mundhu` ki okka Edhuruchoopu kuda ivvaledhu", loc);
    m_lock(&tk);
    for (;;) {
        int64_t wait = -1;
        for (int64_t i = 0; i < n; i++) {
            expect *e = (expect *)es[i];
            int ready = 0;
            if (e->kind == 1) {
                chan *c = (chan *)e->p;
                ready = c->count > 0 || c->senders == 0;
            } else if (e->kind == 2) {
                int64_t left = e->until - now_ms();
                ready = left <= 0;
                if (!ready && (wait < 0 || left < wait)) wait = left;
            } else if (e->kind == 3) {
                ready = ((ante_task *)e->p)->state >= 2;
            } else {
                ready = cancelled_now();
            }
            if (ready) {
                m_unlock(&tk);
                return i;
            }
        }
        tk_wait(wait);
    }
}

/* ── `panchukunna` (§24.4): a count, then the value at offset 16 ── */

void *ante_rc_new(int64_t size) {
    int64_t *p = (int64_t *)tk_alloc(16 + (size_t)size);
    __atomic_store_n(p, 1, __ATOMIC_RELAXED);
    return p;
}

void ante_rc_inc(void *h) { __atomic_fetch_add((int64_t *)h, 1, __ATOMIC_RELAXED); }

/* 1 when this was the last handle: the caller drops the value and frees it. */
int32_t ante_rc_dec(void *h) { return __atomic_fetch_sub((int64_t *)h, 1, __ATOMIC_ACQ_REL) == 1; }

void ante_rc_free(void *h) { free(h); }

/* ── a trap in a task (§24.5) ── */

/* With `tk` held: locks the trapped task still holds are let go. Their
   keys didn't get dropped (or unwinding wasn't possible). */
static void release_held(ante_task *t) {
    for (int32_t i = 0; i < t->nheld; i++) t->held[i]->holder = 0;
    t->nheld = 0;
    c_wake_all(&tk_changed);
}

#ifndef _WIN32
/* Forced unwinding stops once it reaches `run_task`'s own frame. */
static _Unwind_Reason_Code unwind_stop(int version, _Unwind_Action actions, _Unwind_Exception_Class cls,
                                       struct _Unwind_Exception *exc, struct _Unwind_Context *ctx, void *arg) {
    (void)version;
    (void)cls;
    (void)exc;
    ante_task *t = (ante_task *)arg;
    if ((actions & _UA_END_OF_STACK) || _Unwind_GetCFA(ctx) >= t->stack_mark) {
        m_lock(&tk);
        release_held(t);
        m_unlock(&tk);
        longjmp(t->jb, 1);
    }
    return _URC_NO_REASON;
}
#endif

/* Ends only the current task (§24.5). Its locks are poisoned at once. Then
   its frames are unwound, innermost first: each waits for the teams it
   started and drops what it owns, so keys are given back as their frames
   go. Its token gets the details. */
static NORETURN void task_trapped(const char *code, const char *msg, const char *where) {
    ante_task *t = cur;
    snprintf(t->code, sizeof t->code, "%s", code);
    t->msg = (char *)malloc(strlen(msg) + 1);
    if (t->msg) strcpy(t->msg, msg);
    if (where) {
        t->where = (char *)malloc(strlen(where) + 1);
        if (t->where) strcpy(t->where, where);
    }
    m_lock(&tk);
    for (int32_t i = 0; i < t->nheld; i++) t->held[i]->poisoned = 1;
    t->unwinding = 1;
    m_unlock(&tk);
#ifndef _WIN32
    memset(&t->exc, 0, sizeof t->exc);
    t->exc.exception_class = 0x414e544555574e44ULL; /* "ANTEUWND" */
    _Unwind_ForcedUnwind(&t->exc, unwind_stop, t);
    /* No unwind information: fall back to leaving the frames as they are. */
#endif
    m_lock(&tk);
    release_held(t);
    while (t->teams) {
        ante_team *team = t->teams;
        m_unlock(&tk);
        ante_team_end(team);
        m_lock(&tk);
    }
    m_unlock(&tk);
    longjmp(t->jb, 1);
}


/* `antec run` asks for the trap's code and position in a file, when it
   runs a program natively on the interpreter's behalf (§22.6). */
static void report_trap(const char *code, const ante_loc *loc) {
    const char *path = getenv("ANTE_TRAP_REPORT");
    if (!path || !*path) return;
    FILE *f = fopen(path, "w");
    if (!f) return;
    if (loc)
        fprintf(f, "%s %d %d %.*s\n", code, loc->line, loc->col, (int)loc->file_len, (const char *)loc->file);
    else
        fprintf(f, "%s 0 0\n", code);
    fclose(f);
}

/* Every trap ends here. In a task other than the first, it ends only that
   task (§24.5); otherwise the whole program (§20.3). */
static NORETURN void trap_at(const char *code, const char *msg, const ante_loc *loc) {
    char where[600] = "";
    if (loc) snprintf(where, sizeof where, "%.*s:%d:%d", (int)loc->file_len, (const char *)loc->file, loc->line, loc->col);
    m_lock(&out_lock);
    flush_locked();
    if (loc)
        fprintf(stderr, "kompa munigindhi! %s (%s)\n", msg, where);
    else
        fprintf(stderr, "kompa munigindhi! %s\n", msg);
    fflush(stderr);
    m_unlock(&out_lock);
    /* A trap while unwinding (in a destructor) ends the program (§24.5.3). */
    if (cur && !cur->unwinding) task_trapped(code, msg, loc ? where : NULL);
    report_trap(code, loc);
    end_now(101);
}


NORETURN void ante_trap(const ante_str *msg, const ante_loc *loc) {
    char text[1024];
    int n = msg->len < 1000 ? (int)msg->len : 1000;
    memcpy(text, msg->ptr, (size_t)n);
    text[n] = 0;
    /* The generated code's messages say which trap they are. */
    const char *code = "T05";
    if (strncmp(text, "nijame()", 8) == 0)
        code = "T07";
    else if (strstr(text, "kachithanga()"))
        code = "T06";
    else if (strstr(text, "pattadhu"))
        code = "T01";
    trap_at(code, text, loc);
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
    trap_at("T01", text, loc);
}

NORETURN void ante_trap_index(int64_t i, int64_t len, const ante_loc *loc) {
    char text[128];
    snprintf(text, sizeof text, "index %lld bayatiki vellindhi (length %lld)", (long long)i, (long long)len);
    trap_at("T03", text, loc);
}

NORETURN void ante_trap_slice(int64_t lo, int64_t hi, int64_t len, const ante_loc *loc) {
    char text[128];
    snprintf(text, sizeof text, "mukka haddulu tappu: %lld nunchi %lld (length %lld)", (long long)lo, (long long)hi,
             (long long)len);
    trap_at("T10", text, loc);
}

NORETURN void ante_trap_shift(int64_t count, int32_t bits, const ante_loc *loc) {
    char text[128];
    snprintf(text, sizeof text, "shift count %lld tappu (0..%d undali)", (long long)count, bits);
    trap_at("T04", text, loc);
}

NORETURN void ante_trap_code(int32_t code, const ante_loc *loc) {
    switch (code) {
    case 2: trap_at("T02", "sunna (0) tho bhaagham cheyyalem", loc);
    case 6: trap_at("T06", "`ledhu` meeda kachithanga() pilicharu", loc);
    case 7: trap_at("T07", "nijame() fail ayyindhi", loc);
    default: trap_at("T00", "antec bug: unreachable code ki vacchindhi", loc);
    }
}

/* ── memory ───────────────────────────────────────────────────────────── */

void *ante_alloc(int64_t size) {
    if (size < 0) out_of_memory();
    void *p = malloc(size > 0 ? (size_t)size : 1);
    if (!p) out_of_memory();
    return p;
}

/* `memory.allocate` (§21.3, `naa_risk` only): `n` bytes aligned to `a`, a
   power of two. Running out is T09, never a null pointer. */
void *ante_mem_alloc(uint64_t n, uint64_t a) {
    if (n > (uint64_t)INT64_MAX || a == 0 || (a & (a - 1)) != 0) out_of_memory();
    if (a <= 16) return ante_alloc((int64_t)n);
    size_t size = n ? (size_t)n : 1;
#ifdef _WIN32
    void *p = _aligned_malloc(size, (size_t)a);
#else
    void *p = 0;
    if (posix_memalign(&p, (size_t)a, size) != 0) p = 0;
#endif
    if (!p) out_of_memory();
    return p;
}

void ante_mem_release(void *p, uint64_t n, uint64_t a) {
    (void)n;
#ifdef _WIN32
    if (a > 16) {
        _aligned_free(p);
        return;
    }
#else
    (void)a;
#endif
    free(p);
}

void *ante_mem_realloc(void *p, uint64_t n, uint64_t a, uint64_t m) {
    if (m > (uint64_t)INT64_MAX) out_of_memory();
    if (a <= 16) {
        void *q = realloc(p, m ? (size_t)m : 1);
        if (!q) out_of_memory();
        return q;
    }
    void *q = ante_mem_alloc(m, a);
    if (p) memcpy(q, p, (size_t)(n < m ? n : m));
    ante_mem_release(p, n, a);
    return q;
}

/* The buffer of a `varusa` with `n` elements. An empty one owns no buffer
   (`cap == 0`), so it gets none rather than one nobody frees. */
void *ante_vec_alloc(int64_t n, int64_t bytes) {
    return n ? ante_alloc(bytes) : 0;
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

/* ── jodi, gajibiji_jodi, gumpu (§21.2) ───────────────────────────────── */

/* An insertion-ordered hash map. Entries sit in insertion order, each
   `esize` bytes: { u64 hash, u8 live, key, value }. `slots` indexes them by
   hash with open addressing (-1 empty, -2 removed). Generated code hashes
   and compares keys and moves keys and values in and out; this part only
   finds and places entries. A removed entry leaves a hole until the next
   rebuild, which closes the holes. */
typedef struct {
    uint8_t *entries;
    int64_t len, cap;
    int64_t *slots;
    int64_t nslots;
    int64_t live;
} ante_map;

/* Key equality from generated code; the result is an LLVM `i1`, so only
   bit 0 is meaningful. */
typedef uint32_t (*ante_eq_fn)(const void *, const void *);

static uint8_t *map_entry(const ante_map *m, int64_t i, int64_t esize) { return m->entries + i * esize; }

static uint64_t slot_hash(uint64_t h) {
    h ^= h >> 33;
    h *= 0xff51afd7ed558ccdULL;
    h ^= h >> 33;
    return h;
}

/* The entry holding `key`, or -1. */
int64_t ante_map_find(const ante_map *m, uint64_t h, const void *key, int64_t esize, int64_t koff, ante_eq_fn eq) {
    if (!m->nslots) return -1;
    uint64_t mask = (uint64_t)m->nslots - 1;
    for (uint64_t i = slot_hash(h) & mask;; i = (i + 1) & mask) {
        int64_t s = m->slots[i];
        if (s == -1) return -1;
        if (s >= 0) {
            uint8_t *e = map_entry(m, s, esize);
            if (*(uint64_t *)e == h && (eq(e + koff, key) & 1)) return s;
        }
    }
}

/* Every slot points at an entry made since the last rebuild, so keeping
   `len` under half the slots always leaves an empty one to end a search. */
static void map_rebuild(ante_map *m, int64_t esize) {
    if (m->live < m->len) {
        int64_t w = 0;
        for (int64_t k = 0; k < m->len; k++) {
            uint8_t *e = map_entry(m, k, esize);
            if (!e[8]) continue;
            if (w != k) memmove(map_entry(m, w, esize), e, (size_t)esize);
            w++;
        }
        m->len = w;
    }
    int64_t need = size_mul(size_add(m->len, 1), 2);
    if (need > m->nslots) {
        int64_t n = m->nslots ? m->nslots : 8;
        while (n < size_mul(need, 2)) n = size_mul(n, 2);
        free(m->slots);
        m->slots = (int64_t *)ante_alloc(size_mul(n, 8));
        m->nslots = n;
    }
    uint64_t mask = (uint64_t)m->nslots - 1;
    for (int64_t s = 0; s < m->nslots; s++) m->slots[s] = -1;
    for (int64_t k = 0; k < m->len; k++) {
        uint64_t i = slot_hash(*(uint64_t *)map_entry(m, k, esize)) & mask;
        while (m->slots[i] != -1) i = (i + 1) & mask;
        m->slots[i] = k;
    }
}

/* Adds an entry for a key known to be missing and returns its index; the
   caller writes the key and value into it. Earlier entry addresses may
   change. */
int64_t ante_map_add(ante_map *m, uint64_t h, int64_t esize) {
    if (size_mul(size_add(m->len, 1), 2) > m->nslots) map_rebuild(m, esize);
    if (m->len == m->cap) {
        int64_t cap = grow_cap(m->cap, size_add(m->len, 1), 4);
        uint8_t *p = (uint8_t *)ante_alloc(size_mul(cap, esize));
        if (m->len) memcpy(p, m->entries, (size_t)size_mul(m->len, esize));
        free(m->entries);
        m->entries = p;
        m->cap = cap;
    }
    int64_t k = m->len++;
    uint8_t *e = map_entry(m, k, esize);
    *(uint64_t *)e = h;
    e[8] = 1;
    uint64_t mask = (uint64_t)m->nslots - 1;
    uint64_t i = slot_hash(h) & mask;
    while (m->slots[i] >= 0) i = (i + 1) & mask;
    m->slots[i] = k;
    m->live++;
    return k;
}

/* Forgets entry `k`; the caller has already moved out or dropped its key
   and value. */
void ante_map_remove(ante_map *m, int64_t k, int64_t esize) {
    uint8_t *e = map_entry(m, k, esize);
    uint64_t mask = (uint64_t)m->nslots - 1;
    for (uint64_t i = slot_hash(*(uint64_t *)e) & mask;; i = (i + 1) & mask) {
        if (m->slots[i] == k) {
            m->slots[i] = -2;
            break;
        }
    }
    e[8] = 0;
    m->live--;
}

/* Frees the map's memory; the caller has dropped the live entries. */
void ante_map_free(ante_map *m) {
    free(m->entries);
    free(m->slots);
    memset(m, 0, sizeof *m);
}

/* Mixes text into a key hash (FNV-1a). */
uint64_t ante_hash_text(uint64_t h, const ante_str *s) {
    h ^= (uint64_t)s->len;
    for (int64_t i = 0; i < s->len; i++) {
        h ^= s->ptr[i];
        h *= 0x100000001b3ULL;
    }
    return h;
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

static void free_buffer(uint8_t **p) { free(*p); }

/* Stable sort (§21.2: all sorts are stable). `cmp` is "Ante" code, so a
   trapped task may unwind through here (§24.5.2): the buffer is freed on
   the way, and `base` always holds every element exactly once. */
void ante_sort(void *base, int64_t n, int64_t size, ante_cmp_fn cmp, void *ctx) {
    if (n < 2) return;
#if defined(__GNUC__) || defined(__clang__)
    uint8_t *tmp __attribute__((cleanup(free_buffer))) = (uint8_t *)ante_alloc(size_mul(n, size));
    merge_sort((uint8_t *)base, tmp, n, size, cmp, ctx);
#else
    uint8_t *tmp = (uint8_t *)ante_alloc(size_mul(n, size));
    merge_sort((uint8_t *)base, tmp, n, size, cmp, ctx);
    free_buffer(&tmp);
#endif
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

static int32_t read_all(const ante_str *path, ante_str *out, ante_str *err) {
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
        ante_buf_free(&r);
        io_error(err, bad);
        return 0;
    }
    *out = r;
    return 1;
}

/* Where UTF-8 decoding of `s` first fails, as Rust reports it: `*bad` is the
   length of the invalid sequence, or 0 when the text ends inside one.
   Returns 1 when the whole text is valid. */
static int utf8_check(const uint8_t *s, int64_t n, int64_t *upto, int *bad) {
    int64_t i = 0;
    while (i < n) {
        uint8_t c = s[i];
        if (c < 0x80) {
            i++;
            continue;
        }
        int w = c >= 0xC2 && c <= 0xDF ? 2 : c >= 0xE0 && c <= 0xEF ? 3 : c >= 0xF0 && c <= 0xF4 ? 4 : 0;
        *upto = i;
        if (w == 0) {
            *bad = 1;
            return 0;
        }
        uint8_t lo = 0x80, hi = 0xBF;
        if (c == 0xE0) lo = 0xA0;
        if (c == 0xED) hi = 0x9F;
        if (c == 0xF0) lo = 0x90;
        if (c == 0xF4) hi = 0x8F;
        for (int k = 1; k < w; k++) {
            if (i + k >= n) {
                *bad = 0;
                return 0;
            }
            uint8_t d = s[i + k];
            if (k == 1 ? (d < lo || d > hi) : (d & 0xC0) != 0x80) {
                *bad = k;
                return 0;
            }
        }
        i += w;
    }
    return 1;
}

static void utf8_error(ante_str *err, int64_t upto, int bad) {
    char msg[96];
    if (bad)
        snprintf(msg, sizeof msg, "UTF-8 kaadhu: invalid utf-8 sequence of %d bytes from index %lld", bad,
                 (long long)upto);
    else
        snprintf(msg, sizeof msg, "UTF-8 kaadhu: incomplete utf-8 byte sequence from index %lld", (long long)upto);
    set_err(err, msg);
}

/* `padham.baitlu_nunchi`: a copy of the bytes, if they are UTF-8. */
int32_t ante_str_from_bytes(const uint8_t *p, int64_t n, ante_str *out, ante_str *err) {
    int64_t upto;
    int bad;
    if (!utf8_check(p, n, &upto, &bad)) {
        utf8_error(err, upto, bad);
        return 0;
    }
    ante_str r = {0, 0, 0};
    ante_buf_push(&r, p, n);
    *out = r;
    return 1;
}

static int char_start(const ante_str *s, int64_t i) { return i == s->len || (i < s->len && (s->ptr[i] & 0xC0) != 0x80); }

/* `a ni, b tho s.byte_mukka()`: bytes `a` up to `b`, both on character
   boundaries. */
int32_t ante_str_byte_part(const ante_str *s, int64_t a, int64_t b, ante_str *out, ante_str *err) {
    if (a < 0 || b < a || b > s->len || !char_start(s, a) || !char_start(s, b)) {
        char msg[96];
        snprintf(msg, sizeof msg, "%lld..%lld akshara haddu meeda ledhu", (long long)a, (long long)b);
        set_err(err, msg);
        return 0;
    }
    ante_str r = {0, 0, 0};
    ante_buf_push(&r, s->ptr + a, b - a);
    *out = r;
    return 1;
}

int32_t ante_file_read(const ante_str *path, ante_str *out, ante_str *err) {
    ante_str r;
    if (!read_all(path, &r, err)) return 0;
    /* `padham` is always valid UTF-8 (§9.1). */
    if (!utf8_valid(r.ptr, r.len)) {
        ante_buf_free(&r);
        set_err(err, "file lo text valid UTF-8 kaadhu");
        return 0;
    }
    *out = r;
    return 1;
}

/* `file.baitlu_chadhuvu`: the bytes as a `varusa[aasajeevi ankam8]`. */
int32_t ante_file_read_bytes(const ante_str *path, ante_vec *out, ante_str *err) {
    return read_all(path, out, err);
}

int32_t ante_file_write_bytes(const ante_str *path, const uint8_t *data, int64_t len, ante_str *err) {
    char *p = cstr(path);
    FILE *f = fopen(p, "wb");
    if (!f) {
        open_error(err, errno, p);
        free(p);
        return 0;
    }
    free(p);
    size_t wrote = fwrite(data, 1, (size_t)len, f);
    int bad = wrote != (size_t)len ? errno : 0;
    if (fclose(f) != 0 && !bad) bad = errno;
    if (bad) {
        io_error(err, bad);
        return 0;
    }
    return 1;
}

int32_t ante_file_write(const ante_str *path, const ante_str *text, ante_str *err) {
    return ante_file_write_bytes(path, text->ptr, text->len, err);
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

int32_t ante_file_is_folder(const ante_str *path) {
    char *p = cstr(path);
    int r = is_folder(p);
    free(p);
    return r;
}

static int32_t cmp_text(const void *a, const void *b, void *ctx) {
    (void)ctx;
    return ante_str_cmp((const ante_str *)a, (const ante_str *)b);
}

/* `file.list_cheyyi`: the names in a folder, sorted, without `.` and `..`. */
int32_t ante_file_list(const ante_str *path, ante_vec *out, ante_str *err) {
    char *p = cstr(path);
    ante_vec v = {0, 0, 0};
#ifdef _WIN32
    ante_str pat = {0, 0, 0};
    push_c(&pat, p);
    push_c(&pat, "\\*");
    uint8_t z = 0;
    ante_buf_push(&pat, &z, 1);
    WIN32_FIND_DATAA fd;
    HANDLE h = FindFirstFileA((const char *)pat.ptr, &fd);
    ante_buf_free(&pat);
    if (h == INVALID_HANDLE_VALUE) {
        DWORD e = GetLastError();
        io_error(err, e == ERROR_PATH_NOT_FOUND || e == ERROR_FILE_NOT_FOUND ? ENOENT
                      : e == ERROR_DIRECTORY ? ENOTDIR : EACCES);
        free(p);
        return 0;
    }
    do {
        if (!strcmp(fd.cFileName, ".") || !strcmp(fd.cFileName, "..")) continue;
        ante_str s = {0, 0, 0};
        push_c(&s, fd.cFileName);
        ante_vec_push(&v, &s, sizeof s);
    } while (FindNextFileA(h, &fd));
    FindClose(h);
#else
    DIR *d = opendir(p);
    if (!d) {
        io_error(err, errno);
        free(p);
        return 0;
    }
    struct dirent *e;
    while ((e = readdir(d)) != 0) {
        if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, "..")) continue;
        ante_str s = {0, 0, 0};
        push_c(&s, e->d_name);
        ante_vec_push(&v, &s, sizeof s);
    }
    closedir(d);
#endif
    free(p);
    ante_sort(v.ptr, v.len, sizeof(ante_str), cmp_text, 0);
    *out = v;
    return 1;
}

static int make_dir(const char *p) {
#ifdef _WIN32
    if (CreateDirectoryA(p, 0)) return 0;
    DWORD e = GetLastError();
    return e == ERROR_ALREADY_EXISTS ? EEXIST : e == ERROR_PATH_NOT_FOUND || e == ERROR_FILE_NOT_FOUND ? ENOENT : EACCES;
#else
    return mkdir(p, 0777) == 0 ? 0 : errno;
#endif
}

static int is_sep(char c) {
#ifdef _WIN32
    return c == '/' || c == '\\';
#else
    return c == '/';
#endif
}

/* `file.folder_cheyyi`: creates the folder and any missing parents, the
   way Rust's `create_dir_all` does (so the errors match the interpreter). */
static int mkdirs(char *p, size_t n) {
    if (n == 0) return 0;
    char saved = p[n];
    p[n] = 0;
    int e = make_dir(p);
    if (e == 0 || (e != ENOENT && is_folder(p))) {
        p[n] = saved;
        return 0;
    }
    p[n] = saved;
    if (e != ENOENT) return e;
    size_t k = n;
    while (k > 0 && !is_sep(p[k - 1])) k--;
    while (k > 1 && is_sep(p[k - 1])) k--;
    if (k > 0 && !(k == 1 && is_sep(p[0]))) {
        int pe = mkdirs(p, k);
        if (pe) return pe;
    }
    p[n] = 0;
    e = make_dir(p);
    if (e && is_folder(p)) e = 0;
    p[n] = saved;
    return e;
}

int32_t ante_file_mkdirs(const ante_str *path, ante_str *err) {
    char *p = cstr(path);
    size_t n = strlen(p);
    while (n > 1 && is_sep(p[n - 1])) n--;
    int e = mkdirs(p, n);
    free(p);
    if (e) {
        io_error(err, e);
        return 0;
    }
    return 1;
}

/* `file.theesey`: deletes a file. */
int32_t ante_file_remove(const ante_str *path, ante_str *err) {
    char *p = cstr(path);
#ifdef _WIN32
    int ok = DeleteFileA(p);
    int e = ok ? 0 : (GetLastError() == ERROR_FILE_NOT_FOUND || GetLastError() == ERROR_PATH_NOT_FOUND ? ENOENT : EACCES);
#else
    int e = unlink(p) == 0 ? 0 : errno;
#endif
    free(p);
    if (e) {
        io_error(err, e);
        return 0;
    }
    return 1;
}

/* ── paths (the interpreter follows Rust's `Path`; so does this) ────────── */

/* `a ni, b tho file.path_kalupu()`: `b` alone when it is absolute. */
void ante_path_join(ante_str *out, const ante_str *a, const ante_str *b) {
    ante_str r = {0, 0, 0};
    if ((b->len > 0 && is_sep((char)b->ptr[0])) || a->len == 0) {
        ante_buf_push(&r, b->ptr, b->len);
    } else {
        ante_buf_push(&r, a->ptr, a->len);
        if (!is_sep((char)a->ptr[a->len - 1])) push_c(&r, "/");
        ante_buf_push(&r, b->ptr, b->len);
    }
    *out = r;
}

/* The last component of `s[0..n)`, ignoring trailing separators and `.`
   components: its start and length. Returns 0 when there is none. */
static int last_component(const uint8_t *s, int64_t n, int64_t *start, int64_t *len) {
    for (;;) {
        while (n > 0 && is_sep((char)s[n - 1])) n--;
        if (n == 0) return 0;
        int64_t b = n;
        while (b > 0 && !is_sep((char)s[b - 1])) b--;
        if (n - b == 1 && s[b] == '.' && b > 0) {
            n = b;
            continue;
        }
        *start = b;
        *len = n - b;
        return 1;
    }
}

/* `file.path_peru`: the file name, or "" (for `..`, `/` or ""). */
void ante_path_name(ante_str *out, const ante_str *p) {
    ante_str r = {0, 0, 0};
    int64_t b, n;
    if (last_component(p->ptr, p->len, &b, &n) && !(n == 2 && p->ptr[b] == '.' && p->ptr[b + 1] == '.') &&
        !(n == 1 && p->ptr[b] == '.')) {
        ante_buf_push(&r, p->ptr + b, n);
    }
    *out = r;
}

/* `file.path_folder`: everything before the last component, or "". */
void ante_path_folder(ante_str *out, const ante_str *p) {
    ante_str r = {0, 0, 0};
    int64_t b, n;
    if (last_component(p->ptr, p->len, &b, &n)) {
        int64_t end = b;
        /* Trim trailing separators and `.` components, keeping the root. */
        int64_t min = p->len > 0 && is_sep((char)p->ptr[0]) ? 1 : 0;
        for (;;) {
            while (end > min && is_sep((char)p->ptr[end - 1])) end--;
            if (end >= 2 && p->ptr[end - 1] == '.' && is_sep((char)p->ptr[end - 2])) {
                end--;
                continue;
            }
            break;
        }
        ante_buf_push(&r, p->ptr, end);
    }
    *out = r;
}

/* `file.path_extension`: after the last `.` of the file name, unless the
   name starts with its only dot. Returns 0 for none. */
int32_t ante_path_extension(ante_str *out, const ante_str *p) {
    ante_str name;
    ante_path_name(&name, p);
    int64_t dot = -1;
    for (int64_t i = name.len - 1; i >= 0; i--) {
        if (name.ptr[i] == '.') {
            dot = i;
            break;
        }
    }
    if (dot <= 0) {
        ante_buf_free(&name);
        return 0;
    }
    ante_str r = {0, 0, 0};
    ante_buf_push(&r, name.ptr + dot + 1, name.len - dot - 1);
    ante_buf_free(&name);
    *out = r;
    return 1;
}

/* ── running programs (`system.nadupu`) ───────────────────────────────── */

#ifdef _WIN32
typedef struct {
    HANDLE h;
    ante_str buf;
} pipe_reader;

static DWORD WINAPI drain(LPVOID arg) {
    pipe_reader *r = (pipe_reader *)arg;
    uint8_t chunk[8192];
    DWORD n;
    while (ReadFile(r->h, chunk, sizeof chunk, &n, 0) && n > 0) ante_buf_push(&r->buf, chunk, (int64_t)n);
    return 0;
}

/* One argument, quoted the way the C runtime splits a command line. */
static void quote_arg(ante_str *cmd, const ante_str *a) {
    int plain = a->len > 0;
    for (int64_t i = 0; i < a->len; i++)
        if (a->ptr[i] == ' ' || a->ptr[i] == '\t' || a->ptr[i] == '"') plain = 0;
    if (plain) {
        ante_buf_push(cmd, a->ptr, a->len);
        return;
    }
    push_c(cmd, "\"");
    int64_t slashes = 0;
    for (int64_t i = 0; i < a->len; i++) {
        uint8_t c = a->ptr[i];
        if (c == '\\') {
            slashes++;
            continue;
        }
        for (int64_t k = 0; k < (c == '"' ? slashes * 2 + 1 : slashes); k++) push_c(cmd, "\\");
        slashes = 0;
        ante_buf_push(cmd, &c, 1);
    }
    for (int64_t k = 0; k < slashes * 2; k++) push_c(cmd, "\\");
    push_c(cmd, "\"");
}
#endif

/* Runs `prog` with `args` (a `varusa[padham]`), waits for it, and collects
   its exit status and output. A program killed by a signal has status -1. */
int32_t ante_run(const ante_str *prog, const ante_vec *args, int32_t *status, ante_str *out_s, ante_str *err_s,
                 ante_str *err) {
    const ante_str *av = (const ante_str *)args->ptr;
    ante_flush();
#ifdef _WIN32
    ante_str cmd = {0, 0, 0};
    quote_arg(&cmd, prog);
    for (int64_t i = 0; i < args->len; i++) {
        push_c(&cmd, " ");
        quote_arg(&cmd, &av[i]);
    }
    uint8_t z = 0;
    ante_buf_push(&cmd, &z, 1);
    SECURITY_ATTRIBUTES sa = {sizeof sa, 0, TRUE};
    HANDLE or_, ow, er, ew;
    CreatePipe(&or_, &ow, &sa, 0);
    CreatePipe(&er, &ew, &sa, 0);
    SetHandleInformation(or_, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(er, HANDLE_FLAG_INHERIT, 0);
    STARTUPINFOA si;
    memset(&si, 0, sizeof si);
    si.cb = sizeof si;
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    si.hStdOutput = ow;
    si.hStdError = ew;
    PROCESS_INFORMATION pi;
    BOOL ok = CreateProcessA(0, (char *)cmd.ptr, 0, 0, TRUE, 0, 0, 0, &si, &pi);
    ante_buf_free(&cmd);
    CloseHandle(ow);
    CloseHandle(ew);
    if (!ok) {
        CloseHandle(or_);
        CloseHandle(er);
        io_error(err, ENOENT);
        return 0;
    }
    pipe_reader ro = {or_, {0, 0, 0}}, re = {er, {0, 0, 0}};
    HANDLE t1 = CreateThread(0, 0, drain, &ro, 0, 0);
    HANDLE t2 = CreateThread(0, 0, drain, &re, 0, 0);
    WaitForSingleObject(pi.hProcess, INFINITE);
    WaitForSingleObject(t1, INFINITE);
    WaitForSingleObject(t2, INFINITE);
    DWORD code = 0;
    GetExitCodeProcess(pi.hProcess, &code);
    CloseHandle(t1);
    CloseHandle(t2);
    CloseHandle(or_);
    CloseHandle(er);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    *status = (int32_t)code;
    *out_s = ro.buf;
    *err_s = re.buf;
    return 1;
#else
    char **argv = (char **)ante_alloc(size_mul(size_add(args->len, 2), (int64_t)sizeof(char *)));
    argv[0] = cstr(prog);
    for (int64_t i = 0; i < args->len; i++) argv[i + 1] = cstr(&av[i]);
    argv[args->len + 1] = 0;
    int po[2], pe[2];
    if (pipe(po) != 0 || pipe(pe) != 0) {
        io_error(err, errno);
        return 0;
    }
    posix_spawn_file_actions_t fa;
    posix_spawn_file_actions_init(&fa);
    posix_spawn_file_actions_adddup2(&fa, po[1], 1);
    posix_spawn_file_actions_adddup2(&fa, pe[1], 2);
    posix_spawn_file_actions_addclose(&fa, po[0]);
    posix_spawn_file_actions_addclose(&fa, pe[0]);
    pid_t pid;
    int e = posix_spawnp(&pid, argv[0], &fa, 0, argv, environ);
    posix_spawn_file_actions_destroy(&fa);
    for (int64_t i = 0; i <= args->len; i++) free(argv[i]);
    free(argv);
    close(po[1]);
    close(pe[1]);
    if (e != 0) {
        close(po[0]);
        close(pe[0]);
        io_error(err, e);
        return 0;
    }
    /* Both pipes at once, so a child filling one never blocks on the other. */
    ante_str o = {0, 0, 0}, x = {0, 0, 0};
    struct pollfd fds[2] = {{po[0], POLLIN, 0}, {pe[0], POLLIN, 0}};
    int open_n = 2;
    uint8_t chunk[8192];
    while (open_n > 0) {
        if (poll(fds, 2, -1) < 0) {
            if (errno == EINTR) continue;
            break;
        }
        for (int k = 0; k < 2; k++) {
            if (fds[k].fd < 0 || !(fds[k].revents & (POLLIN | POLLHUP | POLLERR))) continue;
            ssize_t n = read(fds[k].fd, chunk, sizeof chunk);
            if (n > 0) {
                ante_buf_push(k == 0 ? &o : &x, chunk, (int64_t)n);
            } else if (n == 0 || errno != EINTR) {
                close(fds[k].fd);
                fds[k].fd = -1;
                open_n--;
            }
        }
    }
    int ws = 0;
    while (waitpid(pid, &ws, 0) < 0 && errno == EINTR) {}
    *status = WIFEXITED(ws) ? WEXITSTATUS(ws) : -1;
    *out_s = o;
    *err_s = x;
    return 1;
#endif
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
