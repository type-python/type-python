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
fn plan_watch_target_update_adds_removes_and_reconfigures_modes() {
    let active = BTreeMap::from([
        (PathBuf::from("/repo/deleted"), RecursiveMode::Recursive),
        (PathBuf::from("/repo/mode"), RecursiveMode::Recursive),
        (PathBuf::from("/repo/stable"), RecursiveMode::Recursive),
    ]);
    let desired = vec![
        (PathBuf::from("/repo/stable"), RecursiveMode::Recursive),
        (PathBuf::from("/repo/new"), RecursiveMode::Recursive),
        (PathBuf::from("/repo/mode"), RecursiveMode::NonRecursive),
    ];

    let plan = plan_watch_target_update(&active, &desired);

    assert_eq!(plan.unwatch, vec![PathBuf::from("/repo/deleted"), PathBuf::from("/repo/mode")]);
    assert_eq!(
        plan.watch,
        vec![
            (PathBuf::from("/repo/mode"), RecursiveMode::NonRecursive),
            (PathBuf::from("/repo/new"), RecursiveMode::Recursive),
        ]
    );
}

#[test]
fn run_watch_rebuild_reloads_project_and_recovers_after_checker_failure() {
    let project_dir =
        temp_project_dir("run_watch_rebuild_reloads_project_and_recovers_after_checker_failure");
    let result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        let source_path = project_dir.join("src/app.tpy");
        fs::write(&source_path, "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");

        let first = run_watch_rebuild(
            Some(&project_dir),
            OutputFormat::Json,
            vec![String::from("initial watch check")],
        )
        .expect("initial rebuild should run");

        fs::write(&source_path, "def build() -> int:\n    return \"oops\"\n")
            .expect("test setup should succeed");
        let failed = run_watch_rebuild(
            Some(&project_dir),
            OutputFormat::Json,
            vec![String::from("failing watch check")],
        )
        .expect("type errors should return a failing exit code, not abort watch");

        fs::write(&source_path, "def build() -> int:\n    return 2\n")
            .expect("test setup should succeed");
        let recovered = run_watch_rebuild(
            Some(&project_dir),
            OutputFormat::Json,
            vec![String::from("recovered watch check")],
        )
        .expect("subsequent rebuild should recover");

        (first, failed, recovered)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(result.0.exit_code, ExitCode::SUCCESS);
    assert_eq!(result.1.exit_code, ExitCode::FAILURE);
    assert_eq!(result.2.exit_code, ExitCode::SUCCESS);
}

#[test]
fn run_watch_rebuild_recovers_after_config_load_failure() {
    let project_dir = temp_project_dir("run_watch_rebuild_recovers_after_config_load_failure");
    let result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");

        let first = run_watch_rebuild(
            Some(&project_dir),
            OutputFormat::Json,
            vec![String::from("initial watch check")],
        )
        .expect("initial rebuild should run");

        fs::write(project_dir.join("typepython.toml"), "[project\n")
            .expect("test setup should corrupt config");
        let failed = run_watch_rebuild(
            Some(&project_dir),
            OutputFormat::Json,
            vec![String::from("broken watch config")],
        )
        .expect_err("invalid config should report an error without poisoning watch state")
        .to_string();

        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should restore config");
        let recovered = run_watch_rebuild(
            Some(&project_dir),
            OutputFormat::Json,
            vec![String::from("recovered watch config")],
        )
        .expect("subsequent rebuild should recover");

        (first, failed, recovered)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(result.0.exit_code, ExitCode::SUCCESS);
    assert!(result.1.contains("unable to load TypePython project configuration"));
    assert_eq!(result.2.exit_code, ExitCode::SUCCESS);
}

