use super::*;

/// Parses a source file into a syntax tree.
#[must_use]
pub fn parse(source: SourceFile) -> SyntaxTree {
    parse_with_options(source, ParseOptions::default())
}

/// Parses a source file with optional feature flags enabled.
#[must_use]
pub fn parse_with_options(source: SourceFile, options: ParseOptions) -> SyntaxTree {
    match source.kind {
        SourceKind::TypePython => parse_typepython_source(source, options),
        SourceKind::Python | SourceKind::Stub => parse_python_source(source, options),
    }
}

/// Normalizes source-authored annotated lambda syntax to ordinary Python lambda syntax.
///
/// This is intended for consumers that need runtime-parseable Python text after TypePython-only
/// lambda parameter annotations have already been checked semantically.
#[must_use]
pub fn normalize_annotated_lambda_source_for_emission(source: &str) -> String {
    normalize_annotated_lambda_source_lossy(source)
}

#[derive(Debug)]
pub(super) struct ActiveSourceLineIndex {
    pub(super) ptr: usize,
    pub(super) len: usize,
    pub(super) line_starts: Vec<usize>,
}

thread_local! {
    pub(super) static ACTIVE_SOURCE_LINE_INDICES: RefCell<Vec<ActiveSourceLineIndex>> =
        const { RefCell::new(Vec::new()) };
}

pub(super) fn build_source_line_starts(source: &str) -> Vec<usize> {
    let mut line_starts = vec![0];
    line_starts.extend(
        source
            .as_bytes()
            .iter()
            .enumerate()
            .filter_map(|(index, byte)| (*byte == b'\n').then_some(index + 1)),
    );
    line_starts
}

pub(super) fn offset_to_line_column_from_line_starts(
    source: &str,
    offset: usize,
    line_starts: &[usize],
) -> (usize, usize) {
    let clamped = offset.min(source.len());
    let line_index = line_starts.partition_point(|start| *start <= clamped).saturating_sub(1);
    let line_start = line_starts.get(line_index).copied().unwrap_or(0);
    let column =
        source.get(line_start..clamped).map(|segment| segment.chars().count() + 1).unwrap_or(1);
    (line_index + 1, column)
}

pub(super) fn with_source_line_index<T>(source: &str, action: impl FnOnce() -> T) -> T {
    struct LineIndexGuard;

    impl Drop for LineIndexGuard {
        fn drop(&mut self) {
            ACTIVE_SOURCE_LINE_INDICES.with(|active| {
                active.borrow_mut().pop();
            });
        }
    }

    ACTIVE_SOURCE_LINE_INDICES.with(|active| {
        active.borrow_mut().push(ActiveSourceLineIndex {
            ptr: source.as_ptr() as usize,
            len: source.len(),
            line_starts: build_source_line_starts(source),
        });
    });
    let _guard = LineIndexGuard;
    action()
}

#[must_use]
pub fn collect_typed_dict_literal_sites(source: &str) -> Vec<TypedDictLiteralSite> {
    let normalized = normalize_annotated_lambda_source_lossy(source);
    with_source_line_index(&normalized, || {
        let Ok(parsed) = parse_module(&normalized) else {
            return Vec::new();
        };

        let mut sites = Vec::new();
        collect_typed_dict_literal_sites_in_suite(
            &normalized,
            parsed.suite(),
            None,
            None,
            &mut sites,
        );
        sites
    })
}

#[must_use]
pub fn collect_direct_call_context_sites(source: &str) -> Vec<DirectCallContextSite> {
    let normalized = normalize_annotated_lambda_source_lossy(source);
    with_source_line_index(&normalized, || {
        let Ok(parsed) = parse_module(&normalized) else {
            return Vec::new();
        };

        let mut sites = Vec::new();
        collect_direct_call_context_sites_in_suite(
            &normalized,
            parsed.suite(),
            None,
            None,
            &mut sites,
        );
        sites
    })
}

#[must_use]
pub fn collect_expression_use_sites(source: &str) -> Vec<ExpressionUseSite> {
    let (normalized, skipped_statement_lines) = expression_use_sites_parse_source(source);
    with_source_line_index(&normalized, || {
        let Ok(parsed) = parse_module(&normalized) else {
            return Vec::new();
        };

        let mut sites = Vec::new();
        collect_expression_use_sites_in_suite(
            &normalized,
            parsed.suite(),
            None,
            None,
            &skipped_statement_lines,
            &mut sites,
        );
        sites
    })
}

#[must_use]
pub fn collect_nested_direct_call_context_sites(source: &str) -> Vec<DirectCallContextSite> {
    let normalized = normalize_annotated_lambda_source_lossy(source);
    with_source_line_index(&normalized, || {
        let Ok(parsed) = parse_module(&normalized) else {
            return Vec::new();
        };

        let mut sites = Vec::new();
        collect_nested_direct_call_context_sites_in_suite(
            &normalized,
            parsed.suite(),
            None,
            &mut sites,
        );
        sites
    })
}

#[must_use]
pub fn collect_typed_dict_mutation_sites(source: &str) -> Vec<TypedDictMutationSite> {
    let normalized = normalize_annotated_lambda_source_lossy(source);
    with_source_line_index(&normalized, || {
        let Ok(parsed) = parse_module(&normalized) else {
            return Vec::new();
        };

        let mut sites = Vec::new();
        collect_typed_dict_mutation_sites_in_suite(
            &normalized,
            parsed.suite(),
            None,
            None,
            &mut sites,
        );
        sites
    })
}

#[must_use]
pub fn collect_typed_dict_class_metadata(source: &str) -> Vec<TypedDictClassMetadata> {
    collect_module_surface_metadata(source).typed_dict_classes
}

#[must_use]
pub fn collect_module_surface_metadata(source: &str) -> ModuleSurfaceMetadata {
    let normalized = module_surface_metadata_parse_source(source);
    with_source_line_index(&normalized, || {
        let Ok(parsed) = parse_module(&normalized) else {
            return ModuleSurfaceMetadata::default();
        };

        let import_bindings = collect_import_bindings(parsed.suite());
        let mut typed_dict_classes = Vec::new();
        let mut dataclass_transform_providers = Vec::new();
        let mut dataclass_transform_classes = Vec::new();
        let mut framework_transform_providers = Vec::new();
        let mut direct_function_signatures = Vec::new();
        let mut direct_method_signatures = Vec::new();

        for stmt in parsed.suite() {
            match stmt {
                Stmt::FunctionDef(function) => {
                    if let Some(metadata) = dataclass_transform_metadata(
                        &normalized,
                        &function.decorator_list,
                        &import_bindings,
                    ) {
                        dataclass_transform_providers.push(DataclassTransformProviderSite {
                            name: function.name.as_str().to_owned(),
                            metadata,
                            line: offset_to_line_column(
                                &normalized,
                                function.range.start().to_usize(),
                            )
                            .0,
                        });
                    }
                    if let Some(provider) = framework_transform_provider_site(
                        &normalized,
                        function.name.as_str(),
                        &function.decorator_list,
                        &import_bindings,
                        offset_to_line_column(&normalized, function.range.start().to_usize()).0,
                    ) {
                        framework_transform_providers.push(provider);
                    }
                    direct_function_signatures.push(DirectFunctionSignatureSite {
                        name: function.name.as_str().to_owned(),
                        params: collect_direct_function_param_sites(
                            &normalized,
                            &function.parameters,
                        ),
                        line: offset_to_line_column(&normalized, function.range.start().to_usize())
                            .0,
                    });
                }
                Stmt::ClassDef(class_def) => {
                    typed_dict_classes.push(TypedDictClassMetadata {
                        name: class_def.name.as_str().to_owned(),
                        total: class_keyword_static_bool(class_def, "total"),
                        closed: class_keyword_static_bool(class_def, "closed"),
                        extra_items: class_keyword_source(&normalized, class_def, "extra_items")
                            .map(|annotation| TypedDictExtraItemsMetadata {
                                annotation_expr: TypeExpr::parse(&annotation),
                                annotation,
                            }),
                        line: offset_to_line_column(
                            &normalized,
                            class_def.range.start().to_usize(),
                        )
                        .0,
                    });
                    if let Some(metadata) = dataclass_transform_metadata(
                        &normalized,
                        &class_def.decorator_list,
                        &import_bindings,
                    ) {
                        dataclass_transform_providers.push(DataclassTransformProviderSite {
                            name: class_def.name.as_str().to_owned(),
                            metadata,
                            line: offset_to_line_column(
                                &normalized,
                                class_def.range.start().to_usize(),
                            )
                            .0,
                        });
                    }
                    if let Some(provider) = framework_transform_provider_site(
                        &normalized,
                        class_def.name.as_str(),
                        &class_def.decorator_list,
                        &import_bindings,
                        offset_to_line_column(&normalized, class_def.range.start().to_usize()).0,
                    ) {
                        framework_transform_providers.push(provider);
                    }
                    dataclass_transform_classes.push(collect_dataclass_transform_class_site(
                        &normalized,
                        class_def,
                        &import_bindings,
                    ));
                    direct_method_signatures.extend(class_def.body.iter().filter_map(|member| {
                        match member {
                            Stmt::FunctionDef(function) => Some(DirectMethodSignatureSite {
                                owner_type_name: class_def.name.as_str().to_owned(),
                                name: function.name.as_str().to_owned(),
                                method_kind: method_kind_from_decorators(&function.decorator_list),
                                params: collect_direct_function_param_sites(
                                    &normalized,
                                    &function.parameters,
                                ),
                                line: offset_to_line_column(
                                    &normalized,
                                    function.range.start().to_usize(),
                                )
                                .0,
                            }),
                            _ => None,
                        }
                    }));
                }
                _ => {}
            }
        }

        let mut decorated_callables = Vec::new();
        collect_decorated_callable_sites(
            &normalized,
            parsed.suite(),
            None,
            &import_bindings,
            &mut decorated_callables,
        );

        ModuleSurfaceMetadata {
            typed_dict_classes,
            dataclass_transform: DataclassTransformModuleInfo {
                providers: dataclass_transform_providers,
                classes: dataclass_transform_classes,
            },
            decorator_transform: DecoratorTransformModuleInfo { callables: decorated_callables },
            framework_transform: FrameworkTransformModuleInfo {
                providers: framework_transform_providers,
            },
            direct_function_signatures,
            direct_method_signatures,
        }
    })
}

