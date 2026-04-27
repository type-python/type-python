#![no_main]

use libfuzzer_sys::fuzz_target;
use typepython_syntax::TypeExpr;

fuzz_target!(|data: &[u8]| {
    if data.len() > 8_192 {
        return;
    }

    let text = String::from_utf8_lossy(data);
    if let Some(expr) = TypeExpr::parse(&text) {
        let rendered = expr.render();
        let _ = TypeExpr::parse(&rendered);
    }
});