#[test]
fn run_watch_rebuild_returns_reloaded_config_for_watch_reconfiguration() {
    let project_dir =
        temp_project_dir("run_watch_rebuild_returns_reloaded_config_for_watch_reconfiguration");
    let result = {
        fs::create_dir_all(project_dir.join("src")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\"]\n\n[watch]\ndebounce_ms = 40\n",
        )
        .expect("test setup should succeed");
        fs::write(project_dir.join("src/app.tpy"), "def build() -> int:\n    return 1\n")
            .expect("test setup should succeed");

        let first = run_watch_rebuild(
            Some(&project_dir),
            OutputFormat::Json,
            vec![String::from("initial watch config")],
        )
        .expect("initial rebuild should run");

        fs::create_dir_all(project_dir.join("generated")).expect("test setup should succeed");
        fs::write(
            project_dir.join("typepython.toml"),
            "[project]\nsrc = [\"src\", \"generated\"]\n\n[watch]\ndebounce_ms = 125\n",
        )
        .expect("test setup should succeed");
        let reloaded = run_watch_rebuild(
            Some(&project_dir),
            OutputFormat::Json,
            vec![String::from("updated watch config")],
        )
        .expect("updated rebuild should run");
        let reloaded_targets = watch_targets(&reloaded.config);

        (first, reloaded, reloaded_targets)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(result.0.config.config.watch.debounce_ms, 40);
    assert_eq!(result.1.config.config.watch.debounce_ms, 125);
    assert!(
        result
            .2
            .iter()
            .any(|(path, mode)| path.ends_with("generated") && *mode == RecursiveMode::Recursive)
    );
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
    assert_eq!(report.high_impact_untyped_files[0].downstream_public_importers, 1);
    assert!(report.high_impact_untyped_files[0].impact_score > 0);
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
fn build_migration_budget_baseline_records_public_any_unknown_and_untyped_imports() {
    let project_dir = temp_project_dir("build_migration_budget_baseline_records_type_debt");
    let baseline = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "import thirdparty.api\n\nPUBLIC_ANY: Any = None\nPUBLIC_UNKNOWN: unknown = None\n",
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
        let report = build_migration_report(&config, &syntax_trees);
        build_migration_budget_baseline(&DiagnosticReport::default(), &report)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(baseline.version, 1);
    assert_eq!(baseline.public_any.len(), 1);
    assert_eq!(baseline.public_any[0].symbol, "PUBLIC_ANY: Any");
    assert_eq!(baseline.public_unknown.len(), 1);
    assert_eq!(baseline.public_unknown[0].symbol, "PUBLIC_UNKNOWN: unknown");
    assert_eq!(baseline.untyped_imports.len(), 1);
    assert!(
        baseline
            .checker_portability_issues
            .iter()
            .any(|issue| { issue.contains("PUBLIC_ANY") && issue.contains("Any/dynamic") })
    );
    assert!(
        baseline
            .checker_portability_issues
            .iter()
            .any(|issue| { issue.contains("PUBLIC_UNKNOWN") && issue.contains("Unknown") })
    );
    assert!(
        baseline
            .checker_portability_issues
            .iter()
            .any(|issue| { issue.contains("untyped import") })
    );
    assert!(baseline.path_budgets.iter().any(|budget| {
        budget.path.ends_with("src/app/__init__.tpy")
            && budget.category == "library public surface"
            && budget.max_public_any == 1
            && budget.max_public_unknown == 1
            && budget.max_untyped_imports == 1
    }));
}

#[test]
fn compare_migration_budget_baseline_reports_new_public_any_and_unknown() {
    let baseline = MigrationBudgetBaseline {
        version: 1,
        diagnostics: Vec::new(),
        public_any: vec![MigrationPublicTypeDebtEntry {
            path: String::from("src/app/old.tpy"),
            symbol: String::from("OLD: Any"),
            kind: String::from("public-export"),
        }],
        public_unknown: Vec::new(),
        untyped_imports: Vec::new(),
        dynamic_framework_boundaries: Vec::new(),
        checker_portability_issues: Vec::new(),
        path_budgets: Vec::new(),
    };
    let current = MigrationBudgetBaseline {
        public_any: vec![
            MigrationPublicTypeDebtEntry {
                path: String::from("src/app/old.tpy"),
                symbol: String::from("OLD: Any"),
                kind: String::from("public-export"),
            },
            MigrationPublicTypeDebtEntry {
                path: String::from("src/app/new.tpy"),
                symbol: String::from("NEW: Any"),
                kind: String::from("public-export"),
            },
        ],
        public_unknown: vec![MigrationPublicTypeDebtEntry {
            path: String::from("src/app/new.tpy"),
            symbol: String::from("MISSING: unknown"),
            kind: String::from("public-export"),
        }],
        ..baseline.clone()
    };

    let comparison = compare_migration_budget_baseline(
        String::from(".typepython/type-budget.json"),
        &baseline,
        &current,
    );

    assert_eq!(comparison.baseline_public_any, 1);
    assert_eq!(comparison.current_public_any, 2);
    assert_eq!(comparison.new_public_any.len(), 1);
    assert_eq!(comparison.new_public_unknown.len(), 1);
}

#[test]
fn migration_dashboard_outputs_include_annotations_sarif_and_trends() {
    let mut diagnostics = DiagnosticReport::default();
    diagnostics.push(
        Diagnostic::warning("TPY9999", "sample warning")
            .with_span(typepython_diagnostics::Span::new("src/app.py", 3, 5, 3, 12)),
    );

    let annotations = migration_ci_annotations(&diagnostics);
    let sarif = migration_sarif(&diagnostics);

    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].path, "src/app.py");
    assert_eq!(annotations[0].line, 3);
    assert_eq!(annotations[0].code, "TPY9999");
    assert_eq!(sarif["version"], "2.1.0");
    assert_eq!(sarif["runs"][0]["results"][0]["ruleId"], "TPY9999");

    let project_dir = temp_project_dir("migration_dashboard_outputs_include_trends");
    let trend = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "def typed(value: int) -> int:\n    return value\n\ndef untyped(value):\n    return value\n\nPUBLIC_UNKNOWN: unknown = None\n",
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
        let report = build_migration_report(&config, &syntax_trees);
        migration_trend_entry(&report)
    };
    remove_temp_project_dir(&project_dir);

    assert!(trend.coverage_percent < 100.0);
    assert!(trend.type_debt_total > 0);
}

