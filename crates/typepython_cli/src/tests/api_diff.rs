use super::*;

#[test]
fn api_diff_command_parses_input_paths_and_format() {
    let cli = Cli::parse_from(["typepython", "api-diff", "old", "new", "--format", "json"]);

    let super::Command::ApiDiff(args) = cli.command else {
        panic!("expected api-diff command");
    };

    assert_eq!(args.old, PathBuf::from("old"));
    assert_eq!(args.new, PathBuf::from("new"));
    assert_eq!(args.format, super::OutputFormat::Json);
}

#[test]
fn diff_api_surfaces_reports_added_removed_and_changed_symbols() {
    let project_dir = temp_project_dir("diff_api_surfaces_reports_changes");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(
            old_dir.join("app.pyi"),
            "VALUE: int\ndef parse(value: str) -> int: ...\nclass Removed: ...\n",
        )
        .expect("old stub should be written");
        fs::write(
            new_dir.join("app.pyi"),
            "VALUE: str\ndef parse(value: bytes) -> int: ...\nclass Added: ...\n",
        )
        .expect("new stub should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should succeed")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(
        report.added.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["Added"]
    );
    assert_eq!(
        report.removed.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["Removed"]
    );
    assert_eq!(
        report.changed.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["VALUE", "parse"]
    );
    assert_eq!(
        report.release_notes,
        vec![
            String::from(
                "Breaking type-surface change: removed class `Removed` from module `app`."
            ),
            String::from(
                "Review required: changed value `VALUE` in module `app` from `VALUE: int` to `VALUE: str`."
            ),
            String::from(
                "Review required: changed function `parse` in module `app` from `def parse(value: str) -> int:` to `def parse(value: bytes) -> int:`."
            ),
            String::from("Added public class `Added` to module `app`."),
        ]
    );
    assert_eq!(report.semver_recommendation, "major");
}

#[test]
fn diff_api_surfaces_accepts_wheel_and_sdist_stub_inputs() {
    let project_dir = temp_project_dir("diff_api_surfaces_accepts_wheel_and_sdist_stub_inputs");
    let report = {
        let old_wheel = project_dir.join("demo-0.1.0-py3-none-any.whl");
        let new_sdist = project_dir.join("demo-0.2.0.tar.gz");
        write_zip_archive(
            &old_wheel,
            &[("app/__init__.pyi", "def parse(value: str) -> int: ...\nclass Removed: ...\n")],
        );
        write_tar_gz_archive(
            &new_sdist,
            "demo-0.2.0",
            &[("app/__init__.pyi", "def parse(value: bytes) -> int: ...\nclass Added: ...\n")],
        );

        diff_api_surfaces(&old_wheel, &new_sdist).expect("api diff should read archives")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(
        report.changed.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["parse"]
    );
    assert_eq!(
        report.added.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["Added"]
    );
    assert_eq!(
        report.removed.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["Removed"]
    );
}

#[test]
fn diff_api_surfaces_rejects_unsafe_archive_paths() {
    let project_dir = temp_project_dir("diff_api_surfaces_rejects_unsafe_archive_paths");
    let error = {
        let old_wheel = project_dir.join("demo-0.1.0-py3-none-any.whl");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&new_dir).expect("new surface should be created");
        fs::write(new_dir.join("app.pyi"), "def parse() -> int: ...\n")
            .expect("new stub should be written");
        write_zip_archive(&old_wheel, &[("../outside.py", "pass\n")]);

        diff_api_surfaces(&old_wheel, &new_dir)
            .expect_err("unsafe archive member path should be rejected")
            .to_string()
    };
    remove_temp_project_dir(&project_dir);

    assert!(error.contains("forbidden component `..`"), "{error}");
}

#[test]
fn diff_api_surfaces_rejects_duplicate_archive_paths() {
    let project_dir = temp_project_dir("diff_api_surfaces_rejects_duplicate_archive_paths");
    let error = {
        let old_wheel = project_dir.join("demo-0.1.0-py3-none-any.whl");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&new_dir).expect("new surface should be created");
        fs::write(new_dir.join("app.pyi"), "def parse() -> int: ...\n")
            .expect("new stub should be written");
        write_zip_archive(
            &old_wheel,
            &[("app/__init__.pyi", "first: int\n"), ("app/__init__.pyi", "second: str\n")],
        );

        diff_api_surfaces(&old_wheel, &new_dir)
            .expect_err("duplicate archive member path should be rejected")
            .to_string()
    };
    remove_temp_project_dir(&project_dir);

    assert!(error.contains("duplicate member path"), "{error}");
}

