//! Base Modelica import: any UTF-8 text through the importer, and a model
//! it imports built and run for 1 s.
#![no_main]

use libfuzzer_sys::fuzz_target;
use lsim_fuzz::{build_and_run, lang_coded};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    match lsim_lang::basemodelica::import(text) {
        Err(e) => lang_coded(&e),
        Ok(def) => build_and_run(lsim_ir::Library::default(), &def, 1.0),
    }
});
