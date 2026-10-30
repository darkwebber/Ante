//! The cryptographic primitives behind `bhadram` (spec §27), for the
//! interpreter. `runtime/ante_crypto.c` has the same algorithms for native
//! code; the two are checked against each other and against published test
//! vectors. Nothing here branches on, or indexes memory by, secret data.

// ── SHA-2 (FIPS 180-4) ──────────────────────────────────────────────────────

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const IV256: [u32; 8] =
    [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];

/// Pads a message for SHA-2: a 1 bit, zeros, and the bit length in `len_bytes`.
fn md_pad(data: &[u8], block: usize, len_bytes: usize) -> Vec<u8> {
    let mut m = data.to_vec();
    m.push(0x80);
    while m.len() % block != block - len_bytes {
        m.push(0);
    }
    let bits = (data.len() as u128) * 8;
    m.extend_from_slice(&bits.to_be_bytes()[16 - len_bytes..]);
    m
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    let mut h = IV256;
    for block in md_pad(data, 64, 8).chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K256[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().flat_map(|x| x.to_be_bytes()).collect()
}

const K512: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

pub fn sha512(data: &[u8]) -> Vec<u8> {
    let mut h: [u64; 8] = [
        0x6a09e667f3bcc908,
        0xbb67ae8584caa73b,
        0x3c6ef372fe94f82b,
        0xa54ff53a5f1d36f1,
        0x510e527fade682d1,
        0x9b05688c2b3e6c1f,
        0x1f83d9abfb41bd6b,
        0x5be0cd19137e2179,
    ];
    for block in md_pad(data, 128, 16).chunks(128) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&block[8 * i..8 * i + 8]);
            w[i] = u64::from_be_bytes(b);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K512[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().flat_map(|x| x.to_be_bytes()).collect()
}

// ── BLAKE3 ──────────────────────────────────────────────────────────────────

const B3_PERM: [usize; 16] = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];
const CHUNK_START: u32 = 1;
const CHUNK_END: u32 = 2;
const PARENT: u32 = 4;
const ROOT: u32 = 8;

fn b3_g(v: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(12);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(8);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(7);
}

fn b3_compress(cv: &[u32; 8], block: &[u32; 16], counter: u64, len: u32, flags: u32) -> [u32; 16] {
    let mut v = [
        cv[0],
        cv[1],
        cv[2],
        cv[3],
        cv[4],
        cv[5],
        cv[6],
        cv[7],
        IV256[0],
        IV256[1],
        IV256[2],
        IV256[3],
        counter as u32,
        (counter >> 32) as u32,
        len,
        flags,
    ];
    let mut m = *block;
    for round in 0..7 {
        b3_g(&mut v, 0, 4, 8, 12, m[0], m[1]);
        b3_g(&mut v, 1, 5, 9, 13, m[2], m[3]);
        b3_g(&mut v, 2, 6, 10, 14, m[4], m[5]);
        b3_g(&mut v, 3, 7, 11, 15, m[6], m[7]);
        b3_g(&mut v, 0, 5, 10, 15, m[8], m[9]);
        b3_g(&mut v, 1, 6, 11, 12, m[10], m[11]);
        b3_g(&mut v, 2, 7, 8, 13, m[12], m[13]);
        b3_g(&mut v, 3, 4, 9, 14, m[14], m[15]);
        if round < 6 {
            let old = m;
            for i in 0..16 {
                m[i] = old[B3_PERM[i]];
            }
        }
    }
    for i in 0..8 {
        v[i] ^= v[i + 8];
        v[i + 8] ^= cv[i];
    }
    v
}

fn b3_words(bytes: &[u8]) -> [u32; 16] {
    let mut block = [0u8; 64];
    block[..bytes.len()].copy_from_slice(bytes);
    let mut w = [0u32; 16];
    for i in 0..16 {
        w[i] = u32::from_le_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
    }
    w
}

