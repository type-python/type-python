use super::*;

#[test]
fn watch_targets_include_config_and_existing_source_roots() {
    let project_dir = temp_project_dir("watch_targets_include_config_and_existing_source_roots");
    let targets = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        watch_targets(&config)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(targets.len(), 2);
    assert!(targets.iter().any(|(path, mode)| {
        path.ends_with("typepython.toml") && *mode == RecursiveMode::NonRecursive
    }));
    assert!(
        targets
            .iter()
            .any(|(path, mode)| path.ends_with("src") && *mode == RecursiveMode::Recursive)
    );
}

#[test]
fn format_watch_rebuild_note_summarizes_changed_paths() {
    let changed = BTreeSet::from([
        PathBuf::from("src/app/__init__.tpy"),
        PathBuf::from("src/app/models.tpy"),
        PathBuf::from("src/app/views.tpy"),
        PathBuf::from("src/app/more.tpy"),
    ]);

    let note = format_watch_rebuild_note(&changed);
    assert!(note.contains("rebuild triggered by"));
    assert!(note.contains("and 1 more path(s)"));
}

#[test]
fn build_migration_report_counts_file_coverage_and_boundaries() {
    let project_dir =
        temp_project_dir("build_migration_report_counts_file_coverage_and_boundaries");
    let report = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "def typed(value: int) -> int:\n    return value\n\ndef untyped(value) -> int:\n    return 0\n\nleak: dynamic = 1\n",
        ).expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let discovery = collect_source_paths(&config).expect("test setup should succeed");
        let syntax_trees = load_syntax_trees(
            &discovery.sources,
            false,
            &config.config.project.target_python.to_string(),
        )
        .expect("test setup should succeed");
        build_migration_report(&config, &syntax_trees)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.total_declarations, 3);
    assert_eq!(report.known_declarations, 1);
    assert_eq!(report.total_dynamic_boundaries, 1);
    assert_eq!(report.total_unknown_boundaries, 0);
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].known_declarations, 1);
}

#[test]
fn build_migration_report_ranks_high_impact_untyped_files() {
    let project_dir = temp_project_dir("build_migration_report_ranks_high_impact_untyped_files");
    let report = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app/__init__.tpy"), "pass\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app/a.tpy"), "def untyped(value) -> int:\n    return 0\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/b.tpy"),
            "from app.a import untyped\n\ndef use(value: int) -> int:\n    return value\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/c.tpy"),
            "def clean(value: int) -> int:\n    return value\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let discovery = collect_source_paths(&config).expect("test setup should succeed");
        let syntax_trees = load_syntax_trees(
            &discovery.sources,
            false,
            &config.config.project.target_python.to_string(),
        )
        .expect("test setup should succeed");
        build_migration_report(&config, &syntax_trees)
    };
    remove_temp_project_dir(&project_dir);

    assert!(!report.high_impact_untyped_files.is_empty());
    assert!(report.high_impact_untyped_files[0].path.ends_with("src/app/a.tpy"));
    assert_eq!(report.high_impact_untyped_files[0].downstream_references, 1);
}

#[test]
fn build_migration_report_flags_framework_transform_candidates() {
    let project_dir =
        temp_project_dir("build_migration_report_flags_framework_transform_candidates");
    let report = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/api.tpy"),
            "from fastapi import FastAPI\nfrom pydantic import BaseModel, Field\n\napp = FastAPI()\n\nclass User(BaseModel):\n    id: int = Field(alias=\"user_id\")\n\n@app.get(\"/users/{user_id}\")\ndef read_user(user_id: int) -> User:\n    return User(id=user_id)\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let discovery = collect_source_paths(&config).expect("test setup should succeed");
        let syntax_trees = load_syntax_trees(
            &discovery.sources,
            false,
            &config.config.project.target_python.to_string(),
        )
        .expect("test setup should succeed");
        build_migration_report(&config, &syntax_trees)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.framework_pattern_files.len(), 1);
    let entry = &report.framework_pattern_files[0];
    assert!(entry.path.ends_with("src/app/api.tpy"));
    assert!(entry.frameworks.contains(&String::from("fastapi")));
    assert!(entry.frameworks.contains(&String::from("pydantic")));
    assert!(entry.signals.iter().any(|signal| signal == "pydantic:BaseModel"));
    assert!(entry.signals.iter().any(|signal| signal == "fastapi:@app."));
}

#[test]
fn build_migration_report_tracks_public_api_annotation_completeness() {
    let project_dir =
        temp_project_dir("build_migration_report_tracks_public_api_annotation_completeness");
    let report = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "PUBLIC_VALUE: int = 1\n_hidden: int = 2\n\ndef typed(value: int) -> int:\n    return value\n\ndef untyped(value) -> int:\n    return 0\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let discovery = collect_source_paths(&config).expect("test setup should succeed");
        let syntax_trees = load_syntax_trees(
            &discovery.sources,
            false,
            &config.config.project.target_python.to_string(),
        )
        .expect("test setup should succeed");
        build_migration_report(&config, &syntax_trees)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.public_api_exports, 3);
    assert_eq!(report.known_public_api_exports, 2);
    assert_eq!(report.public_api_files.len(), 1);
    let entry = &report.public_api_files[0];
    assert!(entry.path.ends_with("src/app/__init__.tpy"));
    assert_eq!(entry.public_exports, 3);
    assert_eq!(entry.known_public_exports, 2);
    assert_eq!(entry.incomplete_exports, vec![String::from("untyped")]);
}

