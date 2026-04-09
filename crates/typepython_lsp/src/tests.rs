use super::*;
use std::{
    env, fs,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn handle_initialize_returns_required_capabilities() {
    let config = temp_config("handle_initialize_returns_required_capabilities", "pass\n");
    let mut server = Server::new(config);
    let responses = server
        .handle_message(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .expect("initialize should succeed");
    let capabilities = &responses[0]["result"]["capabilities"];
    assert_eq!(capabilities["textDocumentSync"]["openClose"], json!(true));
    assert_eq!(capabilities["textDocumentSync"]["change"], json!(2));
    assert_eq!(capabilities["hoverProvider"], json!(true));
    assert_eq!(capabilities["definitionProvider"], json!(true));
    assert_eq!(capabilities["referencesProvider"], json!(true));
    assert_eq!(capabilities["documentFormattingProvider"], json!(true));
    assert_eq!(capabilities["signatureHelpProvider"]["triggerCharacters"], json!(["(", ","]));
    assert_eq!(capabilities["documentSymbolProvider"], json!(true));
    assert_eq!(capabilities["workspaceSymbolProvider"], json!(true));
    assert_eq!(capabilities["renameProvider"], json!(true));
    assert_eq!(capabilities["codeActionProvider"], json!(true));
}

#[test]
fn did_open_publishes_overlay_diagnostics() {
    let config =
        temp_config("did_open_publishes_overlay_diagnostics", "def ok() -> int:\n    return 1\n");
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let responses = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def broken(:\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should publish diagnostics");
    assert!(
        responses
            .iter()
            .any(|response| response["method"] == json!("textDocument/publishDiagnostics"))
    );
    let payload = responses
        .iter()
        .find(|response| {
            response["method"] == json!("textDocument/publishDiagnostics")
                && response["params"]["uri"] == json!(uri)
        })
        .expect("publishDiagnostics notification should be present");
    let diagnostics = payload["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics payload should be an array");
    assert!(!diagnostics.is_empty());
}

#[test]
fn hover_definition_references_and_rename_work() {
    let config = temp_workspace(
        "hover_definition_references_and_rename_work",
        &[
            ("src/app/a.tpy", "def target(value: int) -> int:\n    return value\n"),
            (
                "src/app/b.tpy",
                "from app.a import target\n\ndef use() -> int:\n    return target(1)\n",
            ),
        ],
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/b.tpy"));

    let hover = server
        .handle_hover(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 3, "character": 11}
        }))
        .expect("hover should succeed");
    assert!(
        hover["contents"]["value"]
            .as_str()
            .expect("hover contents should be a string")
            .contains("function target")
    );

    let definition = server
        .handle_definition(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 3, "character": 11}
        }))
        .expect("definition should succeed");
    assert_eq!(definition.as_array().expect("definition should be an array").len(), 1);

    let references = server
        .handle_references(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 3, "character": 11},
            "context": {"includeDeclaration": true}
        }))
        .expect("references should succeed");
    assert!(references.as_array().expect("references should be an array").len() >= 2);

    let rename = server
        .handle_rename(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 3, "character": 11},
            "newName": "renamed"
        }))
        .expect("rename should succeed");
    assert!(rename["changes"].is_object());
}

#[test]
fn signature_help_returns_function_signature() {
    let config = temp_config(
        "signature_help_returns_function_signature",
        "def target(a: int, b: str = \"x\") -> int:\n    return 1\n\nvalue = target(1, )\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let signature_help = server
        .handle_signature_help(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 3, "character": 18}
        }))
        .expect("signatureHelp should succeed");
    assert_eq!(signature_help["activeParameter"], json!(1));
    assert_eq!(signature_help["activeSignature"], json!(0));
    assert_eq!(
        signature_help["signatures"][0]["label"],
        json!("target(a: int, b: str = ...) -> int")
    );
}

#[test]
fn signature_help_returns_member_signature_without_self() {
    let config = temp_config(
        "signature_help_returns_member_signature_without_self",
        "class Box:\n    def put(self, value: int, label: str) -> None:\n        pass\n\nbox = Box()\nbox.put(1, )\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let signature_help = server
        .handle_signature_help(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 5, "character": 11}
        }))
        .expect("signatureHelp should succeed");
    assert_eq!(signature_help["activeParameter"], json!(1));
    assert_eq!(
        signature_help["signatures"][0]["label"],
        json!("Box.put(value: int, label: str) -> None")
    );
}