/// A chunk's chaining value (with `root`, the root output's first words).
fn b3_chunk(chunk: &[u8], index: u64, root: bool) -> [u32; 8] {
    let mut cv = IV256;
    let blocks: Vec<&[u8]> = if chunk.is_empty() { vec![&[][..]] } else { chunk.chunks(64).collect() };
    let last = blocks.len() - 1;
    for (i, b) in blocks.iter().enumerate() {
        let mut flags = 0;
        if i == 0 {
            flags |= CHUNK_START;
        }
        if i == last {
            flags |= CHUNK_END;
            if root {
                flags |= ROOT;
            }
        }
        let out = b3_compress(&cv, &b3_words(b), index, b.len() as u32, flags);
        cv.copy_from_slice(&out[..8]);
    }
    cv
}

/// The chaining value of chunks `first..first + n`: the left subtree holds the
/// largest power of two number of chunks that leaves one for the right.
fn b3_tree(data: &[u8], first: u64, n: u64, root: bool) -> [u32; 8] {
    if n == 1 {
        let start = (first * 1024) as usize;
        let end = (start + 1024).min(data.len());
        return b3_chunk(&data[start..end], first, root);
    }
    let mut left = 1;
    while left * 2 < n {
        left *= 2;
    }
    let l = b3_tree(data, first, left, false);
    let r = b3_tree(data, first + left, n - left, false);
    let mut block = [0u32; 16];
    block[..8].copy_from_slice(&l);
    block[8..].copy_from_slice(&r);
    let out = b3_compress(&IV256, &block, 0, 64, PARENT | if root { ROOT } else { 0 });
    let mut cv = [0u32; 8];
    cv.copy_from_slice(&out[..8]);
    cv
}

/// BLAKE3 with its default 32-byte output.
pub fn blake3(data: &[u8]) -> Vec<u8> {
    let chunks = (data.len() as u64).div_ceil(1024).max(1);
    b3_tree(data, 0, chunks, true).iter().flat_map(|x| x.to_le_bytes()).collect()
}

// ── BLAKE2b (RFC 7693), for Argon2 ──────────────────────────────────────────

const B2_IV: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

