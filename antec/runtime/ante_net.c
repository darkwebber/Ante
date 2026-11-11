/* ── networking (§28) ─────────────────────────────────────────────────────
   Appended to ante_rt.c (one translation unit). The `net` and `http`
   modules are written in "Ante" and call these through `bayata "C"`.

   Errors come back as negative kinds (NetThappu's variants, below); the
   message of the last `Os`/`Limit`/`PeruDorakaledhu` error on this thread
   is read with `ante_net_error`.

   The HTTP server is one event loop per `ante_http` object: it owns the
   sockets, reads and parses requests, and hands complete ones to "Ante" one
   at a time (`ante_http_next`). Responses are queued (`ante_http_respond`)
   and written when the loop next runs out of requests, so a busy server
   writes many responses per system call round. */

#define NET_NAME (-1)    /* PeruDorakaledhu */
#define NET_REFUSED (-2) /* Refused */
#define NET_RESET (-3)   /* Reset */
#define NET_TIMEOUT (-4) /* Timeout */
#define NET_CANCEL (-5)  /* Cancel */
#define NET_CLOSED (-6)  /* Moosesaaru */
#define NET_LIMIT (-8)   /* Limit */
#define NET_OS (-9)      /* Os */

#ifdef _WIN32
typedef SOCKET sock_t;
#define BAD_SOCK INVALID_SOCKET
#define sock_close closesocket
#define sock_errno() WSAGetLastError()
#define E_AGAIN WSAEWOULDBLOCK
#define E_INPROGRESS WSAEWOULDBLOCK
#define E_INTR WSAEINTR
#define E_CONNREFUSED WSAECONNREFUSED
#define E_CONNRESET WSAECONNRESET
#define E_PIPE WSAECONNABORTED
#define E_TIMEDOUT WSAETIMEDOUT
#define poll_fn WSAPoll
typedef WSAPOLLFD pollfd_t;
#else
typedef int sock_t;
#define BAD_SOCK (-1)
#define sock_close close
#define sock_errno() errno
#define E_AGAIN EAGAIN
#define E_INPROGRESS EINPROGRESS
#define E_INTR EINTR
#define E_CONNREFUSED ECONNREFUSED
#define E_CONNRESET ECONNRESET
#define E_PIPE EPIPE
#define E_TIMEDOUT ETIMEDOUT
#define poll_fn poll
typedef struct pollfd pollfd_t;
#endif

#if defined(_MSC_VER) && !defined(__clang__)
static __declspec(thread) char net_msg[256];
#else
static _Thread_local char net_msg[256];
#endif

static int64_t net_fail(int64_t kind, const char *what, int code) {
    if (code) {
#ifdef _WIN32
        snprintf(net_msg, sizeof net_msg, "%s: error %d", what, code);
#else
        snprintf(net_msg, sizeof net_msg, "%s: %s", what, strerror(code));
#endif
    } else {
        snprintf(net_msg, sizeof net_msg, "%s", what);
    }
    return kind;
}

/* Maps a socket error to a NetThappu kind. */
static int64_t net_os(const char *what, int code) {
    if (code == E_CONNREFUSED) return net_fail(NET_REFUSED, what, code);
    if (code == E_CONNRESET || code == E_PIPE) return net_fail(NET_RESET, what, code);
    if (code == E_TIMEDOUT) return net_fail(NET_TIMEOUT, what, code);
    return net_fail(NET_OS, what, code);
}

/* The last error's message; returns its length (at most `cap`). */
int64_t ante_net_error(uint8_t *out, int64_t cap) {
    int64_t n = (int64_t)strlen(net_msg);
    if (n > cap) n = cap;
    memcpy(out, net_msg, (size_t)n);
    return n;
}

static void net_init(void) {
#ifdef _WIN32
    static LONG done;
    if (InterlockedCompareExchange(&done, 1, 0) == 0) {
        WSADATA w;
        WSAStartup(MAKEWORD(2, 2), &w);
    }
#else
    static int done;
    if (!done) {
        done = 1;
        signal(SIGPIPE, SIG_IGN); /* a peer that hangs up is an error, not a kill */
    }
#endif
}

static int set_nonblocking(sock_t s, int on) {
#ifdef _WIN32
    u_long v = (u_long)on;
    return ioctlsocket(s, FIONBIO, &v) == 0 ? 0 : -1;
#else
    int f = fcntl(s, F_GETFL, 0);
    if (f < 0) return -1;
    return fcntl(s, F_SETFL, on ? f | O_NONBLOCK : f & ~O_NONBLOCK);
#endif
}

/* While a task sits in a system call that may wait, its worker doesn't
   count as running (as in `tk_wait`), so other tasks keep going. */
static void net_block_begin(void) {
    m_lock(&tk);
    if (cur) {
        workers_running--;
        dispatch();
    }
    m_unlock(&tk);
}

static void net_block_end(void) {
    m_lock(&tk);
    if (cur) workers_running++;
    m_unlock(&tk);
}

/* Waits until `s` is readable (`out` 0) or writable (`out` 1); `ms` < 0
   waits forever. Every wait is a cancellation point (§24.6), checked at
   least every 100 ms. Returns 1 ready, 0 timed out, or a negative kind. */
static int64_t net_wait(sock_t s, int out, int64_t ms) {
    pollfd_t p;
    memset(&p, 0, sizeof p);
    p.fd = s;
    p.events = out ? POLLOUT : POLLIN;
    int64_t until = ms < 0 ? -1 : now_ms() + ms;
    for (;;) {
        if (ante_task_cancelled()) return net_fail(NET_CANCEL, "cancel ayyindhi", 0);
        int64_t left = until < 0 ? 100 : until - now_ms();
        if (left < 0) left = 0;
        if (left > 100) left = 100;
        p.revents = 0;
        net_block_begin();
        int r = poll_fn(&p, 1, (int)left);
        net_block_end();
        if (r < 0 && sock_errno() != E_INTR) return net_os("poll", sock_errno());
        if (r > 0) return 1;
        if (until >= 0 && now_ms() >= until) return 0;
    }
}

static char *net_cstr(const uint8_t *p, int64_t n) {
    char *s = (char *)ante_alloc(n + 1);
    if (n > 0) memcpy(s, p, (size_t)n);
    s[n] = 0;
    return s;
}

