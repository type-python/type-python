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
                "Review required: changed function `parse` in module `app` from `def parse(value: str) -> int: ...` to `def parse(value: bytes) -> int: ...`."
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