#[test]
fn document_symbol_returns_hierarchical_symbols() {
    let config = temp_config(
        "document_symbol_returns_hierarchical_symbols",
        "typealias UserId = int\n\nclass Box:\n    value: int\n    def get(self) -> int:\n        return self.value\n\ndef build() -> Box:\n    return Box()\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let symbols = server
        .handle_document_symbol(json!({
            "textDocument": {"uri": uri}
        }))
        .expect("documentSymbol should succeed");
    let symbols = symbols.as_array().expect("document symbols should be an array");
    assert_eq!(symbols.len(), 3);
    assert_eq!(symbols[0]["name"], json!("UserId"));
    assert_eq!(symbols[1]["name"], json!("Box"));
    assert_eq!(symbols[1]["kind"], json!(5));
    assert_eq!(symbols[1]["children"][0]["name"], json!("value"));
    assert_eq!(symbols[1]["children"][1]["name"], json!("get"));
    assert_eq!(symbols[2]["name"], json!("build"));
}

#[test]
fn workspace_symbol_returns_matching_declarations() {
    let config = temp_workspace(
        "workspace_symbol_returns_matching_declarations",
        &[
            ("src/app/models.tpy", "class User:\n    name: str\n"),
            (
                "src/app/services.tpy",
                "from app.models import User\n\ndef create_user() -> User:\n    return User()\n",
            ),
            (
                "src/app/handlers.tpy",
                "from app.services import create_user\n\ndef handle_user() -> User:\n    return create_user()\n",
            ),
        ],
    );
    let mut server = Server::new(config);

    let symbols = server
        .handle_workspace_symbol(json!({
            "query": "user"
        }))
        .expect("workspace/symbol should succeed");
    let symbols = symbols.as_array().expect("workspace symbols should be an array");
    assert!(symbols.iter().any(|symbol| symbol["name"] == json!("User")));
    assert!(symbols.iter().any(|symbol| symbol["name"] == json!("create_user")));
    assert!(symbols.iter().any(|symbol| symbol["name"] == json!("handle_user")));
}

#[cfg(unix)]
#[test]
fn formatting_returns_restored_typepython_source_edits() {
    let config = temp_workspace_with_config(
        "formatting_returns_restored_typepython_source_edits",
        "[project]\nsrc = [\"src\"]\n\n[format]\ncommand = [\"python3\", \"bin/fake_formatter.py\", \"{file}\"]\n",
        &[(
            "src/app/__init__.tpy",
            "typealias  Pair[T]=tuple[T,T]\ninterface Box[T]:\n    value:int\n\ndef build( )->Box[T]:\n    return Box()\n",
        )],
    );
    write_fake_formatter(
        &config.config_dir.join("bin/fake_formatter.py"),
        "import sys\n_ = sys.argv[1]\ntext = sys.stdin.read()\ntext = text.replace(\"Pair[T]=tuple[T,T]\", \"Pair[T] = tuple[T, T]\")\ntext = text.replace(\"value:int\", \"value: int\")\ntext = text.replace(\"def build( )->Box[T]:\", \"def build() -> Box[T]:\")\nsys.stdout.write(text)\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let responses = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "id": 1,
            "method":"textDocument/formatting",
            "params": {
                "textDocument": {"uri": uri},
                "options": {"tabSize": 4, "insertSpaces": true}
            }
        }))
        .expect("formatting should succeed");
    let edits = responses[0]["result"].as_array().expect("formatting result should be an array");
    assert_eq!(edits.len(), 1);
    assert_eq!(
        edits[0]["newText"],
        json!(
            "typealias Pair[T] = tuple[T, T]\ninterface Box[T]:\n    value: int\n\ndef build() -> Box[T]:\n    return Box()\n"
        )
    );
}

#[cfg(unix)]
#[test]
fn formatting_reports_missing_explicit_formatter() {
    let config = temp_workspace_with_config(
        "formatting_reports_missing_explicit_formatter",
        "[project]\nsrc = [\"src\"]\n\n[format]\ncommand = [\"bin/missing-formatter.sh\"]\n",
        &[("src/app/__init__.tpy", "pass\n")],
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let error = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "id": 1,
            "method":"textDocument/formatting",
            "params": {
                "textDocument": {"uri": uri},
                "options": {"tabSize": 4, "insertSpaces": true}
            }
        }))
        .expect_err("missing formatter should surface an error");
    assert!(error.to_string().contains("TPY6003"));
}

#[test]
fn import_binding_definition_and_hover_resolve_to_original_declaration() {
    let config = temp_workspace(
        "import_binding_definition_and_hover_resolve_to_original_declaration",
        &[
            ("src/app/a.tpy", "def target(value: int) -> int:\n    return value\n"),
            ("src/app/b.tpy", "from app.a import target\n"),
        ],
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/b.tpy"));
    let a_uri = path_to_uri(&config.config_dir.join("src/app/a.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/b.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let hover = server
        .handle_hover(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 0, "character": 18}
        }))
        .expect("hover should succeed");
    assert!(
        hover["contents"]["value"]
            .as_str()
            .expect("hover contents should be a string")
            .contains("function target")
    );

    let definition = server
        .handle_definition(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 0, "character": 18}
        }))
        .expect("definition should succeed");
    let entries = definition.as_array().expect("definition should be an array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["uri"], json!(a_uri));
}

#[test]
fn completion_returns_local_symbols_and_member_symbols() {
    let config = temp_workspace(
        "completion_returns_local_symbols_and_member_symbols",
        &[(
            "src/app/__init__.tpy",
            "class Box:\n    value: int\n    def method(self) -> int:\n        return self.value\n\ndef build() -> Box:\n    return Box()\n\nbox: Box = build()\nbox.method\n",
        )],
    );
    let mut server = Server::new(config.clone());
    let path = config.config_dir.join("src/app/__init__.tpy");
    let text = fs::read_to_string(&path).expect("source file should be readable");
    let uri = path_to_uri(&path);
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let symbols = server
        .handle_completion(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 8, "character": 3}
        }))
        .expect("completion for local symbols should succeed");
    assert!(
        symbols["items"]
            .as_array()
            .expect("completion items should be an array")
            .iter()
            .any(|item| item["label"] == json!("build"))
    );

    let members = server
        .handle_completion(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 9, "character": 4}
        }))
        .expect("completion for member symbols should succeed");
    assert!(
        members["items"]
            .as_array()
            .expect("member completion items should be an array")
            .iter()
            .any(|item| item["label"] == json!("method"))
    );
}

