use super::*;

#[test]
fn type_health_command_parses_fail_under_and_lock_flags() {
    let cli = Cli::parse_from([
        "typepython",
        "type-health",
        "--project",
        "examples/hello-world",
        "--format",
        "json",
        "--fail-under",
        "90",
        "--write-lock",
    ]);

    let super::Command::TypeHealth(args) = cli.command else {
        panic!("expected type-health command");
    };

    assert_eq!(args.run.project, Some(PathBuf::from("examples/hello-world")));
    assert_eq!(args.run.format, super::OutputFormat::Json);
    assert_eq!(args.fail_under, Some(90));
    assert!(args.write_lock);
}

#[test]
fn build_type_health_report_detects_pep561_and_stub_packages() {
    let project_dir = temp_project_dir("build_type_health_report_detects_packages");
    let report = {
        fs::create_dir_all(project_dir.join("site/demo")).expect("runtime package should exist");
        fs::write(project_dir.join("site/demo/py.typed"), "").expect("marker should be written");
        fs::create_dir_all(project_dir.join("site/demo-1.2.3.dist-info"))
            .expect("runtime metadata should exist");
        fs::write(
            project_dir.join("site/demo-1.2.3.dist-info/METADATA"),
            "Name: demo\nVersion: 1.2.3\n",
        )
        .expect("runtime metadata should be written");
        fs::create_dir_all(project_dir.join("site/demo-stubs/demo"))
            .expect("stub package should exist");
        fs::write(
            project_dir.join("site/demo-stubs/demo/__init__.pyi"),
            "from typing_extensions import Any, ExperimentalFeature, overload\n\npublic_value: Any\n_private_value: Any\npublic_default = ...\n_private_default = ...\nclass Box:\n    item: typing.Any\n    raw = ...\n\ndef fetch() -> Any: ...\ndef _private() -> Any: ...\nasync def load() -> typing.Any: ...\n@overload\ndef coerce(value: str) -> str: ...\n@overload\ndef coerce(value: object) -> Any: ...\n",
        )
        .expect("stub surface should be written");
        fs::write(project_dir.join("site/demo-stubs/py.typed"), "partial\n")
            .expect("partial marker should be written");
        fs::create_dir_all(project_dir.join("site/demo_stubs-1.2.0.dist-info"))
            .expect("stub metadata should exist");
        fs::write(
            project_dir.join("site/demo_stubs-1.2.0.dist-info/METADATA"),
            "Name: demo-stubs\nVersion: 1.2.0\n",
        )
        .expect("stub metadata should be written");
        fs::create_dir_all(project_dir.join("site/untyped")).expect("untyped package should exist");

        build_type_health_report_for_target(
            &project_dir,
            &[String::from("site")],
            typepython_target::PythonTarget::default(),
        )
        .expect("report should build")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.packages.len(), 3);
    assert_eq!(report.score, 50);
    assert!(report.packages.iter().any(|package| package.name == "demo" && package.has_py_typed));
    assert!(report.packages.iter().any(|package| package.name == "demo"
        && package.is_stub_only
        && package.is_partial_stub
        && package.public_any_returns == 3
        && package.public_any_attributes == 2
        && package.overload_any_fallbacks == 1
        && package.public_untyped_attributes == 2
        && package.unsupported_typing_extensions_imports == 1
        && package.precision_debt == 9
        && package.runtime_version.as_deref() == Some("1.2.3")
        && package.stub_version.as_deref() == Some("1.2.0")
        && package.stub_version_matches_runtime == Some(false)));
}