/* Looks up `host:port` (blocking, off the run queue). */
static int64_t net_resolve(const uint8_t *host, int64_t hlen, int64_t port, int passive, struct addrinfo **out) {
    net_init();
    if (port < 0 || port > 65535) return net_fail(NET_OS, "port 0..65535 madhyalo undaali", 0);
    char *h = net_cstr(host, hlen);
    char ps[16];
    snprintf(ps, sizeof ps, "%d", (int)port);
    struct addrinfo hints;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    if (passive) hints.ai_flags = AI_PASSIVE;
    net_block_begin();
    int r = getaddrinfo(hlen > 0 ? h : NULL, ps, &hints, out);
    net_block_end();
    if (r != 0) {
        snprintf(net_msg, sizeof net_msg, "`%s` dorakaledhu", h);
        free(h);
        return NET_NAME;
    }
    free(h);
    return 0;
}

static sock_t net_listen_sock(struct addrinfo *ai, int64_t backlog, int reuse_port, int64_t *err) {
    sock_t s = BAD_SOCK;
    int last = 0;
    for (struct addrinfo *a = ai; a; a = a->ai_next) {
        s = socket(a->ai_family, a->ai_socktype, a->ai_protocol);
        if (s == BAD_SOCK) {
            last = sock_errno();
            continue;
        }
        int one = 1;
        setsockopt(s, SOL_SOCKET, SO_REUSEADDR, (const char *)&one, sizeof one);
#ifdef SO_REUSEPORT
        if (reuse_port) setsockopt(s, SOL_SOCKET, SO_REUSEPORT, (const char *)&one, sizeof one);
#else
        (void)reuse_port;
#endif
        if (bind(s, a->ai_addr, (int)a->ai_addrlen) == 0 && listen(s, (int)backlog) == 0) return s;
        last = sock_errno();
        sock_close(s);
        s = BAD_SOCK;
    }
    *err = net_os("listen", last);
    return BAD_SOCK;
}

/* `net.vinu`: a listening socket. Returns the socket (≥ 0) or a kind. */
int64_t ante_net_listen(const uint8_t *host, int64_t hlen, int64_t port) {
    struct addrinfo *ai;
    int64_t r = net_resolve(host, hlen, port, 1, &ai);
    if (r < 0) return r;
    int64_t err = 0;
    sock_t s = net_listen_sock(ai, 1024, 0, &err);
    freeaddrinfo(ai);
    return s == BAD_SOCK ? err : (int64_t)s;
}

/* The port a listening socket got (useful after asking for port 0). */
int64_t ante_net_local_port(int64_t fd) {
    struct sockaddr_storage a;
    socklen_t n = sizeof a;
    if (getsockname((sock_t)fd, (struct sockaddr *)&a, &n) != 0) return net_os("getsockname", sock_errno());
    if (a.ss_family == AF_INET) return ntohs(((struct sockaddr_in *)&a)->sin_port);
    if (a.ss_family == AF_INET6) return ntohs(((struct sockaddr_in6 *)&a)->sin6_port);
    return 0;
}

/* `l.andhuko()`: waits for a connection. */
int64_t ante_net_accept(int64_t fd, int64_t ms) {
    for (;;) {
        int64_t w = net_wait((sock_t)fd, 0, ms);
        if (w < 0) return w;
        if (w == 0) return net_fail(NET_TIMEOUT, "accept time aipoyindhi", 0);
        sock_t c = accept((sock_t)fd, NULL, NULL);
        if (c != BAD_SOCK) {
            int one = 1;
            setsockopt(c, IPPROTO_TCP, TCP_NODELAY, (const char *)&one, sizeof one);
            return (int64_t)c;
        }
        int e = sock_errno();
        if (e != E_AGAIN && e != E_INTR) return net_os("accept", e);
    }
}

/* `net.connect`: connects with a time limit. */
int64_t ante_net_connect(const uint8_t *host, int64_t hlen, int64_t port, int64_t ms) {
    struct addrinfo *ai;
    int64_t r = net_resolve(host, hlen, port, 0, &ai);
    if (r < 0) return r;
    int64_t err = net_fail(NET_REFUSED, "connect", 0);
    for (struct addrinfo *a = ai; a; a = a->ai_next) {
        sock_t s = socket(a->ai_family, a->ai_socktype, a->ai_protocol);
        if (s == BAD_SOCK) {
            err = net_os("socket", sock_errno());
            continue;
        }
        set_nonblocking(s, 1);
        int c = connect(s, a->ai_addr, (int)a->ai_addrlen);
        if (c != 0 && sock_errno() != E_INPROGRESS && sock_errno() != E_AGAIN) {
            err = net_os("connect", sock_errno());
            sock_close(s);
            continue;
        }
        if (c != 0) {
            int64_t w = net_wait(s, 1, ms);
            int soerr = 0;
            socklen_t n = sizeof soerr;
            if (w <= 0) {
                err = w < 0 ? w : net_fail(NET_TIMEOUT, "connect time aipoyindhi", 0);
                sock_close(s);
                continue;
            }
            getsockopt(s, SOL_SOCKET, SO_ERROR, (char *)&soerr, &n);
            if (soerr) {
                err = net_os("connect", soerr);
                sock_close(s);
                continue;
            }
        }
        set_nonblocking(s, 0);
        int one = 1;
        setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (const char *)&one, sizeof one);
        freeaddrinfo(ai);
        return (int64_t)s;
    }
    freeaddrinfo(ai);
    return err;
}

/* `c.chadhuvu()`: up to `cap` bytes; 0 at end of stream. */
int64_t ante_net_read(int64_t fd, uint8_t *buf, int64_t cap, int64_t ms) {
    if (cap <= 0) return 0;
    for (;;) {
        int64_t w = net_wait((sock_t)fd, 0, ms);
        if (w < 0) return w;
        if (w == 0) return net_fail(NET_TIMEOUT, "read time aipoyindhi", 0);
        int n = (int)recv((sock_t)fd, (char *)buf, (int)(cap > 1 << 30 ? 1 << 30 : cap), 0);
        if (n >= 0) return n;
        int e = sock_errno();
        if (e != E_AGAIN && e != E_INTR) return net_os("read", e);
    }
}