#[test]
fn hover_and_definition_resolve_duplicate_member_names_by_receiver() {
    let config = temp_workspace(
        "hover_and_definition_resolve_duplicate_member_names_by_receiver",
        &[(
            "src/app/__init__.tpy",
            "class Foo:\n    def ping(self) -> int:\n        return 1\n\nclass Bar:\n    def ping(self) -> int:\n        return 2\n\nfoo = Foo()\nfoo.ping()\n",
        )],
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let hover = server
        .handle_hover(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 9, "character": 5}
        }))
        .expect("hover should succeed");
    assert_ne!(hover, Value::Null);

    let definition = server
        .handle_definition(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 9, "character": 5}
        }))
        .expect("definition should succeed");
    let entries = definition.as_array().expect("definition should be an array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["range"]["start"]["line"], json!(1));
}

#[test]
fn completion_resolves_duplicate_member_names_by_receiver() {
    let config = temp_workspace(
        "completion_resolves_duplicate_member_names_by_receiver",
        &[(
            "src/app/__init__.tpy",
            "class Foo:
    def ping(self) -> int:
        return 1
    def only_foo(self) -> int:
        return 1

class Bar:
    def ping(self) -> int:
        return 2
    def only_bar(self) -> int:
        return 2

foo: Foo = Foo()
foo.ping()
",
        )],
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let completion = server
        .handle_completion(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 13, "character": 4}
        }))
        .expect("completion should succeed");
    let labels = completion["items"]
        .as_array()
        .expect("completion items should be an array")
        .iter()
        .map(|item| item["label"].as_str().expect("label should be a string"))
        .collect::<Vec<_>>();
    assert!(labels.contains(&"ping"));
    assert!(labels.contains(&"only_foo"));
    assert!(!labels.contains(&"only_bar"));
}

#[test]
fn completion_includes_inherited_members() {
    let config = temp_workspace(
        "completion_includes_inherited_members",
        &[(
            "src/app/__init__.tpy",
            "class Base:\n    def inherited(self) -> int:\n        return 1\n\nclass Child(Base):\n    def own(self) -> int:\n        return 2\n\nchild: Child = Child()\nchild.inherited\n",
        )],
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let completion = server
        .handle_completion(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 9, "character": 6}
        }))
        .expect("completion should succeed");
    let labels = completion["items"]
        .as_array()
        .expect("completion items should be an array")
        .iter()
        .map(|item| item["label"].as_str().expect("label should be a string"))
        .collect::<Vec<_>>();
    assert!(labels.contains(&"inherited"));
    assert!(labels.contains(&"own"));
}

#[test]
fn completion_uses_isinstance_narrowing_for_members() {
    let config = temp_workspace(
        "completion_uses_isinstance_narrowing_for_members",
        &[(
            "src/app/__init__.tpy",
            "class Foo:\n    def only_foo(self) -> int:\n        return 1\n\nclass Bar:\n    def only_bar(self) -> int:\n        return 2\n\ndef use(value: Foo | Bar) -> int:\n    if isinstance(value, Foo):\n        value.only_foo\n    return 0\n",
        )],
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let completion = server
        .handle_completion(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 10, "character": 14}
        }))
        .expect("completion should succeed");
    let labels = completion["items"]
        .as_array()
        .expect("completion items should be an array")
        .iter()
        .map(|item| item["label"].as_str().expect("label should be a string"))
        .collect::<Vec<_>>();
    assert!(labels.contains(&"only_foo"));
    assert!(!labels.contains(&"only_bar"));
}

#[test]
fn code_actions_offer_missing_type_annotation() {
    let config = temp_workspace(
        "code_actions_offer_missing_type_annotation",
        &[(
            "src/app/__init__.tpy",
            "class Box:\n    pass\n\ndef build() -> Box:\n    return Box()\n\nbox = build()\n",
        )],
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

    let actions = server
        .handle_code_action(json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 6, "character": 0},
                "end": {"line": 6, "character": 3}
            },
            "context": {"diagnostics": []}
        }))
        .expect("code action should succeed");
    let actions = actions.as_array().expect("code actions should be an array");
    let action = actions
        .iter()
        .find(|action| {
            action["title"].as_str().is_some_and(|title| title.contains("Add type annotation"))
        })
        .expect("missing type annotation action should be present");
    assert_eq!(action["edit"]["changes"][uri.as_str()][0]["newText"], json!(": Box"));
}

#[test]
fn code_actions_offer_unsafe_wrapper_fix() {
    let config = temp_workspace(
        "code_actions_offer_unsafe_wrapper_fix",
        &[("src/app/__init__.tpy", "def run() -> None:\n    eval(\"1\")\n")],
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

    let actions = server
        .handle_code_action(json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 1, "character": 4},
                "end": {"line": 1, "character": 11}
            },
            "context": {
                "diagnostics": [{
                    "code": "TPY4019",
                    "range": {
                        "start": {"line": 1, "character": 4},
                        "end": {"line": 1, "character": 11}
                    },
                    "message": "unsafe boundary operation `eval(...)` must appear inside `unsafe:`"
                }]
            }
        }))
        .expect("code action should succeed");
    let actions = actions.as_array().expect("code actions should be an array");
    let action = actions
        .iter()
        .find(|action| action["title"] == json!("Wrap in `unsafe:` block"))
        .expect("unsafe wrapper action should be present");
    assert_eq!(
        action["edit"]["changes"][uri.as_str()][0]["newText"],
        json!("    unsafe:\n        eval(\"1\")")
    );
}