#[test]
fn diff_api_surfaces_rejects_excessive_archive_compression_ratio() {
    let project_dir =
        temp_project_dir("diff_api_surfaces_rejects_excessive_archive_compression_ratio");
    let error = {
        let old_wheel = project_dir.join("demo-0.1.0-py3-none-any.whl");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&new_dir).expect("new surface should be created");
        fs::write(new_dir.join("app.pyi"), "def parse() -> int: ...\n")
            .expect("new stub should be written");
        let payload = "0".repeat(20 * 1024 * 1024);
        write_deflated_zip_archive(&old_wheel, &[("app.pyi", payload.as_str())]);

        diff_api_surfaces(&old_wheel, &new_dir)
            .expect_err("excessive compression ratio should be rejected")
            .to_string()
    };
    remove_temp_project_dir(&project_dir);

    assert!(error.contains("compression ratio"), "{error}");
}

#[test]
fn diff_api_surfaces_reads_inline_typed_archive_sources() {
    let project_dir = temp_project_dir("diff_api_surfaces_reads_inline_typed_archive_sources");
    let report = {
        let old_wheel = project_dir.join("demo-0.1.0-py3-none-any.whl");
        let new_sdist = project_dir.join("demo-0.2.0.tar.gz");
        write_zip_archive(
            &old_wheel,
            &[
                ("app/py.typed", ""),
                ("app/__init__.py", "def parse(value: str) -> int:\n    return 1\n"),
            ],
        );
        write_tar_gz_archive(
            &new_sdist,
            "demo-0.2.0",
            &[
                ("app/py.typed", ""),
                ("app/__init__.py", "def parse(value: str) -> str:\n    return \"1\"\n"),
            ],
        );

        diff_api_surfaces(&old_wheel, &new_sdist)
            .expect("api diff should read inline-typed archives")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].module, "app");
    assert_eq!(report.changed[0].symbol, "parse");
}

#[test]
fn diff_api_surfaces_ignores_false_archive_typed_markers() {
    let project_dir = temp_project_dir("diff_api_surfaces_ignores_false_archive_typed_markers");
    let (wheel_report, sdist_report) = {
        let old_wheel = project_dir.join("demo-0.1.0-py3-none-any.whl");
        let new_wheel = project_dir.join("demo-0.2.0-py3-none-any.whl");
        for (path, returns) in [(&old_wheel, "int"), (&new_wheel, "str")] {
            write_zip_archive(
                path,
                &[
                    ("app/fakepy.typed", ""),
                    ("demo-1.0.dist-info/py.typed", ""),
                    ("app/fake/mod.py", &format!("def parse() -> {returns}:\n    return 1\n")),
                ],
            );
        }

        let old_sdist = project_dir.join("demo-0.1.0.tar.gz");
        let new_sdist = project_dir.join("demo-0.2.0.tar.gz");
        for (path, root, returns) in
            [(&old_sdist, "demo-0.1.0", "int"), (&new_sdist, "demo-0.2.0", "str")]
        {
            write_tar_gz_archive(
                path,
                root,
                &[
                    ("app/copy.typed", ""),
                    ("demo-1.0.dist-info/py.typed", ""),
                    ("app/fake/mod.py", &format!("def parse() -> {returns}:\n    return 1\n")),
                ],
            );
        }

        (
            diff_api_surfaces(&old_wheel, &new_wheel)
                .expect("false wheel markers should be ignored"),
            diff_api_surfaces(&old_sdist, &new_sdist)
                .expect("false sdist markers should be ignored"),
        )
    };
    remove_temp_project_dir(&project_dir);

    for report in [wheel_report, sdist_report] {
        assert!(report.added.is_empty());
        assert!(report.removed.is_empty());
        assert!(report.changed.is_empty());
    }
}

