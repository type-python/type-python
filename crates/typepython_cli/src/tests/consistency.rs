use super::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Cursor;

fn path_to_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

fn lsp_frame(value: &Value) -> String {
    let payload = serde_json::to_string(value).expect("LSP payload should serialize");
    format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload)
}

fn parse_lsp_output(output: &[u8]) -> Vec<Value> {
    let rendered = String::from_utf8(output.to_vec()).expect("LSP output should be UTF-8");
    let mut cursor = 0usize;
    let mut messages = Vec::new();

    while let Some(header_end) = rendered[cursor..].find("\r\n\r\n") {
        let header_end = cursor + header_end;
        let header = &rendered[cursor..header_end];
        let content_length = header
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .expect("LSP output should include Content-Length");
        let body_start = header_end + 4;
        let body_end = body_start + content_length;
        messages.push(
            serde_json::from_str(&rendered[body_start..body_end])
                .expect("LSP response payload should parse"),
        );
        cursor = body_end;
    }

    messages
}

fn lsp_session_messages(config: &typepython_config::ConfigHandle, open_path: &Path) -> Vec<Value> {
    let uri = path_to_uri(open_path);
    let text = fs::read_to_string(open_path).expect("open document should be readable");
    let input = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didOpen",
            "params":{"textDocument":{"uri":uri,"text":text,"languageId":"typepython","version":1}}
        }),
    ]
    .into_iter()
    .map(|message| lsp_frame(&message))
    .collect::<String>();
    let mut output = Vec::<u8>::new();
    typepython_lsp::serve_with_io(config, Cursor::new(input.into_bytes()), &mut output)
        .expect("LSP session should succeed");

    parse_lsp_output(&output)
}

fn lsp_diagnostic_codes(config: &typepython_config::ConfigHandle, open_path: &Path) -> BTreeSet<String> {
    lsp_session_messages(config, open_path)
        .into_iter()
        .filter(|message| message.get("method") == Some(&json!("textDocument/publishDiagnostics")))
        .flat_map(|message| {
            message["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
        })
        .filter_map(|diagnostic| diagnostic.get("code").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

fn cli_diagnostic_codes(config: &typepython_config::ConfigHandle) -> BTreeSet<String> {
    run_pipeline(config)
        .expect("CLI pipeline should succeed")
        .diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect()
}

#[test]
fn cli_and_lsp_report_same_codes_for_syntax_errors() {
    let project_dir = temp_project_dir("cli_and_lsp_report_same_codes_for_syntax_errors");
    let (cli_codes, lsp_codes, lsp_messages) = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let source_path = project_dir.join("src/app/__init__.tpy");
        fs::write(&source_path, "def missing_colon()\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        (
            cli_diagnostic_codes(&config),
            lsp_diagnostic_codes(&config, &source_path),
            lsp_session_messages(&config, &source_path),
        )
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(cli_codes, lsp_codes, "{:?}", lsp_messages);
    assert_eq!(cli_codes, BTreeSet::from([String::from("TPY2001")]));
}

#[test]
fn cli_and_lsp_report_same_codes_for_module_collisions() {
    let project_dir = temp_project_dir("cli_and_lsp_report_same_codes_for_module_collisions");
    let (cli_codes, lsp_codes) = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let open_path = project_dir.join("src/app.tpy");
        fs::write(&open_path, "def build() -> int:\n    return 1\n").expect("test setup should succeed");
        fs::write(project_dir.join("src/app.py"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        (cli_diagnostic_codes(&config), lsp_diagnostic_codes(&config, &open_path))
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(cli_codes, lsp_codes);
    assert_eq!(cli_codes, BTreeSet::from([String::from("TPY3002")]));
}

#[test]
fn cli_and_lsp_agree_when_infer_passthrough_eliminates_errors() {
    let project_dir =
        temp_project_dir("cli_and_lsp_agree_when_infer_passthrough_eliminates_errors");
    let (cli_codes, lsp_codes) = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[typing]\ninfer_passthrough = true\n",
        )
        .expect("test setup should succeed");
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/helpers.py"),
            "class User:\n    def __init__(self):\n        self.age = 3\n\ndef build():\n    return User()\n",
        )
        .expect("test setup should succeed");
        let open_path = project_dir.join("src/app/__init__.tpy");
        fs::write(
            &open_path,
            "from app.helpers import build\n\nuser = build()\nage: int = user.age\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");

        (cli_diagnostic_codes(&config), lsp_diagnostic_codes(&config, &open_path))
    };
    remove_temp_project_dir(&project_dir);

    assert!(cli_codes.is_empty(), "{cli_codes:?}");
    assert_eq!(cli_codes, lsp_codes);
}
