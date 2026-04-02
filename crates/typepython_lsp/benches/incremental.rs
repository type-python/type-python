use std::{
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::{Value, json};
use typepython_config::ConfigHandle;
use typepython_lsp::serve_with_io;
use url::Url;

fn bench_incremental_implementation_edit_session(c: &mut Criterion) {
    let fixture = chain_fixture("incremental_implementation_edit_session", 48);
    let session = build_session(
        &fixture.open_uri,
        &fixture.hover_uri,
        &fixture.initial_text,
        "def produce() -> int:\n    value = 1\n    return value\n",
    );

    c.bench_function("lsp_incremental_impl_edit_session_48_modules", |b| {
        b.iter(|| {
            let mut output = Vec::new();
            serve_with_io(&fixture.config, Cursor::new(session.clone()), &mut output)
                .expect("bench session should succeed");
            criterion::black_box(output.len());
        })
    });
}

fn bench_incremental_public_edit_session(c: &mut Criterion) {
    let fixture = chain_fixture("incremental_public_edit_session", 48);
    let session = build_session(
        &fixture.open_uri,
        &fixture.hover_uri,
        &fixture.initial_text,
        "def produce() -> str:\n    return \"value\"\n",
    );

    c.bench_function("lsp_incremental_public_edit_session_48_modules", |b| {
        b.iter(|| {
            let mut output = Vec::new();
            serve_with_io(&fixture.config, Cursor::new(session.clone()), &mut output)
                .expect("bench session should succeed");
            criterion::black_box(output.len());
        })
    });
}

struct ChainFixture {
    config: ConfigHandle,
    open_uri: String,
    hover_uri: String,
    initial_text: String,
}

fn chain_fixture(test_name: &str, module_count: usize) -> ChainFixture {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("typepython-lsp-bench-{test_name}-{unique}"));
    fs::create_dir_all(root.join("src/app")).expect("workspace app package should be created");
    fs::write(root.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
        .expect("typepython.toml should be written");
    fs::write(root.join("src/app/__init__.tpy"), "pass\n").expect("package marker should exist");

    for index in 0..module_count {
        let name = format!("mod_{index:02}");
        let contents = if index == 0 {
            String::from("def produce() -> int:\n    return 1\n")
        } else {
            let previous = format!("mod_{:02}", index - 1);
            format!(
                "from app.{previous} import produce\n\n\
                 def run_{index:02}() -> int:\n    return produce()\n"
            )
        };
        fs::write(root.join(format!("src/app/{name}.tpy")), contents)
            .expect("chain module should be written");
    }

    let config = typepython_config::load(&root).expect("workspace config should load");
    let open_path = root.join("src/app/mod_00.tpy");
    let hover_path = root.join(format!("src/app/mod_{:02}.tpy", module_count - 1));
    let initial_text = fs::read_to_string(&open_path).expect("initial source should be readable");

    ChainFixture {
        config,
        open_uri: path_to_uri(&open_path),
        hover_uri: path_to_uri(&hover_path),
        initial_text,
    }
}

fn build_session(
    open_uri: &str,
    hover_uri: &str,
    initial_text: &str,
    changed_text: &str,
) -> Vec<u8> {
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": open_uri,
                    "text": initial_text,
                    "languageId": "typepython",
                    "version": 1
                }
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": open_uri, "version": 2},
                "contentChanges": [{"text": changed_text}]
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"textDocument/hover",
            "params": {
                "textDocument": {"uri": hover_uri},
                "position": {"line": 3, "character": 11}
            }
        }),
    ];

    let mut payload = Vec::new();
    for message in messages {
        write_message_frame(&mut payload, &message);
    }
    payload
}

fn write_message_frame(payload: &mut Vec<u8>, message: &Value) {
    let body = serde_json::to_vec(message).expect("message should encode");
    payload.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    payload.extend_from_slice(&body);
}

fn path_to_uri(path: &Path) -> String {
    Url::from_file_path(PathBuf::from(path)).expect("path should convert to file URI").to_string()
}

criterion_group!(
    benches,
    bench_incremental_implementation_edit_session,
    bench_incremental_public_edit_session
);
criterion_main!(benches);