/* `c.raayi()`: all of `buf`. */
int64_t ante_net_write(int64_t fd, const uint8_t *buf, int64_t len, int64_t ms) {
    int64_t done = 0;
    while (done < len) {
        int64_t w = net_wait((sock_t)fd, 1, ms);
        if (w < 0) return w;
        if (w == 0) return net_fail(NET_TIMEOUT, "write time aipoyindhi", 0);
        int64_t chunk = len - done > 1 << 30 ? 1 << 30 : len - done;
#ifdef MSG_NOSIGNAL
        int n = (int)send((sock_t)fd, (const char *)buf + done, (int)chunk, MSG_NOSIGNAL);
#else
        int n = (int)send((sock_t)fd, (const char *)buf + done, (int)chunk, 0);
#endif
        if (n > 0) {
            done += n;
            continue;
        }
        int e = sock_errno();
        if (e != E_AGAIN && e != E_INTR) return net_os("write", e);
    }
    return done;
}

void ante_net_close(int64_t fd) {
    if (fd >= 0) sock_close((sock_t)fd);
}

/* ── the HTTP/1.1 server loop (§28.5) ── */

typedef struct {
    uint64_t conn;
    const uint8_t *method;
    int64_t method_len;
    const uint8_t *path; /* the target up to `?` */
    int64_t path_len;
    const uint8_t *query; /* after `?`, without it */
    int64_t query_len;
    int64_t headers; /* how many; read with `ante_http_header` */
    const uint8_t *body;
    int64_t body_len;
    int64_t keep_alive;
    int64_t id; /* request number, for logs */
} ante_http_req;

typedef struct {
    const uint8_t *name; /* lower case */
    int64_t name_len;
    const uint8_t *value; /* without surrounding spaces */
    int64_t value_len;
} ante_http_hdr;

#define HTTP_MAX_HEADERS 100

typedef struct {
    uint8_t *p;
    int64_t len, cap, off; /* bytes [off, len) are live */
} hbuf;

static void hb_reserve(hbuf *b, int64_t extra) {
    if (b->off > 0 && b->off == b->len) b->off = b->len = 0;
    if (b->len + extra <= b->cap) return;
    if (b->off > 0 && b->len - b->off + extra <= b->cap) {
        memmove(b->p, b->p + b->off, (size_t)(b->len - b->off));
        b->len -= b->off;
        b->off = 0;
        return;
    }
    int64_t cap = b->cap ? b->cap : 1024;
    while (cap < b->len - b->off + extra) cap *= 2;
    uint8_t *p = (uint8_t *)malloc((size_t)cap);
    if (!p) out_of_memory();
    if (b->len > b->off) memcpy(p, b->p + b->off, (size_t)(b->len - b->off));
    free(b->p);
    b->p = p;
    b->len -= b->off;
    b->off = 0;
    b->cap = cap;
}

static void hb_put(hbuf *b, const void *s, int64_t n) {
    if (n <= 0) return;
    hb_reserve(b, n);
    memcpy(b->p + b->len, s, (size_t)n);
    b->len += n;
}

static void hb_free(hbuf *b) {
    free(b->p);
    memset(b, 0, sizeof *b);
}

enum { C_FREE, C_READ, C_QUEUED, C_HANDLER, C_CLOSING };

typedef struct {
    sock_t s;
    uint32_t gen;
    int32_t state;
    int32_t want_out;  /* registered for writability */
    int32_t close_after;
    int64_t last;      /* ms of the last activity */
    int64_t started;   /* ms the current request's first byte came */
    hbuf in, out;
    hbuf body;         /* a chunked body, decoded */
    int64_t req_len;   /* bytes of `in` the handed-out request uses */
    int32_t next_q;    /* ready queue link */
    int32_t chunked;   /* the handed-out request's body is in `body` */
    int32_t old_keep;  /* an HTTP/1.0 request that asked to keep the connection */
} hconn;

typedef struct {
    sock_t ls;
    int64_t max_conns, header_limit, body_limit, idle_ms, header_ms;
    hconn *c;
    int32_t nc, capc;
    int32_t *free_slots;
    int32_t nfree, live;
    int32_t q_head, q_tail; /* ready requests */
    int32_t handler;        /* slot handed out, or -1 */
    int64_t handler_id;     /* that request's number */
    int64_t next_id;
    int64_t last_sweep;
    volatile int32_t stopping;
    ante_task *owner; /* the task in `http.serve`: cancelling it stops the server */
#ifdef __linux__
    int ep;
    int wake[2];
#else
    sock_t wake_r, wake_w;
#endif
    int32_t *dirty; /* slots with output to write */
    int32_t ndirty, capdirty;
    char date[40];
    int64_t date_sec;
    ante_http_hdr hdr[HTTP_MAX_HEADERS]; /* the handed-out request's */
    int64_t nhdr;
} ante_http;

static ante_http *volatile http_servers[64];

static void http_wake(ante_http *h) {
#ifdef __linux__
    char c = 1;
    ssize_t r = write(h->wake[1], &c, 1);
    (void)r;
#else
    char c = 1;
    send(h->wake_w, &c, 1, 0);
#endif
}

#ifndef _WIN32
static void http_on_signal(int sig) {
    (void)sig;
    for (int i = 0; i < 64; i++) {
        ante_http *h = http_servers[i];
        if (h) {
            h->stopping = 1;
            http_wake(h);
        }
    }
}
#else
static BOOL WINAPI http_on_ctrl(DWORD t) {
    (void)t;
    for (int i = 0; i < 64; i++) {
        ante_http *h = http_servers[i];
        if (h) {
            h->stopping = 1;
            http_wake(h);
        }
    }
    return TRUE;
}
#endif

#ifdef __linux__
static void ep_set(ante_http *h, int op, sock_t s, uint32_t events, uint64_t data) {
    struct epoll_event e;
    memset(&e, 0, sizeof e);
    e.events = events;
    e.data.u64 = data;
    epoll_ctl(h->ep, op, s, &e);
}
#endif