#[test]
fn code_actions_offer_missing_import_fix() {
    let config = temp_workspace(
        "code_actions_offer_missing_import_fix",
        &[
            ("src/app/__init__.tpy", "def run() -> None:\n    Foo()\n"),
            ("src/app/types.tpy", "class Foo:\n    pass\n"),
        ],
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

    let actions = server
        .handle_code_action(json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 1, "character": 4},
                "end": {"line": 1, "character": 7}
            },
            "context": {"diagnostics": []}
        }))
        .expect("code action should succeed");
    let actions = actions.as_array().expect("code actions should be an array");
    let action = actions
        .iter()
        .find(|action| action["title"] == json!("Import `Foo` from `app.types`"))
        .expect("missing import action should be present");
    assert_eq!(
        action["edit"]["changes"][uri.as_str()][0]["newText"],
        json!("from app.types import Foo\n")
    );
}

#[test]
fn code_actions_offer_machine_applicable_return_suggestion() {
    let config = temp_workspace(
        "code_actions_offer_machine_applicable_return_suggestion",
        &[(
            "src/app/__init__.tpy",
            "def build(flag: bool) -> int:\n    if flag:\n        return 1\n    return None\n",
        )],
    );
    let path = config.config_dir.join("src/app/__init__.tpy");
    let uri = path_to_uri(&path);
    let text = fs::read_to_string(&path).expect("source file should be readable");
    let syntax = parse_with_options(
        SourceFile {
            path: path.clone(),
            kind: SourceKind::TypePython,
            logical_module: String::from("app"),
            text: text.clone(),
        },
        ParseOptions::default(),
    );
    let document = DocumentState {
        uri: uri.clone(),
        path: path.clone(),
        text,
        syntax,
        local_symbols: BTreeMap::new(),
        local_value_types: BTreeMap::new(),
    };
    let diagnostics = DiagnosticReport {
        diagnostics: vec![
            typepython_diagnostics::Diagnostic::error(
                "TPY4001",
                "function `build` returns `None` where `build` expects `int`",
            )
            .with_span(typepython_diagnostics::Span::new(path.display().to_string(), 4, 1, 4, 12))
            .with_suggestion(
                "Add `| None` to the declared return type",
                typepython_diagnostics::Span::new(path.display().to_string(), 1, 26, 1, 29),
                String::from("int | None"),
                typepython_diagnostics::SuggestionApplicability::MachineApplicable,
            ),
        ],
    };
    let diagnostics = diagnostics_by_uri(std::slice::from_ref(&document), &diagnostics);
    let diagnostics = serde_json::to_value(
        diagnostics.get(&uri).expect("diagnostics should be mapped to the document URI"),
    )
    .expect("diagnostics should serialize");

    let actions = collect_diagnostic_suggestion_code_actions(
        &document,
        LspRange {
            start: LspPosition { line: 0, character: 0 },
            end: LspPosition { line: 0, character: 30 },
        },
        &json!({
            "textDocument": {"uri": uri},
            "context": {"diagnostics": diagnostics}
        }),
    );
    let action = actions
        .iter()
        .find(|action| {
            action["title"]
                .as_str()
                .is_some_and(|title| title.contains("Add `| None` to the declared return type"))
        })
        .expect("return suggestion action should be present");
    assert_eq!(action["edit"]["changes"][uri.as_str()][0]["newText"], json!("int | None"));
}

#[test]
fn did_change_reports_overlay_sync_failure_for_unopened_document() {
    let config = temp_config(
        "did_change_reports_overlay_sync_failure_for_unopened_document",
        "def ok() -> int:\n    return 1\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

    let error = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [{"text": "def changed() -> int:\n    return 2\n"}]
            }
        }))
        .expect_err("didChange without prior didOpen should fail");

    assert!(error.to_string().contains("TPY6002"));
    assert!(error.to_string().contains("unopened overlay"));
}

#[test]
fn did_change_reports_overlay_sync_failure_for_stale_version() {
    let config = temp_config(
        "did_change_reports_overlay_sync_failure_for_stale_version",
        "def ok() -> int:\n    return 1\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def ok() -> int:\n    return 1\n", "languageId": "typepython", "version": 3}}
            }))
            .expect("didOpen should succeed");

    let error = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [{"text": "def stale() -> int:\n    return 0\n"}]
            }
        }))
        .expect_err("stale didChange version should fail");

    assert!(error.to_string().contains("TPY6002"));
    assert!(error.to_string().contains("out of sync"));
}

#[test]
fn did_change_applies_multiple_incremental_content_changes() {
    let config = temp_config(
        "did_change_applies_multiple_incremental_content_changes",
        "def ok() -> int:\n    return 1\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def ok() -> int:\n    return 1\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let responses = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [
                    {
                        "range": {
                            "start": {"line": 0, "character": 4},
                            "end": {"line": 0, "character": 6}
                        },
                        "text": "better"
                    },
                    {
                        "range": {
                            "start": {"line": 1, "character": 11},
                            "end": {"line": 1, "character": 12}
                        },
                        "text": "2"
                    }
                ]
            }
        }))
        .expect("multi-change didChange should succeed");

    assert_eq!(
        server
            .analysis
            .overlays
            .get(&config.config_dir.join("src/app/__init__.tpy"))
            .expect("overlay should still be cached after multi-change update")
            .text,
        "def better() -> int:\n    return 2\n"
    );
    assert!(
        responses
            .iter()
            .all(|response| response.get("method")
                == Some(&json!("textDocument/publishDiagnostics")))
    );
}

#[test]
fn did_change_applies_ranged_content_change() {
    let config =
        temp_config("did_change_applies_ranged_content_change", "def ok() -> int:\n    return 1\n");
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def ok() -> int:\n    return 1\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [
                    {
                        "range": {
                            "start": {"line": 0, "character": 4},
                            "end": {"line": 0, "character": 6}
                        },
                        "text": "name"
                    }
                ]
            }
        }))
        .expect("ranged didChange should succeed");

    assert_eq!(
        server
            .analysis
            .overlays
            .get(&config.config_dir.join("src/app/__init__.tpy"))
            .expect("overlay should still be cached after ranged update")
            .text,
        "def name() -> int:\n    return 1\n"
    );
}

