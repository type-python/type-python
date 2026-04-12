use super::{
    EmitArtifact, InferredStubMode, PlannedModuleSource, RuntimeWriteError, RuntimeWriteSummary,
    StubCallableOverride, StubSealedClass, StubSyntheticMethod, StubValueOverride,
    TypePythonStubContext, generate_inferred_stub_source, generate_typepython_stub_source,
    plan_emits_for_sources, write_runtime_outputs,
};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use typepython_config::load;
use typepython_lowering::{LoweredModule, SourceMapEntry};
use typepython_syntax::{FunctionParam, MethodKind, SourceKind};

#[test]
fn plan_emits_for_sources_matches_source_kinds_without_lowered_modules() {
    let temp_dir = temp_dir("plan_emits_for_sources_matches_source_kinds_without_lowered_modules");
    fs::create_dir_all(temp_dir.join("src/pkg")).expect("test setup should succeed");
    fs::write(
        temp_dir.join("typepython.toml"),
        "[project]\nsrc = [\"src\"]\nout_dir = \"build\"\n[emit]\nemit_pyi = true\n",
    )
    .expect("test setup should succeed");
    let config = load(&temp_dir).expect("config should load");
    let artifacts = plan_emits_for_sources(
        &config,
        &[
            PlannedModuleSource {
                source_path: temp_dir.join("src/pkg/__init__.tpy"),
                source_kind: SourceKind::TypePython,
            },
            PlannedModuleSource {
                source_path: temp_dir.join("src/pkg/helpers.py"),
                source_kind: SourceKind::Python,
            },
            PlannedModuleSource {
                source_path: temp_dir.join("src/pkg/helpers.pyi"),
                source_kind: SourceKind::Stub,
            },
        ],
    );

    assert_eq!(artifacts.len(), 2);
    assert_eq!(artifacts[0].runtime_path, Some(temp_dir.join("build/pkg/__init__.py")));
    assert_eq!(artifacts[0].stub_path, Some(temp_dir.join("build/pkg/__init__.pyi")));
    assert_eq!(artifacts[1].runtime_path, Some(temp_dir.join("build/pkg/helpers.py")));
    assert_eq!(artifacts[1].stub_path, Some(temp_dir.join("build/pkg/helpers.pyi")));
    fs::remove_dir_all(&temp_dir).expect("temp dir cleanup should succeed");
}