#[test]
fn type_health_requires_regular_py_typed_markers() {
    let project_dir = temp_project_dir("type_health_requires_regular_py_typed_markers");
    let report = {
        fs::create_dir_all(project_dir.join("site/runtime_regular"))
            .expect("runtime package should exist");
        fs::write(project_dir.join("site/runtime_regular/py.typed"), "")
            .expect("regular marker should be written");

        fs::create_dir_all(project_dir.join("site/runtime_directory/py.typed"))
            .expect("directory marker should exist");

        fs::create_dir_all(project_dir.join("site/partial-stubs"))
            .expect("partial stub package should exist");
        fs::write(project_dir.join("site/partial-stubs/py.typed"), "partial\n")
            .expect("partial marker should be written");

        fs::create_dir_all(project_dir.join("site/marker-dir-stubs/py.typed"))
            .expect("stub directory marker should exist");

        build_type_health_report_for_target(
            &project_dir,
            &[String::from("site")],
            typepython_target::PythonTarget::default(),
        )
        .expect("report should build")
    };
    remove_temp_project_dir(&project_dir);

    assert!(report.packages.iter().any(|package| {
        package.name == "runtime_regular" && package.has_py_typed && !package.is_stub_only
    }));
    assert!(report.packages.iter().any(|package| {
        package.name == "runtime_directory" && !package.has_py_typed && !package.is_stub_only
    }));
    assert!(report.packages.iter().any(|package| {
        package.name == "partial"
            && package.is_stub_only
            && package.is_partial_stub
            && !package.has_py_typed
    }));
    assert!(report.packages.iter().any(|package| {
        package.name == "marker-dir"
            && package.is_stub_only
            && !package.is_partial_stub
            && !package.has_py_typed
    }));
}

#[test]
fn type_health_reports_invalid_py_typed_contents() {
    let project_dir = temp_project_dir("type_health_reports_invalid_py_typed_contents");
    let error = {
        fs::create_dir_all(project_dir.join("site/demo")).expect("package should exist");
        fs::write(project_dir.join("site/demo/py.typed"), [0xff, 0xfe])
            .expect("invalid UTF-8 marker should be written");

        build_type_health_report_for_target(
            &project_dir,
            &[String::from("site")],
            typepython_target::PythonTarget::default(),
        )
        .expect_err("invalid UTF-8 marker should reject the report")
        .to_string()
    };
    remove_temp_project_dir(&project_dir);

    assert!(error.contains("unable to read PEP 561 marker"), "{error}");
    assert!(error.contains("site/demo/py.typed"), "{error}");
}

#[cfg(unix)]
#[test]
fn type_health_does_not_follow_py_typed_symlinks() {
    use std::os::unix::fs::symlink;

    let project_dir = temp_project_dir("type_health_does_not_follow_py_typed_symlinks");
    let package = {
        fs::create_dir_all(project_dir.join("site/demo")).expect("package should exist");
        fs::write(project_dir.join("site/demo/marker-target"), "partial\n")
            .expect("marker target should be written");
        symlink("marker-target", project_dir.join("site/demo/py.typed"))
            .expect("marker symlink should be created");

        build_type_health_report_for_target(
            &project_dir,
            &[String::from("site")],
            typepython_target::PythonTarget::default(),
        )
        .expect("report should build")
        .packages
        .into_iter()
        .find(|package| package.name == "demo")
        .expect("package should be reported")
    };
    remove_temp_project_dir(&project_dir);

    assert!(!package.has_py_typed);
    assert!(!package.is_partial_stub);
}

