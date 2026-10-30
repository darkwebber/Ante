/* ── bhadram: cryptographic primitives (spec §27) ─────────────────────────
   The same algorithms as `src/interp/crypto.rs`, for native code. Nothing
   here branches on, or indexes memory by, secret data. Compiled as part of
   the runtime (appended to ante_rt.c). */

static uint32_t rotr32(uint32_t x, int n) { return (x >> n) | (x << (32 - n)); }
static uint64_t rotr64(uint64_t x, int n) { return (x >> n) | (x << (64 - n)); }

static uint32_t load_be32(const uint8_t *p) {
    return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3];
}
static uint64_t load_be64(const uint8_t *p) { return (uint64_t)load_be32(p) << 32 | load_be32(p + 4); }
static uint32_t load_le32(const uint8_t *p) {
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}
static uint64_t load_le64(const uint8_t *p) { return (uint64_t)load_le32(p) | (uint64_t)load_le32(p + 4) << 32; }
static void store_le32(uint8_t *p, uint32_t x) {
    for (int i = 0; i < 4; i++) p[i] = (uint8_t)(x >> (8 * i));
}
static void store_le64(uint8_t *p, uint64_t x) {
    for (int i = 0; i < 8; i++) p[i] = (uint8_t)(x >> (8 * i));
}

static void vec_bytes(ante_vec *out, const uint8_t *p, int64_t n) {
    ante_vec r = {0, 0, 0};
    ante_buf_push(&r, p, n);
    *out = r;
}

/* ── SHA-2 (FIPS 180-4) ── */

static const uint32_t K256[64] = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2};

static const uint32_t IV256[8] = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                                  0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};

/* The message padded with a 1 bit, zeros and its bit length (`lenb` bytes). */
static uint8_t *md_pad(const uint8_t *p, int64_t n, int64_t block, int64_t lenb, int64_t *out_len) {
    int64_t len = n + 1;
    while (len % block != block - lenb) len++;
    len += lenb;
    uint8_t *m = (uint8_t *)ante_alloc(len);
    memcpy(m, p, (size_t)n);
    memset(m + n, 0, (size_t)(len - n));
    m[n] = 0x80;
    uint64_t bits = (uint64_t)n * 8;
    for (int i = 0; i < 8; i++) m[len - 1 - i] = (uint8_t)(bits >> (8 * i));
    *out_len = len;
    return m;
}

void ante_sha256(const uint8_t *p, int64_t n, ante_vec *out) {
    int64_t len;
    uint8_t *m = md_pad(p, n, 64, 8, &len);
    uint32_t h[8];
    memcpy(h, IV256, sizeof h);
    for (int64_t off = 0; off < len; off += 64) {
        uint32_t w[64];
        for (int i = 0; i < 16; i++) w[i] = load_be32(m + off + 4 * i);
        for (int i = 16; i < 64; i++) {
            uint32_t s0 = rotr32(w[i - 15], 7) ^ rotr32(w[i - 15], 18) ^ (w[i - 15] >> 3);
            uint32_t s1 = rotr32(w[i - 2], 17) ^ rotr32(w[i - 2], 19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16] + s0 + w[i - 7] + s1;
        }
        uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4], f = h[5], g = h[6], hh = h[7];
        for (int i = 0; i < 64; i++) {
            uint32_t t1 = hh + (rotr32(e, 6) ^ rotr32(e, 11) ^ rotr32(e, 25)) + ((e & f) ^ (~e & g)) + K256[i] + w[i];
            uint32_t t2 = (rotr32(a, 2) ^ rotr32(a, 13) ^ rotr32(a, 22)) + ((a & b) ^ (a & c) ^ (b & c));
            hh = g;
            g = f;
            f = e;
            e = d + t1;
            d = c;
            c = b;
            b = a;
            a = t1 + t2;
        }
        h[0] += a, h[1] += b, h[2] += c, h[3] += d, h[4] += e, h[5] += f, h[6] += g, h[7] += hh;
    }
    free(m);
    uint8_t r[32];
    for (int i = 0; i < 8; i++)
        for (int k = 0; k < 4; k++) r[4 * i + k] = (uint8_t)(h[i] >> (24 - 8 * k));
    vec_bytes(out, r, 32);
}