#[test]
fn did_change_reports_overlay_sync_failure_for_out_of_bounds_range() {
    let config = temp_config(
        "did_change_reports_overlay_sync_failure_for_out_of_bounds_range",
        "def ok() -> int:\n    return 1\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def ok() -> int:\n    return 1\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let error = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [
                    {
                        "range": {
                            "start": {"line": 9, "character": 0},
                            "end": {"line": 9, "character": 1}
                        },
                        "text": "boom"
                    }
                ]
            }
        }))
        .expect_err("out-of-bounds ranged didChange should fail");

    assert!(error.to_string().contains("TPY6002"));
    assert!(error.to_string().contains("references line 9 beyond the current document"));
}

#[test]
fn background_scheduler_debounces_and_discards_stale_diagnostics() {
    let config = temp_config(
        "background_scheduler_debounces_and_discards_stale_diagnostics",
        "def ok() -> int:\n    return 1\n",
    );
    let debounce_wait = Duration::from_millis(config.config.watch.debounce_ms + 20);
    let mut server = Server::new(config.clone());
    server.scheduler.enable_background_mode();
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

    let open_responses = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "text": "def broken(:\n",
                    "languageId": "typepython",
                    "version": 1
                }
            }
        }))
        .expect("didOpen should schedule diagnostics");
    assert!(open_responses.is_empty());

    let change_responses = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [{"text": "def fixed() -> int:\n    return 1\n"}]
            }
        }))
        .expect("didChange should coalesce diagnostics");
    assert!(change_responses.is_empty());

    std::thread::sleep(debounce_wait);
    let notifications = server.scheduler.flush_due_timeout();
    assert_eq!(notifications.len(), 1);
    let payload = notifications
        .iter()
        .find(|response| response["method"] == json!("textDocument/publishDiagnostics"))
        .expect("publishDiagnostics notification should be present");
    let diagnostics = payload["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics payload should be an array");
    assert!(diagnostics.is_empty(), "stale diagnostics should have been discarded");
}

#[test]
fn background_scheduler_defers_diagnostics_for_hover_requests() {
    let config = temp_config(
        "background_scheduler_defers_diagnostics_for_hover_requests",
        "def ok() -> int:\n    return 1\n",
    );
    let debounce_wait = Duration::from_millis(config.config.watch.debounce_ms + 20);
    let mut server = Server::new(config.clone());
    server.scheduler.enable_background_mode();
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

    server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "text": "def ok() -> int:\n    return 1\n",
                    "languageId": "typepython",
                    "version": 1
                }
            }
        }))
        .expect("didOpen should schedule diagnostics");

    std::thread::sleep(debounce_wait);
    for request_id in 1..=3 {
        let responses = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "id": request_id,
                "method":"textDocument/hover",
                "params": {
                    "textDocument": {"uri": uri},
                    "position": {"line": 0, "character": 5}
                }
            }))
            .expect("hover should stay responsive");
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["id"], json!(request_id));
        assert!(
            responses
                .iter()
                .all(|response| response["method"] != json!("textDocument/publishDiagnostics"))
        );
    }

    let responses = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "id": 4,
            "method":"textDocument/hover",
            "params": {
                "textDocument": {"uri": uri},
                "position": {"line": 0, "character": 5}
            }
        }))
        .expect("eventual hover should also flush deferred diagnostics");
    assert_eq!(responses[0]["id"], json!(4));
    assert!(
        responses
            .iter()
            .any(|response| response["method"] == json!("textDocument/publishDiagnostics"))
    );
}

#[test]
fn cancel_request_drops_response_before_execution() {
    let config = temp_config(
        "cancel_request_drops_response_before_execution",
        "def ok() -> int:\n    return 1\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "text": "def ok() -> int:\n    return 1\n",
                    "languageId": "typepython",
                    "version": 1
                }
            }
        }))
        .expect("didOpen should succeed");

    server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"$/cancelRequest",
            "params": {"id": 17}
        }))
        .expect("cancelRequest should succeed");

    let responses = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "id": 17,
            "method":"textDocument/hover",
            "params": {
                "textDocument": {"uri": uri},
                "position": {"line": 0, "character": 5}
            }
        }))
        .expect("canceled hover should be dropped");
    assert!(responses.is_empty());
}

#[test]
fn file_uri_helpers_round_trip_paths_with_spaces() {
    let path = PathBuf::from("/tmp/typepython spaced/project/__init__.tpy");
    let uri = path_to_uri(&path);

    assert_eq!(uri, "file:///tmp/typepython%20spaced/project/__init__.tpy");
    assert_eq!(uri_to_path(&uri).expect("URI should decode to file path"), path);
}

#[test]
fn did_change_updates_overlay_and_republishes_diagnostics() {
    let config = temp_config(
        "did_change_updates_overlay_and_republishes_diagnostics",
        "def ok() -> int:\n    return 1\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def broken(:\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let responses = server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [{"text": "def fixed() -> int:\n    return 1\n"}]
            }
        }))
        .expect("didChange should republish diagnostics");
    assert!(
        responses
            .iter()
            .any(|response| response["method"] == json!("textDocument/publishDiagnostics"))
    );
}