#[test]
fn diff_api_surfaces_accepts_source_directory_inputs_when_stubs_are_absent() {
    let project_dir = temp_project_dir("diff_api_surfaces_accepts_source_directory_inputs");
    let report = {
        let old_dir = project_dir.join("old/src");
        let new_dir = project_dir.join("new/src");
        fs::create_dir_all(old_dir.join("app")).expect("old source package should be created");
        fs::create_dir_all(new_dir.join("app")).expect("new source package should be created");
        fs::write(
            old_dir.join("app/__init__.py"),
            "def parse(value: str) -> int:\n    return 1\n\nclass User:\n    pass\n",
        )
        .expect("old source should be written");
        fs::write(
            new_dir.join("app/__init__.py"),
            "def parse(value: str) -> str:\n    return \"1\"\n\nclass User:\n    pass\n",
        )
        .expect("new source should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should read source dirs")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].module, "app");
    assert_eq!(report.changed[0].symbol, "parse");
}

#[test]
fn diff_api_surfaces_merges_partial_stub_and_source_modules() {
    let project_dir = temp_project_dir("diff_api_surfaces_merges_partial_stub_and_source_modules");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        for root in [&old_dir, &new_dir] {
            fs::create_dir_all(root.join("app")).expect("package should be created");
            fs::write(root.join("app/__init__.pyi"), "def stable() -> None: ...\n")
                .expect("stub should be written");
        }
        fs::write(old_dir.join("app/runtime.py"), "def parse(value: str) -> int:\n    return 1\n")
            .expect("old runtime source should be written");
        fs::write(
            new_dir.join("app/runtime.py"),
            "def parse(value: str) -> str:\n    return \"1\"\n",
        )
        .expect("new runtime source should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should merge partial stub trees")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].module, "app.runtime");
    assert_eq!(report.changed[0].symbol, "parse");
}

#[test]
fn diff_api_surfaces_prefers_stub_for_the_same_module() {
    let project_dir = temp_project_dir("diff_api_surfaces_prefers_stub_for_the_same_module");
    let report = {
        let old_dir = project_dir.join("old/app");
        let new_dir = project_dir.join("new/app");
        fs::create_dir_all(&old_dir).expect("old package should be created");
        fs::create_dir_all(&new_dir).expect("new package should be created");
        fs::write(old_dir.join("__init__.pyi"), "def parse() -> int: ...\n")
            .expect("old stub should be written");
        fs::write(new_dir.join("__init__.pyi"), "def parse() -> int: ...\n")
            .expect("new stub should be written");
        fs::write(old_dir.join("__init__.py"), "def parse():\n    return 1\n")
            .expect("old runtime should be written");
        fs::write(new_dir.join("__init__.py"), "def parse():\n    return \"changed\"\n")
            .expect("new runtime should be written");

        diff_api_surfaces(&project_dir.join("old"), &project_dir.join("new"))
            .expect("api diff should prefer matching stubs")
    };
    remove_temp_project_dir(&project_dir);

    assert!(report.added.is_empty());
    assert!(report.removed.is_empty());
    assert!(report.changed.is_empty());
}

#[test]
fn diff_api_surfaces_accepts_typepython_source_directory_inputs() {
    let project_dir =
        temp_project_dir("diff_api_surfaces_accepts_typepython_source_directory_inputs");
    let report = {
        let old_dir = project_dir.join("old/src");
        let new_dir = project_dir.join("new/src");
        fs::create_dir_all(&old_dir).expect("old source dir should be created");
        fs::create_dir_all(&new_dir).expect("new source dir should be created");
        fs::write(old_dir.join("models.tpy"), "data class Removed:\n    id: int\n")
            .expect("old tpy source should be written");
        fs::write(
            new_dir.join("models.tpy"),
            "interface Added:\n    def render(self) -> str: ...\n",
        )
        .expect("new tpy source should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should read tpy source dirs")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.removed[0].symbol, "Removed");
    assert_eq!(report.added[0].symbol, "Added");
}

#[test]
fn diff_api_surfaces_canonicalizes_multiline_overload_signatures() {
    let project_dir =
        temp_project_dir("diff_api_surfaces_canonicalizes_multiline_overload_signatures");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(
            old_dir.join("app.pyi"),
            concat!(
                "from typing import overload\n\n",
                "@overload\n",
                "def parse(\n",
                "    value: str,\n",
                ") -> int: ...\n",
                "@overload\n",
                "def parse(value: bytes, /) -> int: ...\n",
            ),
        )
        .expect("old stub should be written");
        fs::write(
            new_dir.join("app.pyi"),
            concat!(
                "from typing import overload\n\n",
                "@overload\n",
                "def parse(value: str) -> int: ...\n",
                "@overload\n",
                "def parse(\n",
                "    value: bytes,\n",
                "    /,\n",
                ") -> int: ...\n",
            ),
        )
        .expect("new stub should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should parse overloads")
    };
    remove_temp_project_dir(&project_dir);

    assert!(report.added.is_empty());
    assert!(report.removed.is_empty());
    assert!(report.changed.is_empty());
}

