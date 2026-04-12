use super::*;

#[derive(Debug)]
pub(super) struct PreparedPipelineSyntax {
    pub(super) source_paths: Vec<PathBuf>,
    pub(super) syntax_trees: Vec<typepython_syntax::SyntaxTree>,
    pub(super) all_syntax_trees: Vec<typepython_syntax::SyntaxTree>,
}

pub(crate) fn load_syntax_trees(
    sources: &[DiscoveredSource],
    enable_conditional_returns: bool,
    target_python: &str,
) -> Result<Vec<typepython_syntax::SyntaxTree>> {
    sources
        .par_iter()
        .map(|source| {
            let mut source_file = SourceFile::from_path(&source.path)
                .with_context(|| format!("unable to read {}", source.path.display()))?;
            source_file.logical_module = source.logical_module.clone();
            Ok(typepython_syntax::parse_with_options(
                source_file,
                typepython_syntax::ParseOptions {
                    enable_conditional_returns,
                    target_python: typepython_syntax::ParsePythonVersion::parse(target_python),
                    target_platform: Some(typepython_syntax::ParseTargetPlatform::current()),
                },
            ))
        })
        .collect::<Result<Vec<_>>>()
}

pub(super) fn prepare_pipeline_syntax(
    config: &ConfigHandle,
    discovery_sources: &[DiscoveredSource],
) -> Result<PreparedPipelineSyntax> {
    let source_paths: Vec<_> = discovery_sources.iter().map(|source| source.path.clone()).collect();
    let syntax_trees = load_syntax_trees(
        discovery_sources,
        config.config.typing.conditional_returns,
        &config.config.project.target_python.to_string(),
    )?;
    let shadow_stub_syntax = if config.config.typing.infer_passthrough {
        let shadow_stub_syntax = inferred_shadow_stub_syntax_trees(
            &syntax_trees,
            config.config.typing.conditional_returns,
            &config.config.project.target_python.to_string(),
        )?;
        if !shadow_stub_syntax.is_empty() {
            let cache_root =
                config.resolve_relative_path(&config.config.project.cache_dir).join("shadow-stubs");
            write_shadow_stub_cache(&cache_root, &shadow_stub_syntax)?;
        }
        shadow_stub_syntax
    } else {
        Vec::new()
    };
    let mut all_syntax_trees =
        if config.config.typing.infer_passthrough && !shadow_stub_syntax.is_empty() {
            replace_local_python_surfaces_with_shadow_stubs(&syntax_trees, shadow_stub_syntax)
        } else {
            syntax_trees.clone()
        };
    let checking_support_syntax = load_support_syntax_trees(config, &all_syntax_trees)?;
    all_syntax_trees.extend(checking_support_syntax);

    Ok(PreparedPipelineSyntax { source_paths, syntax_trees, all_syntax_trees })
}

fn load_support_syntax_trees(
    config: &ConfigHandle,
    surface_syntax_trees: &[typepython_syntax::SyntaxTree],
) -> Result<Vec<typepython_syntax::SyntaxTree>> {
    let project_modules = surface_syntax_trees
        .iter()
        .map(|tree| tree.source.logical_module.clone())
        .collect::<BTreeSet<_>>();
    let import_paths = collect_import_source_paths(surface_syntax_trees);
    let external_import_paths = import_paths
        .into_iter()
        .filter(|import_path| !import_resolves_within_modules(import_path, &project_modules))
        .collect::<Vec<_>>();
    if external_import_paths.is_empty() {
        return Ok(Vec::new());
    }

    let support_index = support_source_index(config, &config.config.project.target_python.to_string())?;

    let mut queued_modules = BTreeSet::new();
    let mut queue = VecDeque::new();
    for import_path in external_import_paths {
        for module_key in support_index.matching_module_keys(&import_path) {
            if queued_modules.insert(module_key.clone()) {
                queue.push_back(module_key);
            }
        }
    }

    let mut loaded_modules = BTreeSet::new();
    let mut loaded_paths = BTreeSet::new();
    let mut support_syntax_trees = Vec::new();

    while let Some(module_key) = queue.pop_front() {
        if !loaded_modules.insert(module_key.clone()) {
            continue;
        }
        let Some(module_sources) = support_index.module_sources(&module_key) else {
            continue;
        };

        for source in module_sources.iter().cloned() {
            if !loaded_paths.insert(source.path.clone()) {
                continue;
            }

            let tree = if source.load_as_inferred_stub {
                let runtime_source = fs::read_to_string(&source.path)
                    .with_context(|| format!("unable to read {}", source.path.display()))?;
                let stub_source =
                    generate_inferred_stub_source(&runtime_source, InferredStubMode::Shadow)
                        .with_context(|| {
                            format!(
                                "unable to synthesize shadow stub for {}",
                                source.path.display()
                            )
                        })?;
                typepython_syntax::parse_with_options(
                    SourceFile {
                        path: source.path.clone(),
                        kind: SourceKind::Stub,
                        logical_module: source.logical_module.clone(),
                        text: stub_source,
                    },
                    typepython_syntax::ParseOptions {
                        enable_conditional_returns: config.config.typing.conditional_returns,
                        target_python: typepython_syntax::ParsePythonVersion::parse(
                            &config.config.project.target_python.to_string(),
                        ),
                        target_platform: Some(typepython_syntax::ParseTargetPlatform::current()),
                    },
                )
            } else {
                let mut source_file = SourceFile::from_path(&source.path)
                    .with_context(|| format!("unable to read {}", source.path.display()))?;
                source_file.logical_module = source.logical_module.clone();
                typepython_syntax::parse_with_options(
                    source_file,
                    typepython_syntax::ParseOptions {
                        enable_conditional_returns: config.config.typing.conditional_returns,
                        target_python: typepython_syntax::ParsePythonVersion::parse(
                            &config.config.project.target_python.to_string(),
                        ),
                        target_platform: Some(typepython_syntax::ParseTargetPlatform::current()),
                    },
                )
            };
            for import_path in collect_import_source_paths(std::slice::from_ref(&tree)) {
                for nested_module_key in support_index.matching_module_keys(&import_path) {
                    if queued_modules.insert(nested_module_key.clone()) {
                        queue.push_back(nested_module_key);
                    }
                }
            }
            support_syntax_trees.push(tree);
        }
    }

    support_syntax_trees.sort_by(|left, right| left.source.path.cmp(&right.source.path));
    Ok(support_syntax_trees)
}

fn collect_import_source_paths(syntax_trees: &[typepython_syntax::SyntaxTree]) -> Vec<String> {
    syntax_trees
        .iter()
        .flat_map(|tree| tree.statements.iter())
        .filter_map(|statement| match statement {
            typepython_syntax::SyntaxStatement::Import(statement) => Some(
                statement
                    .bindings
                    .iter()
                    .map(|binding| binding.source_path.clone())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect()
}

fn import_resolves_within_modules(import_path: &str, module_keys: &BTreeSet<String>) -> bool {
    module_path_prefixes(import_path).any(|module_key| module_keys.contains(module_key))
}

fn module_path_prefixes(import_path: &str) -> impl Iterator<Item = &str> {
    let mut candidates = Vec::new();
    let mut current = import_path.strip_suffix(".*").unwrap_or(import_path);
    loop {
        if !current.is_empty() {
            candidates.push(current);
        }
        let Some((parent, _)) = current.rsplit_once('.') else {
            break;
        };
        current = parent;
    }
    candidates.into_iter()
}