#[test]
fn type_health_includes_top_level_module_stub_units() {
    let project_dir = temp_project_dir("type_health_includes_top_level_module_stub_units");
    let module_root = project_dir.join("site/foo.pyi");
    let report = {
        fs::create_dir_all(project_dir.join("site/container-stubs"))
            .expect("stub package should exist");
        fs::write(project_dir.join("site/container-stubs/nested.pyi"), "nested_value: int\n")
            .expect("nested stub should be written");
        fs::write(project_dir.join("site/foo.py"), "value = object()\n")
            .expect("adjacent runtime module should be written");
        fs::write(&module_root, "from typing import Any\n\nvalue: Any\ndef load() -> Any: ...\n")
            .expect("top-level module stub should be written");
        fs::create_dir_all(project_dir.join("site/foo-2.4.0.dist-info"))
            .expect("runtime metadata should exist");
        fs::write(
            project_dir.join("site/foo-2.4.0.dist-info/METADATA"),
            "Name: foo\nVersion: 2.4.0\n",
        )
        .expect("runtime metadata should be written");

        build_type_health_report_for_target(
            &project_dir,
            &[String::from("site")],
            typepython_target::PythonTarget::default(),
        )
        .expect("report should build")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.packages.len(), 2);
    assert_eq!(report.score, 90);
    assert!(!report.packages.iter().any(|package| package.name == "nested"));
    let module = report
        .packages
        .iter()
        .find(|package| package.name == "foo")
        .expect("top-level module should be reported");
    assert_eq!(module.root, module_root.display().to_string());
    assert!(module.is_stub_only);
    assert!(!module.has_py_typed);
    assert!(!module.is_partial_stub);
    assert_eq!(module.public_any_returns, 1);
    assert_eq!(module.public_any_attributes, 1);
    assert_eq!(module.precision_debt, 2);
    assert_eq!(module.runtime_version.as_deref(), Some("2.4.0"));
    assert_eq!(module.stub_version, None);
    assert_eq!(module.stub_version_matches_runtime, None);
}

#[test]
fn type_health_directory_units_take_precedence_over_same_name_module_stubs() {
    let project_dir =
        temp_project_dir("type_health_directory_units_take_precedence_over_module_stubs");
    let runtime_root = project_dir.join("site/foo");
    let stub_root = project_dir.join("site/bar-stubs");
    let report = {
        fs::create_dir_all(&runtime_root).expect("runtime package should exist");
        fs::write(runtime_root.join("py.typed"), "").expect("marker should be written");
        fs::write(runtime_root.join("__init__.pyi"), "value: int\n")
            .expect("package stub should be written");
        fs::write(project_dir.join("site/foo.pyi"), "def invalid(")
            .expect("colliding module stub should be written");

        fs::create_dir_all(&stub_root).expect("stub-only package should exist");
        fs::write(stub_root.join("__init__.pyi"), "value: int\n")
            .expect("stub package surface should be written");
        fs::write(project_dir.join("site/bar.pyi"), "def invalid(")
            .expect("colliding stub module should be written");

        build_type_health_report_for_target(
            &project_dir,
            &[String::from("site")],
            typepython_target::PythonTarget::default(),
        )
        .expect("report should build")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.packages.len(), 2);
    let runtime = report
        .packages
        .iter()
        .find(|package| package.name == "foo")
        .expect("runtime directory should be reported");
    assert_eq!(runtime.root, runtime_root.display().to_string());
    assert!(runtime.has_py_typed);
    assert_eq!(runtime.precision_debt, 0);

    let stubs = report
        .packages
        .iter()
        .find(|package| package.name == "bar")
        .expect("stub directory should be reported");
    assert_eq!(stubs.root, stub_root.display().to_string());
    assert!(stubs.is_stub_only);
    assert_eq!(stubs.precision_debt, 0);
}

#[test]
fn type_health_ignores_private_invalid_and_non_stub_root_files() {
    let project_dir =
        temp_project_dir("type_health_ignores_private_invalid_and_non_stub_root_files");
    let report = {
        fs::create_dir_all(project_dir.join("site")).expect("type root should exist");
        fs::write(project_dir.join("site/visible.pyi"), "value: int\n")
            .expect("public stub should be written");
        for ignored in ["_private.pyi", "bad-name.pyi", "class.pyi"] {
            fs::write(project_dir.join("site").join(ignored), "def invalid(")
                .expect("ignored entry should be written");
        }
        fs::write(project_dir.join("site/runtime.py"), "value = 1\n")
            .expect("runtime module should be written");
        fs::write(project_dir.join("site/README.txt"), "not a stub\n")
            .expect("non-stub entry should be written");

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            fs::write(project_dir.join("outside.pyi"), "def linked() -> Any: ...\n")
                .expect("symlink target should be written");
            symlink(project_dir.join("outside.pyi"), project_dir.join("site/linked.pyi"))
                .expect("stub symlink should be created");
        }

        build_type_health_report_for_target(
            &project_dir,
            &[String::from("site")],
            typepython_target::PythonTarget::default(),
        )
        .expect("ignored entries should not reject the report")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.packages.len(), 1);
    assert_eq!(report.packages[0].name, "visible");
    assert!(report.packages[0].is_stub_only);
}

