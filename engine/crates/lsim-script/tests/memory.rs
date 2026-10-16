//! The memory cap bounds the memory a runner really takes: every heap
//! block a value makes (an iterator object and what it reads from, a
//! bound method, a function and its defaults, a dict's index, an
//! `interp` table's parse, besides the strings, lists, tuples and dicts)
//! counts towards the runner's 512 MB, so a script whose values keep
//! growing is left to Python before the host's memory goes, and the
//! host never runs out of memory under it (on Windows a failed commit
//! ends the process).
//!
//! In its own test binary: the allocator here counts every block the
//! process holds, as the heap keeps it (a header, rounded up to 16
//! bytes), so the tests can compare what the runner counts with what it
//! really holds. The tests take turns (`TURN`): the count is the whole
//! process's.

use lsim_script::{MAX_LIVE_BYTES, Runner, ScriptError};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// The system's allocator, counting the bytes it holds and their peak.
struct Counting;

static HELD: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static TURN: Mutex<()> = Mutex::new(());

/// The bytes the heap takes for a block of `size`.
fn block(size: usize) -> usize {
    (size + 16 + 15) & !15
}

fn add(size: usize) {
    let now = HELD.fetch_add(block(size), Relaxed) + block(size);
    PEAK.fetch_max(now, Relaxed);
}

// SAFETY: every call goes to the system allocator unchanged; the counts
// are only read.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            add(l.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            add(l.size());
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        HELD.fetch_sub(block(l.size()), Relaxed);
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, size) };
        if !q.is_null() {
            // (the old block counts until the new one is there: a move
            // holds both for a moment)
            add(size);
            HELD.fetch_sub(block(l.size()), Relaxed);
        }
        q
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const MB: usize = 1 << 20;

fn script(top: &str, body: &str) -> String {
    format!("{top}def step(t, dt, inputs, state, params):\n{body}")
}

/// Scripts whose values keep growing, each in another kind of value: the
/// top-level code, or every call adding to `state`.
const GROWING: [(&str, &str, &str); 9] = [
    // the checker's case: a zip object over 200 iterators is 10 KB (each
    // kind a few tenths of a second a call, well within the time limit)
    (
        "zip objects",
        "",
        "    r = range(1)\n    l = []\n    for i in range(5000):\n        l.append(zip(r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r))\n    state[len(state)] = l\n    return {'y': len(state)}\n",
    ),
    (
        "enumerate objects",
        "",
        "    r = range(1)\n    l = []\n    for i in range(300000):\n        l.append(enumerate(r))\n    state[len(state)] = l\n    return {'y': len(state)}\n",
    ),
    // a dict's index (its hashes and slots) besides its entries
    (
        "dicts of integers",
        "",
        "    d = {}\n    for i in range(250000):\n        d[i] = i\n    x = d.get(5)\n    state[len(state)] = d\n    return {'y': len(state)}\n",
    ),
    // (a function is defined at the top level only: lists of 436 MB first,
    // then the functions that take it past the cap)
    (
        "functions with defaults",
        "B = []\nfor k in range(13):\n    B.append(list(range(1000000)))\nK = [[]]\nfor i in range(3000000):\n    def g(a=i, b=i, c=i, d=i, e=i, f=i, h=i, j=i):\n        return a\n    if len(K[-1]) == 500000:\n        K.append([])\n    K[-1].append(g)\n",
        "    return {'y': len(K)}\n",
    ),
    (
        "bound methods",
        "",
        "    l = []\n    m = []\n    for i in range(300000):\n        l.append(m.append)\n    state[len(state)] = l\n    return {'y': len(state)}\n",
    ),
    (
        "short strings",
        "",
        "    l = []\n    for i in range(300000):\n        l.append('a' + 'b')\n    state[len(state)] = l\n    return {'y': len(state)}\n",
    ),
    (
        "small tuples",
        "",
        "    l = []\n    for i in range(300000):\n        l.append((i,))\n    state[len(state)] = l\n    return {'y': len(state)}\n",
    ),
    (
        "dict views and lists",
        "",
        "    l = []\n    for i in range(200000):\n        l.append([{}.keys()])\n    state[len(state)] = l\n    return {'y': len(state)}\n",
    ),
    // interp's parse of each table, kept with a copy of it
    (
        "parsed tables",
        "",
        "    if 'k' not in state:\n        state['k'] = []\n    for i in range(4):\n        t = {}\n        for j in range(200000):\n            t[j] = j\n        interp(t, 1.5)\n        state['k'].append(t)\n    return {'y': len(state['k'])}\n",
    ),
];