#[test]
fn diff_api_surfaces_preserves_semantic_tuple_commas_in_annotations() {
    let project_dir =
        temp_project_dir("diff_api_surfaces_preserves_semantic_tuple_commas_in_annotations");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(
            old_dir.join("app.pyi"),
            "from typing import Literal\ndef choose(value: Literal[(1,)]) -> int: ...\n",
        )
        .expect("old stub should be written");
        fs::write(
            new_dir.join("app.pyi"),
            "from typing import Literal\ndef choose(value: Literal[1]) -> int: ...\n",
        )
        .expect("new stub should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should retain tuple semantics")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].symbol, "choose");
}

#[test]
fn diff_api_surfaces_compares_every_overload_variant() {
    let project_dir = temp_project_dir("diff_api_surfaces_compares_every_overload_variant");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(
            old_dir.join("app.pyi"),
            concat!(
                "from typing import overload as _overload\n",
                "@_overload\n",
                "def parse(value: str) -> int: ...\n",
                "@_overload\n",
                "def parse(value: bytes) -> int: ...\n",
            ),
        )
        .expect("old stub should be written");
        fs::write(
            new_dir.join("app.pyi"),
            concat!(
                "from typing import overload as _overload\n",
                "@_overload\n",
                "def parse(value: str) -> int: ...\n",
                "@_overload\n",
                "def parse(value: bytes) -> str: ...\n",
            ),
        )
        .expect("new stub should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should compare overloads")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].symbol, "parse");
    assert!(report.changed[0].old_signature.as_deref().is_some_and(|signature| {
        signature.contains("parse(value: str)") && signature.contains("parse(value: bytes)")
    }));
}

#[test]
fn diff_api_surfaces_reports_class_method_property_and_attribute_changes() {
    let project_dir =
        temp_project_dir("diff_api_surfaces_reports_class_method_property_and_attribute_changes");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(
            old_dir.join("client.pyi"),
            concat!(
                "class Client:\n",
                "    endpoint: str\n",
                "    @property\n",
                "    def status(self) -> str: ...\n",
                "    @status.setter\n",
                "    def status(self, value: str) -> None: ...\n",
                "    def __init__(self) -> None:\n",
                "        self.token: str\n",
                "    def request(self, path: str) -> bytes: ...\n",
                "    def __enter__(self) -> Client: ...\n",
            ),
        )
        .expect("old stub should be written");
        fs::write(
            new_dir.join("client.pyi"),
            concat!(
                "class Client:\n",
                "    endpoint: bytes\n",
                "    @property\n",
                "    def status(self) -> str: ...\n",
                "    @status.setter\n",
                "    def status(self, value: bytes) -> None: ...\n",
                "    def __init__(self) -> None:\n",
                "        self.token: bytes\n",
                "    def __enter__(self) -> Client: ...\n",
            ),
        )
        .expect("new stub should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should compare class members")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(
        report.changed.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["Client.endpoint", "Client.status", "Client.token"]
    );
    assert_eq!(
        report.removed.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["Client.request"]
    );
    assert!(!report.changed.iter().any(|change| change.symbol == "Client.__enter__"));
}

#[test]
fn diff_api_surfaces_uses_static_all_for_private_and_reexported_names() {
    let project_dir =
        temp_project_dir("diff_api_surfaces_uses_static_all_for_private_and_reexported_names");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(
            old_dir.join("app.pyi"),
            concat!(
                "from typing import Any\n",
                "from ._core import Public as Public, _private\n",
                "EXPORTS = [\"Public\"]\n",
                "__all__ = EXPORTS + [\"_private\"]\n",
                "def hidden() -> Any: ...\n",
            ),
        )
        .expect("old stub should be written");
        fs::write(
            new_dir.join("app.pyi"),
            concat!(
                "from typing import Any\n",
                "from ._core import Public as Public, _private\n",
                "EXPORTS = [\"_private\"]\n",
                "__all__ = EXPORTS\n",
                "def hidden() -> Any: ...\n",
            ),
        )
        .expect("new stub should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should honor __all__")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(
        report.removed.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["Public"]
    );
    assert_eq!(report.removed[0].kind, "re-export");
    assert!(report.changed.is_empty());
    assert!(report.added.is_empty());
}