fn module_surface_metadata_parse_source(source: &str) -> String {
    let normalized = normalize_annotated_lambda_source_lossy(source);
    if parse_module(&normalized).is_ok() {
        return normalized;
    }

    let tree = parse(SourceFile {
        path: PathBuf::from("<surface-metadata>.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: source.to_owned(),
    });
    normalize_annotated_lambda_source_lossy(&normalize_typepython_source(source, &tree.statements))
}

fn expression_use_sites_parse_source(source: &str) -> (String, std::collections::BTreeSet<usize>) {
    let normalized = normalize_annotated_lambda_source_lossy(source);
    if parse_module(&normalized).is_ok() {
        return (normalized, std::collections::BTreeSet::new());
    }

    let tree = parse(SourceFile {
        path: PathBuf::from("<expression-use-sites>.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: source.to_owned(),
    });
    let skipped_statement_lines = tree
        .statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::TypeAlias(statement) => Some(statement.line),
            _ => None,
        })
        .collect();
    (
        normalize_annotated_lambda_source_lossy(&normalize_typepython_source(
            source,
            &tree.statements,
        )),
        skipped_statement_lines,
    )
}

#[must_use]
pub fn collect_unsafe_operation_sites(source: &str) -> Vec<UnsafeOperationSite> {
    let tree = parse(SourceFile {
        path: PathBuf::from("<unsafe>.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: source.to_owned(),
    });
    let normalized = normalize_annotated_lambda_source_lossy(&normalize_typepython_source(
        source,
        &tree.statements,
    ));
    with_source_line_index(&normalized, || {
        let Ok(parsed) = parse_module(&normalized) else {
            return Vec::new();
        };

        let unsafe_ranges = collect_unsafe_block_ranges(source, &tree.statements);
        let mut collector =
            UnsafeOperationCollector { source: &normalized, unsafe_ranges, sites: Vec::new() };
        for stmt in parsed.suite() {
            visitor::Visitor::visit_stmt(&mut collector, stmt);
        }
        collector.sites
    })
}

#[must_use]
pub fn collect_unsafe_capability_ranges(source: &str) -> Vec<(usize, usize)> {
    let tree = parse(SourceFile {
        path: PathBuf::from("<unsafe-ranges>.tpy"),
        kind: SourceKind::TypePython,
        logical_module: String::new(),
        text: source.to_owned(),
    });
    collect_unsafe_block_ranges(source, &tree.statements)
}

#[must_use]
pub fn collect_conditional_return_sites(source: &str) -> Vec<ConditionalReturnSite> {
    conditional_return_blocks(source)
        .into_iter()
        .filter_map(|block| {
            let params = block.header.split_once('(')?.1.rsplit_once(')')?.0;
            let target_type = parameter_annotation(params, &block.target_name)?;
            Some(ConditionalReturnSite {
                function_name: block.function_name,
                target_name: block.target_name,
                target_type,
                case_input_types: block.case_input_types,
                line: block.line,
            })
        })
        .collect()
}

#[must_use]
pub fn collect_unsupported_dual_emit_async_construct_sites(
    source: &str,
) -> Vec<UnsupportedDualEmitAsyncConstructSite> {
    let normalized = normalize_annotated_lambda_source_lossy(source);
    with_source_line_index(&normalized, || {
        let Ok(parsed) = parse_module(&normalized) else {
            return Vec::new();
        };

        let import_bindings = collect_import_bindings(parsed.suite());
        let mut sites = Vec::new();
        for stmt in parsed.suite() {
            collect_dual_emit_async_construct_sites(
                &normalized,
                stmt,
                &import_bindings,
                &mut sites,
            );
        }
        sites
    })
}

#[must_use]
pub fn collect_dataclass_transform_module_info(source: &str) -> DataclassTransformModuleInfo {
    collect_module_surface_metadata(source).dataclass_transform
}

#[must_use]
pub fn collect_decorator_transform_module_info(source: &str) -> DecoratorTransformModuleInfo {
    collect_module_surface_metadata(source).decorator_transform
}

#[must_use]
pub fn collect_framework_transform_module_info(source: &str) -> FrameworkTransformModuleInfo {
    collect_module_surface_metadata(source).framework_transform
}

#[must_use]
pub fn collect_direct_function_signature_sites(source: &str) -> Vec<DirectFunctionSignatureSite> {
    collect_module_surface_metadata(source).direct_function_signatures
}

#[must_use]
pub fn collect_direct_method_signature_sites(source: &str) -> Vec<DirectMethodSignatureSite> {
    collect_module_surface_metadata(source).direct_method_signatures
}

pub(super) fn collect_direct_function_param_sites(
    source: &str,
    parameters: &ruff_python_ast::Parameters,
) -> Vec<DirectFunctionParamSite> {
    let positional_only = parameters.posonlyargs.iter().map(|parameter| DirectFunctionParamSite {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        annotation_expr: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .and_then(TypeExpr::parse),
        has_default: parameter.default().is_some(),
        positional_only: true,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    });
    let positional = parameters.args.iter().map(|parameter| DirectFunctionParamSite {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        annotation_expr: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .and_then(TypeExpr::parse),
        has_default: parameter.default().is_some(),
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: false,
    });
    let variadic = parameters.vararg.iter().map(|parameter| DirectFunctionParamSite {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        annotation_expr: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .and_then(TypeExpr::parse),
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: true,
        keyword_variadic: false,
    });
    let keyword_only = parameters.kwonlyargs.iter().map(|parameter| DirectFunctionParamSite {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        annotation_expr: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .and_then(TypeExpr::parse),
        has_default: parameter.default().is_some(),
        positional_only: false,
        keyword_only: true,
        variadic: false,
        keyword_variadic: false,
    });
    let keyword_variadic = parameters.kwarg.iter().map(|parameter| DirectFunctionParamSite {
        name: parameter.name().as_str().to_owned(),
        annotation: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .map(str::to_owned),
        annotation_expr: parameter
            .annotation()
            .and_then(|annotation| slice_range(source, annotation.range()))
            .and_then(TypeExpr::parse),
        has_default: false,
        positional_only: false,
        keyword_only: false,
        variadic: false,
        keyword_variadic: true,
    });

    positional_only
        .chain(positional)
        .chain(variadic)
        .chain(keyword_only)
        .chain(keyword_variadic)
        .collect()
}

#[must_use]
pub fn collect_frozen_field_mutation_sites(source: &str) -> Vec<FrozenFieldMutationSite> {
    let normalized = normalize_annotated_lambda_source_lossy(source);
    with_source_line_index(&normalized, || {
        let Ok(parsed) = parse_module(&normalized) else {
            return Vec::new();
        };

        let mut sites = Vec::new();
        collect_frozen_field_mutation_sites_in_suite(
            &normalized,
            parsed.suite(),
            None,
            None,
            &mut sites,
        );
        sites
    })
}

pub(super) fn collect_frozen_field_mutation_sites_in_suite(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<FrozenFieldMutationSite>,
) {
    for stmt in suite {
        let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
        sites.extend(extract_frozen_field_mutation_sites_from_stmt(
            source,
            stmt,
            line,
            owner_name,
            owner_type_name,
        ));

        match stmt {
            Stmt::FunctionDef(function) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    sites,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    sites,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_frozen_field_mutation_sites_in_suite(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::If(if_stmt) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_frozen_field_mutation_sites_in_suite(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_frozen_field_mutation_sites_in_suite(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &for_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &for_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::While(while_stmt) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &while_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &while_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::With(with_stmt) => {
                collect_frozen_field_mutation_sites_in_suite(
                    source,
                    &with_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            _ => {}
        }
    }
}

pub(super) fn extract_frozen_field_mutation_sites_from_stmt(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Vec<FrozenFieldMutationSite> {
    let owner = MutationOwnerContext { line, owner_name, owner_type_name };
    match stmt {
        Stmt::Assign(assign) => assign
            .targets
            .iter()
            .filter_map(|target| {
                extract_frozen_field_mutation_site(
                    source,
                    target,
                    Some(&assign.value),
                    FrozenFieldMutationKind::Assignment,
                    None,
                    owner,
                )
            })
            .collect(),
        Stmt::AugAssign(assign) => extract_frozen_field_mutation_site(
            source,
            &assign.target,
            Some(&assign.value),
            FrozenFieldMutationKind::AugmentedAssignment,
            Some(direct_operator_text(assign.op)),
            owner,
        )
        .into_iter()
        .collect(),
        Stmt::Delete(delete) => delete
            .targets
            .iter()
            .filter_map(|target| {
                extract_frozen_field_mutation_site(
                    source,
                    target,
                    None,
                    FrozenFieldMutationKind::Delete,
                    None,
                    owner,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MutationOwnerContext<'a> {
    line: usize,
    owner_name: Option<&'a str>,
    owner_type_name: Option<&'a str>,
}

pub(super) fn extract_frozen_field_mutation_site(
    source: &str,
    expr: &Expr,
    value: Option<&Expr>,
    kind: FrozenFieldMutationKind,
    operator: Option<String>,
    owner: MutationOwnerContext<'_>,
) -> Option<FrozenFieldMutationSite> {
    let Expr::Attribute(attribute) = expr else {
        return None;
    };
    Some(FrozenFieldMutationSite {
        kind,
        field_name: attribute.attr.as_str().to_owned(),
        operator,
        target: extract_direct_expr_metadata(source, &attribute.value),
        value: value.map(|expr| extract_direct_expr_metadata(source, expr)),
        owner_name: owner.owner_name.map(str::to_owned),
        owner_type_name: owner.owner_type_name.map(str::to_owned),
        line: owner.line,
    })
}

pub(super) fn collect_dataclass_transform_class_site(
    source: &str,
    class_def: &ruff_python_ast::StmtClassDef,
    import_bindings: &BTreeMap<String, String>,
) -> DataclassTransformClassSite {
    let plain_dataclass = dataclass_decorator_metadata(&class_def.decorator_list, import_bindings);
    DataclassTransformClassSite {
        name: class_def.name.as_str().to_owned(),
        decorators: class_def
            .decorator_list
            .iter()
            .filter_map(|decorator| decorator_target_name(&decorator.expression))
            .map(|name| normalize_imported_name(&name, import_bindings))
            .collect(),
        plain_dataclass_frozen: plain_dataclass.as_ref().is_some_and(|metadata| metadata.frozen),
        plain_dataclass_kw_only: plain_dataclass.as_ref().is_some_and(|metadata| metadata.kw_only),
        plain_dataclass_init: plain_dataclass
            .as_ref()
            .map(|metadata| metadata.init)
            .unwrap_or(true),
        bases: class_def
            .arguments
            .as_ref()
            .map(|arguments| {
                arguments
                    .args
                    .iter()
                    .filter_map(decorator_target_name)
                    .map(|name| normalize_imported_name(&name, import_bindings))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        metaclass: class_def.arguments.as_ref().and_then(|arguments| {
            arguments.keywords.iter().find_map(|keyword| {
                (keyword.arg.as_ref().map(|arg| arg.as_str()) == Some("metaclass"))
                    .then(|| decorator_target_name(&keyword.value))
                    .flatten()
                    .map(|name| normalize_imported_name(&name, import_bindings))
            })
        }),
        methods: class_def
            .body
            .iter()
            .filter_map(|stmt| match stmt {
                Stmt::FunctionDef(function) => Some(function.name.as_str().to_owned()),
                _ => None,
            })
            .collect(),
        fields: class_def
            .body
            .iter()
            .filter_map(|stmt| extract_dataclass_transform_field(source, stmt, import_bindings))
            .collect(),
        untyped_fields: class_def
            .body
            .iter()
            .filter_map(|stmt| extract_dataclass_transform_untyped_field(source, stmt))
            .collect(),
        line: offset_to_line_column(source, class_def.range.start().to_usize()).0,
    }
}

pub(super) fn collect_decorated_callable_sites(
    source: &str,
    suite: &[Stmt],
    owner_type_name: Option<&str>,
    import_bindings: &BTreeMap<String, String>,
    callables: &mut Vec<DecoratedCallableSite>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                let decorators = function
                    .decorator_list
                    .iter()
                    .filter_map(|decorator| {
                        decorator_transform_name(source, &decorator.expression, import_bindings)
                    })
                    .filter(|name| !is_non_transform_builtin_decorator(name))
                    .collect::<Vec<_>>();
                if !decorators.is_empty() {
                    callables.push(DecoratedCallableSite {
                        owner_type_name: owner_type_name.map(str::to_owned),
                        name: function.name.as_str().to_owned(),
                        decorators,
                        line: offset_to_line_column(source, function.name.range.start().to_usize())
                            .0,
                    });
                }
            }
            Stmt::ClassDef(class_def) => {
                collect_decorated_callable_sites(
                    source,
                    &class_def.body,
                    Some(class_def.name.as_str()),
                    import_bindings,
                    callables,
                );
            }
            _ => {}
        }
    }
}

pub(super) fn collect_dual_emit_async_construct_sites(
    source: &str,
    stmt: &Stmt,
    import_bindings: &BTreeMap<String, String>,
    sites: &mut Vec<UnsupportedDualEmitAsyncConstructSite>,
) {
    match stmt {
        Stmt::FunctionDef(function) => {
            let has_dual_emit = function.decorator_list.iter().any(|decorator| {
                decorator_target_name(&decorator.expression)
                    .map(|name| normalize_imported_name(&name, import_bindings))
                    .is_some_and(|name| name.rsplit('.').next() == Some("dual_emit"))
            });
            if !has_dual_emit {
                return;
            }
            let mut collector = DualEmitAsyncConstructCollector {
                source,
                function_name: function.name.as_str().to_owned(),
                sites,
            };
            for body_stmt in &function.body {
                visitor::Visitor::visit_stmt(&mut collector, body_stmt);
            }
        }
        Stmt::ClassDef(class_def) => {
            for member in &class_def.body {
                collect_dual_emit_async_construct_sites(source, member, import_bindings, sites);
            }
        }
        _ => {}
    }
}

pub(super) struct DualEmitAsyncConstructCollector<'source, 'sites> {
    source: &'source str,
    function_name: String,
    sites: &'sites mut Vec<UnsupportedDualEmitAsyncConstructSite>,
}

impl<'source, 'sites, 'ast> visitor::Visitor<'ast>
    for DualEmitAsyncConstructCollector<'source, 'sites>
{
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match stmt {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => return,
            Stmt::For(for_stmt) if for_stmt.is_async => {
                self.push_site(for_stmt.range(), UnsupportedDualEmitAsyncConstructKind::AsyncFor)
            }
            Stmt::With(with_stmt) if with_stmt.is_async => {
                self.push_site(with_stmt.range(), UnsupportedDualEmitAsyncConstructKind::AsyncWith)
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }
}

impl DualEmitAsyncConstructCollector<'_, '_> {
    fn push_site(
        &mut self,
        range: ruff_text_size::TextRange,
        kind: UnsupportedDualEmitAsyncConstructKind,
    ) {
        let line = offset_to_line_column(self.source, range.start().to_usize()).0;
        self.sites.push(UnsupportedDualEmitAsyncConstructSite {
            function_name: self.function_name.clone(),
            kind,
            line,
        });
    }
}

pub(super) fn is_non_transform_builtin_decorator(name: &str) -> bool {
    matches!(
        name,
        "overload"
            | "typing.overload"
            | "override"
            | "typing.override"
            | "typing_extensions.override"
            | "final"
            | "typing.final"
            | "typing_extensions.final"
            | "abstractmethod"
            | "abc.abstractmethod"
            | "classmethod"
            | "staticmethod"
            | "property"
            | "dataclass"
            | "dataclasses.dataclass"
            | "dataclass_transform"
            | "typing.dataclass_transform"
            | "typing_extensions.dataclass_transform"
            | "deprecated"
            | "warnings.deprecated"
            | "typing_extensions.deprecated"
            | "framework_transform"
            | "typepython.framework_transform"
    ) || name.ends_with(".setter")
        || name.ends_with(".getter")
        || name.ends_with(".deleter")
}

pub(super) fn framework_transform_provider_site(
    source: &str,
    provider_name: &str,
    decorators: &[ruff_python_ast::Decorator],
    import_bindings: &BTreeMap<String, String>,
    line: usize,
) -> Option<FrameworkTransformProviderSite> {
    decorators.iter().find_map(|decorator| {
        let expression = &decorator.expression;
        is_framework_transform_expr(expression, import_bindings).then(|| {
            let (provider_kind, capabilities, fallback, frozen_default) =
                framework_transform_metadata_from_expr(source, expression, import_bindings);
            FrameworkTransformProviderSite {
                name: provider_name.to_owned(),
                provider_kind,
                capabilities,
                fallback,
                frozen_default,
                line,
            }
        })
    })
}

pub(super) fn framework_transform_metadata_from_expr(
    source: &str,
    expr: &Expr,
    import_bindings: &BTreeMap<String, String>,
) -> (
    Option<FrameworkTransformProviderKind>,
    Vec<FrameworkTransformCapability>,
    FrameworkTransformFallback,
    bool,
) {
    let Expr::Call(call) = expr else {
        return (None, Vec::new(), FrameworkTransformFallback::default(), false);
    };
    let mut provider_kind = None;
    let mut capabilities = Vec::new();
    let mut fallback = FrameworkTransformFallback::default();
    let mut frozen_default = false;
    for keyword in &call.arguments.keywords {
        let Some(name) = keyword.arg.as_ref().map(|name| name.as_str()) else {
            continue;
        };
        match name {
            "kind" => {
                provider_kind = extract_string_literal_value(source, &keyword.value)
                    .and_then(|value| framework_transform_provider_kind(&value));
            }
            "capabilities" => {
                capabilities = expr_name_list(&keyword.value, source, import_bindings)
                    .into_iter()
                    .filter_map(|value| framework_transform_capability(&value))
                    .collect();
            }
            "fallback" => {
                if let Some(value) = extract_string_literal_value(source, &keyword.value) {
                    fallback = framework_transform_fallback(&value);
                }
            }
            "frozen_default" => {
                frozen_default = expr_static_bool(&keyword.value).unwrap_or(false);
            }
            _ => {}
        }
    }
    (provider_kind, capabilities, fallback, frozen_default)
}

pub(super) fn is_framework_transform_expr(
    expr: &Expr,
    import_bindings: &BTreeMap<String, String>,
) -> bool {
    decorator_target_name(expr)
        .map(|name| normalize_imported_name(&name, import_bindings))
        .is_some_and(|name| {
            matches!(name.as_str(), "framework_transform" | "typepython.framework_transform")
        })
}

pub(super) fn framework_transform_provider_kind(
    value: &str,
) -> Option<FrameworkTransformProviderKind> {
    match value {
        "class_decorator" => Some(FrameworkTransformProviderKind::ClassDecorator),
        "base_class" => Some(FrameworkTransformProviderKind::BaseClass),
        "metaclass" => Some(FrameworkTransformProviderKind::Metaclass),
        "function_decorator" | "function_to_object_decorator" => {
            Some(FrameworkTransformProviderKind::FunctionDecorator)
        }
        _ => None,
    }
}

pub(super) fn framework_transform_capability(value: &str) -> Option<FrameworkTransformCapability> {
    match value {
        "field_collection" => Some(FrameworkTransformCapability::FieldCollection),
        "constructor_generation" => Some(FrameworkTransformCapability::ConstructorGeneration),
        "alias_handling" => Some(FrameworkTransformCapability::AliasHandling),
        "required_optional_fields" => Some(FrameworkTransformCapability::RequiredOptionalFields),
        "readonly_fields" => Some(FrameworkTransformCapability::ReadonlyFields),
        "descriptor_backed_attributes" => {
            Some(FrameworkTransformCapability::DescriptorBackedAttributes)
        }
        "method_synthesis" => Some(FrameworkTransformCapability::MethodSynthesis),
        "function_to_object_replacement" => {
            Some(FrameworkTransformCapability::FunctionToObjectReplacement)
        }
        "generic_preservation" => Some(FrameworkTransformCapability::GenericPreservation),
        "taint_source" => Some(FrameworkTransformCapability::TaintSource),
        "taint_sink" => Some(FrameworkTransformCapability::TaintSink),
        "taint_sanitizer" => Some(FrameworkTransformCapability::TaintSanitizer),
        "validator_witness" => Some(FrameworkTransformCapability::ValidatorWitness),
        "effect_unsafe" => Some(FrameworkTransformCapability::EffectUnsafe),
        "effect_io_fs" => Some(FrameworkTransformCapability::EffectIoFs),
        "effect_io_net" => Some(FrameworkTransformCapability::EffectIoNet),
        "effect_io_proc" => Some(FrameworkTransformCapability::EffectIoProc),
        "effect_time" => Some(FrameworkTransformCapability::EffectTime),
        "effect_random" => Some(FrameworkTransformCapability::EffectRandom),
        "effect_runtime_validation" => Some(FrameworkTransformCapability::EffectRuntimeValidation),
        "effect_taint_sanitize" => Some(FrameworkTransformCapability::EffectTaintSanitize),
        _ => None,
    }
}

pub(super) fn framework_transform_fallback(value: &str) -> FrameworkTransformFallback {
    match value {
        "non_strict_degrade" => FrameworkTransformFallback::NonStrictDegrade,
        _ => FrameworkTransformFallback::StrictDiagnostic,
    }
}

pub(super) fn extract_dataclass_transform_field(
    source: &str,
    stmt: &Stmt,
    import_bindings: &BTreeMap<String, String>,
) -> Option<DataclassTransformFieldSite> {
    let Stmt::AnnAssign(assign) = stmt else {
        return None;
    };
    let Expr::Name(name) = assign.target.as_ref() else {
        return None;
    };
    let value = assign.value.as_deref();
    let field_specifier =
        value.and_then(|expr| extract_field_specifier_site(source, expr, import_bindings));
    Some(DataclassTransformFieldSite {
        name: name.id.as_str().to_owned(),
        annotation: slice_range(source, assign.annotation.range())?.to_owned(),
        annotation_expr: slice_range(source, assign.annotation.range()).and_then(TypeExpr::parse),
        value_type_expr: value
            .map(infer_literal_arg_type)
            .and_then(|value_type| TypeExpr::parse(&value_type)),
        value_metadata: value.map(|expr| extract_direct_expr_metadata(source, expr)),
        has_default: value.is_some(),
        is_class_var: is_classvar_annotation(&assign.annotation),
        field_specifier_name: field_specifier.as_ref().and_then(|site| site.name.clone()),
        field_specifier_has_default: field_specifier.as_ref().is_some_and(|site| site.has_default),
        field_specifier_has_default_factory: field_specifier
            .as_ref()
            .is_some_and(|site| site.has_default_factory),
        field_specifier_init: field_specifier.as_ref().and_then(|site| site.init),
        field_specifier_kw_only: field_specifier.as_ref().and_then(|site| site.kw_only),
        field_specifier_alias: field_specifier.as_ref().and_then(|site| site.alias.clone()),
        field_specifier_has_dynamic_alias: field_specifier
            .as_ref()
            .is_some_and(|site| site.has_dynamic_alias),
        field_specifier_frozen: field_specifier.as_ref().and_then(|site| site.frozen),
        line: offset_to_line_column(source, assign.range.start().to_usize()).0,
    })
}

pub(super) fn extract_dataclass_transform_untyped_field(
    source: &str,
    stmt: &Stmt,
) -> Option<DataclassTransformUntypedFieldSite> {
    let Stmt::Assign(assign) = stmt else {
        return None;
    };
    if assign.targets.len() != 1 {
        return None;
    }
    let Expr::Name(name) = &assign.targets[0] else {
        return None;
    };
    Some(DataclassTransformUntypedFieldSite {
        name: name.id.as_str().to_owned(),
        line: offset_to_line_column(source, assign.range.start().to_usize()).0,
    })
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct FieldSpecifierSite {
    name: Option<String>,
    has_default: bool,
    has_default_factory: bool,
    init: Option<bool>,
    kw_only: Option<bool>,
    alias: Option<String>,
    has_dynamic_alias: bool,
    frozen: Option<bool>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct DataclassDecoratorMetadata {
    frozen: bool,
    kw_only: bool,
    init: bool,
}

pub(super) fn extract_field_specifier_site(
    source: &str,
    expr: &Expr,
    import_bindings: &BTreeMap<String, String>,
) -> Option<FieldSpecifierSite> {
    let Expr::Call(call) = expr else {
        return None;
    };
    let mut result = FieldSpecifierSite {
        name: decorator_target_name(call.func.as_ref())
            .map(|name| normalize_imported_name(&name, import_bindings)),
        has_default: false,
        has_default_factory: false,
        init: None,
        kw_only: None,
        alias: None,
        has_dynamic_alias: false,
        frozen: None,
    };
    for keyword in &call.arguments.keywords {
        let Some(name) = keyword.arg.as_ref().map(|name| name.as_str()) else {
            continue;
        };
        match name {
            "default" => result.has_default = true,
            "default_factory" => result.has_default_factory = true,
            "init" => result.init = expr_static_bool(&keyword.value),
            "kw_only" => result.kw_only = expr_static_bool(&keyword.value),
            "alias" => {
                result.alias = extract_string_literal_value(source, &keyword.value);
                result.has_dynamic_alias = result.alias.is_none();
            }
            "frozen" => result.frozen = expr_static_bool(&keyword.value),
            _ => {}
        }
    }
    Some(result)
}

pub(super) fn dataclass_transform_metadata(
    source: &str,
    decorators: &[ruff_python_ast::Decorator],
    import_bindings: &BTreeMap<String, String>,
) -> Option<DataclassTransformMetadata> {
    decorators.iter().find_map(|decorator| {
        let expression = &decorator.expression;
        if is_dataclass_transform_expr(expression, import_bindings) {
            return Some(dataclass_transform_metadata_from_call(
                source,
                expression,
                import_bindings,
            ));
        }
        None
    })
}

pub(super) fn dataclass_decorator_metadata(
    decorators: &[ruff_python_ast::Decorator],
    import_bindings: &BTreeMap<String, String>,
) -> Option<DataclassDecoratorMetadata> {
    decorators.iter().find_map(|decorator| {
        let expression = &decorator.expression;
        if !is_dataclass_expr(expression, import_bindings) {
            return None;
        }
        Some(dataclass_decorator_metadata_from_expr(expression))
    })
}

pub(super) fn dataclass_decorator_metadata_from_expr(expr: &Expr) -> DataclassDecoratorMetadata {
    let Expr::Call(call) = expr else {
        return DataclassDecoratorMetadata { frozen: false, kw_only: false, init: true };
    };
    let mut metadata = DataclassDecoratorMetadata { frozen: false, kw_only: false, init: true };
    for keyword in &call.arguments.keywords {
        let Some(name) = keyword.arg.as_ref().map(|name| name.as_str()) else {
            continue;
        };
        match name {
            "frozen" => metadata.frozen = expr_static_bool(&keyword.value).unwrap_or(false),
            "kw_only" => metadata.kw_only = expr_static_bool(&keyword.value).unwrap_or(false),
            "init" => metadata.init = expr_static_bool(&keyword.value).unwrap_or(true),
            _ => {}
        }
    }
    metadata
}

pub(super) fn is_dataclass_expr(expr: &Expr, import_bindings: &BTreeMap<String, String>) -> bool {
    decorator_target_name(expr)
        .map(|name| normalize_imported_name(&name, import_bindings))
        .is_some_and(|name| matches!(name.as_str(), "dataclass" | "dataclasses.dataclass"))
}

pub(super) fn dataclass_transform_metadata_from_call(
    source: &str,
    expr: &Expr,
    import_bindings: &BTreeMap<String, String>,
) -> DataclassTransformMetadata {
    let Expr::Call(call) = expr else {
        return DataclassTransformMetadata::default();
    };
    let mut metadata =
        DataclassTransformMetadata { eq_default: true, ..DataclassTransformMetadata::default() };
    for keyword in &call.arguments.keywords {
        let Some(name) = keyword.arg.as_ref().map(|name| name.as_str()) else {
            continue;
        };
        match name {
            "kw_only_default" => {
                metadata.kw_only_default = expr_static_bool(&keyword.value).unwrap_or(false)
            }
            "frozen_default" => {
                metadata.frozen_default = expr_static_bool(&keyword.value).unwrap_or(false)
            }
            "eq_default" => metadata.eq_default = expr_static_bool(&keyword.value).unwrap_or(true),
            "order_default" => {
                metadata.order_default = expr_static_bool(&keyword.value).unwrap_or(false)
            }
            "field_specifiers" => {
                metadata.field_specifiers = expr_name_list(&keyword.value, source, import_bindings);
            }
            _ => {}
        }
    }
    metadata
}

pub(super) fn is_dataclass_transform_expr(
    expr: &Expr,
    import_bindings: &BTreeMap<String, String>,
) -> bool {
    decorator_target_name(expr)
        .map(|name| normalize_imported_name(&name, import_bindings))
        .is_some_and(|name| {
            matches!(
                name.as_str(),
                "dataclass_transform"
                    | "typing.dataclass_transform"
                    | "typing_extensions.dataclass_transform"
            )
        })
}

pub(super) fn decorator_target_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.id.as_str().to_owned()),
        Expr::Attribute(attribute) => Some(format!(
            "{}.{}",
            decorator_target_name(attribute.value.as_ref())?,
            attribute.attr.as_str()
        )),
        Expr::Call(call) => decorator_target_name(call.func.as_ref()),
        _ => None,
    }
}

fn decorator_transform_name(
    source: &str,
    expr: &Expr,
    import_bindings: &BTreeMap<String, String>,
) -> Option<String> {
    let target = decorator_target_name(expr)?;
    let normalized = normalize_imported_name(&target, import_bindings);
    if normalized.rsplit('.').next() == Some("effect")
        && let Expr::Call(call) = expr
        && let Some(first_arg) = call.arguments.args.first()
        && let Some(effect) = extract_string_literal_value(source, first_arg)
    {
        return Some(format!("{normalized}:{effect}"));
    }
    if matches!(normalized.rsplit('.').next(), Some("source" | "sink" | "sanitizer"))
        && let Expr::Call(call) = expr
        && let Some(context) = call
            .arguments
            .args
            .first()
            .and_then(|arg| extract_string_literal_value(source, arg))
            .or_else(|| {
                call.arguments.keywords.iter().find_map(|keyword| {
                    matches!(
                        keyword.arg.as_ref().map(|name| name.as_str()),
                        Some("context" | "taint" | "label" | "from_taint" | "to_taint")
                    )
                    .then(|| extract_string_literal_value(source, &keyword.value))
                    .flatten()
                })
            })
    {
        return Some(format!("{normalized}:{context}"));
    }
    if let Expr::Call(call) = expr
        && let Some(first_arg) = call.arguments.args.first()
        && let Some(target) = decorator_target_name(first_arg)
        && let Some(trust) = call.arguments.keywords.iter().find_map(|keyword| {
            (keyword.arg.as_ref().map(|name| name.as_str()) == Some("trust"))
                .then(|| extract_string_literal_value(source, &keyword.value))
                .flatten()
        })
        && matches!(trust.as_str(), "trusted" | "generated")
    {
        return Some(format!("{normalized}:{target}:{trust}"));
    }
    Some(normalized)
}

pub(super) fn expr_static_bool(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::BooleanLiteral(boolean) => Some(boolean.value),
        Expr::Name(name) if name.id.as_str() == "True" => Some(true),
        Expr::Name(name) if name.id.as_str() == "False" => Some(false),
        _ => None,
    }
}

pub(super) fn class_keyword_static_bool(
    class_def: &ruff_python_ast::StmtClassDef,
    keyword_name: &str,
) -> Option<bool> {
    class_def.arguments.as_ref()?.keywords.iter().find_map(|keyword| {
        (keyword.arg.as_ref().map(|name| name.as_str()) == Some(keyword_name))
            .then(|| expr_static_bool(&keyword.value))
            .flatten()
    })
}

pub(super) fn class_keyword_source(
    source: &str,
    class_def: &ruff_python_ast::StmtClassDef,
    keyword_name: &str,
) -> Option<String> {
    class_def.arguments.as_ref()?.keywords.iter().find_map(|keyword| {
        (keyword.arg.as_ref().map(|name| name.as_str()) == Some(keyword_name))
            .then(|| slice_range(source, keyword.value.range()).map(str::to_owned))
            .flatten()
    })
}

pub(super) fn expr_name_list(
    expr: &Expr,
    source: &str,
    import_bindings: &BTreeMap<String, String>,
) -> Vec<String> {
    match expr {
        Expr::Tuple(tuple) => tuple
            .elts
            .iter()
            .flat_map(|expr| expr_name_list(expr, source, import_bindings))
            .collect(),
        Expr::List(list) => list
            .elts
            .iter()
            .flat_map(|expr| expr_name_list(expr, source, import_bindings))
            .collect(),
        _ => decorator_target_name(expr)
            .map(|name| normalize_imported_name(&name, import_bindings))
            .or_else(|| extract_string_literal_value(source, expr))
            .into_iter()
            .collect(),
    }
}

pub(super) fn collect_import_bindings(suite: &[Stmt]) -> BTreeMap<String, String> {
    let mut bindings = BTreeMap::new();
    for stmt in suite {
        match stmt {
            Stmt::Import(import) => {
                for alias in &import.names {
                    bindings.insert(
                        alias
                            .asname
                            .as_ref()
                            .map(|name| name.as_str())
                            .unwrap_or_else(|| alias.name.as_str())
                            .to_owned(),
                        alias.name.as_str().to_owned(),
                    );
                }
            }
            Stmt::ImportFrom(import) => {
                let module = import.module.as_deref().unwrap_or("");
                for alias in &import.names {
                    bindings.insert(
                        alias
                            .asname
                            .as_ref()
                            .map(|name| name.as_str())
                            .unwrap_or_else(|| alias.name.as_str())
                            .to_owned(),
                        if module.is_empty() {
                            alias.name.as_str().to_owned()
                        } else {
                            format!("{module}.{}", alias.name)
                        },
                    );
                }
            }
            _ => {}
        }
    }
    bindings
}

pub(super) fn normalize_imported_name(
    name: &str,
    import_bindings: &BTreeMap<String, String>,
) -> String {
    let mut parts = name.split('.');
    let head = parts.next().unwrap_or(name);
    let tail = parts.collect::<Vec<_>>();
    let head = import_bindings.get(head).cloned().unwrap_or_else(|| head.to_owned());
    if tail.is_empty() { head } else { format!("{head}.{}", tail.join(".")) }
}

pub(super) fn parameter_annotation(params: &str, target_name: &str) -> Option<String> {
    split_top_level_commas(params).into_iter().find_map(|param| {
        let (name, annotation) = param.split_once(':')?;
        let name = name.split('=').next()?.trim();
        (name == target_name)
            .then(|| annotation.split('=').next().unwrap_or(annotation).trim().to_owned())
    })
}

pub(super) fn normalize_conditional_return_source(source: &str) -> String {
    let blocks = conditional_return_blocks(source);
    if blocks.is_empty() {
        return source.to_owned();
    }

    let lines: Vec<&str> = source.lines().collect();
    let mut output = Vec::with_capacity(lines.len());
    let mut line_number = 1usize;
    let mut blocks = blocks.into_iter().peekable();
    while line_number <= lines.len() {
        if let Some(block) = blocks.peek()
            && block.line == line_number
        {
            let original = lines[line_number - 1];
            let indent = &original[..original.len() - original.trim_start().len()];
            output.push(format!("{indent}{}:", block.header));
            let case_indent = format!("{indent}    ");
            output.push(format!("{case_indent}pass"));
            for _ in block.line + 1..=block.end_line {
                output.push(String::new());
            }
            line_number = block.end_line + 1;
            blocks.next();
            continue;
        }
        output.push(lines[line_number - 1].to_owned());
        line_number += 1;
    }

    let mut normalized = output.join("\n");
    if source.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

#[derive(Debug, Clone)]
pub(super) struct ConditionalReturnBlock {
    function_name: String,
    header: String,
    target_name: String,
    case_input_types: Vec<String>,
    line: usize,
    end_line: usize,
}

pub(super) fn conditional_return_blocks(source: &str) -> Vec<ConditionalReturnBlock> {
    let lines: Vec<&str> = source.lines().collect();
    let mut blocks = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        if !trimmed.starts_with("def ") {
            index += 1;
            continue;
        }
        let indent = line.len() - trimmed.len();

        let mut header_parts = vec![trimmed];
        let mut header_cursor = index;
        let mut conditional_header = None;
        while header_cursor < lines.len() {
            let header_line = lines[header_cursor];
            let header_trimmed = header_line.trim_start();
            if header_cursor > index {
                header_parts.push(header_trimmed);
            }

            if header_trimmed.contains("-> match ") && header_trimmed.ends_with(':') {
                conditional_header = Some(header_parts.join(" "));
                break;
            }

            if header_cursor > index {
                let continuation_indent = header_line.len() - header_trimmed.len();
                if continuation_indent <= indent || header_trimmed.starts_with("case ") {
                    break;
                }
            }

            header_cursor += 1;
        }

        let Some(header_line) = conditional_header else {
            index += 1;
            continue;
        };
        let Some((header, rest)) = header_line.split_once("-> match ") else {
            index += 1;
            continue;
        };
        let Some(target_name) =
            rest.strip_suffix(':').map(str::trim).filter(|name| !name.is_empty())
        else {
            index += 1;
            continue;
        };
        let Some(function_name) = header
            .strip_prefix("def ")
            .and_then(|rest| rest.split('(').next())
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            index += 1;
            continue;
        };

        let mut case_input_types = Vec::new();
        let mut cursor = header_cursor + 1;
        while cursor < lines.len() {
            let case_line = lines[cursor];
            let case_trimmed = case_line.trim_start();
            if case_trimmed.is_empty() {
                cursor += 1;
                continue;
            }
            let case_indent = case_line.len() - case_trimmed.len();
            if case_indent <= indent || !case_trimmed.starts_with("case ") {
                break;
            }
            if let Some((case_type, _)) =
                case_trimmed.strip_prefix("case ").and_then(|rest| rest.split_once(':'))
            {
                case_input_types.push(case_type.trim().to_owned());
            }
            cursor += 1;
        }

        if !case_input_types.is_empty() {
            blocks.push(ConditionalReturnBlock {
                function_name: function_name.to_owned(),
                header: header.trim_end().to_owned(),
                target_name: target_name.to_owned(),
                case_input_types,
                line: index + 1,
                end_line: cursor,
            });
            index = cursor;
        } else {
            index += 1;
        }
    }
    blocks
}

pub(super) fn collect_typed_dict_mutation_sites_in_suite(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<TypedDictMutationSite>,
) {
    for stmt in suite {
        let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
        sites.extend(extract_typed_dict_mutation_sites_from_stmt(
            source,
            stmt,
            line,
            owner_name,
            owner_type_name,
        ));

        match stmt {
            Stmt::FunctionDef(function) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    sites,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    sites,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_typed_dict_mutation_sites_in_suite(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::If(if_stmt) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_typed_dict_mutation_sites_in_suite(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_typed_dict_mutation_sites_in_suite(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &for_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &for_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::While(while_stmt) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &while_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &while_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::With(with_stmt) => {
                collect_typed_dict_mutation_sites_in_suite(
                    source,
                    &with_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            _ => {}
        }
    }
}

pub(super) fn extract_typed_dict_mutation_sites_from_stmt(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Vec<TypedDictMutationSite> {
    let owner = MutationOwnerContext { line, owner_name, owner_type_name };
    match stmt {
        Stmt::Assign(assign) => assign
            .targets
            .iter()
            .filter_map(|target| {
                extract_typed_dict_mutation_site(
                    source,
                    target,
                    Some(&assign.value),
                    TypedDictMutationKind::Assignment,
                    None,
                    owner,
                )
            })
            .collect(),
        Stmt::AugAssign(assign) => extract_typed_dict_mutation_site(
            source,
            &assign.target,
            Some(&assign.value),
            TypedDictMutationKind::AugmentedAssignment,
            Some(direct_operator_text(assign.op)),
            owner,
        )
        .into_iter()
        .collect(),
        Stmt::Delete(delete) => delete
            .targets
            .iter()
            .filter_map(|target| {
                extract_typed_dict_mutation_site(
                    source,
                    target,
                    None,
                    TypedDictMutationKind::Delete,
                    None,
                    owner,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn extract_typed_dict_mutation_site(
    source: &str,
    expr: &Expr,
    value: Option<&Expr>,
    kind: TypedDictMutationKind,
    operator: Option<String>,
    owner: MutationOwnerContext<'_>,
) -> Option<TypedDictMutationSite> {
    let Expr::Subscript(subscript) = expr else {
        return None;
    };
    Some(TypedDictMutationSite {
        kind,
        key: extract_string_literal_value(source, &subscript.slice),
        operator,
        key_value: extract_direct_expr_metadata(source, &subscript.slice),
        target: extract_direct_expr_metadata(source, &subscript.value),
        value: value.map(|expr| extract_direct_expr_metadata(source, expr)),
        owner_name: owner.owner_name.map(str::to_owned),
        owner_type_name: owner.owner_type_name.map(str::to_owned),
        line: owner.line,
    })
}

pub(super) fn collect_direct_call_context_sites_in_suite(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<DirectCallContextSite>,
) {
    for stmt in suite {
        let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
        if let Some(site) =
            extract_direct_call_context_site(stmt, line, owner_name, owner_type_name)
        {
            sites.push(site);
        }

        match stmt {
            Stmt::FunctionDef(function) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    sites,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    sites,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_direct_call_context_sites_in_suite(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
                collect_direct_call_context_sites_in_suite(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_direct_call_context_sites_in_suite(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::If(if_stmt) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_direct_call_context_sites_in_suite(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_direct_call_context_sites_in_suite(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &for_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_direct_call_context_sites_in_suite(
                    source,
                    &for_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::While(while_stmt) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &while_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_direct_call_context_sites_in_suite(
                    source,
                    &while_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::With(with_stmt) => {
                collect_direct_call_context_sites_in_suite(
                    source,
                    &with_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            _ => {}
        }
    }
}

fn collect_nested_direct_call_context_sites_in_suite(
    source: &str,
    suite: &[Stmt],
    owner_type_name: Option<&str>,
    sites: &mut Vec<DirectCallContextSite>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                let mut collector = NestedDirectCallContextCollector {
                    source,
                    owner_name: function.name.as_str().to_owned(),
                    owner_type_name: owner_type_name.map(str::to_owned),
                    sites,
                };
                for body_stmt in &function.body {
                    visitor::Visitor::visit_stmt(&mut collector, body_stmt);
                }
                collect_nested_direct_call_context_sites_in_suite(
                    source,
                    &function.body,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_nested_direct_call_context_sites_in_suite(
                    source,
                    &class_def.body,
                    Some(class_def.name.as_str()),
                    sites,
                );
            }
            _ => {
                for_each_nested_suite(stmt, |suite| {
                    collect_nested_direct_call_context_sites_in_suite(
                        source,
                        suite,
                        owner_type_name,
                        sites,
                    );
                });
            }
        }
    }
}

struct NestedDirectCallContextCollector<'source, 'sites> {
    source: &'source str,
    owner_name: String,
    owner_type_name: Option<String>,
    sites: &'sites mut Vec<DirectCallContextSite>,
}

impl<'source, 'sites, 'ast> visitor::Visitor<'ast>
    for NestedDirectCallContextCollector<'source, 'sites>
{
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
            return;
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let Some(callee) = extract_direct_call_context_callee(expr) {
            self.sites.push(DirectCallContextSite {
                callee,
                owner_name: Some(self.owner_name.clone()),
                owner_type_name: self.owner_type_name.clone(),
                positional_arg_count: extract_direct_call_positional_arg_count(expr).unwrap_or(0),
                keyword_arg_count: extract_direct_call_keyword_arg_count(expr).unwrap_or(0),
                has_starred_args: direct_call_has_starred_args(expr).unwrap_or(false),
                has_unpacked_kwargs: direct_call_has_unpacked_kwargs(expr).unwrap_or(false),
                line: offset_to_line_column(self.source, expr.range().start().to_usize()).0,
            });
        }
        visitor::walk_expr(self, expr);
    }
}

pub(super) fn extract_direct_call_context_site(
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<DirectCallContextSite> {
    let expr = match stmt {
        Stmt::Expr(expr) => Some(expr.value.as_ref()),
        Stmt::Assign(assign) => Some(assign.value.as_ref()),
        Stmt::AnnAssign(assign) => assign.value.as_deref(),
        Stmt::Return(return_stmt) => return_stmt.value.as_deref(),
        _ => None,
    }?;

    Some(DirectCallContextSite {
        callee: extract_direct_call_context_callee(expr)?,
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        positional_arg_count: extract_direct_call_positional_arg_count(expr)?,
        keyword_arg_count: extract_direct_call_keyword_arg_count(expr)?,
        has_starred_args: direct_call_has_starred_args(expr)?,
        has_unpacked_kwargs: direct_call_has_unpacked_kwargs(expr)?,
        line,
    })
}

pub(super) fn extract_direct_call_context_callee(expr: &Expr) -> Option<String> {
    if let Expr::Await(await_expr) = expr {
        return extract_direct_call_context_callee(&await_expr.value);
    }

    let Expr::Call(call) = expr else {
        return None;
    };
    let Expr::Name(name) = call.func.as_ref() else {
        return None;
    };
    Some(name.id.as_str().to_owned())
}

pub(super) fn extract_direct_call_positional_arg_count(expr: &Expr) -> Option<usize> {
    if let Expr::Await(await_expr) = expr {
        return extract_direct_call_positional_arg_count(&await_expr.value);
    }
    let Expr::Call(call) = expr else {
        return None;
    };
    Some(call.arguments.args.iter().filter(|arg| !matches!(arg, Expr::Starred(_))).count())
}

pub(super) fn extract_direct_call_keyword_arg_count(expr: &Expr) -> Option<usize> {
    if let Expr::Await(await_expr) = expr {
        return extract_direct_call_keyword_arg_count(&await_expr.value);
    }
    let Expr::Call(call) = expr else {
        return None;
    };
    Some(call.arguments.keywords.iter().filter(|keyword| keyword.arg.is_some()).count())
}

pub(super) fn direct_call_has_starred_args(expr: &Expr) -> Option<bool> {
    if let Expr::Await(await_expr) = expr {
        return direct_call_has_starred_args(&await_expr.value);
    }
    let Expr::Call(call) = expr else {
        return None;
    };
    Some(call.arguments.args.iter().any(|arg| matches!(arg, Expr::Starred(_))))
}

pub(super) fn direct_call_has_unpacked_kwargs(expr: &Expr) -> Option<bool> {
    if let Expr::Await(await_expr) = expr {
        return direct_call_has_unpacked_kwargs(&await_expr.value);
    }
    let Expr::Call(call) = expr else {
        return None;
    };
    Some(call.arguments.keywords.iter().any(|keyword| keyword.arg.is_none()))
}

pub(super) fn collect_expression_use_sites_in_suite(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    skipped_statement_lines: &std::collections::BTreeSet<usize>,
    sites: &mut Vec<ExpressionUseSite>,
) {
    for stmt in suite {
        let line = offset_to_line_column(source, stmt.range().start().to_usize()).0;
        if !skipped_statement_lines.contains(&line) {
            collect_expression_use_sites_from_statement_exprs(
                source,
                stmt,
                line,
                owner_name,
                owner_type_name,
                sites,
            );
        }

        match stmt {
            Stmt::FunctionDef(function) => {
                collect_expression_use_sites_in_suite(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    skipped_statement_lines,
                    sites,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_expression_use_sites_in_suite(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    skipped_statement_lines,
                    sites,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_expression_use_sites_in_suite(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    skipped_statement_lines,
                    sites,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_expression_use_sites_in_suite(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        skipped_statement_lines,
                        sites,
                    );
                }
                collect_expression_use_sites_in_suite(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    skipped_statement_lines,
                    sites,
                );
                collect_expression_use_sites_in_suite(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    skipped_statement_lines,
                    sites,
                );
            }
            Stmt::If(if_stmt) => {
                collect_expression_use_sites_in_suite(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    skipped_statement_lines,
                    sites,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_expression_use_sites_in_suite(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        skipped_statement_lines,
                        sites,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_expression_use_sites_in_suite(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        skipped_statement_lines,
                        sites,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_expression_use_sites_in_suite(
                    source,
                    &for_stmt.body,
                    owner_name,
                    owner_type_name,
                    skipped_statement_lines,
                    sites,
                );
                collect_expression_use_sites_in_suite(
                    source,
                    &for_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    skipped_statement_lines,
                    sites,
                );
            }
            Stmt::While(while_stmt) => {
                collect_expression_use_sites_in_suite(
                    source,
                    &while_stmt.body,
                    owner_name,
                    owner_type_name,
                    skipped_statement_lines,
                    sites,
                );
                collect_expression_use_sites_in_suite(
                    source,
                    &while_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    skipped_statement_lines,
                    sites,
                );
            }
            Stmt::With(with_stmt) => {
                collect_expression_use_sites_in_suite(
                    source,
                    &with_stmt.body,
                    owner_name,
                    owner_type_name,
                    skipped_statement_lines,
                    sites,
                );
            }
            _ => {}
        }
    }
}

fn collect_expression_use_sites_from_statement_exprs(
    source: &str,
    stmt: &Stmt,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<ExpressionUseSite>,
) {
    match stmt {
        Stmt::Expr(expr) => collect_expression_use_sites_in_expr_with_line(
            source,
            &expr.value,
            owner_name,
            owner_type_name,
            Some(line),
            sites,
        ),
        Stmt::FunctionDef(function) => {
            collect_expression_use_sites_in_decorator_exprs(
                source,
                &function.decorator_list,
                owner_name,
                owner_type_name,
                sites,
            );
            collect_expression_use_sites_in_parameter_default_exprs(
                source,
                &function.parameters,
                owner_name,
                owner_type_name,
                sites,
            );
        }
        Stmt::ClassDef(class_def) => {
            collect_expression_use_sites_in_decorator_exprs(
                source,
                &class_def.decorator_list,
                owner_name,
                owner_type_name,
                sites,
            );
            if let Some(arguments) = class_def.arguments.as_ref() {
                for argument in &arguments.args {
                    collect_expression_use_sites_in_expr(
                        source,
                        argument,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
                for keyword in &arguments.keywords {
                    collect_expression_use_sites_in_expr(
                        source,
                        &keyword.value,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
            }
        }
        Stmt::Assign(assign) => {
            for target in &assign.targets {
                collect_expression_use_sites_in_expr_with_line(
                    source,
                    target,
                    owner_name,
                    owner_type_name,
                    Some(line),
                    sites,
                );
            }
            collect_expression_use_sites_in_expr_with_line(
                source,
                &assign.value,
                owner_name,
                owner_type_name,
                Some(line),
                sites,
            );
        }
        Stmt::AnnAssign(assign) => {
            collect_expression_use_sites_in_expr_with_line(
                source,
                &assign.target,
                owner_name,
                owner_type_name,
                Some(line),
                sites,
            );
            if let Some(value) = assign.value.as_deref() {
                collect_expression_use_sites_in_expr_with_line(
                    source,
                    value,
                    owner_name,
                    owner_type_name,
                    Some(line),
                    sites,
                );
            }
        }
        Stmt::AugAssign(assign) => {
            collect_expression_use_sites_in_expr_with_line(
                source,
                &assign.target,
                owner_name,
                owner_type_name,
                Some(line),
                sites,
            );
            collect_expression_use_sites_in_expr_with_line(
                source,
                &assign.value,
                owner_name,
                owner_type_name,
                Some(line),
                sites,
            );
        }
        Stmt::Return(return_stmt) => {
            if let Some(value) = return_stmt.value.as_deref() {
                collect_expression_use_sites_in_expr_with_line(
                    source,
                    value,
                    owner_name,
                    owner_type_name,
                    Some(line),
                    sites,
                );
            }
        }
        Stmt::If(if_stmt) => collect_guard_expression_use_sites_in_expr_with_line(
            source,
            &if_stmt.test,
            owner_name,
            owner_type_name,
            Some(line),
            sites,
        ),
        Stmt::Assert(assert_stmt) => {
            collect_guard_expression_use_sites_in_expr_with_line(
                source,
                &assert_stmt.test,
                owner_name,
                owner_type_name,
                Some(line),
                sites,
            );
            if let Some(message) = assert_stmt.msg.as_deref() {
                collect_expression_use_sites_in_expr_with_line(
                    source,
                    message,
                    owner_name,
                    owner_type_name,
                    Some(line),
                    sites,
                );
            }
        }
        Stmt::While(while_stmt) => collect_guard_expression_use_sites_in_expr_with_line(
            source,
            &while_stmt.test,
            owner_name,
            owner_type_name,
            Some(line),
            sites,
        ),
        Stmt::Match(match_stmt) => {
            collect_expression_use_sites_in_expr_with_line(
                source,
                &match_stmt.subject,
                owner_name,
                owner_type_name,
                Some(line),
                sites,
            );
            for case in &match_stmt.cases {
                if let Some(guard) = case.guard.as_deref() {
                    collect_guard_expression_use_sites_in_expr_with_line(
                        source,
                        guard,
                        owner_name,
                        owner_type_name,
                        Some(offset_to_line_column(source, guard.range().start().to_usize()).0),
                        sites,
                    );
                }
            }
        }
        Stmt::For(for_stmt) => collect_expression_use_sites_in_expr_with_line(
            source,
            &for_stmt.iter,
            owner_name,
            owner_type_name,
            Some(line),
            sites,
        ),
        Stmt::With(with_stmt) => {
            for item in &with_stmt.items {
                collect_expression_use_sites_in_expr_with_line(
                    source,
                    &item.context_expr,
                    owner_name,
                    owner_type_name,
                    Some(line),
                    sites,
                );
            }
        }
        Stmt::Raise(raise_stmt) => {
            if let Some(exception) = raise_stmt.exc.as_deref() {
                collect_expression_use_sites_in_expr_with_line(
                    source,
                    exception,
                    owner_name,
                    owner_type_name,
                    Some(line),
                    sites,
                );
            }
            if let Some(cause) = raise_stmt.cause.as_deref() {
                collect_expression_use_sites_in_expr_with_line(
                    source,
                    cause,
                    owner_name,
                    owner_type_name,
                    Some(line),
                    sites,
                );
            }
        }
        Stmt::Delete(delete_stmt) => {
            for target in &delete_stmt.targets {
                collect_expression_use_sites_in_expr_with_line(
                    source,
                    target,
                    owner_name,
                    owner_type_name,
                    Some(line),
                    sites,
                );
            }
        }
        _ => {}
    }
}

fn collect_expression_use_sites_in_decorator_exprs(
    source: &str,
    decorators: &[ruff_python_ast::Decorator],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<ExpressionUseSite>,
) {
    for decorator in decorators {
        collect_expression_use_sites_in_expr(
            source,
            &decorator.expression,
            owner_name,
            owner_type_name,
            sites,
        );
    }
}

fn collect_expression_use_sites_in_parameter_default_exprs(
    source: &str,
    parameters: &ruff_python_ast::Parameters,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<ExpressionUseSite>,
) {
    for parameter in &parameters.posonlyargs {
        if let Some(default) = parameter.default() {
            collect_expression_use_sites_in_expr(
                source,
                default,
                owner_name,
                owner_type_name,
                sites,
            );
        }
    }
    for parameter in &parameters.args {
        if let Some(default) = parameter.default() {
            collect_expression_use_sites_in_expr(
                source,
                default,
                owner_name,
                owner_type_name,
                sites,
            );
        }
    }
    for parameter in &parameters.kwonlyargs {
        if let Some(default) = parameter.default() {
            collect_expression_use_sites_in_expr(
                source,
                default,
                owner_name,
                owner_type_name,
                sites,
            );
        }
    }
}

fn collect_guard_expression_use_sites_in_expr_with_line(
    source: &str,
    expr: &Expr,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    line_override: Option<usize>,
    sites: &mut Vec<ExpressionUseSite>,
) {
    let mut collector = ExpressionUseSiteCollector {
        source,
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        suppressed_names: std::collections::BTreeSet::new(),
        line_override,
        sites,
    };
    collector.visit_guard_expr(expr);
}

fn collect_expression_use_sites_in_expr(
    source: &str,
    expr: &Expr,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<ExpressionUseSite>,
) {
    collect_expression_use_sites_in_expr_with_line(
        source,
        expr,
        owner_name,
        owner_type_name,
        None,
        sites,
    );
}

fn collect_expression_use_sites_in_expr_with_line(
    source: &str,
    expr: &Expr,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    line_override: Option<usize>,
    sites: &mut Vec<ExpressionUseSite>,
) {
    let mut collector = ExpressionUseSiteCollector {
        source,
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        suppressed_names: std::collections::BTreeSet::new(),
        line_override,
        sites,
    };
    collector.visit_expr(expr);
}

struct ExpressionUseSiteCollector<'source, 'sites> {
    source: &'source str,
    owner_name: Option<String>,
    owner_type_name: Option<String>,
    suppressed_names: std::collections::BTreeSet<String>,
    line_override: Option<usize>,
    sites: &'sites mut Vec<ExpressionUseSite>,
}

impl<'source, 'sites, 'ast> visitor::Visitor<'ast> for ExpressionUseSiteCollector<'source, 'sites> {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if self.visit_comprehension_expr(expr) {
            return;
        }

        for value in expression_use_site_metadata(self.source, expr) {
            self.push_expression_use_site(expr, value);
        }

        visitor::walk_expr(self, expr);
    }
}

impl ExpressionUseSiteCollector<'_, '_> {
    fn push_expression_use_site(&mut self, expr: &Expr, value: DirectExprMetadata) {
        self.sites.push(ExpressionUseSite {
            owner_name: self.owner_name.clone(),
            owner_type_name: self.owner_type_name.clone(),
            suppressed_names: self.suppressed_names.iter().cloned().collect(),
            value,
            line: self.line_override.unwrap_or_else(|| {
                offset_to_line_column(self.source, expr.range().start().to_usize()).0
            }),
        });
    }

    fn visit_guard_expr(&mut self, expr: &Expr) {
        self.push_expression_use_site(
            expr,
            synthetic_operation_metadata(
                "truthiness",
                extract_direct_expr_metadata(self.source, expr),
                None,
            ),
        );
        self.visit_expr(expr);
    }

    fn visit_comprehension_expr(&mut self, expr: &Expr) -> bool {
        match expr {
            Expr::ListComp(comp) => {
                self.visit_comprehension(&comp.generators, None, &comp.elt);
                true
            }
            Expr::SetComp(comp) => {
                self.visit_comprehension(&comp.generators, None, &comp.elt);
                true
            }
            Expr::DictComp(comp) => {
                self.visit_comprehension(&comp.generators, Some(&comp.key), &comp.value);
                true
            }
            Expr::Generator(comp) => {
                self.visit_comprehension(&comp.generators, None, &comp.elt);
                true
            }
            _ => false,
        }
    }

    fn visit_comprehension(
        &mut self,
        generators: &[ruff_python_ast::Comprehension],
        key: Option<&Expr>,
        element: &Expr,
    ) {
        let outer_suppressed_names = self.suppressed_names.clone();
        for generator in generators {
            self.visit_expr(&generator.iter);
            self.suppressed_names.extend(extract_assignment_names(&generator.target));
            for condition in &generator.ifs {
                self.visit_guard_expr(condition);
            }
        }
        if let Some(key) = key {
            self.visit_expr(key);
        }
        self.visit_expr(element);
        self.suppressed_names = outer_suppressed_names;
    }
}

fn expression_use_site_metadata(source: &str, expr: &Expr) -> Vec<DirectExprMetadata> {
    match expr {
        Expr::Subscript(_) | Expr::BinOp(_) => vec![extract_direct_expr_metadata(source, expr)],
        Expr::BoolOp(bool_op) => bool_op
            .values
            .iter()
            .map(|value| {
                synthetic_operation_metadata(
                    match bool_op.op {
                        ruff_python_ast::BoolOp::And => "and",
                        ruff_python_ast::BoolOp::Or => "or",
                    },
                    extract_direct_expr_metadata(source, value),
                    None,
                )
            })
            .collect(),
        Expr::UnaryOp(unary) => unary_operator_text(unary.op)
            .map(|operator| {
                vec![synthetic_operation_metadata(
                    operator,
                    extract_direct_expr_metadata(source, &unary.operand),
                    None,
                )]
            })
            .unwrap_or_default(),
        Expr::Compare(compare) => compare
            .ops
            .iter()
            .zip(compare.comparators.iter())
            .scan(compare.left.as_ref(), |left, (operator, right)| {
                let metadata = (!compare_operator_is_identity(*operator)).then(|| {
                    synthetic_operation_metadata(
                        compare_operator_text(*operator),
                        extract_direct_expr_metadata(source, left),
                        Some(extract_direct_expr_metadata(source, right)),
                    )
                });
                *left = right;
                Some(metadata)
            })
            .flatten()
            .collect(),
        Expr::If(if_expr) => vec![synthetic_operation_metadata(
            "truthiness",
            extract_direct_expr_metadata(source, &if_expr.test),
            None,
        )],
        _ => Vec::new(),
    }
}

fn synthetic_operation_metadata(
    operator: impl Into<String>,
    left: DirectExprMetadata,
    right: Option<DirectExprMetadata>,
) -> DirectExprMetadata {
    DirectExprMetadata {
        value_binop_left: Some(Box::new(left)),
        value_binop_right: right.map(Box::new),
        value_binop_operator: Some(operator.into()),
        ..DirectExprMetadata::empty()
    }
}

fn unary_operator_text(operator: ruff_python_ast::UnaryOp) -> Option<&'static str> {
    match operator {
        ruff_python_ast::UnaryOp::Not => Some("not"),
        ruff_python_ast::UnaryOp::UAdd => Some("+"),
        ruff_python_ast::UnaryOp::USub => Some("-"),
        ruff_python_ast::UnaryOp::Invert => Some("~"),
    }
}

fn compare_operator_text(operator: ruff_python_ast::CmpOp) -> &'static str {
    match operator {
        ruff_python_ast::CmpOp::Eq => "==",
        ruff_python_ast::CmpOp::NotEq => "!=",
        ruff_python_ast::CmpOp::Lt => "<",
        ruff_python_ast::CmpOp::LtE => "<=",
        ruff_python_ast::CmpOp::Gt => ">",
        ruff_python_ast::CmpOp::GtE => ">=",
        ruff_python_ast::CmpOp::In => "in",
        ruff_python_ast::CmpOp::NotIn => "not in",
        ruff_python_ast::CmpOp::Is => "is",
        ruff_python_ast::CmpOp::IsNot => "is not",
    }
}

fn compare_operator_is_identity(operator: ruff_python_ast::CmpOp) -> bool {
    matches!(operator, ruff_python_ast::CmpOp::Is | ruff_python_ast::CmpOp::IsNot)
}

pub(super) fn collect_typed_dict_literal_sites_in_suite(
    source: &str,
    suite: &[Stmt],
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
    sites: &mut Vec<TypedDictLiteralSite>,
) {
    for stmt in suite {
        match stmt {
            Stmt::FunctionDef(function) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &function.body,
                    Some(function.name.as_str()),
                    owner_type_name,
                    sites,
                );
            }
            Stmt::ClassDef(class_def) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &class_def.body,
                    owner_name,
                    Some(class_def.name.as_str()),
                    sites,
                );
            }
            Stmt::Try(try_stmt) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &try_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for handler in &try_stmt.handlers {
                    let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                    collect_typed_dict_literal_sites_in_suite(
                        source,
                        &handler.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &try_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &try_stmt.finalbody,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::If(if_stmt) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &if_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                for_each_if_false_suite(if_stmt, |suite| {
                    collect_typed_dict_literal_sites_in_suite(
                        source,
                        suite,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                });
            }
            Stmt::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_typed_dict_literal_sites_in_suite(
                        source,
                        &case.body,
                        owner_name,
                        owner_type_name,
                        sites,
                    );
                }
            }
            Stmt::For(for_stmt) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &for_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &for_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::While(while_stmt) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &while_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &while_stmt.orelse,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::With(with_stmt) => {
                collect_typed_dict_literal_sites_in_suite(
                    source,
                    &with_stmt.body,
                    owner_name,
                    owner_type_name,
                    sites,
                );
            }
            Stmt::AnnAssign(assign) => {
                let line = offset_to_line_column(source, assign.range.start().to_usize()).0;
                if let Some(site) = extract_typed_dict_literal_site(
                    source,
                    assign,
                    line,
                    owner_name,
                    owner_type_name,
                ) {
                    sites.push(site);
                }
            }
            _ => {}
        }
    }
}

pub(super) struct UnsafeOperationCollector<'source> {
    source: &'source str,
    unsafe_ranges: Vec<(usize, usize)>,
    sites: Vec<UnsafeOperationSite>,
}

impl<'source, 'ast> visitor::Visitor<'ast> for UnsafeOperationCollector<'source> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match stmt {
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    self.push_unsafe_write_target(target);
                }
            }
            Stmt::AnnAssign(assign) => self.push_unsafe_write_target(&assign.target),
            Stmt::AugAssign(assign) => self.push_unsafe_write_target(&assign.target),
            Stmt::Delete(delete) => {
                for target in &delete.targets {
                    self.push_unsafe_write_target(target);
                }
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &'ast Expr) {
        if let Expr::Call(call) = expr {
            let Expr::Name(name) = call.func.as_ref() else {
                visitor::walk_expr(self, expr);
                return;
            };
            match name.id.as_str() {
                "eval" => self.push_site(expr.range(), UnsafeOperationKind::EvalCall),
                "exec" => self.push_site(expr.range(), UnsafeOperationKind::ExecCall),
                "setattr" if call.arguments.args.len() >= 2 => {
                    if !matches!(call.arguments.args[1], Expr::StringLiteral(_)) {
                        self.push_site(expr.range(), UnsafeOperationKind::SetAttrNonLiteral);
                    }
                }
                "delattr" if call.arguments.args.len() >= 2 => {
                    if !matches!(call.arguments.args[1], Expr::StringLiteral(_)) {
                        self.push_site(expr.range(), UnsafeOperationKind::DelAttrNonLiteral);
                    }
                }
                _ => {}
            }
        }
        visitor::walk_expr(self, expr);
    }
}

impl<'source> UnsafeOperationCollector<'source> {
    fn push_unsafe_write_target(&mut self, target: &Expr) {
        if let Some(kind) = unsafe_write_target_kind(target) {
            self.push_site(target.range(), kind);
        }
    }

    fn push_site(&mut self, range: ruff_text_size::TextRange, kind: UnsafeOperationKind) {
        let line = offset_to_line_column(self.source, range.start().to_usize()).0;
        let in_unsafe_block =
            self.unsafe_ranges.iter().any(|(start, end)| *start <= line && line <= *end);
        self.sites.push(UnsafeOperationSite { kind, line, in_unsafe_block });
    }
}

pub(super) fn unsafe_write_target_kind(target: &Expr) -> Option<UnsafeOperationKind> {
    match target {
        Expr::Subscript(subscript) => match subscript.value.as_ref() {
            Expr::Call(call) => {
                let Expr::Name(name) = call.func.as_ref() else {
                    return None;
                };
                match name.id.as_str() {
                    "globals" => Some(UnsafeOperationKind::GlobalsWrite),
                    "locals" => Some(UnsafeOperationKind::LocalsWrite),
                    _ => None,
                }
            }
            Expr::Attribute(attribute) if attribute.attr.as_str() == "__dict__" => {
                Some(UnsafeOperationKind::DictWrite)
            }
            _ => None,
        },
        Expr::Attribute(attribute) if attribute.attr.as_str() == "__dict__" => {
            Some(UnsafeOperationKind::DictWrite)
        }
        _ => None,
    }
}

pub(super) fn collect_unsafe_block_ranges(
    source: &str,
    statements: &[SyntaxStatement],
) -> Vec<(usize, usize)> {
    let unsafe_lines = statements
        .iter()
        .filter_map(|statement| match statement {
            SyntaxStatement::Unsafe(statement) => Some(statement.line),
            _ => None,
        })
        .collect::<Vec<_>>();
    let lines = source.lines().collect::<Vec<_>>();
    unsafe_lines
        .into_iter()
        .filter_map(|line_number| {
            let header = lines.get(line_number.saturating_sub(1))?;
            let header_indent =
                header.chars().take_while(|character| character.is_whitespace()).count();
            let mut end_line = line_number;
            for (index, line) in lines.iter().enumerate().skip(line_number) {
                let trimmed = line.trim_start();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let indent = line.chars().take_while(|character| character.is_whitespace()).count();
                if indent <= header_indent {
                    break;
                }
                end_line = index + 1;
            }
            Some((line_number, end_line))
        })
        .collect()
}

pub(super) fn extract_typed_dict_literal_site(
    source: &str,
    assign: &ruff_python_ast::StmtAnnAssign,
    line: usize,
    owner_name: Option<&str>,
    owner_type_name: Option<&str>,
) -> Option<TypedDictLiteralSite> {
    let annotation = slice_range(source, assign.annotation.range())?.to_owned();
    let value = assign.value.as_deref()?.as_dict_expr()?;
    let entries = extract_typed_dict_literal_entries(source, value);
    Some(TypedDictLiteralSite {
        annotation,
        entries,
        owner_name: owner_name.map(str::to_owned),
        owner_type_name: owner_type_name.map(str::to_owned),
        line,
    })
}

pub(super) fn extract_typed_dict_literal_entries(
    source: &str,
    value: &ruff_python_ast::ExprDict,
) -> Vec<TypedDictLiteralEntry> {
    value
        .iter()
        .map(|item| TypedDictLiteralEntry {
            key: item.key.as_ref().and_then(|key| extract_string_literal_value(source, key)),
            key_value: item
                .key
                .as_ref()
                .map(|key| Box::new(extract_direct_expr_metadata(source, key))),
            is_expansion: item.key.is_none(),
            value: extract_direct_expr_metadata(source, &item.value),
        })
        .collect()
}

pub(super) fn extract_string_literal_value(source: &str, expr: &Expr) -> Option<String> {
    let Expr::StringLiteral(_) = expr else {
        return None;
    };
    let raw = slice_range(source, expr.range())?.trim();
    let quote_start = raw.find(['\'', '"'])?;
    let quoted = &raw[quote_start..];
    if let Some(inner) =
        quoted.strip_prefix("\"\"\"").and_then(|inner| inner.strip_suffix("\"\"\""))
    {
        return Some(inner.to_owned());
    }
    if let Some(inner) =
        quoted.strip_prefix("'''").and_then(|inner| inner.strip_suffix("'''")).map(str::to_owned)
    {
        return Some(inner);
    }
    if let Some(inner) = quoted.strip_prefix('"').and_then(|inner| inner.strip_suffix('"')) {
        return Some(inner.to_owned());
    }
    quoted.strip_prefix('\'').and_then(|inner| inner.strip_suffix('\'')).map(str::to_owned)
}