/// A script whose values grow past the cap is left to Python before the
/// memory it holds passes about 600 MB (the cap and one container's
/// growth), whatever kind of value it keeps.
#[test]
fn values_that_outgrow_the_cap_stop_before_the_memory_does() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    for (what, top, body) in GROWING {
        let base = HELD.load(Relaxed);
        PEAK.store(base, Relaxed);
        let mut stopped = None;
        let mut r = match Runner::start(&script(top, body), "probe", &serde_json::Map::new()) {
            Ok(r) => Some(r),
            Err(ScriptError::Unsupported(m)) => {
                assert!(m.contains("512 MB"), "{what}: {m}");
                stopped = Some(0);
                None
            }
            Err(e) => panic!("{what}: {e}"),
        };
        let mut slowest = 0.0f64;
        for k in 1..60 {
            let Some(r) = r.as_mut() else { break };
            let started = std::time::Instant::now();
            let got = r.step(k as f64 * 0.01, 0.01, &[]);
            slowest = slowest.max(started.elapsed().as_secs_f64());
            match got {
                Ok(_) => {}
                Err(ScriptError::Unsupported(m)) => {
                    assert!(m.contains("512 MB"), "{what}: {m}");
                    stopped = Some(k);
                    break;
                }
                Err(e) => panic!("{what}: {e}"),
            }
            let held = (HELD.load(Relaxed) - base) / MB;
            assert!(held < 600, "{what}: {held} MB held after call {k}, and no stop");
        }
        let peak = (PEAK.load(Relaxed) - base) / MB;
        eprintln!(
            "{what}: left to Python at call {stopped:?}, at most {peak} MB held, calls of up to \
             {slowest:.2} s"
        );
        assert!(stopped.is_some(), "{what}: never stopped");
        assert!(peak < 600, "{what}: {peak} MB held at the most");
        drop(r);
        // and everything it held is given back
        let after = HELD.load(Relaxed).saturating_sub(base) / MB;
        assert!(after < 2, "{what}: {after} MB still held");
    }
    assert_eq!(MAX_LIVE_BYTES, 512 * MB);
}

/// What each kind of value holds, a few MB of it added to `state` by a
/// call.
const SHAPES: [(&str, &str); 12] = [
    (
        "zip objects",
        "    r = range(1)\n    l = []\n    for i in range(1000):\n        l.append(zip(r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r, r))\n    state[len(state)] = l\n",
    ),
    (
        "enumerate objects",
        "    l = []\n    m = [1]\n    for i in range(50000):\n        l.append(enumerate(m))\n    state[len(state)] = l\n",
    ),
    (
        "a dict of integers",
        "    d = {}\n    for i in range(100000):\n        d[i] = i * 0.5\n    x = d.get(5)\n    state[len(state)] = d\n",
    ),
    (
        "a dict of strings",
        "    d = {}\n    k = 'k'\n    for i in range(5000):\n        k = k + 'x'\n        d[k[-1] + k[-2] + 'ab' + k] = i\n    state[len(state)] = d\n",
    ),
    (
        "dict copies",
        "    d = {}\n    for i in range(20000):\n        d[i] = i\n    l = []\n    for i in range(20):\n        l.append(d.copy())\n    state[len(state)] = l\n",
    ),
    (
        "bound methods",
        "    l = []\n    m = {}\n    for i in range(50000):\n        l.append(m.get)\n    state[len(state)] = l\n",
    ),
    (
        "strings",
        "    l = []\n    s = 'abc'\n    for i in range(50000):\n        l.append(s + s + s)\n    state[len(state)] = l\n",
    ),
    (
        "tuples",
        "    l = []\n    for i in range(50000):\n        l.append((i, i + 1, (i,)))\n    state[len(state)] = l\n",
    ),
    (
        "nested lists",
        "    l = []\n    for i in range(50000):\n        l.append([[i], []])\n    state[len(state)] = l\n",
    ),
    (
        "dict views",
        "    l = []\n    for i in range(50000):\n        d = {i: i}\n        l.append(d.items())\n    state[len(state)] = l\n",
    ),
    (
        "parsed tables",
        "    t = {}\n    for j in range(50000):\n        t[j] = j\n    state[len(state)] = t\n    interp(t, 1.5)\n    interp({0: t}, 1.5, 2.5)\n",
    ),
    (
        "lists from iterators",
        "    l = list(range(100000))\n    state[len(state)] = [list(enumerate(l)), tuple(zip(l, l))]\n",
    ),
];