#[test]
fn diff_api_surfaces_tracks_stub_and_runtime_reexports() {
    let project_dir = temp_project_dir("diff_api_surfaces_tracks_stub_and_runtime_reexports");
    let (stub_report, runtime_report) = {
        let old_stubs = project_dir.join("old-stubs");
        let new_stubs = project_dir.join("new-stubs");
        let old_runtime = project_dir.join("old-runtime");
        let new_runtime = project_dir.join("new-runtime");
        for directory in [&old_stubs, &new_stubs, &old_runtime, &new_runtime] {
            fs::create_dir_all(directory).expect("surface dir should be created");
        }
        fs::write(old_stubs.join("app.pyi"), "from ._core import Public as Public\n")
            .expect("old stub should be written");
        fs::write(new_stubs.join("app.pyi"), "from .v2 import Public as Public\n")
            .expect("new stub should be written");
        fs::write(old_runtime.join("app.py"), "from ._core import Public\n")
            .expect("old runtime source should be written");
        fs::write(new_runtime.join("app.py"), "from .v2 import Public\n")
            .expect("new runtime source should be written");

        (
            diff_api_surfaces(&old_stubs, &new_stubs).expect("stub re-export should be compared"),
            diff_api_surfaces(&old_runtime, &new_runtime)
                .expect("runtime re-export should be compared"),
        )
    };
    remove_temp_project_dir(&project_dir);

    for report in [stub_report, runtime_report] {
        assert_eq!(report.changed.len(), 1);
        assert_eq!(report.changed[0].symbol, "Public");
        assert_eq!(report.changed[0].kind, "re-export");
    }
}

#[test]
fn diff_api_surfaces_compares_typepython_class_members() {
    let project_dir = temp_project_dir("diff_api_surfaces_compares_typepython_class_members");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(
            old_dir.join("models.tpy"),
            concat!(
                "data class User:\n",
                "    name: str\n",
                "    def render(self, prefix: str) -> str:\n",
                "        return prefix + self.name\n",
            ),
        )
        .expect("old TypePython source should be written");
        fs::write(
            new_dir.join("models.tpy"),
            concat!(
                "data class User:\n",
                "    name: bytes\n",
                "    def render(self, prefix: bytes) -> str:\n",
                "        return str(prefix)\n",
            ),
        )
        .expect("new TypePython source should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should compare TypePython members")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(
        report.changed.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["User.name", "User.render"]
    );
}

#[test]
fn diff_api_surfaces_reports_py_typed_metadata_regression() {
    let project_dir = temp_project_dir("diff_api_surfaces_reports_py_typed_metadata_regression");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(old_dir.join("app")).expect("old package should be created");
        fs::create_dir_all(new_dir.join("app")).expect("new package should be created");
        fs::write(old_dir.join("app/__init__.pyi"), "def parse(value: str) -> int: ...\n")
            .expect("old stub should be written");
        fs::write(new_dir.join("app/__init__.pyi"), "def parse(value: str) -> int: ...\n")
            .expect("new stub should be written");
        fs::write(old_dir.join("app/py.typed"), "").expect("old marker should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should detect metadata drift")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.removed.len(), 1);
    assert_eq!(report.removed[0].module, "__typing_metadata__");
    assert_eq!(report.removed[0].symbol, "py.typed");
    assert_eq!(report.removed[0].classification, "runtime-breaking signal");
    assert_eq!(
        report.release_notes,
        vec![String::from(
            "Runtime typing metadata changed: `py.typed` was removed; downstream tools may no longer treat the package as typed."
        )]
    );
    assert_eq!(report.semver_recommendation, "major");
}

#[test]
fn diff_api_surfaces_recommends_minor_for_additive_changes() {
    let project_dir = temp_project_dir("diff_api_surfaces_recommends_minor_for_additive_changes");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(old_dir.join("app.pyi"), "def parse(value: str) -> int: ...\n")
            .expect("old stub should be written");
        fs::write(
            new_dir.join("app.pyi"),
            "def parse(value: str) -> int: ...\ndef format(value: int) -> str: ...\n",
        )
        .expect("new stub should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should succeed")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(
        report.added.iter().map(|change| change.symbol.as_str()).collect::<Vec<_>>(),
        vec!["format"]
    );
    assert_eq!(report.semver_recommendation, "minor");
}