#[test]
fn build_migration_report_tracks_untyped_import_candidates() {
    let project_dir = temp_project_dir("build_migration_report_tracks_untyped_import_candidates");
    let report = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "from app.local import helper\nimport json\nimport thirdparty.api\n\ndef typed(value: int) -> int:\n    return helper(value)\n",
        )
        .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/local.tpy"),
            "def helper(value: int) -> int:\n    return value\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let discovery = collect_source_paths(&config).expect("test setup should succeed");
        let mut syntax_trees = load_syntax_trees(
            &discovery.sources,
            false,
            &config.config.project.target_python.to_string(),
        )
        .expect("test setup should succeed");
        let bundled_sources =
            crate::discovery::bundled_stdlib_sources(&config.analysis_python().to_string())
                .expect("stdlib sources should load");
        syntax_trees.extend(
            load_syntax_trees(
                &bundled_sources,
                false,
                &config.config.project.target_python.to_string(),
            )
            .expect("bundled syntax trees should load"),
        );
        build_migration_report(&config, &syntax_trees)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(report.untyped_import_files.len(), 1);
    let entry = &report.untyped_import_files[0];
    assert!(entry.path.ends_with("src/app/__init__.tpy"));
    assert_eq!(entry.untyped_import_count, 1);
    assert_eq!(entry.imports, vec![String::from("thirdparty.api")]);
}

#[test]
fn migrate_command_parses_emit_stubs_flags() {
    let cli = Cli::parse_from([
        "typepython",
        "migrate",
        "--project",
        "examples/hello-world",
        "--report",
        "--emit-stubs",
        "src/app",
        "--emit-stubs",
        "src/lib.py",
        "--stub-out-dir",
        ".generated-stubs",
    ]);

    let super::Command::Migrate(args) = cli.command else {
        panic!("expected migrate command");
    };

    assert!(args.report);
    assert_eq!(args.baseline, None);
    assert_eq!(args.write_baseline, None);
    assert!(!args.no_new_diagnostics);
    assert_eq!(args.emit_stubs, vec![PathBuf::from("src/app"), PathBuf::from("src/lib.py")]);
    assert_eq!(args.stub_out_dir, Some(PathBuf::from(".generated-stubs")));
}

#[test]
fn migrate_command_parses_diagnostic_baseline_flags() {
    let cli = Cli::parse_from([
        "typepython",
        "migrate",
        "--project",
        "examples/hello-world",
        "--baseline",
        ".typepython/migration-baseline.json",
        "--write-baseline",
        ".typepython/new-baseline.json",
        "--no-new-diagnostics",
    ]);

    let super::Command::Migrate(args) = cli.command else {
        panic!("expected migrate command");
    };

    assert_eq!(args.baseline, Some(PathBuf::from(".typepython/migration-baseline.json")));
    assert_eq!(args.write_baseline, Some(PathBuf::from(".typepython/new-baseline.json")));
    assert!(args.no_new_diagnostics);
}

#[test]
fn migration_diagnostic_baseline_reports_new_and_resolved_diagnostics() {
    let mut baseline_report = DiagnosticReport::default();
    baseline_report.push(Diagnostic::error("TPY1001", "old parse error").with_span(Span::new(
        "src/app/__init__.tpy",
        1,
        1,
        1,
        5,
    )));
    baseline_report.push(Diagnostic::warning("TPY2001", "resolved warning").with_span(Span::new(
        "src/app/legacy.tpy",
        2,
        1,
        2,
        5,
    )));
    let mut current_report = DiagnosticReport::default();
    current_report.push(Diagnostic::error("TPY1001", "old parse error").with_span(Span::new(
        "src/app/__init__.tpy",
        1,
        1,
        1,
        5,
    )));
    current_report.push(Diagnostic::error("TPY3001", "new type debt").with_span(Span::new(
        "src/app/new.tpy",
        3,
        1,
        3,
        5,
    )));

    let baseline = build_migration_diagnostic_baseline(&baseline_report);
    let current = build_migration_diagnostic_baseline(&current_report);
    let comparison = compare_migration_diagnostic_baseline(
        String::from(".typepython/migration-baseline.json"),
        &baseline,
        &current,
    );

    assert_eq!(comparison.baseline_diagnostics, 2);
    assert_eq!(comparison.current_diagnostics, 2);
    assert_eq!(comparison.new_diagnostics.len(), 1);
    assert_eq!(comparison.new_diagnostics[0].code, "TPY3001");
    assert_eq!(comparison.resolved_diagnostics.len(), 1);
    assert_eq!(comparison.resolved_diagnostics[0].code, "TPY2001");
}

#[test]
fn emit_migration_stubs_writes_generated_pyi_to_configured_output_dir() {
    let project_dir =
        temp_project_dir("emit_migration_stubs_writes_generated_pyi_to_configured_output_dir");
    let (written, stub) = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app/__init__.py"), "").expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/helpers.py"),
            "VALUE = 1\n\ndef parse(text):\n    return VALUE\n",
        )
        .expect("test setup should succeed");
        let config = load(&project_dir).expect("test setup should succeed");
        let discovery = collect_source_paths(&config).expect("test setup should succeed");
        let written = emit_migration_stubs(
            &config,
            &discovery.sources,
            &[PathBuf::from("src/app")],
            Some(Path::new(".generated-stubs")),
        )
        .expect("migration stub emission should succeed");
        let stub_path = project_dir.join(".generated-stubs/app/helpers.pyi");
        let stub =
            fs::read_to_string(&stub_path).expect("generated migration stub should be readable");

        (written, stub)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(
        written,
        vec![
            project_dir.join(".generated-stubs/app/__init__.pyi"),
            project_dir.join(".generated-stubs/app/helpers.pyi"),
        ]
    );
    assert!(stub.starts_with("# auto-generated by typepython migrate"));
    assert!(stub.contains("VALUE: int"));
    assert!(stub.contains("# TODO: add type annotation"));
    assert!(stub.contains("def parse(text: ...) -> int: ..."));
}