#[test]
fn write_runtime_outputs_emits_lowered_typepython_and_python_modules() {
    let temp_dir = temp_dir("write_runtime_outputs_emits_lowered_typepython_and_python_modules");
    let modules = vec![
        LoweredModule {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from(
                "from typing import TypeAlias\nUserId: TypeAlias = int\ncount: int = 1\n\ndef build_user() -> int:\n    return 1\n",
            ),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        },
        LoweredModule {
            source_path: PathBuf::from("src/app/helpers.py"),
            source_kind: SourceKind::Python,
            python_source: String::from("def helper():\n    return 1\n"),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        },
        LoweredModule {
            source_path: PathBuf::from("src/app/parse.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from(
                "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n\ndef parse(x):\n    return 0\n",
            ),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        },
        LoweredModule {
            source_path: PathBuf::from("src/app/empty.tpy"),
            source_kind: SourceKind::TypePython,
            python_source: String::from("class Empty:\n    pass\n"),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        },
        LoweredModule {
            source_path: PathBuf::from("src/app/helpers.pyi"),
            source_kind: SourceKind::Stub,
            python_source: String::from("def helper() -> int: ...\n"),
            source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
            span_map: Vec::new(),
            required_imports: Vec::new(),
            metadata: typepython_lowering::LoweringMetadata::default(),
        },
    ];
    let artifacts = vec![
        EmitArtifact {
            source_path: PathBuf::from("src/app/__init__.tpy"),
            runtime_path: Some(temp_dir.join("build/app/__init__.py")),
            stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
        },
        EmitArtifact {
            source_path: PathBuf::from("src/app/helpers.py"),
            runtime_path: Some(temp_dir.join("build/app/helpers.py")),
            stub_path: Some(temp_dir.join("build/app/helpers.pyi")),
        },
        EmitArtifact {
            source_path: PathBuf::from("src/app/parse.tpy"),
            runtime_path: Some(temp_dir.join("build/app/parse.py")),
            stub_path: Some(temp_dir.join("build/app/parse.pyi")),
        },
        EmitArtifact {
            source_path: PathBuf::from("src/app/empty.tpy"),
            runtime_path: Some(temp_dir.join("build/app/empty.py")),
            stub_path: Some(temp_dir.join("build/app/empty.pyi")),
        },
    ];

    let summary = write_runtime_outputs(&artifacts, &modules, true, false, None)
        .expect("runtime outputs should be written");
    let runtime_init = fs::read_to_string(temp_dir.join("build/app/__init__.py"))
        .expect("runtime __init__.py should be readable");
    let stub_init = fs::read_to_string(temp_dir.join("build/app/__init__.pyi"))
        .expect("stub __init__.pyi should be readable");
    let runtime_helpers = fs::read_to_string(temp_dir.join("build/app/helpers.py"))
        .expect("helpers.py should be readable");
    let runtime_parse = fs::read_to_string(temp_dir.join("build/app/parse.py"))
        .expect("parse.py should be readable");
    let stub_parse = fs::read_to_string(temp_dir.join("build/app/parse.pyi"))
        .expect("parse.pyi should be readable");
    let runtime_empty = fs::read_to_string(temp_dir.join("build/app/empty.py"))
        .expect("empty.py should be readable");
    let stub_empty = fs::read_to_string(temp_dir.join("build/app/empty.pyi"))
        .expect("empty.pyi should be readable");
    let stub_helpers = fs::read_to_string(temp_dir.join("build/app/helpers.pyi"))
        .expect("helpers.pyi should be readable");
    let py_typed = fs::read_to_string(temp_dir.join("build/app/py.typed"))
        .expect("py.typed should be readable");

    let result = (
        summary,
        runtime_init,
        stub_init,
        runtime_helpers,
        runtime_parse,
        stub_parse,
        runtime_empty,
        stub_empty,
        stub_helpers,
        py_typed,
    );
    remove_temp_dir(&temp_dir);

    let (
        summary,
        runtime_init,
        stub_init,
        runtime_helpers,
        runtime_parse,
        stub_parse,
        runtime_empty,
        stub_empty,
        stub_helpers,
        py_typed,
    ) = result;
    assert_eq!(
        summary,
        RuntimeWriteSummary {
            runtime_files_written: 4,
            stub_files_written: 4,
            py_typed_written: 1,
        }
    );
    assert_eq!(
        runtime_init,
        "from typing import TypeAlias\nUserId: TypeAlias = int\ncount: int = 1\n\ndef build_user() -> int:\n    return 1\n"
    );
    assert_eq!(
        stub_init,
        "from typing import TypeAlias\nUserId: TypeAlias = int\ncount: int\n\ndef build_user() -> int: ...\n"
    );
    assert_eq!(runtime_helpers, "def helper():\n    return 1\n");
    assert_eq!(
        runtime_parse,
        "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n\ndef parse(x):\n    return 0\n"
    );
    assert_eq!(
        stub_parse,
        "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n\n"
    );
    assert_eq!(runtime_empty, "class Empty:\n    pass\n");
    assert_eq!(stub_empty, "class Empty: ...\n");
    assert_eq!(stub_helpers, "def helper() -> int: ...\n");
    assert_eq!(py_typed, "");
}