#[test]
fn overlay_export_change_republishes_dependent_module_diagnostics() {
    let config = temp_workspace(
        "overlay_export_change_republishes_dependent_module_diagnostics",
        &[
            ("src/app/a.tpy", "class Producer:\n    pass\n"),
            ("src/app/b.tpy", "from app.a import Producer\nvalue = Producer()\n"),
        ],
    );
    let mut server = Server::new(config.clone());
    let a_uri = path_to_uri(&config.config_dir.join("src/app/a.tpy"));

    server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": a_uri,
                    "text": "class Replacement:\n    pass\n",
                    "languageId": "typepython",
                    "version": 1
                }
            }
        }))
        .expect("didOpen should publish dependent diagnostics");

    let workspace = server
        .analysis
        .cached_workspace
        .as_ref()
        .expect("workspace should be materialized after diagnostics publish");
    let diagnostics = workspace
        .check_diagnostics_by_module
        .get("app.b")
        .expect("dependent module diagnostics should be tracked");
    assert!(!diagnostics.is_empty());
    assert!(
        diagnostics[0].message.contains("Producer") || diagnostics[0].message.contains("import")
    );
}

#[test]
fn incremental_workspace_keeps_public_fingerprints_stable_for_implementation_only_changes() {
    let config = temp_workspace(
        "incremental_workspace_keeps_public_fingerprints_stable_for_implementation_only_changes",
        &[
            ("src/app/a.tpy", "def produce() -> int:\n    return 1\n"),
            ("src/app/b.tpy", "from app.a import produce\nvalue: int = produce()\n"),
        ],
    );
    let a_path = config.config_dir.join("src/app/a.tpy");
    let a_uri = path_to_uri(&a_path);
    let overlays = BTreeMap::new();
    let mut workspace =
        IncrementalWorkspace::new(config.clone(), &overlays).expect("workspace should build");
    let before_fingerprint = workspace
        .incremental
        .fingerprints
        .get("app.a")
        .copied()
        .expect("module fingerprint should exist");
    let before_b_diagnostics =
        workspace.check_diagnostics_by_module.get("app.b").cloned().unwrap_or_default();

    let overlay = OverlayDocument {
        uri: a_uri,
        text: String::from("def produce() -> int:\n    value = 1\n    return value\n"),
        version: 1,
    };
    workspace
        .apply_project_path_update(&a_path, Some(&overlay))
        .expect("overlay update should be applied incrementally");

    let after_fingerprint = workspace
        .incremental
        .fingerprints
        .get("app.a")
        .copied()
        .expect("module fingerprint should still exist");
    let after_b_diagnostics =
        workspace.check_diagnostics_by_module.get("app.b").cloned().unwrap_or_default();

    assert_eq!(before_fingerprint, after_fingerprint);
    assert_eq!(before_b_diagnostics, after_b_diagnostics);
}

#[test]
fn active_support_set_changes_stay_incremental() {
    let config = temp_config(
        "active_support_set_changes_stay_incremental",
        "def run() -> None:\n    pass\n",
    );
    let path = config.config_dir.join("src/app/__init__.tpy");
    let uri = path_to_uri(&path);
    let mut server = Server::new(config.clone());

    server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "text": "def run() -> None:\n    pass\n",
                    "languageId": "typepython",
                    "version": 1
                }
            }
        }))
        .expect("didOpen should initialize the workspace");
    let workspace = server
        .analysis
        .cached_workspace
        .as_ref()
        .expect("workspace should be cached after didOpen");
    assert!(workspace.active_support_paths.is_empty());

    server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": uri, "version": 2},
                "contentChanges": [{
                    "text": "from typing import Final\n\nVALUE: Final[int] = 1\n"
                }]
            }
        }))
        .expect("didChange should add support modules incrementally");
    let workspace = server
        .analysis
        .cached_workspace
        .as_ref()
        .expect("workspace should remain cached after support activation");
    assert!(!workspace.last_state_refresh_was_full);
    assert!(!workspace.active_support_paths.is_empty());
    assert!(workspace.state.documents.len() > 1);
    assert!(
        workspace
            .state
            .documents
            .iter()
            .any(|document| document.syntax.source.kind == SourceKind::Stub)
    );

    server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didChange",
            "params": {
                "textDocument": {"uri": uri, "version": 3},
                "contentChanges": [{
                    "text": "def run() -> None:\n    pass\n"
                }]
            }
        }))
        .expect("didChange should remove support modules incrementally");
    let workspace = server
        .analysis
        .cached_workspace
        .as_ref()
        .expect("workspace should remain cached after support removal");
    assert!(!workspace.last_state_refresh_was_full);
    assert!(workspace.active_support_paths.is_empty());
    assert_eq!(workspace.state.documents.len(), 1);
    assert!(
        workspace
            .state
            .documents
            .iter()
            .all(|document| document.syntax.source.kind != SourceKind::Stub)
    );
}

