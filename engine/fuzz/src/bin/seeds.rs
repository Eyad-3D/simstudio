//! Writes the seed corpora into `DIR/<target>/` (benchmarks/fuzz/PLAN.md):
//! the standard library printed as text, the texts of lsim-lang's tests
//! and of the text format's documentation, and the example projects.
//!
//!   cargo +nightly run --manifest-path fuzz/Cargo.toml --bin seeds -- DIR

use std::path::Path;

/// The raw strings (`r#"…"#`) of a Rust source and the fenced blocks of a
/// Markdown file.
fn texts(source: &str) -> Vec<String> {
    let mut out = vec![];
    for chunk in source.split("r#\"").skip(1) {
        if let Some(end) = chunk.find("\"#") {
            out.push(chunk[..end].to_string());
        }
    }
    for (i, chunk) in source.split("```").enumerate() {
        if i % 2 == 1 {
            // (the info string up to the first line break)
            out.push(chunk.split_once('\n').map_or("", |x| x.1).to_string());
        }
    }
    out
}

fn write(dir: &Path, target: &str, items: impl IntoIterator<Item = String>) -> usize {
    let d = dir.join(target);
    std::fs::create_dir_all(&d).expect("the seed folder");
    let mut n = 0;
    for (i, t) in items.into_iter().filter(|t| !t.trim().is_empty()).enumerate() {
        std::fs::write(d.join(format!("seed-{i:04}")), t).expect("a seed");
        n += 1;
    }
    n
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: seeds DIR");
    let dir = Path::new(&dir);
    let engine = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let read = |p: &str| std::fs::read_to_string(engine.join(p)).expect(p);

    let lib = lsim_lib::library();
    let mut text: Vec<String> = lib.components.values().map(lsim_lang::to_text).collect();
    text.extend(lib.connectors.values().map(lsim_lang::connector_to_text));
    text.push(lsim_lang::library_to_text(&lib));
    for f in ["crates/lsim-lang/tests/end_to_end.rs", "crates/lsim-lang/tests/malformed.rs"] {
        text.extend(texts(&read(f)));
    }
    text.extend(texts(&read("docs/text-format.md")));
    let modelica = texts(&read("crates/lsim-lang/tests/basemodelica.rs"));
    let projects: Vec<String> = ["aero-bev", "bev-car", "fs-electric", "hybrid-car"]
        .iter()
        .map(|n| {
            let t = read(&format!("../backend/projects/{n}.json"));
            let v: serde_json::Value = serde_json::from_str(&t).expect("json");
            serde_json::to_string(&v).expect("json")
        })
        .collect();
    println!("text {}", write(dir, "text", text));
    println!("basemodelica {}", write(dir, "basemodelica", modelica));
    println!("project {}", write(dir, "project", projects));
}
