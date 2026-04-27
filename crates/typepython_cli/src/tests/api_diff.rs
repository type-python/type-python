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