#[test]
fn diff_api_surfaces_classifies_precision_improvements_as_likely_type_compatible() {
    let project_dir = temp_project_dir("diff_api_surfaces_likely_type_compatible");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(old_dir.join("app.pyi"), "from typing import Any\ndef load() -> Any: ...\n")
            .expect("old stub should be written");
        fs::write(new_dir.join("app.pyi"), "def load() -> str: ...\n")
            .expect("new stub should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should succeed")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].classification, "likely type-compatible");
}

#[test]
fn diff_api_surfaces_classifies_new_any_as_likely_type_breaking() {
    let project_dir = temp_project_dir("diff_api_surfaces_new_any_type_breaking");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(old_dir.join("app.pyi"), "def load() -> str: ...\n")
            .expect("old stub should be written");
        fs::write(new_dir.join("app.pyi"), "from typing import Any\ndef load() -> Any: ...\n")
            .expect("new stub should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should succeed")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.changed.len(), 1);
    assert_eq!(report.changed[0].classification, "likely type-breaking");
}

#[test]
fn diff_api_surfaces_classifies_parameter_any_by_contravariance() {
    let project_dir = temp_project_dir("diff_api_surfaces_parameter_any_variance");
    let (narrowed, widened) = {
        let any_dir = project_dir.join("any");
        let str_dir = project_dir.join("str");
        fs::create_dir_all(&any_dir).expect("Any surface should be created");
        fs::create_dir_all(&str_dir).expect("str surface should be created");
        fs::write(
            any_dir.join("app.pyi"),
            "from typing import Any\ndef load(value: Any) -> int: ...\n",
        )
        .expect("Any surface should be written");
        fs::write(str_dir.join("app.pyi"), "def load(value: str) -> int: ...\n")
            .expect("str surface should be written");

        (
            diff_api_surfaces(&any_dir, &str_dir).expect("parameter narrowing should compare"),
            diff_api_surfaces(&str_dir, &any_dir).expect("parameter widening should compare"),
        )
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(narrowed.changed[0].classification, "likely type-breaking");
    assert_eq!(widened.changed[0].classification, "likely type-compatible");
}

#[test]
fn diff_api_surfaces_marks_mixed_any_variance_as_unknown_risk() {
    let project_dir = temp_project_dir("diff_api_surfaces_mixed_any_variance");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old surface should be created");
        fs::create_dir_all(&new_dir).expect("new surface should be created");
        fs::write(
            old_dir.join("app.pyi"),
            "from typing import Any\ndef load(value: Any) -> Any: ...\n",
        )
        .expect("old surface should be written");
        fs::write(new_dir.join("app.pyi"), "def load(value: str) -> str: ...\n")
            .expect("new surface should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("mixed variance should compare")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.changed[0].classification, "unknown risk");
}

#[test]
fn diff_api_surfaces_recommends_patch_for_unchanged_surfaces() {
    let project_dir = temp_project_dir("diff_api_surfaces_recommends_patch_for_unchanged_surfaces");
    let report = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(old_dir.join("app.pyi"), "def parse(value: str) -> int: ...\n")
            .expect("old stub should be written");
        fs::write(new_dir.join("app.pyi"), "def parse(value: str) -> int: ...\n")
            .expect("new stub should be written");

        diff_api_surfaces(&old_dir, &new_dir).expect("api diff should succeed")
    };
    remove_temp_project_dir(&project_dir);

    assert!(report.added.is_empty());
    assert!(report.removed.is_empty());
    assert!(report.changed.is_empty());
    assert_eq!(report.semver_recommendation, "patch");
}

#[test]
fn run_api_diff_fails_for_likely_breaking_changes() {
    let project_dir = temp_project_dir("run_api_diff_fails_for_likely_breaking_changes");
    let result = {
        let old_dir = project_dir.join("old");
        let new_dir = project_dir.join("new");
        fs::create_dir_all(&old_dir).expect("old dir should be created");
        fs::create_dir_all(&new_dir).expect("new dir should be created");
        fs::write(old_dir.join("app.pyi"), "def parse(value: str) -> int: ...\n")
            .expect("old stub should be written");
        fs::write(new_dir.join("app.pyi"), "def parse(value: bytes) -> int: ...\n")
            .expect("new stub should be written");

        run_api_diff(ApiDiffArgs { old: old_dir, new: new_dir, format: super::OutputFormat::Json })
            .expect("api diff should run")
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(result, ExitCode::FAILURE);
}