#[test]
fn write_runtime_outputs_adds_runtime_validators_only_when_enabled() {
    let temp_dir = temp_dir("write_runtime_outputs_adds_runtime_validators_only_when_enabled");
    fs::create_dir_all(temp_dir.join("src/app")).expect("src/app should be created");
    fs::write(
        temp_dir.join("typepython.toml"),
        "[project]\nsrc = [\"src\"]\n\n[emit]\nruntime_validators = true\n",
    )
    .expect("typepython.toml should be written");
    let _config = load(&temp_dir).expect("config should load");
    let modules = vec![LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "from dataclasses import dataclass\n\n@dataclass\nclass UserInput:\n    name: str\n    age: int\n    email: str | None = None\n",
        ),
        source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    }];
    let artifacts = vec![EmitArtifact {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        runtime_path: Some(temp_dir.join("build/app/__init__.py")),
        stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
    }];

    write_runtime_outputs(&artifacts, &modules, true, true, None)
        .expect("runtime validator outputs should be written");
    let runtime = fs::read_to_string(temp_dir.join("build/app/__init__.py"))
        .expect("runtime validator file should be readable");
    let stub = fs::read_to_string(temp_dir.join("build/app/__init__.pyi"))
        .expect("stub validator file should be readable");
    let result = (runtime, stub);
    remove_temp_dir(&temp_dir);

    let (runtime, stub) = result;
    assert!(runtime.contains("def __tpy_validate__(cls, __data: dict) -> \"UserInput\":"));
    assert!(runtime.contains("field `name' expected str but got"));
    assert!(!stub.contains("__tpy_validate__"));
}

#[test]
fn write_runtime_outputs_skips_runtime_validators_when_disabled() {
    let temp_dir = temp_dir("write_runtime_outputs_skips_runtime_validators_when_disabled");
    let modules = vec![LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "from dataclasses import dataclass\n\n@dataclass\nclass UserInput:\n    name: str\n",
        ),
        source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    }];
    let artifacts = vec![EmitArtifact {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        runtime_path: Some(temp_dir.join("build/app/__init__.py")),
        stub_path: None,
    }];
    write_runtime_outputs(&artifacts, &modules, true, false, None)
        .expect("runtime outputs should be written without validators");
    let runtime = fs::read_to_string(temp_dir.join("build/app/__init__.py"))
        .expect("runtime file should be readable");
    remove_temp_dir(&temp_dir);

    assert!(!runtime.contains("__tpy_validate__"));
}

#[test]
fn write_runtime_outputs_reports_pyi_generation_failure() {
    let temp_dir = temp_dir("write_runtime_outputs_reports_pyi_generation_failure");
    let modules = vec![LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from("def broken(:\n"),
        source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    }];
    let artifacts = vec![EmitArtifact {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        runtime_path: Some(temp_dir.join("build/app/__init__.py")),
        stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
    }];

    let result = write_runtime_outputs(&artifacts, &modules, true, false, None);
    remove_temp_dir(&temp_dir);

    let error = result.expect_err("invalid lowered python should fail stub generation");
    assert!(matches!(
        error,
        RuntimeWriteError::StubGeneration { ref source_path, .. }
            if source_path == &PathBuf::from("src/app/__init__.tpy")
    ));
    assert!(error.to_string().contains("TPY5001"));
}

#[test]
fn write_runtime_outputs_writes_py_typed_for_stub_only_package() {
    let temp_dir = temp_dir("write_runtime_outputs_writes_py_typed_for_stub_only_package");
    let modules = vec![LoweredModule {
        source_path: PathBuf::from("src/app/__init__.pyi"),
        source_kind: SourceKind::Stub,
        python_source: String::from("def helper() -> int: ...\n"),
        source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    }];
    let artifacts = vec![EmitArtifact {
        source_path: PathBuf::from("src/app/__init__.pyi"),
        runtime_path: None,
        stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
    }];

    let summary = write_runtime_outputs(&artifacts, &modules, true, false, None)
        .expect("stub-only package outputs should be written");
    let stub = fs::read_to_string(temp_dir.join("build/app/__init__.pyi"))
        .expect("stub-only __init__.pyi should be readable");
    let py_typed =
        fs::read_to_string(temp_dir.join("build/app/py.typed")).expect("py.typed should exist");
    remove_temp_dir(&temp_dir);

    assert_eq!(summary.stub_files_written, 1);
    assert_eq!(summary.py_typed_written, 1);
    assert_eq!(stub, "def helper() -> int: ...\n");
    assert_eq!(py_typed, "");
}

