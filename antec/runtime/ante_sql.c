/* ── SQLite (§29.3) ───────────────────────────────────────────────────────
   Compiled together with the SQLite amalgamation into one object, cached
   like the runtime, and linked only into programs that use `sql`. The `sql`
   module is written in "Ante" and calls these through `bayata "C"`.

   A database is one SQLite connection behind a lock that a thread may take
   again (a transaction holds it while its function runs queries).
   Statements are prepared once per text and kept. */

#include "sqlite3.c"

#ifdef _WIN32
typedef CRITICAL_SECTION sql_mtx;
#define sql_mtx_init InitializeCriticalSection
#define sql_mtx_lock EnterCriticalSection
#define sql_mtx_unlock LeaveCriticalSection
#define sql_mtx_free DeleteCriticalSection
#else
#include <pthread.h>
typedef pthread_mutex_t sql_mtx;
static void sql_mtx_init(sql_mtx *m) {
    pthread_mutexattr_t a;
    pthread_mutexattr_init(&a);
    pthread_mutexattr_settype(&a, PTHREAD_MUTEX_RECURSIVE);
    pthread_mutex_init(m, &a);
    pthread_mutexattr_destroy(&a);
}
#define sql_mtx_lock pthread_mutex_lock
#define sql_mtx_unlock pthread_mutex_unlock
#define sql_mtx_free pthread_mutex_destroy
#endif

#if defined(_MSC_VER) && !defined(__clang__)
static __declspec(thread) char sql_msg[512];
#else
static _Thread_local char sql_msg[512];
#endif

typedef struct {
    char *text;
    int64_t len;
    sqlite3_stmt *st;
} sql_cached;

#define SQL_CACHE 256

typedef struct {
    sqlite3 *db;
    sql_mtx lock;
    sql_cached cache[SQL_CACHE];
    int32_t ncache;
} ante_db;

static int64_t sql_fail(sqlite3 *db, int code) {
    snprintf(sql_msg, sizeof sql_msg, "%s", db ? sqlite3_errmsg(db) : sqlite3_errstr(code));
    return code > 0 ? -code : -1;
}

/* The last error's message on this thread; its length (at most `cap`). */
int64_t ante_sql_error(uint8_t *out, int64_t cap) {
    int64_t n = (int64_t)strlen(sql_msg);
    if (n > cap) n = cap;
    memcpy(out, sql_msg, (size_t)n);
    return n;
}

/* Opens (creating if needed) the database file at `path`, in WAL mode with
   foreign keys on. Returns NULL with `*err` < 0 on failure. */
void *ante_sql_open(const uint8_t *path, int64_t n, int64_t *err) {
    char *p = (char *)malloc((size_t)n + 1);
    if (!p) abort();
    if (n > 0) memcpy(p, path, (size_t)n);
    p[n] = 0;
    sqlite3 *db = NULL;
    int rc = sqlite3_open_v2(p, &db, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_NOMUTEX, NULL);
    free(p);
    if (rc != SQLITE_OK) {
        *err = sql_fail(db, rc);
        sqlite3_close(db);
        return NULL;
    }
    sqlite3_busy_timeout(db, 5000);
    sqlite3_exec(db,
                 "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON; "
                 "PRAGMA temp_store = MEMORY;",
                 NULL, NULL, NULL);
    ante_db *d = (ante_db *)calloc(1, sizeof *d);
    if (!d) abort();
    d->db = db;
    sql_mtx_init(&d->lock);
    *err = 0;
    return d;
}

void ante_sql_close(void *h) {
    ante_db *d = (ante_db *)h;
    if (!d) return;
    for (int32_t i = 0; i < d->ncache; i++) {
        sqlite3_finalize(d->cache[i].st);
        free(d->cache[i].text);
    }
    sqlite3_close(d->db);
    sql_mtx_free(&d->lock);
    free(d);
}

void ante_sql_lock(void *h) { sql_mtx_lock(&((ante_db *)h)->lock); }
void ante_sql_unlock(void *h) { sql_mtx_unlock(&((ante_db *)h)->lock); }

/* The statement for `text`, prepared once and reset for reuse. NULL with
   `*err` < 0 if it doesn't compile. Call with the lock held. */
void *ante_sql_prepare(void *h, const uint8_t *text, int64_t n, int64_t *err) {
    ante_db *d = (ante_db *)h;
    for (int32_t i = 0; i < d->ncache; i++) {
        sql_cached *c = &d->cache[i];
        if (c->len == n && memcmp(c->text, text, (size_t)n) == 0) {
            sqlite3_reset(c->st);
            sqlite3_clear_bindings(c->st);
            *err = 0;
            return c->st;
        }
    }
    sqlite3_stmt *st = NULL;
    int rc = sqlite3_prepare_v3(d->db, (const char *)text, (int)n, SQLITE_PREPARE_PERSISTENT, &st, NULL);
    if (rc != SQLITE_OK || !st) {
        *err = rc != SQLITE_OK ? sql_fail(d->db, rc) : (snprintf(sql_msg, sizeof sql_msg, "query khaali"), -1);
        sqlite3_finalize(st);
        return NULL;
    }
    if (d->ncache == SQL_CACHE) {
        /* Full: start again rather than track use. */
        for (int32_t i = 0; i < d->ncache; i++) {
            sqlite3_finalize(d->cache[i].st);
            free(d->cache[i].text);
        }
        d->ncache = 0;
    }
    sql_cached *c = &d->cache[d->ncache++];
    c->text = (char *)malloc((size_t)n + 1);
    if (!c->text) abort();
    memcpy(c->text, text, (size_t)n);
    c->len = n;
    c->st = st;
    *err = 0;
    return st;
}