static const uint64_t K512[80] = {
    0x428a2f98d728ae22ULL, 0x7137449123ef65cdULL, 0xb5c0fbcfec4d3b2fULL, 0xe9b5dba58189dbbcULL, 0x3956c25bf348b538ULL,
    0x59f111f1b605d019ULL, 0x923f82a4af194f9bULL, 0xab1c5ed5da6d8118ULL, 0xd807aa98a3030242ULL, 0x12835b0145706fbeULL,
    0x243185be4ee4b28cULL, 0x550c7dc3d5ffb4e2ULL, 0x72be5d74f27b896fULL, 0x80deb1fe3b1696b1ULL, 0x9bdc06a725c71235ULL,
    0xc19bf174cf692694ULL, 0xe49b69c19ef14ad2ULL, 0xefbe4786384f25e3ULL, 0x0fc19dc68b8cd5b5ULL, 0x240ca1cc77ac9c65ULL,
    0x2de92c6f592b0275ULL, 0x4a7484aa6ea6e483ULL, 0x5cb0a9dcbd41fbd4ULL, 0x76f988da831153b5ULL, 0x983e5152ee66dfabULL,
    0xa831c66d2db43210ULL, 0xb00327c898fb213fULL, 0xbf597fc7beef0ee4ULL, 0xc6e00bf33da88fc2ULL, 0xd5a79147930aa725ULL,
    0x06ca6351e003826fULL, 0x142929670a0e6e70ULL, 0x27b70a8546d22ffcULL, 0x2e1b21385c26c926ULL, 0x4d2c6dfc5ac42aedULL,
    0x53380d139d95b3dfULL, 0x650a73548baf63deULL, 0x766a0abb3c77b2a8ULL, 0x81c2c92e47edaee6ULL, 0x92722c851482353bULL,
    0xa2bfe8a14cf10364ULL, 0xa81a664bbc423001ULL, 0xc24b8b70d0f89791ULL, 0xc76c51a30654be30ULL, 0xd192e819d6ef5218ULL,
    0xd69906245565a910ULL, 0xf40e35855771202aULL, 0x106aa07032bbd1b8ULL, 0x19a4c116b8d2d0c8ULL, 0x1e376c085141ab53ULL,
    0x2748774cdf8eeb99ULL, 0x34b0bcb5e19b48a8ULL, 0x391c0cb3c5c95a63ULL, 0x4ed8aa4ae3418acbULL, 0x5b9cca4f7763e373ULL,
    0x682e6ff3d6b2b8a3ULL, 0x748f82ee5defb2fcULL, 0x78a5636f43172f60ULL, 0x84c87814a1f0ab72ULL, 0x8cc702081a6439ecULL,
    0x90befffa23631e28ULL, 0xa4506cebde82bde9ULL, 0xbef9a3f7b2c67915ULL, 0xc67178f2e372532bULL, 0xca273eceea26619cULL,
    0xd186b8c721c0c207ULL, 0xeada7dd6cde0eb1eULL, 0xf57d4f7fee6ed178ULL, 0x06f067aa72176fbaULL, 0x0a637dc5a2c898a6ULL,
    0x113f9804bef90daeULL, 0x1b710b35131c471bULL, 0x28db77f523047d84ULL, 0x32caab7b40c72493ULL, 0x3c9ebe0a15c9bebcULL,
    0x431d67c49c100d4cULL, 0x4cc5d4becb3e42b6ULL, 0x597f299cfc657e2aULL, 0x5fcb6fab3ad6faecULL, 0x6c44198c4a475817ULL};

static const uint64_t IV512[8] = {0x6a09e667f3bcc908ULL, 0xbb67ae8584caa73bULL, 0x3c6ef372fe94f82bULL,
                                  0xa54ff53a5f1d36f1ULL, 0x510e527fade682d1ULL, 0x9b05688c2b3e6c1fULL,
                                  0x1f83d9abfb41bd6bULL, 0x5be0cd19137e2179ULL};

void ante_sha512(const uint8_t *p, int64_t n, ante_vec *out) {
    int64_t len;
    uint8_t *m = md_pad(p, n, 128, 16, &len);
    uint64_t h[8];
    memcpy(h, IV512, sizeof h);
    for (int64_t off = 0; off < len; off += 128) {
        uint64_t w[80];
        for (int i = 0; i < 16; i++) w[i] = load_be64(m + off + 8 * i);
        for (int i = 16; i < 80; i++) {
            uint64_t s0 = rotr64(w[i - 15], 1) ^ rotr64(w[i - 15], 8) ^ (w[i - 15] >> 7);
            uint64_t s1 = rotr64(w[i - 2], 19) ^ rotr64(w[i - 2], 61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16] + s0 + w[i - 7] + s1;
        }
        uint64_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4], f = h[5], g = h[6], hh = h[7];
        for (int i = 0; i < 80; i++) {
            uint64_t t1 = hh + (rotr64(e, 14) ^ rotr64(e, 18) ^ rotr64(e, 41)) + ((e & f) ^ (~e & g)) + K512[i] + w[i];
            uint64_t t2 = (rotr64(a, 28) ^ rotr64(a, 34) ^ rotr64(a, 39)) + ((a & b) ^ (a & c) ^ (b & c));
            hh = g;
            g = f;
            f = e;
            e = d + t1;
            d = c;
            c = b;
            b = a;
            a = t1 + t2;
        }
        h[0] += a, h[1] += b, h[2] += c, h[3] += d, h[4] += e, h[5] += f, h[6] += g, h[7] += hh;
    }
    free(m);
    uint8_t r[64];
    for (int i = 0; i < 8; i++)
        for (int k = 0; k < 8; k++) r[8 * i + k] = (uint8_t)(h[i] >> (56 - 8 * k));
    vec_bytes(out, r, 64);
}