#ifndef __linux__
/* A connected pair of sockets, to wake `poll` from a signal handler. */
static int wake_pair(sock_t out[2]) {
    sock_t l = socket(AF_INET, SOCK_STREAM, 0);
    if (l == BAD_SOCK) return -1;
    struct sockaddr_in a;
    memset(&a, 0, sizeof a);
    a.sin_family = AF_INET;
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    socklen_t n = sizeof a;
    if (bind(l, (struct sockaddr *)&a, sizeof a) != 0 || listen(l, 1) != 0 ||
        getsockname(l, (struct sockaddr *)&a, &n) != 0) {
        sock_close(l);
        return -1;
    }
    sock_t w = socket(AF_INET, SOCK_STREAM, 0);
    if (w == BAD_SOCK || connect(w, (struct sockaddr *)&a, sizeof a) != 0) {
        sock_close(l);
        return -1;
    }
    sock_t r = accept(l, NULL, NULL);
    sock_close(l);
    if (r == BAD_SOCK) return -1;
    set_nonblocking(r, 1);
    set_nonblocking(w, 1);
    out[0] = r;
    out[1] = w;
    return 0;
}
#endif

/* `http.serve`'s loop. `reuse_port` lets several loops share the address.
   Returns the server, or NULL with `*err` set to a kind. */
void *ante_http_open(const uint8_t *host, int64_t hlen, int64_t port, int64_t max_conns, int64_t header_limit,
                     int64_t body_limit, int64_t idle_ms, int64_t header_ms, int64_t reuse_port, int64_t *err) {
    struct addrinfo *ai;
    *err = net_resolve(host, hlen, port, 1, &ai);
    if (*err < 0) return NULL;
    sock_t ls = net_listen_sock(ai, 65535, (int)reuse_port, err);
    freeaddrinfo(ai);
    if (ls == BAD_SOCK) return NULL;
    set_nonblocking(ls, 1);
#ifndef _WIN32
    /* As many open connections as the system lets this process have. */
    struct rlimit rl;
    if (getrlimit(RLIMIT_NOFILE, &rl) == 0 && rl.rlim_cur < rl.rlim_max) {
        rl.rlim_cur = rl.rlim_max;
        setrlimit(RLIMIT_NOFILE, &rl);
    }
#endif
    ante_http *h = (ante_http *)tk_alloc(sizeof *h);
    h->ls = ls;
    h->max_conns = max_conns;
    h->header_limit = header_limit;
    h->body_limit = body_limit;
    h->idle_ms = idle_ms;
    h->header_ms = header_ms;
    h->q_head = h->q_tail = -1;
    h->handler = -1;
    h->last_sweep = now_ms();
    h->owner = current();
#ifdef __linux__
    h->ep = epoll_create1(EPOLL_CLOEXEC);
    if (h->ep < 0 || pipe(h->wake) != 0) {
        *err = net_os("epoll", errno);
        sock_close(ls);
        free(h);
        return NULL;
    }
    fcntl(h->wake[0], F_SETFL, O_NONBLOCK);
    fcntl(h->wake[1], F_SETFL, O_NONBLOCK);
    ep_set(h, EPOLL_CTL_ADD, ls, EPOLLIN, UINT64_MAX);
    ep_set(h, EPOLL_CTL_ADD, h->wake[0], EPOLLIN, UINT64_MAX - 1);
#else
    sock_t pr[2];
    if (wake_pair(pr) != 0) {
        *err = net_fail(NET_OS, "wake socket", 0);
        sock_close(ls);
        free(h);
        return NULL;
    }
    h->wake_r = pr[0];
    h->wake_w = pr[1];
#endif
    for (int i = 0; i < 64; i++) {
        if (!http_servers[i]) {
            http_servers[i] = h;
            break;
        }
    }
#ifndef _WIN32
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = http_on_signal;
    sigaction(SIGTERM, &sa, NULL);
    sigaction(SIGINT, &sa, NULL);
#else
    SetConsoleCtrlHandler(http_on_ctrl, TRUE);
#endif
    *err = 0;
    return h;
}

int64_t ante_http_port(void *srv) { return ante_net_local_port((int64_t)((ante_http *)srv)->ls); }

/* Asks the loop to stop: no new connections; `next` returns 0 once the
   queued requests are answered. Safe from any task. */
void ante_http_stop(void *srv) {
    ante_http *h = (ante_http *)srv;
    h->stopping = 1;
    http_wake(h);
}

static void http_dirty(ante_http *h, int32_t slot) {
    if (h->ndirty == h->capdirty) {
        h->capdirty = h->capdirty ? h->capdirty * 2 : 64;
        h->dirty = (int32_t *)realloc(h->dirty, sizeof(int32_t) * (size_t)h->capdirty);
        if (!h->dirty) out_of_memory();
    }
    h->dirty[h->ndirty++] = slot;
}

static void http_drop(ante_http *h, int32_t slot) {
    hconn *c = &h->c[slot];
    if (c->state == C_FREE) return;
#ifdef __linux__
    epoll_ctl(h->ep, EPOLL_CTL_DEL, c->s, NULL);
#endif
    sock_close(c->s);
    hb_free(&c->in);
    hb_free(&c->out);
    hb_free(&c->body);
    c->state = C_FREE;
    c->gen++;
    h->live--;
    h->free_slots[h->nfree++] = slot;
}

static void http_date(ante_http *h) {
    time_t t = time(NULL);
    if ((int64_t)t == h->date_sec) return;
    h->date_sec = (int64_t)t;
    struct tm g;
#ifdef _WIN32
    gmtime_s(&g, &t);
#else
    gmtime_r(&t, &g);
#endif
    strftime(h->date, sizeof h->date, "%a, %d %b %Y %H:%M:%S GMT", &g);
}

static const char *http_reason(int64_t code) {
    switch (code) {
    case 100: return "Continue";
    case 200: return "OK";
    case 201: return "Created";
    case 202: return "Accepted";
    case 204: return "No Content";
    case 301: return "Moved Permanently";
    case 302: return "Found";
    case 303: return "See Other";
    case 304: return "Not Modified";
    case 307: return "Temporary Redirect";
    case 308: return "Permanent Redirect";
    case 400: return "Bad Request";
    case 401: return "Unauthorized";
    case 403: return "Forbidden";
    case 404: return "Not Found";
    case 405: return "Method Not Allowed";
    case 408: return "Request Timeout";
    case 409: return "Conflict";
    case 413: return "Content Too Large";
    case 415: return "Unsupported Media Type";
    case 422: return "Unprocessable Content";
    case 429: return "Too Many Requests";
    case 431: return "Request Header Fields Too Large";
    case 500: return "Internal Server Error";
    case 501: return "Not Implemented";
    case 502: return "Bad Gateway";
    case 503: return "Service Unavailable";
    default: return "";
    }
}