#[test]
fn incremental_workspace_refreshes_query_indexes() {
    let config = temp_config(
        "incremental_workspace_refreshes_query_indexes",
        "def produce() -> int:\n    return 1\n",
    );
    let path = config.config_dir.join("src/app/__init__.tpy");
    let uri = path_to_uri(&path);
    let overlays = BTreeMap::new();
    let mut workspace =
        IncrementalWorkspace::new(config.clone(), &overlays).expect("workspace should build");
    assert!(workspace.state.queries.occurrences_by_uri.get(&uri).is_some_and(|occurrences| {
        occurrences.iter().any(|occurrence| occurrence.canonical == "app.produce")
    }));

    let overlay = OverlayDocument {
        uri: uri.clone(),
        text: String::from("def build() -> int:\n    return 1\n"),
        version: 1,
    };
    workspace
        .apply_project_path_update(&path, Some(&overlay))
        .expect("overlay update should refresh query indexes");

    let queried_document = workspace
        .state
        .queries
        .documents_by_uri
        .get(&uri)
        .expect("query document cache should contain the updated document");
    assert!(queried_document.text.contains("def build()"));
    assert!(!workspace.last_state_refresh_was_full);
    assert!(workspace.state.queries.occurrences_by_uri.get(&uri).is_some_and(|occurrences| {
        occurrences.iter().any(|occurrence| occurrence.canonical == "app.build")
            && occurrences.iter().all(|occurrence| occurrence.canonical != "app.produce")
    }));
    assert!(workspace.state.queries.nodes_by_module_key.get("app").is_some_and(|node| {
        node.declarations.iter().any(|declaration| declaration.name == "build")
            && node.declarations.iter().all(|declaration| declaration.name != "produce")
    }));
}

#[test]
fn large_workspace_implementation_change_keeps_affected_set_local() {
    let config =
        temp_chain_workspace("large_workspace_implementation_change_keeps_affected_set_local", 24);
    let path = config.config_dir.join("src/app/mod_00.tpy");
    let uri = path_to_uri(&path);
    let overlays = BTreeMap::new();
    let mut workspace =
        IncrementalWorkspace::new(config.clone(), &overlays).expect("workspace should build");
    let before_incremental = workspace.incremental.clone();
    let before_dependency_index = workspace.dependency_index.clone();

    let overlay = OverlayDocument {
        uri,
        text: String::from("def produce() -> int:\n    value = 1\n    return value\n"),
        version: 1,
    };
    workspace
        .apply_project_path_update(&path, Some(&overlay))
        .expect("implementation-only overlay should update incrementally");

    let snapshot_diff = diff(&before_incremental, &workspace.incremental);
    let summary_changed_modules = snapshot_diff_modules(&snapshot_diff);
    let direct_changes = BTreeSet::from([String::from("app.mod_00")]);
    let affected = affected_modules(
        Some(&before_dependency_index),
        &workspace.dependency_index,
        &direct_changes,
        &summary_changed_modules,
    );

    assert!(summary_changed_modules.is_empty());
    assert_eq!(affected, direct_changes);
    assert!(!workspace.last_state_refresh_was_full);
}

#[test]
fn large_workspace_public_change_rechecks_transitive_dependents() {
    let module_count = 24usize;
    let config = temp_chain_workspace(
        "large_workspace_public_change_rechecks_transitive_dependents",
        module_count,
    );
    let path = config.config_dir.join("src/app/mod_00.tpy");
    let uri = path_to_uri(&path);
    let overlays = BTreeMap::new();
    let mut workspace =
        IncrementalWorkspace::new(config.clone(), &overlays).expect("workspace should build");
    let before_incremental = workspace.incremental.clone();
    let before_dependency_index = workspace.dependency_index.clone();

    let overlay = OverlayDocument {
        uri,
        text: String::from("def produce() -> str:\n    return \"value\"\n"),
        version: 1,
    };
    workspace
        .apply_project_path_update(&path, Some(&overlay))
        .expect("public overlay should update incrementally");

    let snapshot_diff = diff(&before_incremental, &workspace.incremental);
    let summary_changed_modules = snapshot_diff_modules(&snapshot_diff);
    let direct_changes = BTreeSet::from([String::from("app.mod_00")]);
    let affected = affected_modules(
        Some(&before_dependency_index),
        &workspace.dependency_index,
        &direct_changes,
        &summary_changed_modules,
    );

    assert_eq!(summary_changed_modules, direct_changes);
    assert_eq!(affected.len(), module_count + 1);
    assert!(affected.contains("app"));
    assert!(affected.contains("app.mod_23"));
    assert!(!workspace.last_state_refresh_was_full);
}

#[test]
fn did_close_clears_overlay() {
    let config = temp_config("did_close_clears_overlay", "def ok() -> int:\n    return 1\n");
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def ok() -> int:\n    return 1\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    server
        .handle_message(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/didClose",
            "params": {"textDocument": {"uri": uri}}
        }))
        .expect("didClose should succeed");
}

#[test]
fn hover_returns_null_for_whitespace_position() {
    let config = temp_config(
        "hover_returns_null_for_whitespace_position",
        "def ok() -> int:\n    return 1\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let hover = server
        .handle_hover(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 1, "character": 0}
        }))
        .expect("hover should succeed");
    assert_eq!(hover, Value::Null);
}

#[test]
fn definition_returns_empty_for_unresolved_symbol() {
    let config = temp_config(
        "definition_returns_empty_for_unresolved_symbol",
        "def ok() -> int:\n    return nonexistent\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let definition = server
        .handle_definition(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 1, "character": 11}
        }))
        .expect("definition should succeed");
    assert_eq!(definition, Value::Null);
}

#[test]
fn completion_returns_items_for_empty_prefix() {
    let config = temp_config(
        "completion_returns_items_for_empty_prefix",
        "def greet() -> int:\n    return 1\n\nclass Widget:\n    pass\n\n",
    );
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));
    let text = fs::read_to_string(config.config_dir.join("src/app/__init__.tpy"))
        .expect("source file should be readable");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should succeed");

    let completion = server
        .handle_completion(json!({
            "textDocument": {"uri": uri},
            "position": {"line": 5, "character": 0}
        }))
        .expect("completion should succeed");
    let labels = completion["items"]
        .as_array()
        .expect("completion items should be an array")
        .iter()
        .map(|item| item["label"].as_str().expect("label should be a string"))
        .collect::<Vec<_>>();
    assert!(labels.contains(&"greet"));
    assert!(labels.contains(&"Widget"));
}