#[test]
fn run_migrate_writes_budget_baseline_and_gates_new_public_any() {
    let project_dir = temp_project_dir("run_migrate_writes_budget_baseline_and_gates_any");
    let (write_result, gate_result, baseline_text) = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(project_dir.join("src/app/__init__.tpy"), "PUBLIC_ANY: Any = None\n")
            .expect("test setup should succeed");
        let baseline_path = PathBuf::from(".typepython/type-budget.json");
        let write_result = run_migrate(MigrateArgs {
            run: RunArgs { project: Some(project_dir.clone()), format: OutputFormat::Json },
            report: true,
            baseline: None,
            write_baseline: None,
            no_new_diagnostics: false,
            budget_baseline: None,
            write_budget_baseline: Some(baseline_path.clone()),
            no_new_public_any: false,
            no_new_public_unknown: false,
            emit_stubs: Vec::new(),
            stub_out_dir: None,
        })
        .expect("migrate should write budget baseline");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "PUBLIC_ANY: Any = None\nNEW_ANY: Any = None\n",
        )
        .expect("test setup should update source");
        let gate_result = run_migrate(MigrateArgs {
            run: RunArgs { project: Some(project_dir.clone()), format: OutputFormat::Json },
            report: true,
            baseline: None,
            write_baseline: None,
            no_new_diagnostics: false,
            budget_baseline: Some(baseline_path.clone()),
            write_budget_baseline: None,
            no_new_public_any: true,
            no_new_public_unknown: false,
            emit_stubs: Vec::new(),
            stub_out_dir: None,
        })
        .expect("migrate should compare budget baseline");
        let baseline_text = fs::read_to_string(project_dir.join(baseline_path))
            .expect("budget baseline should be written");
        (write_result, gate_result, baseline_text)
    };
    remove_temp_project_dir(&project_dir);

    assert_eq!(write_result, ExitCode::SUCCESS);
    assert_eq!(gate_result, ExitCode::FAILURE);
    assert!(baseline_text.contains("public_any"));
    assert!(baseline_text.contains("PUBLIC_ANY: Any"));
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
fn build_migration_report_tracks_inline_suppressions() {
    let project_dir = temp_project_dir("build_migration_report_tracks_inline_suppressions");
    let report = {
        fs::create_dir_all(project_dir.join("src/app")).expect("test setup should succeed");
        fs::write(project_dir.join("typepython.toml"), "[project]\nsrc = [\"src\"]\n")
            .expect("test setup should succeed");
        fs::write(
            project_dir.join("src/app/__init__.tpy"),
            "value: int = \"x\"  # type: ignore[TPY4001]\nother = 1  # type: ignore\n",
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

    assert_eq!(report.inline_suppression_files.len(), 1);
    let entry = &report.inline_suppression_files[0];
    assert!(entry.path.ends_with("src/app/__init__.tpy"));
    assert_eq!(entry.suppression_count, 2);
    assert_eq!(entry.directives[0].line, 1);
    assert_eq!(entry.directives[0].codes, Some(vec![String::from("TPY4001")]));
    assert_eq!(entry.directives[1].line, 2);
    assert_eq!(entry.directives[1].codes, None);
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
fn migrate_command_parses_budget_baseline_flags() {
    let cli = Cli::parse_from([
        "typepython",
        "migrate",
        "--project",
        "examples/hello-world",
        "--budget-baseline",
        ".typepython/type-budget.json",
        "--write-budget-baseline",
        ".typepython/type-budget-new.json",
        "--no-new-public-any",
        "--no-new-public-unknown",
    ]);

    let super::Command::Migrate(args) = cli.command else {
        panic!("expected migrate command");
    };

    assert_eq!(args.budget_baseline, Some(PathBuf::from(".typepython/type-budget.json")));
    assert_eq!(args.write_budget_baseline, Some(PathBuf::from(".typepython/type-budget-new.json")));
    assert!(args.no_new_public_any);
    assert!(args.no_new_public_unknown);
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
    assert_eq!(comparison.blocking_new_diagnostics, 1);
    assert_eq!(comparison.new_diagnostics.len(), 1);
    assert_eq!(comparison.new_diagnostics[0].code, "TPY3001");
    assert_eq!(comparison.resolved_diagnostics.len(), 1);
    assert_eq!(comparison.resolved_diagnostics[0].code, "TPY2001");
}

#[test]
fn migration_diagnostic_baseline_applies_severity_overrides() {
    let mut baseline_report = DiagnosticReport::default();
    baseline_report.push(Diagnostic::error("TPY1001", "baseline parse error"));
    let mut current_report = DiagnosticReport::default();
    current_report.push(Diagnostic::error("TPY3001", "new import debt"));

    let mut baseline = build_migration_diagnostic_baseline(&baseline_report);
    baseline.severity_overrides.insert(String::from("TPY3001"), String::from("warning"));
    let current = build_migration_diagnostic_baseline(&current_report);
    let comparison = compare_migration_diagnostic_baseline(
        String::from(".typepython/migration-baseline.json"),
        &baseline,
        &current,
    );

    assert_eq!(comparison.new_diagnostics.len(), 1);
    assert_eq!(comparison.blocking_new_diagnostics, 0);
    assert_eq!(comparison.new_diagnostics[0].severity, "warning");
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