/* Queues a whole response on `slot`. `head_only` (HEAD) keeps the length
   but drops the body. */
static void http_write_response(ante_http *h, int32_t slot, int64_t status, const uint8_t *extra, int64_t extra_len,
                                const uint8_t *body, int64_t body_len, int head_only, int close) {
    hconn *c = &h->c[slot];
    http_date(h);
    char line[256];
    int n = snprintf(line, sizeof line,
                     "HTTP/1.1 %d %s\r\nDate: %s\r\nContent-Length: %lld\r\n%s", (int)status, http_reason(status),
                     h->date, (long long)body_len,
                     close ? "Connection: close\r\n" : c->old_keep ? "Connection: keep-alive\r\n" : "");
    hb_reserve(&c->out, n + extra_len + 2 + (head_only ? 0 : body_len));
    hb_put(&c->out, line, n);
    hb_put(&c->out, extra, extra_len);
    hb_put(&c->out, "\r\n", 2);
    if (!head_only) hb_put(&c->out, body, body_len);
    if (close) c->close_after = 1;
    http_dirty(h, slot);
}

/* An error answered without calling the handler; the connection closes. */
static void http_reject(ante_http *h, int32_t slot, int64_t status, const char *msg) {
    hconn *c = &h->c[slot];
    static const char ct[] = "Content-Type: text/plain; charset=utf-8\r\n";
    http_write_response(h, slot, status, (const uint8_t *)ct, (int64_t)sizeof ct - 1, (const uint8_t *)msg,
                        (int64_t)strlen(msg), 0, 1);
    c->state = C_CLOSING;
}

static int ci_eq(const uint8_t *a, int64_t n, const char *b) {
    if ((int64_t)strlen(b) != n) return 0;
    for (int64_t i = 0; i < n; i++) {
        uint8_t x = a[i], y = (uint8_t)b[i];
        if (x >= 'A' && x <= 'Z') x = (uint8_t)(x + 32);
        if (y >= 'A' && y <= 'Z') y = (uint8_t)(y + 32);
        if (x != y) return 0;
    }
    return 1;
}

static int ci_has_token(const uint8_t *v, int64_t n, const char *tok) {
    int64_t i = 0;
    while (i < n) {
        while (i < n && (v[i] == ' ' || v[i] == '\t' || v[i] == ',')) i++;
        int64_t s = i;
        while (i < n && v[i] != ',') i++;
        int64_t e = i;
        while (e > s && (v[e - 1] == ' ' || v[e - 1] == '\t')) e--;
        if (ci_eq(v + s, e - s, tok)) return 1;
    }
    return 0;
}

static int is_tchar(uint8_t c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || strchr("!#$%&'*+-.^_`|~", c);
}

/* Decodes a chunked body that starts at `p` (`n` bytes buffered) into
   `out`. Returns the bytes used, 0 if more are needed, -1 if malformed, -2
   if it passes `limit`. */
static int64_t http_chunked(const uint8_t *p, int64_t n, hbuf *out, int64_t limit) {
    int64_t i = 0;
    out->len = out->off = 0;
    for (;;) {
        int64_t size = 0, digits = 0;
        while (i < n) {
            uint8_t c = p[i];
            int d = c >= '0' && c <= '9' ? c - '0' : c >= 'a' && c <= 'f' ? c - 'a' + 10 : c >= 'A' && c <= 'F' ? c - 'A' + 10 : -1;
            if (d < 0) break;
            if (size > (INT64_MAX >> 4)) return -1;
            size = size * 16 + d;
            digits++;
            i++;
        }
        if (i >= n) return 0;
        if (digits == 0) return -1;
        while (i < n && p[i] != '\n') i++; /* chunk extensions */
        if (i >= n) return 0;
        i++;
        if (size == 0) {
            /* trailers end with an empty line */
            for (;;) {
                int64_t s = i;
                while (i < n && p[i] != '\n') i++;
                if (i >= n) return 0;
                i++;
                if (i - s <= 2) return i;
            }
        }
        if (out->len + size > limit) return -2;
        if (n - i < size + 2) return 0;
        hb_put(out, p + i, size);
        i += size;
        if (p[i] == '\r') i++;
        if (i >= n) return 0;
        if (p[i] != '\n') return -1;
        i++;
    }
}

/* Tries to parse one request at the front of `c->in`. 1: complete (queued),
   0: needs more bytes, -1: rejected (an error response is queued). */
