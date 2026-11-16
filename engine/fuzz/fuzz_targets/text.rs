//! The text format: any UTF-8 text through the parser (alone, against the
//! standard library, as a library), every definition printed back and
//! parsed again, and the last definition of a text that parses built and
//! run for 1 s.
#![no_main]

use libfuzzer_sys::fuzz_target;
use lsim_fuzz::{build_and_run, lang_coded, std_lib};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let lib = std_lib();
    if let Err(e) = lsim_lang::parse(text) {
        lang_coded(&e);
    }
    if let Err(e) = lsim_lang::parse_library(text, Some(lib)) {
        lang_coded(&e);
    }
    match lsim_lang::parse_with(text, lib) {
        Err(e) => lang_coded(&e),
        Ok(defs) => {
            for d in &defs {
                if let Err(e) = lsim_lang::parse_with(&lsim_lang::to_text(d), lib) {
                    lang_coded(&e);
                }
            }
            if let Some((top, rest)) = defs.split_last() {
                let mut l = lib.clone();
                for d in rest {
                    l.add(d.clone());
                }
                build_and_run(l, top, 1.0);
            }
        }
    }
});
