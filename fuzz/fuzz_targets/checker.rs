#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use typepython_binding::bind;
use typepython_checking::check;
use typepython_graph::build;
use typepython_syntax::{SourceFile, SourceKind, parse};

fuzz_target!(|data: &[u8]| {
    if data.len() > 32_768 {
        return;
    }

    let tree = parse(SourceFile {
        path: PathBuf::from("/fuzz/input.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::from("fuzz.input"),
        text: String::from_utf8_lossy(data).into_owned(),
    });
    let binding = bind(&tree);
    let graph = build(&[binding]);
    let _ = check(&graph);
});
