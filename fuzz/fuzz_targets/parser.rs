#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use typepython_syntax::{SourceFile, SourceKind, parse};

fuzz_target!(|data: &[u8]| {
    if data.is_empty() || data.len() > 65_536 {
        return;
    }

    let kind = match data[0] % 3 {
        0 => SourceKind::TypePython,
        1 => SourceKind::Python,
        _ => SourceKind::Stub,
    };
    let text = String::from_utf8_lossy(&data[1..]).into_owned();
    let source = SourceFile {
        path: PathBuf::from("/fuzz/input.tpy"),
        kind,
        logical_module: String::from("fuzz.input"),
        text,
    };

    let _ = parse(source);
});
