#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use typepython_emit::{TypePythonStubContext, generate_typepython_stub_source};
use typepython_lowering::lower;
use typepython_syntax::{SourceFile, SourceKind, parse};

fuzz_target!(|data: &[u8]| {
    if data.len() > 65_536 {
        return;
    }

    let source = SourceFile {
        path: PathBuf::from("/fuzz/input.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::from("fuzz.input"),
        text: String::from_utf8_lossy(data).into_owned(),
    };

    let tree = parse(source);
    let lowered = lower(&tree);
    let _ = generate_typepython_stub_source(&lowered.module, &TypePythonStubContext::default());
});