#[test]
fn write_runtime_outputs_skips_py_typed_when_disabled() {
    let temp_dir = temp_dir("write_runtime_outputs_skips_py_typed_when_disabled");
    let modules = vec![LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from("def build() -> int:\n    return 1\n"),
        source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    }];
    let artifacts = vec![EmitArtifact {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        runtime_path: Some(temp_dir.join("build/app/__init__.py")),
        stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
    }];

    let summary = write_runtime_outputs(&artifacts, &modules, false, false, None)
        .expect("runtime outputs should be written without py.typed");
    let py_typed_path = temp_dir.join("build/app/py.typed");
    let py_typed_exists = py_typed_path.exists();
    remove_temp_dir(&temp_dir);

    assert_eq!(summary.py_typed_written, 0);
    assert!(!py_typed_exists);
}

#[test]
fn write_runtime_outputs_preserves_multiline_stub_headers() {
    let temp_dir = temp_dir("write_runtime_outputs_preserves_multiline_stub_headers");
    let modules = vec![LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "class Box(\n    Generic[T],\n):\n    pass\n\ndef build(\n    value: int,\n) -> int:\n    return value\n",
        ),
        source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    }];
    let artifacts = vec![EmitArtifact {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        runtime_path: Some(temp_dir.join("build/app/__init__.py")),
        stub_path: Some(temp_dir.join("build/app/__init__.pyi")),
    }];

    write_runtime_outputs(&artifacts, &modules, true, false, None)
        .expect("multiline runtime outputs should be written");
    let stub = fs::read_to_string(temp_dir.join("build/app/__init__.pyi"))
        .expect("multiline stub should be readable");
    remove_temp_dir(&temp_dir);

    assert!(stub.contains("class Box(\n    Generic[T],\n): ..."));
    assert!(stub.contains("def build(\n    value: int,\n) -> int: ..."));
}

#[test]
fn generate_inferred_shadow_stub_uses_unknown_fallback_and_infers_simple_returns() {
    let stub = generate_inferred_stub_source(
        "VALUE = 1\n\ndef parse(text, retries=3):\n    return 1\n",
        InferredStubMode::Shadow,
    )
    .expect("shadow stub generation should succeed");

    assert!(stub.contains("VALUE: int"));
    assert!(stub.contains("def parse(text: unknown, retries: int = ...) -> int: ..."));
}

#[test]
fn generate_inferred_migration_stub_marks_missing_types_and_init_attrs() {
    let stub = generate_inferred_stub_source(
            "class User:\n    def __init__(self, name):\n        self.name = name\n        self.age = 3\n\n    @property\n    def title(self):\n        return self.name\n",
            InferredStubMode::Migration,
        )
        .expect("migration stub generation should succeed");

    assert!(stub.starts_with("# auto-generated by typepython migrate"));
    assert!(stub.contains("    # TODO: add type annotation\n    name: ..."));
    assert!(stub.contains("    age: int"));
    assert!(stub.contains("    @property"));
    assert!(stub.contains("    # TODO: add type annotation\n    def title(self) -> ...: ..."));
}