/// Every kind of value counts at least the memory it really holds (so the
/// cap bounds the host's memory), and not much more (so a script the
/// app's worker runs is not left to Python much earlier).
#[test]
fn every_value_counts_the_memory_it_holds() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    for (what, body) in SHAPES {
        let mut r = Runner::start(
            &script("", &format!("{body}    return {{'y': len(state)}}\n")),
            "probe",
            &serde_json::Map::new(),
        )
        .unwrap_or_else(|e| panic!("{what}: {e}"));
        r.step(0.0, 0.01, &[]).unwrap_or_else(|e| panic!("{what}: {e}"));
        let (held, counted) = (HELD.load(Relaxed), r.memory());
        r.step(0.01, 0.01, &[]).unwrap_or_else(|e| panic!("{what}: {e}"));
        let real = HELD.load(Relaxed) - held;
        let mine = r.memory() - counted;
        eprintln!("{what}: holds {real} bytes, counts {mine}");
        assert!(real > MB, "{what}: {real} bytes");
        assert!(real <= mine, "{what}: holds {real} bytes, counts {mine}");
        assert!(mine <= real + real / 8, "{what}: holds {real} bytes, counts {mine}");
    }
    // functions, made by the top-level code only: two scripts of the same
    // length, one making 20 000 of them
    let start = |n: &str| {
        let top = format!(
            "K = []\nfor i in range({n}):\n    def g(a=i, b=i, c=i):\n        return a\n    K.append(g)\n"
        );
        let held = HELD.load(Relaxed);
        let r =
            Runner::start(&script(&top, "    return {'y': 1}\n"), "probe", &serde_json::Map::new())
                .expect("it starts");
        (HELD.load(Relaxed) - held, r.memory(), r)
    };
    let (held0, counted0, _r0) = start("0 * 20000");
    let (held1, counted1, _r1) = start("1 * 20000");
    let (real, mine) = (held1 - held0, counted1 - counted0);
    eprintln!("functions: hold {real} bytes, count {mine}");
    assert!(real > MB && real <= mine && mine <= real + real / 8, "functions: {real}, {mine}");
}

/// The syntax tree of a script counts too, as more than it holds at the
/// most (every kind of statement and expression, one after the other);
/// reading it takes at most 300 bytes for each byte of code for a moment,
/// and a script of more than a MiB is left to Python before it is read.
#[test]
fn the_code_counts_more_than_its_tree_takes() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let lines = |line: &str, n: usize| -> String { line.repeat(n) };
    for (what, top) in [
        (
            "a table",
            format!(
                "T = {{{}}}\n",
                (0..20000).map(|i| format!("{i}: {i}.5")).collect::<Vec<_>>().join(", ")
            ),
        ),
        ("a tuple", format!("X = ({})\n", vec!["1"; 50000].join(","))),
        ("assignments", lines("x=1\n", 30000)),
        ("statements on a line", format!("{}x=1\n", lines("x=1;", 30000))),
        ("item assignments", format!("x=[1,2]\n{}", lines("x[1]=1\n", 20000))),
        ("conditions", format!("x=0\n{}", lines("if x:x\n", 20000))),
        ("loops", format!("x=[]\n{}", lines("for i in x:i\n", 20000))),
        ("passes", lines("pass\n", 30000)),
        ("functions", (0..10000).map(|i| format!("def f{i}(a=1):\n    return a\n")).collect()),
        (
            "calls",
            format!("x=[1]\n{}", lines("x.append((max(1, 2), (3, 4), [5], {6: 7}))\n", 5000)),
        ),
    ] {
        let code = format!("{top}def step(t, dt, inputs, state, params):\n    return {{'y': 1}}\n");
        let before = HELD.load(Relaxed);
        PEAK.store(before, Relaxed);
        let r = Runner::start(&code, "probe", &serde_json::Map::new())
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        let held = HELD.load(Relaxed) - before;
        let peak = PEAK.load(Relaxed) - before;
        eprintln!(
            "{what}: {} bytes of code hold {held} (at most {peak}), count {}",
            code.len(),
            r.memory()
        );
        assert!(held <= r.memory(), "{what}: holds {held}, counts {}", r.memory());
        // (reading it holds its tokens too for a moment)
        assert!(peak <= 300 * code.len(), "{what}: held {peak} at most");
    }
    let long = format!(
        "X = 1{}\ndef step(t, dt, inputs, state, params):\n    pass\n",
        " ".repeat(1 << 20)
    );
    let e = Runner::start(&long, "probe", &serde_json::Map::new()).err().expect("too long");
    assert!(matches!(&e, ScriptError::Unsupported(m) if m.contains("1 MiB")), "{e:?}");
}