static int http_parse(ante_http *h, int32_t slot) {
    hconn *c = &h->c[slot];
    const uint8_t *p = c->in.p + c->in.off;
    int64_t n = c->in.len - c->in.off;
    /* Blank lines before a request are allowed (RFC 9112 §2.2). */
    int64_t skip = 0;
    while (skip < n && (p[skip] == '\r' || p[skip] == '\n')) skip++;
    if (skip > 0) {
        c->in.off += skip;
        p += skip;
        n -= skip;
    }
    if (n == 0) return 0;
    int64_t end = -1;
    for (int64_t i = 0; i + 1 < n; i++) {
        if (p[i] == '\n' && (p[i + 1] == '\n' || (p[i + 1] == '\r' && i + 2 < n && p[i + 2] == '\n'))) {
            end = p[i + 1] == '\n' ? i + 2 : i + 3;
            break;
        }
        if (i >= h->header_limit) break;
    }
    if (end < 0) {
        if (n > h->header_limit) {
            http_reject(h, slot, 431, "headers chaala pedhavi");
            return -1;
        }
        return 0;
    }
    if (end > h->header_limit) {
        http_reject(h, slot, 431, "headers chaala pedhavi");
        return -1;
    }
    /* request line: METHOD SP target SP HTTP/1.x */
    int64_t i = 0;
    while (i < end && is_tchar(p[i])) i++;
    int64_t mlen = i;
    if (mlen == 0 || p[i] != ' ') goto bad;
    int64_t ts = ++i;
    while (i < end && p[i] > ' ' && p[i] != 0x7f) i++;
    int64_t tlen = i - ts;
    if (tlen == 0 || p[i] != ' ') goto bad;
    i++;
    if (end - i < 8 || memcmp(p + i, "HTTP/1.", 7) != 0 || (p[i + 7] != '1' && p[i + 7] != '0')) goto bad;
    int minor = p[i + 7] - '0';
    i += 8;
    if (p[i] == '\r') i++;
    if (p[i] != '\n') goto bad;
    i++;
    int64_t clen = -1, nheaders = 0;
    int chunked = 0, keep = minor == 1, expect = 0;
    while (i < end) {
        int64_t ls = i;
        while (i < end && p[i] != '\n') i++;
        int64_t le = i;
        i++;
        if (le > ls && p[le - 1] == '\r') le--;
        if (le == ls) break; /* the blank line */
        if (++nheaders > HTTP_MAX_HEADERS) {
            http_reject(h, slot, 431, "headers chaala unnayi");
            return -1;
        }
        int64_t k = ls;
        while (k < le && is_tchar(p[k])) k++;
        if (k == ls || k >= le || p[k] != ':') goto bad;
        const uint8_t *name = p + ls;
        int64_t nlen = k - ls;
        k++;
        while (k < le && (p[k] == ' ' || p[k] == '\t')) k++;
        int64_t ve = le;
        while (ve > k && (p[ve - 1] == ' ' || p[ve - 1] == '\t')) ve--;
        for (int64_t j = k; j < ve; j++)
            if (p[j] == 0 || p[j] == '\r' || p[j] == '\n') goto bad;
        const uint8_t *v = p + k;
        int64_t vlen = ve - k;
        if (ci_eq(name, nlen, "content-length")) {
            int64_t x = 0;
            if (vlen == 0) goto bad;
            for (int64_t j = 0; j < vlen; j++) {
                if (v[j] < '0' || v[j] > '9' || x > (INT64_MAX - 9) / 10) goto bad;
                x = x * 10 + (v[j] - '0');
            }
            if (clen >= 0 && clen != x) goto bad;
            clen = x;
        } else if (ci_eq(name, nlen, "transfer-encoding")) {
            if (!ci_has_token(v, vlen, "chunked")) {
                http_reject(h, slot, 501, "transfer-encoding ardham kaaledhu");
                return -1;
            }
            chunked = 1;
        } else if (ci_eq(name, nlen, "connection")) {
            if (ci_has_token(v, vlen, "close")) keep = 0;
            else if (ci_has_token(v, vlen, "keep-alive")) keep = 1;
        } else if (ci_eq(name, nlen, "expect")) {
            if (ci_eq(v, vlen, "100-continue")) expect = 1;
        }
    }
    if (chunked && clen >= 0) goto bad; /* request smuggling */
    int64_t used = end;
    c->chunked = 0;
    if (chunked) {
        if (expect && n == end) {
            static const char cont[] = "HTTP/1.1 100 Continue\r\n\r\n";
            hb_put(&c->out, cont, (int64_t)sizeof cont - 1);
            http_dirty(h, slot);
        }
        int64_t r = http_chunked(p + end, n - end, &c->body, h->body_limit);
        if (r == -2) {
            http_reject(h, slot, 413, "body chaala pedhadhi");
            return -1;
        }
        if (r < 0) goto bad;
        if (r == 0) return 0;
        used = end + r;
        c->chunked = 1;
    } else if (clen > 0) {
        if (clen > h->body_limit) {
            http_reject(h, slot, 413, "body chaala pedhadhi");
            return -1;
        }
        if (n - end < clen) {
            if (expect && n == end) {
                static const char cont[] = "HTTP/1.1 100 Continue\r\n\r\n";
                hb_put(&c->out, cont, (int64_t)sizeof cont - 1);
                http_dirty(h, slot);
            }
            return 0;
        }
        used = end + clen;
    }
    c->req_len = used;
    c->old_keep = minor == 0 && keep;
    c->close_after = c->close_after || !keep;
    c->state = C_QUEUED;
    c->next_q = -1;
    if (h->q_tail >= 0) h->c[h->q_tail].next_q = slot;
    else h->q_head = slot;
    h->q_tail = slot;
    return 1;
bad:
    http_reject(h, slot, 400, "request ardham kaaledhu");
    return -1;
}

/* Fills `out` (and the header table) from the queued request on `slot`.
   It was checked when parsed, so this walks it without checks. */
static void http_describe(ante_http *h, int32_t slot, ante_http_req *out) {
    hconn *c = &h->c[slot];
    uint8_t *p = c->in.p + c->in.off;
    int64_t i = 0;
    while (p[i] != ' ') i++;
    out->method = p;
    out->method_len = i;
    int64_t ts = ++i;
    int64_t q = -1;
    while (p[i] != ' ') {
        if (p[i] == '?' && q < 0) q = i;
        i++;
    }
    out->path = p + ts;
    out->path_len = (q < 0 ? i : q) - ts;
    out->query = q < 0 ? p + i : p + q + 1;
    out->query_len = q < 0 ? 0 : i - q - 1;
    while (p[i] != '\n') i++;
    i++;
    h->nhdr = 0;
    for (;;) {
        int64_t ls = i;
        while (p[i] != '\n') i++;
        int64_t le = i;
        i++;
        if (le > ls && p[le - 1] == '\r') le--;
        if (le == ls) break;
        int64_t k = ls;
        while (p[k] != ':') {
            if (p[k] >= 'A' && p[k] <= 'Z') p[k] = (uint8_t)(p[k] + 32);
            k++;
        }
        ante_http_hdr *e = &h->hdr[h->nhdr++];
        e->name = p + ls;
        e->name_len = k - ls;
        k++;
        while (k < le && (p[k] == ' ' || p[k] == '\t')) k++;
        while (le > k && (p[le - 1] == ' ' || p[le - 1] == '\t')) le--;
        e->value = p + k;
        e->value_len = le - k;
    }
    out->headers = h->nhdr;
    if (c->chunked) {
        out->body = c->body.p;
        out->body_len = c->body.len;
    } else {
        out->body = p + i;
        out->body_len = c->req_len - i;
    }
    out->keep_alive = !c->close_after;
    out->conn = ((uint64_t)c->gen << 32) | (uint32_t)slot;
    out->id = h->handler_id = ++h->next_id;
}