#[test]
fn generate_inferred_migration_stub_infers_local_and_attribute_returns() {
    let stub = generate_inferred_stub_source(
            "DEFAULT_RETRIES = 3\n\nclass User:\n    def __init__(self, age: int):\n        self.age = age\n\n    @property\n    def years(self):\n        return self.age\n\ndef parse(text: str):\n    retries = DEFAULT_RETRIES\n    return retries\n",
            InferredStubMode::Migration,
        )
        .expect("migration stub generation should succeed");

    assert!(stub.contains("DEFAULT_RETRIES: int"));
    assert!(stub.contains("def parse(text: str) -> int: ..."));
    assert!(stub.contains("    def years(self) -> int: ..."));
    assert!(!stub.contains("# TODO: add type annotation\ndef parse"));
    assert!(!stub.contains("# TODO: add type annotation\n    def years"));
}

#[test]
fn generate_typepython_stub_source_materializes_semantic_callable_and_synthetic_init() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "@decorate\ndef build(name: str) -> int:\n    return 1\n\n@model\nclass User:\n    name: str\n",
        ),
        source_map: vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
            SourceMapEntry { original_line: 3, lowered_line: 3 },
            SourceMapEntry { original_line: 4, lowered_line: 4 },
            SourceMapEntry { original_line: 5, lowered_line: 5 },
            SourceMapEntry { original_line: 6, lowered_line: 6 },
        ],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };
    let context = TypePythonStubContext {
        value_overrides: Vec::new(),
        callable_overrides: vec![StubCallableOverride {
            line: 2,
            params: vec![FunctionParam {
                name: String::from("name"),
                annotation: Some(String::from("str")),
                annotation_expr: None,
                has_default: false,
                positional_only: false,
                keyword_only: false,
                variadic: false,
                keyword_variadic: false,
            }],
            returns: Some(String::from("str")),
            use_async_syntax: false,
            drop_non_builtin_decorators: true,
        }],
        synthetic_methods: vec![StubSyntheticMethod {
            class_line: 6,
            name: String::from("__init__"),
            method_kind: MethodKind::Instance,
            params: vec![
                FunctionParam {
                    name: String::from("self"),
                    annotation: None,
                    annotation_expr: None,
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                },
                FunctionParam {
                    name: String::from("name"),
                    annotation: Some(String::from("str")),
                    annotation_expr: None,
                    has_default: false,
                    positional_only: false,
                    keyword_only: false,
                    variadic: false,
                    keyword_variadic: false,
                },
            ],
            returns: Some(String::from("None")),
        }],
        sealed_classes: Vec::new(),
        guarded_declaration_lines: BTreeSet::new(),
    };

    let stub =
        generate_typepython_stub_source(&module, &context).expect("semantic stub should generate");

    assert!(!stub.contains("@decorate"));
    assert!(stub.contains("def build(name: str) -> str: ..."));
    assert!(!stub.contains("@model"));
    assert!(stub.contains("def __init__(self, name: str) -> None: ..."));
}

#[test]
fn generate_typepython_stub_source_preserves_detailed_sealed_metadata_comments() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "class Expr:  # tpy:sealed\n    ...\n\nclass Num(Expr):\n    ...\n\nclass Add(Expr):\n    ...\n",
        ),
        source_map: vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
            SourceMapEntry { original_line: 4, lowered_line: 4 },
            SourceMapEntry { original_line: 5, lowered_line: 5 },
            SourceMapEntry { original_line: 7, lowered_line: 7 },
            SourceMapEntry { original_line: 8, lowered_line: 8 },
        ],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };
    let context = TypePythonStubContext {
        value_overrides: Vec::new(),
        callable_overrides: Vec::new(),
        synthetic_methods: Vec::new(),
        sealed_classes: vec![StubSealedClass {
            line: 1,
            name: String::from("Expr"),
            members: vec![String::from("Add"), String::from("Num")],
        }],
        guarded_declaration_lines: BTreeSet::new(),
    };

    let stub = generate_typepython_stub_source(&module, &context)
        .expect("sealed metadata stub should generate");

    assert!(stub.contains("# tpy:sealed Expr -> {Add, Num}"));
    assert!(stub.contains("class Expr:  # tpy:sealed"));
}

