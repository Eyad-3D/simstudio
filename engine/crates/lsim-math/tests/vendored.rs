//! The vendored CORE-MATH sources are the ones recorded: upstream revision
//! e78d268 with the engine's two patches (`core-math/README.md`,
//! `core-math/lsim.patch`), each file's SHA-256 in `core-math/SHA256SUMS`
//! (`sha256sum -c SHA256SUMS` in `core-math` checks the same). An
//! accidental edit fails here; a deliberate one updates the patch, the
//! README and the sums.
//!
//! And what the patches are for holds of every file, so that a new
//! upstream revision copied without them fails too: no call that ends the
//! process (`exit`, `abort`), and nothing whose width differs between
//! Windows and Linux (`long`, 32 bits on Windows and 64 on Linux; an
//! integer literal with an `l` or `ul` suffix; a builtin on `long`).

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("core-math")
}

/// Every file under `dir`, relative to `base`, with `/` separators.
fn files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).expect("the vendored folder") {
        let p = e.expect("an entry").path();
        if p.is_dir() {
            files(base, &p, out);
        } else {
            let rel = p.strip_prefix(base).unwrap();
            let parts: Vec<String> =
                rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            out.push(parts.join("/"));
        }
    }
}

/// The text as committed: a Windows checkout may turn LF into CRLF.
fn read(path: &Path) -> Vec<u8> {
    let b = std::fs::read(path).expect("a vendored file");
    let mut out = Vec::with_capacity(b.len());
    for (i, c) in b.iter().enumerate() {
        if !(*c == b'\r' && b.get(i + 1) == Some(&b'\n')) {
            out.push(*c);
        }
    }
    out
}

#[test]
fn the_vendored_files_are_the_recorded_ones() {
    let root = root();
    let sums = String::from_utf8(read(&root.join("SHA256SUMS"))).unwrap();
    let mut listed: Vec<(String, String)> = sums
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (h, f) = l.split_once("  ").expect("sha256sum's format: hash, two spaces, file");
            (f.trim().to_string(), h.to_string())
        })
        .collect();
    listed.sort();
    let mut present = vec![];
    files(&root, &root.join("binary64"), &mut present);
    present.sort();
    let names: Vec<&String> = listed.iter().map(|(f, _)| f).collect();
    assert_eq!(names, present.iter().collect::<Vec<_>>(), "the files are those SHA256SUMS lists");
    let mut changed = vec![];
    for (f, h) in &listed {
        let got = hex(&sha256(&read(&root.join(f))));
        if &got != h {
            changed.push(format!("{f}: {got}, recorded {h}"));
        }
    }
    assert!(
        changed.is_empty(),
        "vendored CORE-MATH files differ from the recorded ones (core-math/README.md says how \
         to change them):\n{}",
        changed.join("\n")
    );
}

#[test]
fn no_vendored_code_ends_the_process_or_depends_on_the_width_of_long() {
    let root = root();
    let mut all = vec![];
    files(&root, &root.join("binary64"), &mut all);
    let mut found = vec![];
    for f in &all {
        let text = String::from_utf8(read(&root.join(f))).unwrap();
        let code = strip_comments_and_strings(&text);
        let tokens = tokens(&code);
        for (k, t) in tokens.iter().enumerate() {
            let next = tokens.get(k + 1).map(String::as_str);
            let prev = if k > 0 { tokens.get(k - 1).map(String::as_str) } else { None };
            let call = next == Some("(");
            let bad = match t.as_str() {
                "exit" | "abort" | "_Exit" | "quick_exit" if call => true,
                "long" => prev != Some("long") && !matches!(next, Some("long" | "double")),
                _ if t.starts_with("__builtin_") => {
                    // a builtin on long: clzl, ctzl, popcountl, …
                    t.ends_with('l') && !t.ends_with("ll") && !t.ends_with("_fmal")
                }
                _ => integer_suffix(t).is_some_and(|s| s.matches('l').count() == 1),
            };
            if bad {
                found.push(format!("{f}: `{t}`"));
            }
        }
    }
    assert!(found.is_empty(), "in the vendored code:\n{}", found.join("\n"));
}

/// C source without its comments and string and character literals.
fn strip_comments_and_strings(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            i += 2;
            while i < b.len() && !b[i..].starts_with(b"*/") {
                i += 1;
            }
            i += 2;
            out.push(' ');
        } else if b[i] == b'"' || b[i] == b'\'' {
            let q = b[i];
            i += 1;
            while i < b.len() && b[i] != q {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            out.push(' ');
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    out
}

/// Identifiers, numbers (C's preprocessing numbers) and single
/// punctuation characters.
fn tokens(s: &str) -> Vec<String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < c.len() {
        let start = i;
        if c[i].is_ascii_alphabetic() || c[i] == '_' {
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '_') {
                i += 1;
            }
        } else if c[i].is_ascii_digit()
            || (c[i] == '.' && c.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            i += 1;
            while i < c.len()
                && (c[i].is_ascii_alphanumeric()
                    || c[i] == '.'
                    || c[i] == '_'
                    || (matches!(c[i], '+' | '-') && matches!(c[i - 1], 'e' | 'E' | 'p' | 'P')))
            {
                i += 1;
            }
        } else if c[i].is_whitespace() {
            i += 1;
            continue;
        } else {
            i += 1;
        }
        out.push(c[start..i].iter().collect());
    }
    out
}

/// The suffix of an integer literal (`u`, `l`, `ull`, …), lower case;
/// `None` for anything else.
fn integer_suffix(t: &str) -> Option<String> {
    let first = t.chars().next()?;
    if !first.is_ascii_digit() {
        return None;
    }
    let hex = t.starts_with("0x") || t.starts_with("0X");
    if t.contains('.') || (hex && t.contains(['p', 'P'])) || (!hex && t.contains(['e', 'E'])) {
        return None; // a floating-point literal
    }
    let n = t.chars().rev().take_while(|c| matches!(c, 'u' | 'U' | 'l' | 'L')).count();
    Some(t[t.len() - n..].to_lowercase())
}

/// SHA-256 (FIPS 180-4).
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for t in 0..16 {
            w[t] = u32::from_be_bytes(block[4 * t..4 * t + 4].try_into().unwrap());
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for t in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[t]).wrapping_add(w[t]);
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
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn sha256_is_fips_180_4s() {
    // FIPS 180-4's examples, and two padding edges
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(&sha256(&[b'a'; 64])),
        "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
    );
}

#[test]
fn the_checks_find_what_they_look_for() {
    let code = strip_comments_and_strings(
        "long x = 1l; /* exit(1) */ unsigned long long y = ~0ul >> 2; // abort()\n\
         \"exit(2)\"; exit (3); double z = 0x1.8p-2 + 1e-5; long double w; __builtin_clzl(x);",
    );
    let t = tokens(&code);
    assert!(!code.contains("exit(1)") && !code.contains("exit(2)") && !code.contains("abort"));
    assert_eq!(t.iter().filter(|t| *t == "exit").count(), 1);
    let suffixes: Vec<String> = t.iter().filter_map(|t| integer_suffix(t)).collect();
    assert_eq!(suffixes, ["l", "ul", "", ""]);
    assert!(t.contains(&"0x1.8p-2".to_string()) && t.contains(&"1e-5".to_string()));
    assert!(t.contains(&"__builtin_clzl".to_string()));
}