const B2_SIGMA: [[usize; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

fn b2_g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

/// Unkeyed BLAKE2b with an `out_len`-byte digest (1 to 64).
pub fn blake2b(out_len: usize, data: &[u8]) -> Vec<u8> {
    let mut h = B2_IV;
    h[0] ^= 0x01010000 ^ out_len as u64;
    let blocks = data.len().div_ceil(128).max(1);
    for i in 0..blocks {
        let last = i == blocks - 1;
        let part = &data[i * 128..((i + 1) * 128).min(data.len())];
        let mut block = [0u8; 128];
        block[..part.len()].copy_from_slice(part);
        let mut m = [0u64; 16];
        for (j, w) in m.iter_mut().enumerate() {
            let mut b = [0u8; 8];
            b.copy_from_slice(&block[8 * j..8 * j + 8]);
            *w = u64::from_le_bytes(b);
        }
        let t = (i * 128 + part.len()) as u128;
        let mut v = [0u64; 16];
        v[..8].copy_from_slice(&h);
        v[8..].copy_from_slice(&B2_IV);
        v[12] ^= t as u64;
        v[13] ^= (t >> 64) as u64;
        if last {
            v[14] = !v[14];
        }
        for s in &B2_SIGMA {
            b2_g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
            b2_g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
            b2_g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
            b2_g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
            b2_g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
            b2_g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
            b2_g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
            b2_g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
        }
        for j in 0..8 {
            h[j] ^= v[j] ^ v[j + 8];
        }
    }
    h.iter().flat_map(|x| x.to_le_bytes()).take(out_len).collect()
}

// ── Argon2id (RFC 9106) ─────────────────────────────────────────────────────

/// H' (RFC 9106 §3.3): a hash of any length built from BLAKE2b.
fn h_prime(out_len: usize, data: &[u8]) -> Vec<u8> {
    let mut input = (out_len as u32).to_le_bytes().to_vec();
    input.extend_from_slice(data);
    if out_len <= 64 {
        return blake2b(out_len, &input);
    }
    let r = out_len.div_ceil(32) - 2;
    let mut v = blake2b(64, &input);
    let mut out = v[..32].to_vec();
    for _ in 1..r {
        v = blake2b(64, &v);
        out.extend_from_slice(&v[..32]);
    }
    out.extend_from_slice(&blake2b(out_len - 32 * r, &v));
    out
}

type Block = [u64; 128];

fn fblamka(x: u64, y: u64) -> u64 {
    let m = (x & 0xffff_ffff).wrapping_mul(y & 0xffff_ffff);
    x.wrapping_add(y).wrapping_add(m.wrapping_mul(2))
}

fn gb(v: &mut Block, a: usize, b: usize, c: usize, d: usize) {
    v[a] = fblamka(v[a], v[b]);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = fblamka(v[c], v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = fblamka(v[a], v[b]);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = fblamka(v[c], v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

/// The permutation P on 16 words, given their indexes in the block.
fn perm(v: &mut Block, i: [usize; 16]) {
    gb(v, i[0], i[4], i[8], i[12]);
    gb(v, i[1], i[5], i[9], i[13]);
    gb(v, i[2], i[6], i[10], i[14]);
    gb(v, i[3], i[7], i[11], i[15]);
    gb(v, i[0], i[5], i[10], i[15]);
    gb(v, i[1], i[6], i[11], i[12]);
    gb(v, i[2], i[7], i[8], i[13]);
    gb(v, i[3], i[4], i[9], i[14]);
}

/// The compression function G(X, Y).
fn compress(x: &Block, y: &Block) -> Block {
    let mut r = [0u64; 128];
    for i in 0..128 {
        r[i] = x[i] ^ y[i];
    }
    let mut q = r;
    for row in 0..8 {
        let mut idx = [0usize; 16];
        for (k, slot) in idx.iter_mut().enumerate() {
            *slot = 16 * row + k;
        }
        perm(&mut q, idx);
    }
    for col in 0..8 {
        let mut idx = [0usize; 16];
        for k in 0..8 {
            idx[2 * k] = 2 * col + 16 * k;
            idx[2 * k + 1] = 2 * col + 16 * k + 1;
        }
        perm(&mut q, idx);
    }
    for i in 0..128 {
        q[i] ^= r[i];
    }
    q
}

fn block_from(bytes: &[u8]) -> Block {
    let mut b = [0u64; 128];
    for (i, w) in b.iter_mut().enumerate() {
        let mut x = [0u8; 8];
        x.copy_from_slice(&bytes[8 * i..8 * i + 8]);
        *w = u64::from_le_bytes(x);
    }
    b
}

/// Argon2id, version 0x13: `m` KiB of memory, `t` passes, `p` lanes.
#[allow(clippy::too_many_arguments)]
pub fn argon2id(
    password: &[u8],
    salt: &[u8],
    secret: &[u8],
    ad: &[u8],
    m: u32,
    t: u32,
    p: u32,
    tag_len: u32,
) -> Vec<u8> {
    let mut h0_in = Vec::new();
    for x in [p, tag_len, m, t, 0x13, 2] {
        h0_in.extend_from_slice(&x.to_le_bytes());
    }
    for part in [password, salt, secret, ad] {
        h0_in.extend_from_slice(&(part.len() as u32).to_le_bytes());
        h0_in.extend_from_slice(part);
    }
    let h0 = blake2b(64, &h0_in);
    let lanes = p as usize;
    let m_blocks = (4 * p * (m / (4 * p))) as usize;
    let q = m_blocks / lanes;
    let seg = q / 4;
    let mut mem: Vec<Block> = vec![[0u64; 128]; m_blocks];
    for l in 0..lanes {
        for j in 0..2u32 {
            let mut input = h0.clone();
            input.extend_from_slice(&j.to_le_bytes());
            input.extend_from_slice(&(l as u32).to_le_bytes());
            mem[l * q + j as usize] = block_from(&h_prime(1024, &input));
        }
    }
    let zero = [0u64; 128];
    for pass in 0..t as usize {
        for slice in 0..4 {
            for l in 0..lanes {
                let independent = pass == 0 && slice < 2;
                let mut addresses = [0u64; 128];
                let mut input = [0u64; 128];
                if independent {
                    input[0] = pass as u64;
                    input[1] = l as u64;
                    input[2] = slice as u64;
                    input[3] = m_blocks as u64;
                    input[4] = t as u64;
                    input[5] = 2;
                }
                let start = if pass == 0 && slice == 0 { 2 } else { 0 };
                if independent && start == 2 {
                    input[6] += 1;
                    addresses = compress(&zero, &compress(&zero, &input));
                }
                for idx in start..seg {
                    let cur = l * q + slice * seg + idx;
                    let prev = if slice * seg + idx == 0 { l * q + q - 1 } else { cur - 1 };
                    let rand = if independent {
                        if idx % 128 == 0 {
                            input[6] += 1;
                            addresses = compress(&zero, &compress(&zero, &input));
                        }
                        addresses[idx % 128]
                    } else {
                        mem[prev][0]
                    };
                    let j1 = rand & 0xffff_ffff;
                    let j2 = rand >> 32;
                    let ref_lane = if pass == 0 && slice == 0 { l } else { (j2 % lanes as u64) as usize };
                    let same = ref_lane == l;
                    let area = if pass == 0 {
                        if slice == 0 || same {
                            slice * seg + idx - 1
                        } else if idx == 0 {
                            slice * seg - 1
                        } else {
                            slice * seg
                        }
                    } else if same {
                        q - seg + idx - 1
                    } else if idx == 0 {
                        q - seg - 1
                    } else {
                        q - seg
                    } as u64;
                    let x = (j1 * j1) >> 32;
                    let rel = area - 1 - ((area * x) >> 32);
                    let start_pos = if pass != 0 && slice != 3 { ((slice + 1) * seg) as u64 } else { 0 };
                    let ref_index = ref_lane * q + ((start_pos + rel) % q as u64) as usize;
                    let mut next = compress(&mem[prev], &mem[ref_index]);
                    if pass > 0 {
                        for (n, o) in next.iter_mut().zip(mem[cur].iter()) {
                            *n ^= o;
                        }
                    }
                    mem[cur] = next;
                }
            }
        }
    }
    let mut c = mem[q - 1];
    for l in 1..lanes {
        for (x, y) in c.iter_mut().zip(mem[l * q + q - 1].iter()) {
            *x ^= y;
        }
    }
    let bytes: Vec<u8> = c.iter().flat_map(|x| x.to_le_bytes()).collect();
    h_prime(tag_len as usize, &bytes)
}

// ── AES-256-GCM (FIPS 197, SP 800-38D) ──────────────────────────────────────

/// Multiplication in GF(2^8), without branches or tables.
fn gmul(a: u8, b: u8) -> u8 {
    let (mut a, mut b, mut p) = (a, b, 0u8);
    for _ in 0..8 {
        p ^= a & 0u8.wrapping_sub(b & 1);
        let hi = 0u8.wrapping_sub(a >> 7);
        a = (a << 1) ^ (0x1b & hi);
        b >>= 1;
    }
    p
}

/// The AES S-box, computed (inverse, then the affine map) so that no table
/// is indexed by secret bytes.
fn sbox(x: u8) -> u8 {
    // x^254 = x^-1 (and 0 for 0).
    let x2 = gmul(x, x);
    let x3 = gmul(x2, x);
    let x6 = gmul(x3, x3);
    let x12 = gmul(x6, x6);
    let x15 = gmul(x12, x3);
    let x30 = gmul(x15, x15);
    let x60 = gmul(x30, x30);
    let x120 = gmul(x60, x60);
    let x126 = gmul(x120, x6);
    let x127 = gmul(x126, x);
    let inv = gmul(x127, x127);
    inv ^ inv.rotate_left(1) ^ inv.rotate_left(2) ^ inv.rotate_left(3) ^ inv.rotate_left(4) ^ 0x63
}

fn expand_key(key: &[u8]) -> [[u8; 16]; 15] {
    let mut w = [[0u8; 4]; 60];
    for i in 0..8 {
        w[i].copy_from_slice(&key[4 * i..4 * i + 4]);
    }
    let mut rcon = 1u8;
    for i in 8..60 {
        let mut t = w[i - 1];
        if i % 8 == 0 {
            t = [sbox(t[1]) ^ rcon, sbox(t[2]), sbox(t[3]), sbox(t[0])];
            rcon = gmul(rcon, 2);
        } else if i % 8 == 4 {
            t = [sbox(t[0]), sbox(t[1]), sbox(t[2]), sbox(t[3])];
        }
        for k in 0..4 {
            w[i][k] = w[i - 8][k] ^ t[k];
        }
    }
    let mut rk = [[0u8; 16]; 15];
    for (r, k) in rk.iter_mut().enumerate() {
        for c in 0..4 {
            k[4 * c..4 * c + 4].copy_from_slice(&w[4 * r + c]);
        }
    }
    rk
}

fn aes_encrypt(rk: &[[u8; 16]; 15], block: &[u8; 16]) -> [u8; 16] {
    let mut s = *block;
    for i in 0..16 {
        s[i] ^= rk[0][i];
    }
    for (round, k) in rk.iter().enumerate().skip(1) {
        for b in s.iter_mut() {
            *b = sbox(*b);
        }
        // ShiftRows: row r of the column-major state moves left by r.
        let t = s;
        for c in 0..4 {
            for r in 0..4 {
                s[4 * c + r] = t[4 * ((c + r) % 4) + r];
            }
        }
        if round != 14 {
            for c in 0..4 {
                let a = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                s[4 * c] = gmul(a[0], 2) ^ gmul(a[1], 3) ^ a[2] ^ a[3];
                s[4 * c + 1] = a[0] ^ gmul(a[1], 2) ^ gmul(a[2], 3) ^ a[3];
                s[4 * c + 2] = a[0] ^ a[1] ^ gmul(a[2], 2) ^ gmul(a[3], 3);
                s[4 * c + 3] = gmul(a[0], 3) ^ a[1] ^ a[2] ^ gmul(a[3], 2);
            }
        }
        for i in 0..16 {
            s[i] ^= k[i];
        }
    }
    s
}

/// Multiplication in GF(2^128) as GCM defines it, in constant time.
fn ghash_mul(x: u128, y: u128) -> u128 {
    let mut z = 0u128;
    let mut v = y;
    for i in 0..128 {
        let bit = (x >> (127 - i)) & 1;
        z ^= v & 0u128.wrapping_sub(bit);
        let lsb = v & 1;
        v = (v >> 1) ^ (0xe1u128 << 120 & 0u128.wrapping_sub(lsb));
    }
    z
}

fn ghash(h: u128, aad: &[u8], c: &[u8]) -> u128 {
    let mut y = 0u128;
    for part in [aad, c] {
        for chunk in part.chunks(16) {
            let mut b = [0u8; 16];
            b[..chunk.len()].copy_from_slice(chunk);
            y = ghash_mul(y ^ u128::from_be_bytes(b), h);
        }
    }
    let lens = ((aad.len() as u128 * 8) << 64) | (c.len() as u128 * 8);
    ghash_mul(y ^ lens, h)
}

fn gcm_ctr(rk: &[[u8; 16]; 15], nonce: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for (i, chunk) in data.chunks(16).enumerate() {
        let mut ctr = [0u8; 16];
        ctr[..12].copy_from_slice(nonce);
        ctr[12..].copy_from_slice(&(i as u32 + 2).to_be_bytes());
        let ks = aes_encrypt(rk, &ctr);
        out.extend(chunk.iter().zip(ks.iter()).map(|(a, b)| a ^ b));
    }
    out
}

fn gcm_tag(rk: &[[u8; 16]; 15], nonce: &[u8], aad: &[u8], c: &[u8]) -> [u8; 16] {
    let h = u128::from_be_bytes(aes_encrypt(rk, &[0u8; 16]));
    let mut j0 = [0u8; 16];
    j0[..12].copy_from_slice(nonce);
    j0[15] = 1;
    let s = ghash(h, aad, c) ^ u128::from_be_bytes(aes_encrypt(rk, &j0));
    s.to_be_bytes()
}

/// AES-256-GCM with a 12-byte nonce: the ciphertext followed by the tag.
pub fn aes256gcm_seal(key: &[u8], nonce: &[u8], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    let rk = expand_key(key);
    let mut c = gcm_ctr(&rk, nonce, plain);
    let tag = gcm_tag(&rk, nonce, aad, &c);
    c.extend_from_slice(&tag);
    c
}

/// Checks the tag (in constant time) and decrypts; `None` if it is wrong.
pub fn aes256gcm_open(key: &[u8], nonce: &[u8], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 16 {
        return None;
    }
    let (c, tag) = sealed.split_at(sealed.len() - 16);
    let rk = expand_key(key);
    let want = gcm_tag(&rk, nonce, aad, c);
    if !ct_eq(&want, tag) {
        return None;
    }
    Some(gcm_ctr(&rk, nonce, c))
}

// ── comparison and randomness ───────────────────────────────────────────────

/// Equal bytes? The time depends only on the length of `a`.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u64;
    for (i, x) in a.iter().enumerate() {
        let y = b.get(i % b.len().max(1)).copied().unwrap_or(0);
        diff |= u64::from(x ^ y);
    }
    std::hint::black_box(diff) == 0
}

/// `n` bytes from the operating system's secure generator.
pub fn random_bytes(n: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; n];
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut f = std::fs::File::open("/dev/urandom").map_err(|e| e.to_string())?;
        f.read_exact(&mut out).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        extern "system" {
            fn BCryptGenRandom(alg: *mut u8, buf: *mut u8, len: u32, flags: u32) -> i32;
        }
        // BCRYPT_USE_SYSTEM_PREFERRED_RNG
        let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), out.as_mut_ptr(), n as u32, 2) };
        if status != 0 {
            return Err(format!("BCryptGenRandom failed ({status})"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex")).collect()
    }

    #[test]
    fn sha2_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            hex(&sha512(b"abc")),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn blake_vectors() {
        assert_eq!(hex(&blake3(b"")), "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262");
        // The official test vectors' input: bytes 0, 1, …, 250, 0, 1, ….
        let input = |n: usize| (0..n).map(|i| (i % 251) as u8).collect::<Vec<u8>>();
        assert_eq!(
            hex(&blake3(&input(1025))),
            "d00278ae47eb27b34faecf67b4fe263f82d5412916c1ffd97c8cb7fb814b8444"
        );
        assert_eq!(
            hex(&blake3(&input(2049))),
            "5f4d72f40d7a5f82b15ca2b2e44b1de3c2ef86c426c95c1af0b6879522563030"
        );
        assert_eq!(
            hex(&blake2b(64, b"abc")),
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        );
    }

    #[test]
    fn argon2id_rfc9106() {
        let tag = argon2id(&[1; 32], &[2; 16], &[3; 8], &[4; 12], 32, 3, 4, 32);
        assert_eq!(hex(&tag), "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659");
    }

    #[test]
    fn aes_gcm_vectors() {
        // GCM spec test cases 13 and 14 (AES-256, zero key and nonce).
        let key = [0u8; 32];
        let nonce = [0u8; 12];
        assert_eq!(hex(&aes256gcm_seal(&key, &nonce, &[], &[])), "530f8afbc74536b9a963b4f1c4cb738b");
        assert_eq!(
            hex(&aes256gcm_seal(&key, &nonce, &[], &[0u8; 16])),
            "cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919"
        );
        let sealed = aes256gcm_seal(
            &unhex("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308"),
            &unhex("cafebabefacedbaddecaf888"),
            &[],
            b"ante",
        );
        let opened = aes256gcm_open(
            &unhex("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308"),
            &unhex("cafebabefacedbaddecaf888"),
            &[],
            &sealed,
        );
        assert_eq!(opened.as_deref(), Some(&b"ante"[..]));
    }
}