#[test]
fn generate_typepython_stub_source_preserves_typeddict_transform_provenance_comment() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "class User(TypedDict):\n    id: int\n\n# tpy:derived Partial[User]\nclass UserCreate(TypedDict):\n    id: NotRequired[int]\n",
        ),
        source_map: vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
            SourceMapEntry { original_line: 4, lowered_line: 4 },
            SourceMapEntry { original_line: 5, lowered_line: 5 },
            SourceMapEntry { original_line: 6, lowered_line: 6 },
        ],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };

    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("TypedDict provenance stub should generate");

    assert!(stub.contains("# tpy:derived Partial[User]"));
    assert!(stub.contains("class UserCreate(TypedDict):"));
}

#[test]
fn generate_typepython_stub_source_lifts_selected_guarded_declarations() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "import typing\nif typing.TYPE_CHECKING:\n    class User:\n        pass\n\ndef take(user: User) -> User:\n    return user\n",
        ),
        source_map: vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
            SourceMapEntry { original_line: 3, lowered_line: 3 },
            SourceMapEntry { original_line: 4, lowered_line: 4 },
            SourceMapEntry { original_line: 6, lowered_line: 6 },
            SourceMapEntry { original_line: 7, lowered_line: 7 },
        ],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };

    let mut context = TypePythonStubContext::default();
    context.guarded_declaration_lines.insert(3);
    let stub = generate_typepython_stub_source(&module, &context)
        .expect("guarded declaration stub generation should succeed");

    assert!(stub.contains("class User:"));
    assert!(stub.contains("def take(user: User) -> User: ..."));
}

#[test]
fn generate_typepython_stub_source_normalizes_intrinsic_boundary_types() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "from typing import TypeAlias\nAlias: TypeAlias = dynamic\n\ndef take(value: unknown, other: dynamic) -> unknown:\n    return value\n",
        ),
        source_map: vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
            SourceMapEntry { original_line: 4, lowered_line: 4 },
            SourceMapEntry { original_line: 5, lowered_line: 5 },
        ],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };

    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("intrinsic type normalization should succeed");

    assert!(stub.contains("from typing import Any"));
    assert!(stub.contains("# tpy:unknown take"));
    assert!(stub.contains("Alias: TypeAlias = Any"));
    assert!(stub.contains("def take(value: object, other: Any) -> object: ..."));
    assert!(!stub.contains("value: unknown"));
    assert!(!stub.contains("-> unknown"));
    assert!(!stub.contains("dynamic"));
}

#[test]
fn generate_typepython_stub_source_drops_runtime_control_flow_and_rewrites_assignments() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "VALUE = 1\nif True:\n    VALUE = 2\n\ndef build() -> int:\n    return VALUE\n",
        ),
        source_map: vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
            SourceMapEntry { original_line: 3, lowered_line: 3 },
            SourceMapEntry { original_line: 4, lowered_line: 4 },
            SourceMapEntry { original_line: 5, lowered_line: 5 },
        ],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };

    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("stub should generate");

    assert!(stub.contains("VALUE: int"));
    assert!(!stub.contains("if True"));
    assert!(!stub.contains("return VALUE"));
    assert!(stub.contains("def build() -> int: ..."));
}

#[test]
fn plan_emits_for_sources_returns_empty_for_no_modules() {
    let temp_dir = temp_dir("plan_emits_for_sources_returns_empty_for_no_modules");
    fs::create_dir_all(temp_dir.join("src")).expect("test setup should succeed");
    fs::write(
        temp_dir.join("typepython.toml"),
        "[project]\nsrc = [\"src\"]\nout_dir = \"build\"\n[emit]\nemit_pyi = true\n",
    )
    .expect("test setup should succeed");
    let config = load(&temp_dir).expect("config should load");
    let artifacts = plan_emits_for_sources(&config, &[]);

    remove_temp_dir(&temp_dir);
    assert!(artifacts.is_empty());
}

