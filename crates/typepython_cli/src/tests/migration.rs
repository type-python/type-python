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
        let syntax_trees =
            load_syntax_trees(
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
        let syntax_trees =
            load_syntax_trees(
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
    assert_eq!(args.emit_stubs, vec![PathBuf::from("src/app"), PathBuf::from("src/lib.py")]);
    assert_eq!(args.stub_out_dir, Some(PathBuf::from(".generated-stubs")));
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