/* ── BLAKE3 ── */

static const int B3_PERM[16] = {2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8};

#define B3G(a, b, c, d, x, y)                                                                                  \
    do {                                                                                                       \
        v[a] = v[a] + v[b] + (x);                                                                              \
        v[d] = rotr32(v[d] ^ v[a], 16);                                                                        \
        v[c] = v[c] + v[d];                                                                                    \
        v[b] = rotr32(v[b] ^ v[c], 12);                                                                        \
        v[a] = v[a] + v[b] + (y);                                                                              \
        v[d] = rotr32(v[d] ^ v[a], 8);                                                                         \
        v[c] = v[c] + v[d];                                                                                    \
        v[b] = rotr32(v[b] ^ v[c], 7);                                                                         \
    } while (0)

static void b3_compress(const uint32_t cv[8], const uint32_t block[16], uint64_t counter, uint32_t len,
                        uint32_t flags, uint32_t out[16]) {
    uint32_t v[16] = {cv[0],    cv[1],    cv[2],    cv[3],    cv[4],    cv[5],
                      cv[6],    cv[7],    IV256[0], IV256[1], IV256[2], IV256[3],
                      (uint32_t)counter, (uint32_t)(counter >> 32), len, flags};
    uint32_t m[16];
    memcpy(m, block, sizeof m);
    for (int round = 0; round < 7; round++) {
        B3G(0, 4, 8, 12, m[0], m[1]);
        B3G(1, 5, 9, 13, m[2], m[3]);
        B3G(2, 6, 10, 14, m[4], m[5]);
        B3G(3, 7, 11, 15, m[6], m[7]);
        B3G(0, 5, 10, 15, m[8], m[9]);
        B3G(1, 6, 11, 12, m[10], m[11]);
        B3G(2, 7, 8, 13, m[12], m[13]);
        B3G(3, 4, 9, 14, m[14], m[15]);
        if (round < 6) {
            uint32_t old[16];
            memcpy(old, m, sizeof old);
            for (int i = 0; i < 16; i++) m[i] = old[B3_PERM[i]];
        }
    }
    for (int i = 0; i < 8; i++) {
        out[i] = v[i] ^ v[i + 8];
        out[i + 8] = v[i + 8] ^ cv[i];
    }
}

static void b3_chunk(const uint8_t *p, int64_t n, uint64_t index, int root, uint32_t cv[8]) {
    memcpy(cv, IV256, 32);
    int64_t blocks = n == 0 ? 1 : (n + 63) / 64;
    for (int64_t i = 0; i < blocks; i++) {
        uint8_t buf[64] = {0};
        int64_t len = n - i * 64 < 64 ? n - i * 64 : 64;
        if (len > 0) memcpy(buf, p + i * 64, (size_t)len);
        uint32_t w[16], out[16];
        for (int k = 0; k < 16; k++) w[k] = load_le32(buf + 4 * k);
        uint32_t flags = (i == 0 ? 1u : 0u) | (i == blocks - 1 ? 2u | (root ? 8u : 0u) : 0u);
        b3_compress(cv, w, index, (uint32_t)(len < 0 ? 0 : len), flags, out);
        memcpy(cv, out, 32);
    }
}

static void b3_tree(const uint8_t *p, int64_t n, uint64_t first, uint64_t count, int root, uint32_t cv[8]) {
    if (count == 1) {
        int64_t start = (int64_t)first * 1024;
        int64_t len = n - start < 1024 ? n - start : 1024;
        b3_chunk(p + start, len, first, root, cv);
        return;
    }
    uint64_t left = 1;
    while (left * 2 < count) left *= 2;
    uint32_t block[16], out[16];
    b3_tree(p, n, first, left, 0, block);
    b3_tree(p, n, first + left, count - left, 0, block + 8);
    b3_compress(IV256, block, 0, 64, 4u | (root ? 8u : 0u), out);
    memcpy(cv, out, 32);
}

