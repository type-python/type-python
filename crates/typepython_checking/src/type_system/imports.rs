pub(super) struct ImportedSymbolSemanticTarget<'a> {
    pub(super) import: &'a Declaration,
    pub(super) provider_node: &'a typepython_graph::ModuleNode,
    pub(super) target_declaration: Option<&'a Declaration>,
}

impl<'a> ImportedSymbolSemanticTarget<'a> {
    pub(super) fn module_target(&self) -> Option<&'a typepython_graph::ModuleNode> {
        self.target_declaration.is_none().then_some(self.provider_node)
    }

    pub(super) fn declaration_target(&self) -> Option<&'a Declaration> {
        self.target_declaration
    }

    pub(super) fn function_provider(
        &self,
    ) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
        let declaration = self.target_declaration?;
        (declaration.owner.is_none() && declaration.kind == DeclarationKind::Function)
            .then_some((self.provider_node, declaration))
    }

    pub(super) fn type_alias_provider(
        &self,
    ) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
        let declaration = self.target_declaration?;
        (declaration.owner.is_none() && declaration.kind == DeclarationKind::TypeAlias)
            .then_some((self.provider_node, declaration))
    }

    pub(super) fn class_provider(
        &self,
    ) -> Option<(&'a typepython_graph::ModuleNode, &'a Declaration)> {
        let declaration = self.target_declaration?;
        (declaration.owner.is_none() && declaration.kind == DeclarationKind::Class)
            .then_some((self.provider_node, declaration))
    }

    pub(super) fn semantic_type(
        &self,
        request_node: &typepython_graph::ModuleNode,
    ) -> Option<SemanticType> {
        let declaration = self.target_declaration?;
        match declaration.kind {
            DeclarationKind::Value => {
                let detail = rewrite_imported_typing_aliases(
                    request_node,
                    &declaration_value_annotation_text(declaration)?,
                );
                normalized_direct_return_annotation(&detail).map(lower_type_text_or_name)
            }
            DeclarationKind::Function => {
                let callable = declaration_callable_semantics(declaration)?;
                Some(SemanticType::Callable {
                    params: SemanticCallableParams::ParamList(
                        callable
                            .semantic_params
                            .iter()
                            .map(SemanticCallableParam::annotation_or_dynamic)
                            .collect(),
                    ),
                    return_type: Box::new(callable.return_type?),
                })
            }
            DeclarationKind::Class => Some(SemanticType::Name(declaration.name.clone())),
            _ => None,
        }
    }
}

pub(super) fn resolve_imported_symbol_semantic_target_from_declaration<'a>(
    nodes: &'a [typepython_graph::ModuleNode],
    declaration: &'a Declaration,
) -> Option<ImportedSymbolSemanticTarget<'a>> {
    let import_target = declaration_import_target_ref(declaration)?;
    if let Some(module_node) =
        nodes.iter().find(|candidate| candidate.module_key == import_target.module_target)
    {
        return Some(ImportedSymbolSemanticTarget {
            import: declaration,
            provider_node: module_node,
            target_declaration: None,
        });
    }

    let symbol_target = import_target.symbol_target?;
    let provider_node =
        nodes.iter().find(|candidate| candidate.module_key == symbol_target.module_key)?;
    let target_declaration = provider_node
        .declarations
        .iter()
        .find(|target| target.owner.is_none() && target.name == symbol_target.symbol_name)?;
    Some(ImportedSymbolSemanticTarget {
        import: declaration,
        provider_node,
        target_declaration: Some(target_declaration),
    })
}

pub(super) fn resolve_imported_symbol_semantic_target<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    local_name: &str,
) -> Option<ImportedSymbolSemanticTarget<'a>> {
    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == local_name
    })?;
    resolve_imported_symbol_semantic_target_from_declaration(nodes, import)
}

pub(super) fn resolve_import_target<'a>(
    _node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    declaration: &'a Declaration,
) -> Option<&'a Declaration> {
    resolve_imported_symbol_semantic_target_from_declaration(nodes, declaration)?
        .declaration_target()
}

pub(super) fn resolve_imported_module_target<'a>(
    node: &'a typepython_graph::ModuleNode,
    nodes: &'a [typepython_graph::ModuleNode],
    local_name: &str,
) -> Option<&'a typepython_graph::ModuleNode> {
    resolve_imported_symbol_semantic_target(node, nodes, local_name)?.module_target()
}

pub(super) fn resolve_imported_module_member_reference_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    owner_name: &str,
    member_name: &str,
) -> Option<SemanticType> {
    let module_target = resolve_imported_symbol_semantic_target(node, nodes, owner_name)?;
    let declaration = module_target
        .module_target()?
        .declarations
        .iter()
        .find(|declaration| declaration.owner.is_none() && declaration.name == member_name)?;
    ImportedSymbolSemanticTarget {
        import: module_target.import,
        provider_node: module_target.provider_node,
        target_declaration: Some(declaration),
    }
    .semantic_type(node)
}