#[test]
fn generate_typepython_stub_source_handles_async_function() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from("async def build() -> int:\n    return 1\n"),
        source_map: vec![SourceMapEntry { original_line: 1, lowered_line: 1 }],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };

    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("async stub should generate");

    assert!(stub.contains("async def build() -> int: ..."));
}

#[test]
fn generate_typepython_stub_source_handles_property_decorator() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "class User:\n    @property\n    def name(self) -> str:\n        return self._name\n",
        ),
        source_map: vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
            SourceMapEntry { original_line: 3, lowered_line: 3 },
            SourceMapEntry { original_line: 4, lowered_line: 4 },
        ],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };

    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("property stub should generate");

    assert!(stub.contains("@property"));
    assert!(stub.contains("def name(self) -> str: ..."));
}

#[test]
fn generate_typepython_stub_source_handles_multiple_classes() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "class First:\n    value: int\n\nclass Second:\n    name: str\n",
        ),
        source_map: vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
            SourceMapEntry { original_line: 3, lowered_line: 3 },
            SourceMapEntry { original_line: 4, lowered_line: 4 },
            SourceMapEntry { original_line: 5, lowered_line: 5 },
        ],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };

    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("multi-class stub should generate");

    assert!(stub.contains("class First:"));
    assert!(stub.contains("class Second:"));
    assert!(stub.contains("value: int"));
    assert!(stub.contains("name: str"));
}

#[test]
fn generate_typepython_stub_source_handles_import_statements() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "from typing import Optional\nimport os\n\ndef build() -> Optional[str]:\n    return None\n",
        ),
        source_map: vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
            SourceMapEntry { original_line: 3, lowered_line: 3 },
            SourceMapEntry { original_line: 4, lowered_line: 4 },
            SourceMapEntry { original_line: 5, lowered_line: 5 },
        ],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };

    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("import stub should generate");

    assert!(stub.contains("from typing import Optional"));
    assert!(stub.contains("import os"));
    assert!(stub.contains("def build() -> Optional[str]: ..."));
}

#[test]
fn generate_typepython_stub_source_preserves_native_type_alias_surface() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/native_alias.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "type Pair[T] = tuple[T, T]\n\ndef first[T](value: T) -> T:\n    return value\n",
        ),
        source_map: Vec::new(),
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };

    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("native alias stub should generate");

    assert!(stub.contains("type Pair[T] = tuple[T, T]"));
    assert!(stub.contains("def first[T](value: T) -> T: ..."));
}

#[test]
fn generate_inferred_shadow_stub_handles_empty_module() {
    let stub = generate_inferred_stub_source("", InferredStubMode::Shadow)
        .expect("empty shadow stub generation should succeed");

    assert!(stub.is_empty() || stub.trim().is_empty());
}

#[test]
fn generate_inferred_migration_stub_handles_standalone_functions() {
    let stub = generate_inferred_stub_source(
        "def first(a, b):\n    return a + b\n\ndef second(x):\n    return x * 2\n",
        InferredStubMode::Migration,
    )
    .expect("migration stub with multiple functions should succeed");

    assert!(stub.contains("def first("));
    assert!(stub.contains("def second("));
}

#[test]
fn generate_typepython_stub_source_with_value_override() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from("VALUE = 1\n\ndef build() -> int:\n    return VALUE\n"),
        source_map: vec![
            SourceMapEntry { original_line: 1, lowered_line: 1 },
            SourceMapEntry { original_line: 2, lowered_line: 2 },
            SourceMapEntry { original_line: 3, lowered_line: 3 },
            SourceMapEntry { original_line: 4, lowered_line: 4 },
        ],
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };
    let context = TypePythonStubContext {
        value_overrides: vec![StubValueOverride {
            line: 1,
            annotation: String::from("Final[int]"),
        }],
        callable_overrides: Vec::new(),
        synthetic_methods: Vec::new(),
        sealed_classes: Vec::new(),
        guarded_declaration_lines: BTreeSet::new(),
    };

    let stub = generate_typepython_stub_source(&module, &context)
        .expect("value override stub should generate");

    assert!(stub.contains("VALUE: Final[int]"));
    assert!(stub.contains("def build() -> int: ..."));
}