void ante_blake3(const uint8_t *p, int64_t n, ante_vec *out) {
    uint64_t chunks = n == 0 ? 1 : (uint64_t)(n + 1023) / 1024;
    uint32_t cv[8];
    uint8_t r[32];
    b3_tree(p, n, 0, chunks, 1, cv);
    for (int i = 0; i < 8; i++) store_le32(r + 4 * i, cv[i]);
    vec_bytes(out, r, 32);
}

/* ── BLAKE2b (RFC 7693) ── */

static const uint8_t B2_SIGMA[12][16] = {
    {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15}, {14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3},
    {11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4}, {7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8},
    {9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13}, {2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9},
    {12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11}, {13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10},
    {6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5}, {10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0},
    {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15}, {14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3}};

#define B2G(a, b, c, d, x, y)                                                                                  \
    do {                                                                                                       \
        v[a] = v[a] + v[b] + (x);                                                                              \
        v[d] = rotr64(v[d] ^ v[a], 32);                                                                        \
        v[c] = v[c] + v[d];                                                                                    \
        v[b] = rotr64(v[b] ^ v[c], 24);                                                                        \
        v[a] = v[a] + v[b] + (y);                                                                              \
        v[d] = rotr64(v[d] ^ v[a], 16);                                                                        \
        v[c] = v[c] + v[d];                                                                                    \
        v[b] = rotr64(v[b] ^ v[c], 63);                                                                        \
    } while (0)

static void blake2b(int out_len, const uint8_t *p, int64_t n, uint8_t *out) {
    uint64_t h[8];
    memcpy(h, IV512, sizeof h);
    h[0] ^= 0x01010000ULL ^ (uint64_t)out_len;
    int64_t blocks = n == 0 ? 1 : (n + 127) / 128;
    for (int64_t i = 0; i < blocks; i++) {
        uint8_t buf[128] = {0};
        int64_t len = n - i * 128 < 128 ? n - i * 128 : 128;
        if (len > 0) memcpy(buf, p + i * 128, (size_t)len);
        uint64_t m[16], v[16];
        for (int k = 0; k < 16; k++) m[k] = load_le64(buf + 8 * k);
        memcpy(v, h, 64);
        memcpy(v + 8, IV512, 64);
        v[12] ^= (uint64_t)(i * 128 + (len < 0 ? 0 : len));
        if (i == blocks - 1) v[14] = ~v[14];
        for (int r = 0; r < 12; r++) {
            const uint8_t *s = B2_SIGMA[r];
            B2G(0, 4, 8, 12, m[s[0]], m[s[1]]);
            B2G(1, 5, 9, 13, m[s[2]], m[s[3]]);
            B2G(2, 6, 10, 14, m[s[4]], m[s[5]]);
            B2G(3, 7, 11, 15, m[s[6]], m[s[7]]);
            B2G(0, 5, 10, 15, m[s[8]], m[s[9]]);
            B2G(1, 6, 11, 12, m[s[10]], m[s[11]]);
            B2G(2, 7, 8, 13, m[s[12]], m[s[13]]);
            B2G(3, 4, 9, 14, m[s[14]], m[s[15]]);
        }
        for (int k = 0; k < 8; k++) h[k] ^= v[k] ^ v[k + 8];
    }
    uint8_t full[64];
    for (int k = 0; k < 8; k++) store_le64(full + 8 * k, h[k]);
    memcpy(out, full, (size_t)out_len);
}

/* ── Argon2id (RFC 9106) ── */

/* H': BLAKE2b stretched to any length. */
static void h_prime(int64_t out_len, const uint8_t *p, int64_t n, uint8_t *out) {
    uint8_t *in = (uint8_t *)ante_alloc(n + 4);
    store_le32(in, (uint32_t)out_len);
    memcpy(in + 4, p, (size_t)n);
    if (out_len <= 64) {
        blake2b((int)out_len, in, n + 4, out);
        free(in);
        return;
    }
    int64_t r = (out_len + 31) / 32 - 2;
    uint8_t v[64];
    blake2b(64, in, n + 4, v);
    free(in);
    memcpy(out, v, 32);
    for (int64_t i = 1; i < r; i++) {
        uint8_t next[64];
        blake2b(64, v, 64, next);
        memcpy(v, next, 64);
        memcpy(out + 32 * i, v, 32);
    }
    blake2b((int)(out_len - 32 * r), v, 64, out + 32 * r);
}

typedef struct {
    uint64_t v[128];
} a2block;

static uint64_t fblamka(uint64_t x, uint64_t y) { return x + y + 2 * ((x & 0xffffffffULL) * (y & 0xffffffffULL)); }