/* Header `i` of the request `next` last gave. */
void ante_http_header(void *srv, int64_t i, ante_http_hdr *out) {
    ante_http *h = (ante_http *)srv;
    if (i >= 0 && i < h->nhdr) *out = h->hdr[i];
}

static void http_accept(ante_http *h) {
    for (;;) {
        if (h->stopping || h->live >= h->max_conns) return;
        sock_t s = accept(h->ls, NULL, NULL);
        if (s == BAD_SOCK) return; /* EAGAIN, or out of descriptors: try again later */
        set_nonblocking(s, 1);
        int one = 1;
        setsockopt(s, IPPROTO_TCP, TCP_NODELAY, (const char *)&one, sizeof one);
        int32_t slot;
        if (h->nfree > 0) {
            slot = h->free_slots[--h->nfree];
        } else {
            if (h->nc == h->capc) {
                int32_t cap = h->capc ? h->capc * 2 : 256;
                hconn *nc = (hconn *)realloc(h->c, sizeof(hconn) * (size_t)cap);
                int32_t *nf = (int32_t *)realloc(h->free_slots, sizeof(int32_t) * (size_t)cap);
                if (!nc || !nf) out_of_memory();
                memset(nc + h->capc, 0, sizeof(hconn) * (size_t)(cap - h->capc));
                h->c = nc;
                h->free_slots = nf;
                h->capc = cap;
            }
            slot = h->nc++;
        }
        hconn *c = &h->c[slot];
        uint32_t gen = c->gen;
        memset(c, 0, sizeof *c);
        c->gen = gen;
        c->s = s;
        c->state = C_READ;
        c->last = c->started = now_ms();
        h->live++;
#ifdef __linux__
        ep_set(h, EPOLL_CTL_ADD, s, EPOLLIN | EPOLLRDHUP, (uint64_t)slot);
#endif
    }
}

/* Reads what's there; parses a request if the connection is reading. */
static void http_readable(ante_http *h, int32_t slot) {
    hconn *c = &h->c[slot];
    if (c->state == C_FREE || c->state == C_CLOSING) return;
    for (;;) {
        hb_reserve(&c->in, 16384);
        int n = (int)recv(c->s, (char *)c->in.p + c->in.len, (int)(c->in.cap - c->in.len), 0);
        if (n > 0) {
            if (c->in.len == c->in.off) c->started = now_ms();
            c->in.len += n;
            c->last = now_ms();
            if (c->in.len - c->in.off > h->header_limit + h->body_limit + 65536) break;
            continue;
        }
        if (n == 0) {
            /* The peer is done sending; answer what it asked, then close. */
            if (c->state == C_READ && c->in.len == c->in.off && c->out.len == c->out.off) {
                http_drop(h, slot);
                return;
            }
            c->close_after = 1;
            break;
        }
        int e = sock_errno();
        if (e == E_AGAIN || e == E_INTR) break;
        http_drop(h, slot);
        return;
    }
    if (c->state == C_READ) http_parse(h, slot);
}

/* Writes queued output. Returns 0 if the connection was dropped. */
static int http_flush(ante_http *h, int32_t slot) {
    hconn *c = &h->c[slot];
    if (c->state == C_FREE) return 0;
    while (c->out.len > c->out.off) {
#ifdef MSG_NOSIGNAL
        int n = (int)send(c->s, (const char *)c->out.p + c->out.off, (int)(c->out.len - c->out.off), MSG_NOSIGNAL);
#else
        int n = (int)send(c->s, (const char *)c->out.p + c->out.off, (int)(c->out.len - c->out.off), 0);
#endif
        if (n > 0) {
            c->out.off += n;
            continue;
        }
        int e = sock_errno();
        if (e == E_AGAIN || e == E_INTR) {
#ifdef __linux__
            if (!c->want_out) {
                ep_set(h, EPOLL_CTL_MOD, c->s, EPOLLIN | EPOLLOUT | EPOLLRDHUP, (uint64_t)slot);
                c->want_out = 1;
            }
#else
            c->want_out = 1;
#endif
            return 1;
        }
        http_drop(h, slot);
        return 0;
    }
    c->out.off = c->out.len = 0;
#ifdef __linux__
    if (c->want_out) {
        ep_set(h, EPOLL_CTL_MOD, c->s, EPOLLIN | EPOLLRDHUP, (uint64_t)slot);
        c->want_out = 0;
    }
#else
    c->want_out = 0;
#endif
    if (c->close_after && c->state != C_QUEUED && c->state != C_HANDLER &&
        (c->state == C_CLOSING || c->in.len == c->in.off)) {
        http_drop(h, slot);
        return 0;
    }
    return 1;
}

static void http_flush_all(ante_http *h) {
    for (int32_t i = 0; i < h->ndirty; i++) http_flush(h, h->dirty[i]);
    h->ndirty = 0;
}

/* Closes connections idle too long, or too slow sending their headers. */
static void http_sweep(ante_http *h) {
    int64_t now = now_ms();
    if (now - h->last_sweep < 1000) return;
    h->last_sweep = now;
    for (int32_t i = 0; i < h->nc; i++) {
        hconn *c = &h->c[i];
        if (c->state != C_READ) continue;
        if (c->in.len > c->in.off) {
            if (now - c->started > h->header_ms) {
                http_reject(h, i, 408, "request time aipoyindhi");
                http_flush(h, i);
            }
        } else if (c->out.len == c->out.off && now - c->last > h->idle_ms) {
            http_drop(h, i);
        }
    }
    h->ndirty = 0;
    for (int32_t i = 0; i < h->nc; i++)
        if (h->c[i].state != C_FREE && h->c[i].out.len > h->c[i].out.off) http_dirty(h, i);
}

