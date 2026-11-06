//! Hashing for the `تشفير` native module (catalog task 17,
//! from `crypto/`): SHA-256 in pure Rust (no crate — the
//! build plan §17 decides so, and the engine takes no new
//! runtime dependency without a meeting) verified against
//! the NIST test vectors, plus system randomness from
//! /dev/urandom on Unix. One implementation, shared by
//! both engines (interpreter + VM) through the stdlib
//! native functions — never implemented twice.
//!
//! Permission design (written before the code — see
//! `CATALOG-09-CRYPTO.md` §1): hashing only in v1 — no
//! encryption, no signing (task 18); no key or secret ever
//! passes through the module; `جزّئ` takes text and
//! returns its hex fingerprint; `عشوائي_آمن` takes a count
//! and returns that many bytes from the OS. No seed, no
//! shared state: hashing is pure and deterministic, and
//! randomness is read fresh from the system on every call.
//! On Windows and in the browser `عشوائي_آمن` refuses
//! honestly (no secure randomness in std there); SHA-256
//! works everywhere, including the browser.

use crate::error::{RioError, RioResult};

/// At most 1 MiB of secure random bytes per call: a loop
/// that asks for bytes without using them cannot exhaust
/// the process's memory (the same reason ذاكرة caps a
/// block at 64 MiB).
pub const MAX_SECURE_RANDOM_BYTES: usize = 1024 * 1024;

/// The WASM refusal: the flat builtins' own message.
#[cfg(target_arch = "wasm32")]
const WASM_REFUSAL: &str = "هذه الدالة غير مدعومة في متصفح الويب";

/// SHA-256 round constants (FIPS 180-4 §4.2.2).
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 initial hash values (FIPS 180-4 §5.3.3).
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// SHA-256 of `input`, as 32 bytes (FIPS 180-4 §6.2).
/// Pure Rust, no I/O, no state — the same digest on every
/// platform, including the browser.
pub fn sha256(input: &[u8]) -> [u8; 32] {
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(input.len() + 72);
    msg.extend_from_slice(input);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    let mut h = H0;
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[4 * i..4 * i + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

/// SHA-256 of `input`, as 64 lowercase hex characters.
pub fn sha256_hex(input: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = sha256(input);
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// `count` bytes of system randomness. Unix reads
/// /dev/urandom with std only (read_exact fills fully —
/// a short read is completed, never returned short).
/// A read failure is an Arabic error with no English OS
/// text at all. The cap is enforced here, in the one
/// place every caller goes through.
#[cfg(unix)]
pub fn secure_random(count: usize, line: usize, col: usize) -> RioResult<Vec<u8>> {
    use std::io::Read;
    if count > MAX_SECURE_RANDOM_BYTES {
        return Err(RioError::runtime(
            "تشفير.عشوائي_آمن: العدد تجاوز الحد (١ ميبيبايت)",
            line,
            col,
        ));
    }
    let mut buf = vec![0u8; count];
    let mut f = std::fs::File::open("/dev/urandom")
        .map_err(|_| RioError::runtime("تعذّر قراءة العشوائية من النظام", line, col))?;
    f.read_exact(&mut buf)
        .map_err(|_| RioError::runtime("تعذّر قراءة العشوائية من النظام", line, col))?;
    Ok(buf)
}

/// The browser has no secure randomness in std: refuse
/// honestly with the flat builtins' own message (no
/// getRandomValues in v1 — a WASM-build feature change,
/// a later step if the owner wants it).
#[cfg(target_arch = "wasm32")]
pub fn secure_random(count: usize, line: usize, col: usize) -> RioResult<Vec<u8>> {
    if count > MAX_SECURE_RANDOM_BYTES {
        return Err(RioError::runtime(
            "تشفير.عشوائي_آمن: العدد تجاوز الحد (١ ميبيبايت)",
            line,
            col,
        ));
    }
    Err(RioError::runtime(WASM_REFUSAL, line, col))
}

/// Windows has no secure randomness in std (it needs
/// BCryptGenRandom, outside std): refuse honestly with
/// the platform message (the ذاكرة.المتاح pattern).
#[cfg(all(not(unix), not(target_arch = "wasm32")))]
pub fn secure_random(count: usize, line: usize, col: usize) -> RioResult<Vec<u8>> {
    if count > MAX_SECURE_RANDOM_BYTES {
        return Err(RioError::runtime(
            "تشفير.عشوائي_آمن: العدد تجاوز الحد (١ ميبيبايت)",
            line,
            col,
        ));
    }
    Err(RioError::runtime(
        "تشفير.عشوائي_آمن غير مدعوم على هذه المنصة",
        line,
        col,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // NIST test vectors (FIPS 180-4): the implementation
    // is checked against known answers, never against
    // itself — a bug in the code cannot also hide in the
    // expected value.

    #[test]
    fn sha256_empty() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_56_byte_boundary() {
        // 56 bytes: the padding block boundary — the
        // message plus 0x80 plus the length spills into
        // a second block.
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_million_a() {
        let input = vec![b'a'; 1_000_000];
        assert_eq!(
            sha256_hex(&input),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn sha256_quick_brown_fox() {
        assert_eq!(
            sha256_hex(b"The quick brown fox jumps over the lazy dog"),
            "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
        );
    }

    #[test]
    fn sha256_deterministic_and_sensitive() {
        // Same input, same digest; one character changes
        // everything; the output is always 64 lowercase
        // hex characters.
        assert_eq!(sha256_hex("محمود".as_bytes()), sha256_hex("محمود".as_bytes()));
        assert_ne!(sha256_hex("محمود".as_bytes()), sha256_hex("محمود!".as_bytes()));
        assert_eq!(
            sha256_hex("محمود".as_bytes()),
            "7a975f75dfe67b4da5d69269c602a827f2c1efeadee4bf1de4029b8b3c91accb"
        );
        let h = sha256_hex(b"abc");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[cfg(unix)]
    #[test]
    fn secure_random_length_and_zero() {
        assert_eq!(secure_random(0, 0, 0).unwrap().len(), 0);
        assert_eq!(secure_random(16, 0, 0).unwrap().len(), 16);
        assert_eq!(secure_random(1024, 0, 0).unwrap().len(), 1024);
    }

    #[cfg(unix)]
    #[test]
    fn secure_random_differs_between_calls() {
        // Two 32-byte draws from the OS are never equal
        // in practice (2^-256); equality would mean the
        // source is broken, not the test.
        let a = secure_random(32, 0, 0).unwrap();
        let b = secure_random(32, 0, 0).unwrap();
        assert_ne!(a, b);
    }

    #[cfg(unix)]
    #[test]
    fn secure_random_over_cap() {
        let err = secure_random(MAX_SECURE_RANDOM_BYTES + 1, 0, 0).unwrap_err();
        assert!(err.to_string().contains("تجاوز الحد"), "got: {}", err);
    }

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn secure_random_refused_in_browser() {
        let err = secure_random(16, 0, 0).unwrap_err();
        assert!(
            err.to_string().contains("غير مدعومة في متصفح الويب"),
            "got: {}",
            err
        );
    }

    #[cfg(all(not(unix), not(target_arch = "wasm32")))]
    #[test]
    fn secure_random_refused_on_windows() {
        let err = secure_random(16, 0, 0).unwrap_err();
        assert!(
            err.to_string().contains("غير مدعوم على هذه المنصة"),
            "got: {}",
            err
        );
    }
}