#define A2G(a, b, c, d)                                                                                        \
    do {                                                                                                       \
        q[a] = fblamka(q[a], q[b]);                                                                            \
        q[d] = rotr64(q[d] ^ q[a], 32);                                                                        \
        q[c] = fblamka(q[c], q[d]);                                                                            \
        q[b] = rotr64(q[b] ^ q[c], 24);                                                                        \
        q[a] = fblamka(q[a], q[b]);                                                                            \
        q[d] = rotr64(q[d] ^ q[a], 16);                                                                        \
        q[c] = fblamka(q[c], q[d]);                                                                            \
        q[b] = rotr64(q[b] ^ q[c], 63);                                                                        \
    } while (0)

static void a2perm(uint64_t *q, const int i[16]) {
    A2G(i[0], i[4], i[8], i[12]);
    A2G(i[1], i[5], i[9], i[13]);
    A2G(i[2], i[6], i[10], i[14]);
    A2G(i[3], i[7], i[11], i[15]);
    A2G(i[0], i[5], i[10], i[15]);
    A2G(i[1], i[6], i[11], i[12]);
    A2G(i[2], i[7], i[8], i[13]);
    A2G(i[3], i[4], i[9], i[14]);
}

/* G(X, Y), into `out` (which may be neither input). */
static void a2compress(const a2block *x, const a2block *y, a2block *out) {
    a2block r;
    for (int i = 0; i < 128; i++) r.v[i] = x->v[i] ^ y->v[i];
    *out = r;
    uint64_t *q = out->v;
    for (int row = 0; row < 8; row++) {
        int idx[16];
        for (int k = 0; k < 16; k++) idx[k] = 16 * row + k;
        a2perm(q, idx);
    }
    for (int col = 0; col < 8; col++) {
        int idx[16];
        for (int k = 0; k < 8; k++) {
            idx[2 * k] = 2 * col + 16 * k;
            idx[2 * k + 1] = 2 * col + 16 * k + 1;
        }
        a2perm(q, idx);
    }
    for (int i = 0; i < 128; i++) q[i] ^= r.v[i];
}