#[test]
fn code_action_returns_empty_when_no_actions_apply() {
    let config = temp_config("code_action_returns_empty_when_no_actions_apply", "x: int = 1\n");
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

    let actions = server
        .handle_code_action(json!({
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 1}
            },
            "context": {"diagnostics": []}
        }))
        .expect("code action should succeed");
    let actions = actions.as_array().expect("code actions should be an array");
    assert!(actions.is_empty());
}

#[test]
fn multi_file_workspace_navigation() {
    let config = temp_workspace(
        "multi_file_workspace_navigation",
        &[
            ("src/app/models.tpy", "class User:\n    name: str\n"),
            (
                "src/app/services.tpy",
                "from app.models import User\n\ndef create_user() -> User:\n    return User()\n",
            ),
            (
                "src/app/handlers.tpy",
                "from app.services import create_user\nfrom app.models import User\n\ndef handle() -> User:\n    return create_user()\n",
            ),
        ],
    );
    let mut server = Server::new(config.clone());

    let models_uri = path_to_uri(&config.config_dir.join("src/app/models.tpy"));
    let services_uri = path_to_uri(&config.config_dir.join("src/app/services.tpy"));
    let handlers_uri = path_to_uri(&config.config_dir.join("src/app/handlers.tpy"));

    let models_text = fs::read_to_string(config.config_dir.join("src/app/models.tpy"))
        .expect("source file should be readable");
    let services_text = fs::read_to_string(config.config_dir.join("src/app/services.tpy"))
        .expect("source file should be readable");
    let handlers_text = fs::read_to_string(config.config_dir.join("src/app/handlers.tpy"))
        .expect("source file should be readable");

    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": models_uri, "text": models_text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen models should succeed");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": services_uri, "text": services_text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen services should succeed");
    server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": handlers_uri, "text": handlers_text, "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen handlers should succeed");

    let definition = server
        .handle_definition(json!({
            "textDocument": {"uri": handlers_uri},
            "position": {"line": 4, "character": 11}
        }))
        .expect("definition should succeed");
    let entries = definition.as_array().expect("definition should be an array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["uri"], json!(services_uri));

    let references = server
        .handle_references(json!({
            "textDocument": {"uri": models_uri},
            "position": {"line": 0, "character": 6},
            "context": {"includeDeclaration": true}
        }))
        .expect("references should succeed");
    let refs = references.as_array().expect("references should be an array");
    assert!(refs.len() >= 2);
    let ref_uris =
        refs.iter().map(|r| r["uri"].as_str().expect("uri should be a string")).collect::<Vec<_>>();
    assert!(ref_uris.iter().any(|uri| uri.contains("models")));
    assert!(ref_uris.iter().any(|uri| uri.contains("services")));
    assert!(ref_uris.iter().any(|uri| uri.contains("handlers")));
}

#[test]
fn did_open_with_syntax_error_reports_diagnostics() {
    let config = temp_config("did_open_with_syntax_error_reports_diagnostics", "pass\n");
    let mut server = Server::new(config.clone());
    let uri = path_to_uri(&config.config_dir.join("src/app/__init__.tpy"));

    let responses = server
            .handle_message(json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params": {"textDocument": {"uri": uri, "text": "def missing_colon()\n    return 1\n", "languageId": "typepython", "version": 1}}
            }))
            .expect("didOpen should publish diagnostics");
    let payload = responses
        .iter()
        .find(|response| {
            response["method"] == json!("textDocument/publishDiagnostics")
                && response["params"]["uri"] == json!(uri)
        })
        .expect("publishDiagnostics notification should be present");
    let diagnostics = payload["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics payload should be an array");
    assert!(!diagnostics.is_empty());
}

fn temp_config(test_name: &str, source: &str) -> ConfigHandle {
    temp_workspace(test_name, &[("src/app/__init__.tpy", source)])
}

fn temp_workspace(test_name: &str, files: &[(&str, &str)]) -> ConfigHandle {
    temp_workspace_with_config(test_name, "[project]\nsrc = [\"src\"]\n", files)
}

fn temp_workspace_with_config(
    test_name: &str,
    config_text: &str,
    files: &[(&str, &str)],
) -> ConfigHandle {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("typepython-lsp-{test_name}-{unique}"));
    fs::create_dir_all(&root).expect("workspace root should be created");
    fs::write(root.join("typepython.toml"), config_text)
        .expect("typepython.toml should be written");
    for (path, content) in files {
        let file_path = root.join(path);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("parent directory should be created");
            let src_root = root.join("src");
            let mut current = parent.to_path_buf();
            while current.starts_with(&src_root) && current != src_root {
                let init_path = current.join("__init__.tpy");
                if !init_path.exists() {
                    fs::write(&init_path, "pass\n").expect("package marker should be written");
                }
                current = current.parent().expect("parent directory should exist").to_path_buf();
            }
        }
        fs::write(file_path, content).expect("workspace file should be written");
    }
    typepython_config::load(&root).expect("workspace config should load")
}

fn temp_chain_workspace(test_name: &str, module_count: usize) -> ConfigHandle {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let root = env::temp_dir().join(format!("typepython-lsp-{test_name}-{unique}"));
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

    typepython_config::load(&root).expect("workspace config should load")
}

#[cfg(unix)]
fn write_fake_formatter(path: &Path, script: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("formatter parent directory should be created");
    }
    fs::write(path, script).expect("formatter script should be written");
    let mut permissions =
        fs::metadata(path).expect("formatter metadata should be readable").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("formatter should be executable");
}