// ─── Snapshot (golden) tests for stub generation ──────────────────────

#[test]
fn snapshot_stub_basic_module() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "from typing import TypeAlias\nUserId: TypeAlias = int\ncount: int = 1\n\ndef build_user() -> int:\n    return 1\n",
        ),
        source_map: (1..=6).map(|i| SourceMapEntry { original_line: i, lowered_line: i }).collect(),
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };
    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("stub generation should succeed");
    insta::assert_snapshot!(stub);
}

#[test]
fn snapshot_stub_class_with_methods() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "class User:\n    name: str\n    age: int\n\n    def greet(self) -> str:\n        return self.name\n\n    @staticmethod\n    def default() -> \"User\":\n        return User()\n",
        ),
        source_map: (1..=10)
            .map(|i| SourceMapEntry { original_line: i, lowered_line: i })
            .collect(),
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };
    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("stub generation should succeed");
    insta::assert_snapshot!(stub);
}

#[test]
fn snapshot_stub_overloaded_function() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "from typing import overload\n\n@overload\ndef parse(x: str) -> int: ...\n@overload\ndef parse(x: bytes) -> int: ...\n\ndef parse(x):\n    return 0\n",
        ),
        source_map: (1..=9).map(|i| SourceMapEntry { original_line: i, lowered_line: i }).collect(),
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };
    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("stub generation should succeed");
    insta::assert_snapshot!(stub);
}

#[test]
fn snapshot_stub_async_function() {
    let module = LoweredModule {
        source_path: PathBuf::from("src/app/__init__.tpy"),
        source_kind: SourceKind::TypePython,
        python_source: String::from(
            "async def fetch(url: str) -> str:\n    return \"\"\n\nasync def process(data: list[int]) -> int:\n    return sum(data)\n",
        ),
        source_map: (1..=5).map(|i| SourceMapEntry { original_line: i, lowered_line: i }).collect(),
        span_map: Vec::new(),
        required_imports: Vec::new(),
        metadata: typepython_lowering::LoweringMetadata::default(),
    };
    let stub = generate_typepython_stub_source(&module, &TypePythonStubContext::default())
        .expect("stub generation should succeed");
    insta::assert_snapshot!(stub);
}

#[test]
fn snapshot_inferred_shadow_stub() {
    let stub = generate_inferred_stub_source(
            "VALUE = 1\nNAME = \"hello\"\n\ndef parse(text, retries=3):\n    return 1\n\nclass Config:\n    debug = False\n    def __init__(self, host):\n        self.host = host\n",
            InferredStubMode::Shadow,
        )
        .expect("shadow stub generation should succeed");
    insta::assert_snapshot!(stub);
}

#[test]
fn snapshot_inferred_migration_stub() {
    let stub = generate_inferred_stub_source(
            "class User:\n    def __init__(self, name, age):\n        self.name = name\n        self.age = age\n\n    @property\n    def title(self):\n        return self.name\n\ndef greet(user):\n    return f\"Hello, {user.name}\"\n",
            InferredStubMode::Migration,
        )
        .expect("migration stub generation should succeed");
    insta::assert_snapshot!(stub);
}

fn temp_dir(test_name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let directory = env::temp_dir().join(format!("typepython-emit-{test_name}-{unique}"));
    fs::create_dir_all(&directory).expect("temp directory should be created");
    directory
}

fn remove_temp_dir(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path).expect("temp directory should be removed");
    }
}