#[test]
fn type_health_rejects_unavailable_configured_roots() {
    let project_dir = temp_project_dir("type_health_rejects_unavailable_configured_roots");
    let (report_error, command_error) = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[resolution]\ntype_roots = [\"missing-stubs\"]\n",
        )
        .expect("config should be written");
        fs::create_dir_all(project_dir.join("src")).expect("src should exist");

        let report_error = build_type_health_report_for_target(
            &project_dir,
            &[String::from("missing-stubs")],
            typepython_target::PythonTarget::default(),
        )
        .expect_err("missing configured root should reject the report")
        .to_string();
        let command_error = run_type_health(TypeHealthArgs {
            run: RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            fail_under: Some(100),
            write_lock: false,
        })
        .expect_err("missing configured root should fail the command")
        .to_string();
        (report_error, command_error)
    };
    remove_temp_project_dir(&project_dir);

    assert!(report_error.contains("configured type root"), "{report_error}");
    assert!(report_error.contains("unavailable"), "{report_error}");
    assert!(command_error.contains("configured type root"), "{command_error}");
}

#[test]
fn type_health_does_not_treat_stub_metadata_as_runtime_metadata() {
    let project_dir =
        temp_project_dir("type_health_does_not_treat_stub_metadata_as_runtime_metadata");
    let package = {
        fs::create_dir_all(project_dir.join("site/demo-stubs/demo"))
            .expect("stub package should exist");
        fs::write(project_dir.join("site/demo-stubs/demo/__init__.pyi"), "value: int\n")
            .expect("stub should be written");
        fs::write(project_dir.join("site/demo-stubs/py.typed"), "partial\n")
            .expect("marker should be written");
        fs::create_dir_all(project_dir.join("site/demo_stubs-1.2.0.dist-info"))
            .expect("stub metadata should exist");
        fs::write(
            project_dir.join("site/demo_stubs-1.2.0.dist-info/METADATA"),
            "Metadata-Version: 2.1\nName: demo-stubs\nVersion: 1.2.0\n",
        )
        .expect("stub metadata should be written");
        fs::create_dir_all(project_dir.join("site/demo_extra-9.9.9.dist-info"))
            .expect("unrelated metadata should exist");
        fs::write(
            project_dir.join("site/demo_extra-9.9.9.dist-info/METADATA"),
            "Metadata-Version: 2.1\nName: demo-extra\nVersion: 9.9.9\n",
        )
        .expect("unrelated metadata should be written");

        build_type_health_report_for_target(
            &project_dir,
            &[String::from("site")],
            typepython_target::PythonTarget::default(),
        )
        .expect("report should build")
        .packages
        .into_iter()
        .find(|package| package.is_stub_only && package.name == "demo")
        .expect("stub package should be reported")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(package.runtime_version, None);
    assert_eq!(package.stub_version.as_deref(), Some("1.2.0"));
    assert_eq!(package.stub_version_matches_runtime, None);
}

#[test]
fn type_health_ast_counts_only_public_stub_scopes() {
    let package = type_health_for_stub(
        "type_health_ast_counts_only_public_stub_scopes",
        r#"from typing import (
    Any as Dynamic,
    NamedTuple,
    ParamSpec,
    TYPE_CHECKING,
    TypeAlias,
    TypeVar,
    TypedDict,
)
import typing as t
import typing_extensions as te
import models
from models import DEFAULT, User as ImportedUser
from typing_extensions import (
    Any as ExtensionAny,
    ExperimentalFeature as ExperimentalAlias,
)

__all__ = ["_explicit_private"]
visible_not_in_all: Dynamic
_explicit_private: Dynamic
public_value: (
    Dynamic
)
qualified_value: te.Any | None
public_default = ...
_private_default = ...
Alias = list[int]
LegacyAlias = t.List[int]
Alias2: TypeAlias = dict[str, int]
T = TypeVar("T")
P = ParamSpec("P")
Ts = te.TypeVarTuple("Ts")
Point = NamedTuple("Point", [("x", int)])
Payload = TypedDict("Payload", {"id": int})
AliasType = te.TypeAliasType("AliasType", int)
AliasBeforeUser = User
ImportedAlias = ImportedUser
QualifiedImportedAlias = models.User
imported_value = DEFAULT
typing_runtime_value = TYPE_CHECKING

def multiline(
    value: int,
) -> (
    t.Any
): ...

async def load() -> ExtensionAny: ...
def _private_function() -> Dynamic: ...

def outer() -> int:
    local: Dynamic
    local_default = ...
    def nested() -> Dynamic: ...
    class Local:
        field: Dynamic
        raw = ...
    ...

class Public:
    field: Dynamic
    raw = ...
    Alias = dict[str, int]
    T = TypeVar("T")

    def method(self) -> t.Any: ...
    def _private_method(self) -> Dynamic: ...

    def container(self) -> int:
        local: Dynamic
        def nested() -> Dynamic: ...
        ...

    class Nested:
        field: ExtensionAny
        raw = ...

    class _PrivateNested:
        field: Dynamic
        raw = ...
        def method(self) -> Dynamic: ...

class _Private:
    field: Dynamic
    raw = ...
    def method(self) -> Dynamic: ...

class User: ...
"#,
    );

    assert_eq!(package.public_any_returns, 3);
    assert_eq!(package.public_any_attributes, 5);
    assert_eq!(package.public_untyped_attributes, 7);
    assert_eq!(package.overload_any_fallbacks, 0);
    assert_eq!(package.unsupported_typing_extensions_imports, 1);
    assert_eq!(package.precision_debt, 16);
}