void ante_argon2id(const uint8_t *pw, int64_t pwn, const uint8_t *salt, int64_t saltn, const uint8_t *secret,
                   int64_t secretn, const uint8_t *ad, int64_t adn, int64_t m, int64_t t, int64_t p, int64_t taglen,
                   ante_vec *out) {
    /* Parameters out of range give nothing (`bhadram` checks them first). */
    if (p < 1 || t < 1 || m < 8 * p || taglen < 4 || m > 0xffffffffLL || t > 0xffffffffLL || p > 0xffffffffLL ||
        taglen > 0xffffffffLL) {
        ante_vec r = {0, 0, 0};
        *out = r;
        return;
    }
    int64_t inlen = 24 + 16 + pwn + saltn + secretn + adn;
    uint8_t *in = (uint8_t *)ante_alloc(inlen), *w = in;
    uint32_t head[6] = {(uint32_t)p, (uint32_t)taglen, (uint32_t)m, (uint32_t)t, 0x13, 2};
    for (int i = 0; i < 6; i++, w += 4) store_le32(w, head[i]);
    const uint8_t *parts[4] = {pw, salt, secret, ad};
    int64_t lens[4] = {pwn, saltn, secretn, adn};
    for (int i = 0; i < 4; i++) {
        store_le32(w, (uint32_t)lens[i]);
        w += 4;
        if (lens[i] > 0) memcpy(w, parts[i], (size_t)lens[i]);
        w += lens[i];
    }
    uint8_t h0[72];
    blake2b(64, in, inlen, h0);
    free(in);
    int64_t lanes = p, mblocks = 4 * p * (m / (4 * p)), q = mblocks / lanes, seg = q / 4;
    a2block *mem = (a2block *)ante_alloc(mblocks * (int64_t)sizeof(a2block));
    for (int64_t l = 0; l < lanes; l++) {
        for (uint32_t j = 0; j < 2; j++) {
            uint8_t bytes[1024];
            store_le32(h0 + 64, j);
            store_le32(h0 + 68, (uint32_t)l);
            h_prime(1024, h0, 72, bytes);
            for (int k = 0; k < 128; k++) mem[l * q + j].v[k] = load_le64(bytes + 8 * k);
        }
    }
    a2block zero, input, addresses, tmp, next;
    memset(&zero, 0, sizeof zero);
    for (int64_t pass = 0; pass < t; pass++) {
        for (int64_t slice = 0; slice < 4; slice++) {
            for (int64_t l = 0; l < lanes; l++) {
                int independent = pass == 0 && slice < 2;
                memset(&input, 0, sizeof input);
                memset(&addresses, 0, sizeof addresses);
                if (independent) {
                    input.v[0] = (uint64_t)pass;
                    input.v[1] = (uint64_t)l;
                    input.v[2] = (uint64_t)slice;
                    input.v[3] = (uint64_t)mblocks;
                    input.v[4] = (uint64_t)t;
                    input.v[5] = 2;
                }
                int64_t start = pass == 0 && slice == 0 ? 2 : 0;
                if (independent && start == 2) {
                    input.v[6]++;
                    a2compress(&zero, &input, &tmp);
                    a2compress(&zero, &tmp, &addresses);
                }
                for (int64_t idx = start; idx < seg; idx++) {
                    int64_t cur = l * q + slice * seg + idx;
                    int64_t prev = slice * seg + idx == 0 ? l * q + q - 1 : cur - 1;
                    uint64_t rnd;
                    if (independent) {
                        if (idx % 128 == 0) {
                            input.v[6]++;
                            a2compress(&zero, &input, &tmp);
                            a2compress(&zero, &tmp, &addresses);
                        }
                        rnd = addresses.v[idx % 128];
                    } else {
                        rnd = mem[prev].v[0];
                    }
                    uint64_t j1 = rnd & 0xffffffffULL, j2 = rnd >> 32;
                    int64_t ref_lane = pass == 0 && slice == 0 ? l : (int64_t)(j2 % (uint64_t)lanes);
                    int same = ref_lane == l;
                    int64_t area;
                    if (pass == 0)
                        area = slice == 0 || same ? slice * seg + idx - 1 : idx == 0 ? slice * seg - 1 : slice * seg;
                    else
                        area = same ? q - seg + idx - 1 : idx == 0 ? q - seg - 1 : q - seg;
                    uint64_t x = (j1 * j1) >> 32;
                    uint64_t rel = (uint64_t)area - 1 - (((uint64_t)area * x) >> 32);
                    uint64_t start_pos = pass != 0 && slice != 3 ? (uint64_t)((slice + 1) * seg) : 0;
                    int64_t ref = ref_lane * q + (int64_t)((start_pos + rel) % (uint64_t)q);
                    a2compress(&mem[prev], &mem[ref], &next);
                    if (pass > 0)
                        for (int k = 0; k < 128; k++) next.v[k] ^= mem[cur].v[k];
                    mem[cur] = next;
                }
            }
        }
    }
    a2block c = mem[q - 1];
    for (int64_t l = 1; l < lanes; l++)
        for (int k = 0; k < 128; k++) c.v[k] ^= mem[l * q + q - 1].v[k];
    uint8_t bytes[1024];
    for (int k = 0; k < 128; k++) store_le64(bytes + 8 * k, c.v[k]);
    ante_secure_zero(mem, mblocks * (int64_t)sizeof(a2block));
    free(mem);
    uint8_t *tag = (uint8_t *)ante_alloc(taglen);
    h_prime(taglen, bytes, 1024, tag);
    vec_bytes(out, tag, taglen);
    free(tag);
}

/* ── AES-256-GCM (FIPS 197, SP 800-38D) ── */

/* GF(2^8) multiplication without branches or tables. */
static uint8_t gmul(uint8_t a, uint8_t b) {
    uint8_t p = 0;
    for (int i = 0; i < 8; i++) {
        p ^= (uint8_t)(a & (uint8_t)(0 - (b & 1)));
        uint8_t hi = (uint8_t)(0 - (a >> 7));
        a = (uint8_t)((a << 1) ^ (0x1b & hi));
        b >>= 1;
    }
    return p;
}

static uint8_t rotl8(uint8_t x, int n) { return (uint8_t)((x << n) | (x >> (8 - n))); }

/* The S-box computed (inverse, then the affine map): no secret-indexed table. */
static uint8_t sbox(uint8_t x) {
    uint8_t x2 = gmul(x, x), x3 = gmul(x2, x), x6 = gmul(x3, x3), x12 = gmul(x6, x6), x15 = gmul(x12, x3);
    uint8_t x30 = gmul(x15, x15), x60 = gmul(x30, x30), x120 = gmul(x60, x60), x126 = gmul(x120, x6);
    uint8_t x127 = gmul(x126, x), inv = gmul(x127, x127);
    return (uint8_t)(inv ^ rotl8(inv, 1) ^ rotl8(inv, 2) ^ rotl8(inv, 3) ^ rotl8(inv, 4) ^ 0x63);
}