pub(super) fn resolve_imported_module_method_return_semantic_type(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    current_line: usize,
    owner_name: &str,
    method_name: &str,
) -> Option<SemanticType> {
    let module_node = resolve_imported_module_target(node, nodes, owner_name)?;
    let methods = module_node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.owner.is_none()
                && declaration.name == method_name
                && matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .collect::<Vec<_>>();
    let method_return =
        if methods.iter().any(|declaration| declaration.kind == DeclarationKind::Overload) {
            let call = node.method_calls.iter().find(|call| {
                call.owner_name == owner_name
                    && call.method == method_name
                    && !call.through_instance
                    && call.line == current_line
            })?;
            let call = imported_module_method_call_site(module_node, call);
            let overloads = methods
                .iter()
                .copied()
                .filter(|declaration| declaration.kind == DeclarationKind::Overload)
                .collect::<Vec<_>>();
            match resolve_direct_overload_selection(node, nodes, &call, &overloads) {
                ResolvedOverloadSelection::Selected(candidate) => candidate.return_type,
                _ => None,
            }
        } else {
            let call = node.method_calls.iter().find(|call| {
                call.owner_name == owner_name
                    && call.method == method_name
                    && !call.through_instance
                    && call.line == current_line
            })?;
            let call = imported_module_method_call_site(module_node, call);
            resolve_direct_call_candidate(node, nodes, *methods.first()?, &call)?.return_type
        };
    method_return.map(|return_type| rewrite_imported_typing_semantic_type(node, &return_type))
}

pub(super) fn imported_module_method_call_site(
    module_node: &typepython_graph::ModuleNode,
    call: &typepython_binding::MethodCallSite,
) -> typepython_binding::CallSite {
    typepython_binding::CallSite {
        callee: format!("{}.{}", module_node.module_key, call.method),
        arg_count: call.arg_count,
        arg_values: call.arg_values.clone(),
        starred_arg_values: call.starred_arg_values.clone(),
        keyword_names: call.keyword_names.clone(),
        keyword_arg_values: call.keyword_arg_values.clone(),
        keyword_expansion_values: call.keyword_expansion_values.clone(),
        line: 1,
    }
}

pub(super) fn imported_module_method_call_diagnostics(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    call: &typepython_binding::MethodCallSite,
) -> Option<Vec<Diagnostic>> {
    let module_node = resolve_imported_module_target(node, nodes, &call.owner_name)?;
    let direct_call = imported_module_method_call_site(module_node, call);
    let callable_candidates = module_node
        .declarations
        .iter()
        .filter(|declaration| {
            declaration.owner.is_none()
                && declaration.name == call.method
                && matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .collect::<Vec<_>>();
    if callable_candidates.is_empty() {
        let has_member = module_node
            .declarations
            .iter()
            .any(|declaration| declaration.owner.is_none() && declaration.name == call.method);
        return Some(if has_member {
            Vec::new()
        } else {
            vec![Diagnostic::error(
                "TPY4002",
                format!(
                    "module `{}` in module `{}` has no member `{}`",
                    module_node.module_key,
                    node.module_path.display(),
                    call.method
                ),
            )]
        });
    }

    let mut diagnostics = Vec::new();
    let overloads = callable_candidates
        .iter()
        .copied()
        .filter(|declaration| declaration.kind == DeclarationKind::Overload)
        .collect::<Vec<_>>();
    if !overloads.is_empty() {
        match resolve_direct_overload_selection(node, nodes, &direct_call, &overloads) {
            ResolvedOverloadSelection::Selected(candidate) => {
                let signature = candidate.signature_sites;
                if let Some(diagnostic) =
                    direct_source_function_arity_diagnostic(node, nodes, &direct_call, &signature)
                {
                    diagnostics.push(diagnostic);
                }
                diagnostics.extend(direct_source_function_keyword_diagnostics(
                    node,
                    nodes,
                    &direct_call,
                    &signature,
                ));
                diagnostics.extend(direct_source_function_type_diagnostics(
                    node,
                    nodes,
                    &direct_call,
                    &signature,
                ));
                return Some(diagnostics);
            }
            ResolvedOverloadSelection::Ambiguous { applicable_count } => {
                diagnostics.push(Diagnostic::error(
                    "TPY4012",
                    format!(
                        "call to `{}.{}` in module `{}` is ambiguous across {} overloads after applicability filtering",
                        module_node.module_key,
                        call.method,
                        node.module_path.display(),
                        applicable_count
                    ),
                ));
                return Some(diagnostics);
            }
            ResolvedOverloadSelection::NotApplicable { runtime_generic_failures } => {
                if let Some((_declaration, failure)) = runtime_generic_failures.first() {
                    diagnostics.push(unresolved_generic_call_diagnostic(
                        node,
                        call.line,
                        &format!("{}.{}", module_node.module_key, call.method),
                        failure,
                    ));
                    return Some(diagnostics);
                }
            }
        }
    }

    if let Some(failure) =
        direct_call_unresolved_typepack_failure(node, nodes, callable_candidates[0], &direct_call)
    {
        diagnostics.push(unresolved_generic_call_diagnostic(
            node,
            call.line,
            &format!("{}.{}", module_node.module_key, call.method),
            &failure,
        ));
        return Some(diagnostics);
    }

    let signature = match resolve_direct_call_candidate_detailed(
        node,
        nodes,
        callable_candidates[0],
        &direct_call,
    ) {
        Ok(candidate) => candidate.signature_sites,
        Err(failure) if declaration_has_runtime_generic_paramlist(callable_candidates[0]) => {
            diagnostics.push(unresolved_generic_call_diagnostic(
                node,
                call.line,
                &format!("{}.{}", module_node.module_key, call.method),
                &failure,
            ));
            return Some(diagnostics);
        }
        Err(_) => declaration_signature_sites(callable_candidates[0]),
    };
    if let Some(diagnostic) =
        direct_source_function_arity_diagnostic(node, nodes, &direct_call, &signature)
    {
        diagnostics.push(diagnostic);
    }
    diagnostics.extend(direct_source_function_keyword_diagnostics(
        node,
        nodes,
        &direct_call,
        &signature,
    ));
    diagnostics.extend(direct_source_function_type_diagnostics(
        node,
        nodes,
        &direct_call,
        &signature,
    ));
    Some(diagnostics)
}