int64_t ante_sql_params(void *st) { return sqlite3_bind_parameter_count((sqlite3_stmt *)st); }
int64_t ante_sql_bind_null(void *st, int64_t i) { return sqlite3_bind_null((sqlite3_stmt *)st, (int)i); }
int64_t ante_sql_bind_int(void *st, int64_t i, int64_t v) { return sqlite3_bind_int64((sqlite3_stmt *)st, (int)i, v); }
int64_t ante_sql_bind_float(void *st, int64_t i, double v) { return sqlite3_bind_double((sqlite3_stmt *)st, (int)i, v); }
int64_t ante_sql_bind_text(void *st, int64_t i, const uint8_t *p, int64_t n) {
    static const char empty[1] = "";
    return sqlite3_bind_text64((sqlite3_stmt *)st, (int)i, n > 0 ? (const char *)p : empty, (sqlite3_uint64)n,
                               SQLITE_TRANSIENT, SQLITE_UTF8);
}
int64_t ante_sql_bind_blob(void *st, int64_t i, const uint8_t *p, int64_t n) {
    static const char empty[1] = "";
    return sqlite3_bind_blob64((sqlite3_stmt *)st, (int)i, n > 0 ? (const void *)p : empty, (sqlite3_uint64)n,
                               SQLITE_TRANSIENT);
}

/* One step: 1 a row, 0 done, < 0 an error. */
int64_t ante_sql_step(void *h, void *st) {
    int rc = sqlite3_step((sqlite3_stmt *)st);
    if (rc == SQLITE_ROW) return 1;
    if (rc == SQLITE_DONE) return 0;
    int64_t e = sql_fail(((ante_db *)h)->db, rc);
    sqlite3_reset((sqlite3_stmt *)st);
    return e;
}

void ante_sql_reset(void *st) { sqlite3_reset((sqlite3_stmt *)st); }
int64_t ante_sql_columns(void *st) { return sqlite3_column_count((sqlite3_stmt *)st); }

/* 1 INTEGER, 2 REAL, 3 TEXT, 4 BLOB, 5 NULL; 6 for an INTEGER in a column
   declared BOOL/BOOLEAN, which reads as `ato_ito`. */
int64_t ante_sql_column_type(void *st, int64_t i) {
    int t = sqlite3_column_type((sqlite3_stmt *)st, (int)i);
    if (t == SQLITE_INTEGER) {
        const char *d = sqlite3_column_decltype((sqlite3_stmt *)st, (int)i);
        if (d && (sqlite3_strnicmp(d, "BOOL", 4) == 0)) return 6;
    }
    return t;
}

const uint8_t *ante_sql_column_name(void *st, int64_t i, int64_t *n) {
    const char *s = sqlite3_column_name((sqlite3_stmt *)st, (int)i);
    *n = s ? (int64_t)strlen(s) : 0;
    return (const uint8_t *)(s ? s : "");
}

int64_t ante_sql_column_int(void *st, int64_t i) { return sqlite3_column_int64((sqlite3_stmt *)st, (int)i); }
double ante_sql_column_float(void *st, int64_t i) { return sqlite3_column_double((sqlite3_stmt *)st, (int)i); }

const uint8_t *ante_sql_column_text(void *st, int64_t i, int64_t *n) {
    const unsigned char *s = sqlite3_column_text((sqlite3_stmt *)st, (int)i);
    *n = sqlite3_column_bytes((sqlite3_stmt *)st, (int)i);
    return s ? s : (const uint8_t *)"";
}

const uint8_t *ante_sql_column_blob(void *st, int64_t i, int64_t *n) {
    const void *s = sqlite3_column_blob((sqlite3_stmt *)st, (int)i);
    *n = sqlite3_column_bytes((sqlite3_stmt *)st, (int)i);
    return s ? (const uint8_t *)s : (const uint8_t *)"";
}

int64_t ante_sql_changes(void *h) { return sqlite3_changes64(((ante_db *)h)->db); }

/* Runs `text` (one or more statements, no parameters): transactions and
   migrations. 0, or < 0 with the message saved. */
int64_t ante_sql_exec(void *h, const uint8_t *text, int64_t n) {
    ante_db *d = (ante_db *)h;
    char *s = (char *)malloc((size_t)n + 1);
    if (!s) abort();
    if (n > 0) memcpy(s, text, (size_t)n);
    s[n] = 0;
    char *msg = NULL;
    int rc = sqlite3_exec(d->db, s, NULL, NULL, &msg);
    free(s);
    if (rc != SQLITE_OK) {
        snprintf(sql_msg, sizeof sql_msg, "%s", msg ? msg : sqlite3_errstr(rc));
        sqlite3_free(msg);
        return -rc;
    }
    return 0;
}

/* Whether the connection is inside a transaction. */
int64_t ante_sql_in_transaction(void *h) { return !sqlite3_get_autocommit(((ante_db *)h)->db); }

/* The package's `migrations/*.sql`, which the build writes next to the
   program (§29.3.3). */
extern const int64_t ante_migration_count;
extern const uint8_t *const ante_migration_names[];
extern const uint8_t *const ante_migration_texts[];
extern const int64_t ante_migration_name_lens[];
extern const int64_t ante_migration_text_lens[];

int64_t ante_sql_migrations(void) { return ante_migration_count; }

const uint8_t *ante_sql_migration_name(int64_t i, int64_t *n) {
    *n = ante_migration_name_lens[i];
    return ante_migration_names[i];
}

const uint8_t *ante_sql_migration_text(int64_t i, int64_t *n) {
    *n = ante_migration_text_lens[i];
    return ante_migration_texts[i];
}