static void aes_expand(const uint8_t key[32], uint8_t rk[15][16]) {
    uint8_t w[60][4];
    memcpy(w, key, 32);
    uint8_t rcon = 1;
    for (int i = 8; i < 60; i++) {
        uint8_t t[4];
        memcpy(t, w[i - 1], 4);
        if (i % 8 == 0) {
            uint8_t t0 = t[0];
            t[0] = (uint8_t)(sbox(t[1]) ^ rcon);
            t[1] = sbox(t[2]);
            t[2] = sbox(t[3]);
            t[3] = sbox(t0);
            rcon = gmul(rcon, 2);
        } else if (i % 8 == 4) {
            for (int k = 0; k < 4; k++) t[k] = sbox(t[k]);
        }
        for (int k = 0; k < 4; k++) w[i][k] = w[i - 8][k] ^ t[k];
    }
    for (int r = 0; r < 15; r++)
        for (int c = 0; c < 4; c++) memcpy(rk[r] + 4 * c, w[4 * r + c], 4);
}

static void aes_encrypt(uint8_t rk[15][16], const uint8_t in[16], uint8_t out[16]) {
    uint8_t s[16], t[16];
    for (int i = 0; i < 16; i++) s[i] = in[i] ^ rk[0][i];
    for (int round = 1; round < 15; round++) {
        for (int i = 0; i < 16; i++) s[i] = sbox(s[i]);
        memcpy(t, s, 16);
        for (int c = 0; c < 4; c++)
            for (int r = 0; r < 4; r++) s[4 * c + r] = t[4 * ((c + r) % 4) + r];
        if (round != 14) {
            for (int c = 0; c < 4; c++) {
                uint8_t a0 = s[4 * c], a1 = s[4 * c + 1], a2 = s[4 * c + 2], a3 = s[4 * c + 3];
                s[4 * c] = (uint8_t)(gmul(a0, 2) ^ gmul(a1, 3) ^ a2 ^ a3);
                s[4 * c + 1] = (uint8_t)(a0 ^ gmul(a1, 2) ^ gmul(a2, 3) ^ a3);
                s[4 * c + 2] = (uint8_t)(a0 ^ a1 ^ gmul(a2, 2) ^ gmul(a3, 3));
                s[4 * c + 3] = (uint8_t)(gmul(a0, 3) ^ a1 ^ a2 ^ gmul(a3, 2));
            }
        }
        for (int i = 0; i < 16; i++) s[i] ^= rk[round][i];
    }
    memcpy(out, s, 16);
}

/* GF(2^128) as GCM defines it (bit 0 is the top bit of byte 0), in
   constant time; `x` and `y` are big-endian pairs of 64-bit halves. */
static void ghash_mul(uint64_t x[2], const uint64_t y[2]) {
    uint64_t z0 = 0, z1 = 0, v0 = y[0], v1 = y[1];
    for (int i = 0; i < 128; i++) {
        uint64_t bit = i < 64 ? (x[0] >> (63 - i)) & 1 : (x[1] >> (127 - i)) & 1;
        uint64_t mask = 0 - bit;
        z0 ^= v0 & mask;
        z1 ^= v1 & mask;
        uint64_t lsb = 0 - (v1 & 1);
        v1 = (v1 >> 1) | (v0 << 63);
        v0 = (v0 >> 1) ^ (0xe100000000000000ULL & lsb);
    }
    x[0] = z0;
    x[1] = z1;
}

static void ghash_part(uint64_t y[2], const uint64_t h[2], const uint8_t *p, int64_t n) {
    for (int64_t off = 0; off < n; off += 16) {
        uint8_t b[16] = {0};
        memcpy(b, p + off, (size_t)(n - off < 16 ? n - off : 16));
        y[0] ^= load_be64(b);
        y[1] ^= load_be64(b + 8);
        ghash_mul(y, h);
    }
}

static void gcm_tag(uint8_t rk[15][16], const uint8_t nonce[12], const uint8_t *aad, int64_t aadn, const uint8_t *c,
                    int64_t cn, uint8_t tag[16]) {
    uint8_t zero[16] = {0}, hb[16], j0[16] = {0}, ej0[16];
    aes_encrypt(rk, zero, hb);
    uint64_t h[2] = {load_be64(hb), load_be64(hb + 8)}, y[2] = {0, 0};
    ghash_part(y, h, aad, aadn);
    ghash_part(y, h, c, cn);
    y[0] ^= (uint64_t)aadn * 8;
    y[1] ^= (uint64_t)cn * 8;
    ghash_mul(y, h);
    memcpy(j0, nonce, 12);
    j0[15] = 1;
    aes_encrypt(rk, j0, ej0);
    for (int i = 0; i < 16; i++) tag[i] = (uint8_t)((i < 8 ? y[0] >> (56 - 8 * i) : y[1] >> (120 - 8 * i)) ^ ej0[i]);
}

