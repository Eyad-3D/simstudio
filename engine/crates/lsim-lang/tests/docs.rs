//! The examples of the text format's documentation (`engine/docs/
//! text-format.md`) parse, checked against the library, and print back.

use lsim_lang::{Report, parse_library, to_text};

#[test]
fn every_example_of_the_documentation_parses() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/text-format.md");
    let doc = std::fs::read_to_string(path).expect("the documentation is there");
    let lib = lsim_lib::library();
    let mut blocks = 0;
    let mut rest = doc.as_str();
    while let Some(start) = rest.find("```modelica\n") {
        let body = &rest[start + 12..];
        let end = body.find("```").expect("a closed block");
        let text = &body[..end];
        let parsed =
            parse_library(text, Some(&lib)).unwrap_or_else(|e| panic!("{}\n{text}", Report(&e)));
        for def in parsed.components.values() {
            let again = parse_library(&to_text(def), Some(&lib)).expect("prints back");
            assert_eq!(&again.components[&def.name], def);
        }
        blocks += 1;
        rest = &body[end + 3..];
    }
    assert!(blocks >= 7, "{blocks} examples");
    println!("{blocks} examples parse");
}