#[test]
fn type_health_ast_groups_overload_fallbacks_by_name_and_scope() {
    let package = type_health_for_stub(
        "type_health_ast_groups_overload_fallbacks",
        r#"from typing import Any, overload as ov
import typing as t
from typing_extensions import overload as extension_overload

@ov
def many(value: int) -> Any: ...
unrelated: int
@ov
def many(value: str) -> Any: ...

@t.overload
def last_wide(value: int) -> int: ...
other = ...
@t.overload
def last_wide(value: object) -> Any: ...

@extension_overload
def implemented(value: int) -> int: ...
@extension_overload
def implemented(value: str) -> str: ...
def implemented(value: object) -> Any: ...

@ov
def safe_implementation(value: int) -> Any: ...
def safe_implementation(value: object) -> object: ...

@ov
def shared(value: int) -> int: ...
@ov
def shared(value: object) -> Any: ...

class Box:
    @ov
    def shared(self, value: int) -> Any: ...
    @ov
    def shared(self, value: object) -> str: ...

class Other:
    @ov
    def shared(self, value: int) -> str: ...
    @ov
    def shared(self, value: object) -> Any: ...
"#,
    );

    assert_eq!(package.public_any_returns, 8);
    assert_eq!(package.overload_any_fallbacks, 5);
    assert_eq!(package.public_any_attributes, 0);
    assert_eq!(package.public_untyped_attributes, 1);
    assert_eq!(package.precision_debt, 14);
}