/* Waits for socket events (off the run queue) and handles them. */
static void http_poll(ante_http *h, int ms) {
#ifdef __linux__
    struct epoll_event ev[512];
    net_block_begin();
    int n = epoll_wait(h->ep, ev, 512, ms);
    net_block_end();
    for (int i = 0; i < n; i++) {
        uint64_t d = ev[i].data.u64;
        if (d == UINT64_MAX) {
            http_accept(h);
        } else if (d == UINT64_MAX - 1) {
            char buf[64];
            while (read(h->wake[0], buf, sizeof buf) > 0) {
            }
        } else {
            int32_t slot = (int32_t)d;
            if (ev[i].events & EPOLLOUT) http_flush(h, slot);
            if (ev[i].events & (EPOLLIN | EPOLLRDHUP | EPOLLERR | EPOLLHUP)) http_readable(h, slot);
        }
    }
#else
    int32_t cap = h->nc + 2;
    pollfd_t *fds = (pollfd_t *)malloc(sizeof(pollfd_t) * (size_t)cap);
    int32_t *slots = (int32_t *)malloc(sizeof(int32_t) * (size_t)cap);
    if (!fds || !slots) out_of_memory();
    int32_t k = 0;
    fds[k].fd = h->ls;
    fds[k].events = POLLIN;
    fds[k].revents = 0;
    slots[k++] = -1;
    fds[k].fd = h->wake_r;
    fds[k].events = POLLIN;
    fds[k].revents = 0;
    slots[k++] = -2;
    for (int32_t i = 0; i < h->nc; i++) {
        hconn *c = &h->c[i];
        if (c->state == C_FREE) continue;
        fds[k].fd = c->s;
        fds[k].events = (short)((c->state == C_READ ? POLLIN : 0) | (c->want_out ? POLLOUT : 0));
        fds[k].revents = 0;
        if (!fds[k].events) continue;
        slots[k++] = i;
    }
    net_block_begin();
    int r = poll_fn(fds, (unsigned)k, ms);
    net_block_end();
    if (r > 0) {
        for (int32_t i = 0; i < k; i++) {
            if (!fds[i].revents) continue;
            if (slots[i] == -1) {
                http_accept(h);
            } else if (slots[i] == -2) {
                char buf[64];
                while (recv(h->wake_r, buf, sizeof buf, 0) > 0) {
                }
            } else {
                if (fds[i].revents & POLLOUT) http_flush(h, slots[i]);
                if (fds[i].revents & (POLLIN | POLLHUP | POLLERR)) http_readable(h, slots[i]);
            }
        }
    }
    free(fds);
    free(slots);
#endif
}

/* The next complete request, written to `out`. Returns 1, or 0 when the
   server is stopping and every handed-out request has been answered. */
int64_t ante_http_next(void *srv, ante_http_req *out) {
    ante_http *h = (ante_http *)srv;
    for (;;) {
        if (h->q_head >= 0) {
            int32_t slot = h->q_head;
            h->q_head = h->c[slot].next_q;
            if (h->q_head < 0) h->q_tail = -1;
            hconn *c = &h->c[slot];
            if (c->state != C_QUEUED) continue;
            c->state = C_HANDLER;
            h->handler = slot;
            http_describe(h, slot, out);
            return 1;
        }
        http_flush_all(h);
        if (h->owner->cancel) h->stopping = 1;
        if (h->stopping) {
            if (h->ls != BAD_SOCK) {
#ifdef __linux__
                epoll_ctl(h->ep, EPOLL_CTL_DEL, h->ls, NULL);
#endif
                sock_close(h->ls);
                h->ls = BAD_SOCK;
            }
            return 0;
        }
        http_sweep(h);
        http_poll(h, 200);
    }
}

/* Answers the request `conn` (as `next` gave it). `extra` holds the
   handler's header lines ("Name: value\r\n"). */
void ante_http_respond(void *srv, uint64_t conn, int64_t status, const uint8_t *extra, int64_t extra_len,
                       const uint8_t *body, int64_t body_len, int64_t head_only, int64_t close) {
    ante_http *h = (ante_http *)srv;
    int32_t slot = (int32_t)(conn & 0xffffffffu);
    if (slot < 0 || slot >= h->nc) return;
    hconn *c = &h->c[slot];
    if (c->gen != (uint32_t)(conn >> 32) || c->state != C_HANDLER) return;
    if (h->handler == slot) h->handler = -1;
    if (close) c->close_after = 1;
    http_write_response(h, slot, status, extra, extra_len, body, body_len, (int)head_only, c->close_after);
    c->in.off += c->req_len;
    c->req_len = 0;
    c->body.len = c->body.off = 0;
    c->chunked = 0;
    c->state = C_READ;
    c->started = c->last = now_ms();
    if (c->close_after) {
        c->state = C_CLOSING;
        return;
    }
    if (c->in.len > c->in.off) http_parse(h, slot); /* pipelined */
}

/* After the handler trapped: answers the request it was given with 500,
   and returns its number so the trap can be logged with it. */
int64_t ante_http_recover(void *srv) {
    ante_http *h = (ante_http *)srv;
    if (h->handler < 0) return 0;
    hconn *c = &h->c[h->handler];
    uint64_t conn = ((uint64_t)c->gen << 32) | (uint32_t)h->handler;
    static const char ct[] = "Content-Type: text/plain; charset=utf-8\r\n";
    static const char msg[] = "internal server error";
    ante_http_respond(srv, conn, 500, (const uint8_t *)ct, (int64_t)sizeof ct - 1, (const uint8_t *)msg,
                      (int64_t)sizeof msg - 1, 0, 0);
    return h->handler_id;
}

void ante_http_close(void *srv) {
    ante_http *h = (ante_http *)srv;
    http_flush_all(h);
    for (int32_t i = 0; i < h->nc; i++) http_drop(h, i);
    for (int i = 0; i < 64; i++)
        if (http_servers[i] == h) http_servers[i] = NULL;
    if (h->ls != BAD_SOCK) sock_close(h->ls);
#ifdef __linux__
    close(h->ep);
    close(h->wake[0]);
    close(h->wake[1]);
#else
    sock_close(h->wake_r);
    sock_close(h->wake_w);
#endif
    free(h->c);
    free(h->free_slots);
    free(h->dirty);
    free(h);
}