static void gcm_ctr(uint8_t rk[15][16], const uint8_t nonce[12], const uint8_t *in, int64_t n, uint8_t *out) {
    for (int64_t off = 0, i = 0; off < n; off += 16, i++) {
        uint8_t ctr[16], ks[16];
        memcpy(ctr, nonce, 12);
        uint32_t c = (uint32_t)i + 2;
        for (int k = 0; k < 4; k++) ctr[12 + k] = (uint8_t)(c >> (24 - 8 * k));
        aes_encrypt(rk, ctr, ks);
        for (int64_t k = 0; k < 16 && off + k < n; k++) out[off + k] = in[off + k] ^ ks[k];
    }
}

/* Constant time: the time depends only on `an`. */
int32_t ante_ct_eq(const uint8_t *a, int64_t an, const uint8_t *b, int64_t bn) {
    volatile uint64_t diff = (uint64_t)(an ^ bn);
    for (int64_t i = 0; i < an; i++) diff |= (uint64_t)(a[i] ^ (bn > 0 ? b[i % bn] : 0));
    return diff == 0;
}

void ante_aes_gcm_seal(const uint8_t *key, const uint8_t *nonce, const uint8_t *aad, int64_t aadn,
                       const uint8_t *p, int64_t n, ante_vec *out) {
    uint8_t rk[15][16];
    aes_expand(key, rk);
    uint8_t *c = (uint8_t *)ante_alloc(n + 16);
    gcm_ctr(rk, nonce, p, n, c);
    gcm_tag(rk, nonce, aad, aadn, c, n, c + n);
    ante_secure_zero(rk, sizeof rk);
    vec_bytes(out, c, n + 16);
    free(c);
}

int32_t ante_aes_gcm_open(const uint8_t *key, const uint8_t *nonce, const uint8_t *aad, int64_t aadn,
                          const uint8_t *s, int64_t n, ante_vec *out) {
    if (n < 16) return 0;
    uint8_t rk[15][16], tag[16];
    aes_expand(key, rk);
    gcm_tag(rk, nonce, aad, aadn, s, n - 16, tag);
    if (!ante_ct_eq(tag, 16, s + n - 16, 16)) {
        ante_secure_zero(rk, sizeof rk);
        return 0;
    }
    uint8_t *p = (uint8_t *)ante_alloc(n - 16);
    gcm_ctr(rk, nonce, s, n - 16, p);
    ante_secure_zero(rk, sizeof rk);
    vec_bytes(out, p, n - 16);
    free(p);
    return 1;
}

/* The intrinsics: a key other than 32 bytes or a nonce other than 12 gives
   nothing (`bhadram` checks them first, as T05). */
void ante_aes_gcm_seal_n(const uint8_t *key, int64_t keyn, const uint8_t *nonce, int64_t noncen, const uint8_t *aad,
                         int64_t aadn, const uint8_t *p, int64_t n, ante_vec *out) {
    if (keyn != 32 || noncen != 12) {
        ante_vec r = {0, 0, 0};
        *out = r;
        return;
    }
    ante_aes_gcm_seal(key, nonce, aad, aadn, p, n, out);
}

int32_t ante_aes_gcm_open_n(const uint8_t *key, int64_t keyn, const uint8_t *nonce, int64_t noncen,
                            const uint8_t *aad, int64_t aadn, const uint8_t *s, int64_t n, ante_vec *out) {
    if (keyn != 32 || noncen != 12) return 0;
    return ante_aes_gcm_open(key, nonce, aad, aadn, s, n, out);
}

/* ── randomness from the operating system ── */

#if defined(_WIN32)
static int os_random(uint8_t *p, int64_t n) {
    for (int64_t i = 0; i < n; i += 4) {
        unsigned int r;
        if (rand_s(&r) != 0) return 0;
        for (int k = 0; k < 4 && i + k < n; k++) p[i + k] = (uint8_t)(r >> (8 * k));
    }
    return 1;
}
#elif defined(__APPLE__) || defined(__FreeBSD__) || defined(__OpenBSD__) || defined(__NetBSD__)
static int os_random(uint8_t *p, int64_t n) {
    arc4random_buf(p, (size_t)n);
    return 1;
}
#else
static int os_random(uint8_t *p, int64_t n) {
    FILE *f = fopen("/dev/urandom", "rb");
    if (!f) return 0;
    size_t got = fread(p, 1, (size_t)n, f);
    fclose(f);
    return got == (size_t)n;
}
#endif

/* `n ni bhadram.random_baitlu()`; 0 if the system generator failed. */
int32_t ante_random_bytes(int64_t n, ante_vec *out) {
    ante_vec r = {0, 0, 0};
    if (n > 0) {
        buf_grow(&r, n);
        if (!os_random(r.ptr, n)) {
            free(r.ptr);
            return 0;
        }
        r.len = n;
    }
    *out = r;
    return 1;
}