#[test]
fn run_type_health_writes_lock_and_enforces_threshold() {
    let project_dir = temp_project_dir("run_type_health_writes_lock_and_enforces_threshold");
    let module_root = project_dir.join("site/module_health.pyi");
    let (success, failure, lock) = {
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\ntarget_python = \"3.12\"\n\n[resolution]\nanalysis_python = \"3.11\"\ntype_roots = [\"site\"]\n",
        )
        .expect("config should be written");
        fs::create_dir_all(project_dir.join("src")).expect("src should exist");
        fs::create_dir_all(project_dir.join("site/demo")).expect("package should exist");
        fs::write(project_dir.join("site/demo/py.typed"), "").expect("marker should be written");
        fs::write(&module_root, "from typing import Any\n\ndef load() -> Any: ...\n")
            .expect("module stub should be written");
        fs::create_dir_all(project_dir.join("site/typing_extensions-4.12.2.dist-info"))
            .expect("typing_extensions metadata should exist");
        fs::write(
            project_dir.join("site/typing_extensions-4.12.2.dist-info/METADATA"),
            "Name: typing_extensions\nVersion: 4.12.2\n",
        )
        .expect("typing_extensions metadata should be written");

        let success = run_type_health(TypeHealthArgs {
            run: RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            fail_under: Some(95),
            write_lock: true,
        })
        .expect("type-health should run");
        let failure = run_type_health(TypeHealthArgs {
            run: RunArgs { project: Some(project_dir.clone()), format: super::OutputFormat::Json },
            fail_under: Some(96),
            write_lock: false,
        })
        .expect("type-health should run");
        let lock = fs::read_to_string(project_dir.join(".typepython/type-lock.toml"))
            .expect("lock should be written");
        (success, failure, lock)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(success, ExitCode::SUCCESS);
    assert_eq!(failure, ExitCode::FAILURE);
    assert!(lock.contains("score = 95"));
    assert!(lock.contains("[inputs]"));
    assert!(lock.contains("target_python = \"3.12\""));
    assert!(lock.contains("analysis_python = \"3.11\""));
    assert!(lock.contains("typing_extensions_version = \"4.12.2\""));
    assert!(lock.contains("typeshed_commit = \"68517355a3269be407bde20fea8fd66af2dc4241\""));
    assert!(lock.contains("[[checker]]"));
    assert!(lock.contains("name = \"mypy\""));
    assert!(lock.contains("name = \"pyright\""));
    assert!(lock.contains("name = \"ty\""));
    assert!(lock.contains("[[package]]"));
    assert_eq!(lock.matches("[[package]]").count(), 2);
    let locked_module = lock
        .split("[[package]]")
        .find(|package| package.contains("name = \"module_health\""))
        .expect("module typing unit should be locked");
    assert!(locked_module.contains(&format!("root = \"{}\"", module_root.display())));
    assert!(locked_module.contains("has_py_typed = false"));
    assert!(locked_module.contains("is_stub_only = true"));
    assert!(locked_module.contains("public_any_returns = 1"));
    assert!(locked_module.contains("precision_debt = 1"));
    assert!(lock.contains("has_py_typed = true"));
    assert!(lock.contains("public_any_returns = 0"));
    assert!(lock.contains("public_any_attributes = 0"));
    assert!(lock.contains("overload_any_fallbacks = 0"));
    assert!(lock.contains("public_untyped_attributes = 0"));
    assert!(lock.contains("unsupported_typing_extensions_imports = 0"));
    assert!(lock.contains("precision_debt = 0"));
}

fn type_health_for_stub(test_name: &str, source: &str) -> crate::type_health::TypePackageHealth {
    let project_dir = temp_project_dir(test_name);
    let package = {
        fs::create_dir_all(project_dir.join("site/demo-stubs/demo"))
            .expect("stub package should exist");
        fs::write(project_dir.join("site/demo-stubs/demo/__init__.pyi"), source)
            .expect("stub should be written");
        build_type_health_report_for_target(
            &project_dir,
            &[String::from("site")],
            typepython_target::PythonTarget::default(),
        )
        .expect("type-health report should build")
        .packages
        .into_iter()
        .find(|candidate| candidate.name == "demo" && candidate.is_stub_only)
        .expect("stub package should be reported")
    };
    remove_temp_project_dir(&project_dir);
    package
}
