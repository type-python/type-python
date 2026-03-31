use super::*;

#[must_use]
pub fn collect_effective_callable_stub_overrides(
    graph: &ModuleGraph,
) -> Vec<EffectiveCallableStubOverride> {
    let context = CheckerContext::new(&graph.nodes, ImportFallback::Unknown);
    let mut overrides = graph
        .nodes
        .iter()
        .filter(|node| node.module_kind == SourceKind::TypePython)
        .flat_map(|node| {
            node.declarations
                .iter()
                .filter(|declaration| declaration.kind == DeclarationKind::Function)
                .filter_map(|declaration| {
                    let site =
                        resolve_decorated_callable_site_with_context(&context, node, declaration)?;
                    let callable =
                        resolve_decorated_callable_annotation_for_declaration_with_context(
                            &context,
                            node,
                            context.nodes,
                            declaration,
                        )?;
                    let params =
                        direct_function_signature_sites_from_callable_annotation(&callable)?;
                    let returns =
                        decorated_function_return_type_from_callable_annotation(&callable)?;
                    Some(EffectiveCallableStubOverride {
                        module_key: node.module_key.clone(),
                        owner_type_name: site.owner_type_name,
                        name: site.name,
                        line: site.line,
                        params: function_params_from_direct_sites(&params),
                        returns,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    overrides.sort_by(|left, right| {
        left.module_key
            .cmp(&right.module_key)
            .then(left.owner_type_name.cmp(&right.owner_type_name))
            .then(left.line.cmp(&right.line))
            .then(left.name.cmp(&right.name))
    });
    overrides
}

#[must_use]
pub fn collect_synthetic_method_stubs(graph: &ModuleGraph) -> Vec<SyntheticMethodStub> {
    let context = CheckerContext::new(&graph.nodes, ImportFallback::Unknown);
    let mut methods = graph
        .nodes
        .iter()
        .filter(|node| node.module_kind == SourceKind::TypePython)
        .flat_map(|node| {
            let module_info =
                context.load_dataclass_transform_module_info(node).unwrap_or_default();
            node.declarations
                .iter()
                .filter(|declaration| {
                    declaration.owner.is_none() && declaration.kind == DeclarationKind::Class
                })
                .filter_map(|declaration| {
                    let class_line = module_info
                        .classes
                        .iter()
                        .find(|class_site| class_site.name == declaration.name)
                        .map(|class_site| class_site.line)?;
                    let shape = resolve_dataclass_transform_class_shape_from_decl(
                        &graph.nodes,
                        node,
                        declaration,
                        &mut BTreeSet::new(),
                    )
                    .or_else(|| {
                        resolve_plain_dataclass_class_shape_from_decl(
                            &graph.nodes,
                            node,
                            declaration,
                            &mut BTreeSet::new(),
                        )
                    })?;
                    if shape.has_explicit_init {
                        return None;
                    }
                    let mut params = vec![typepython_syntax::FunctionParam {
                        name: String::from("self"),
                        annotation: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    }];
                    params.extend(shape.fields.iter().map(|field| {
                        typepython_syntax::FunctionParam {
                            name: field.keyword_name.clone(),
                            annotation: Some(field.annotation.clone()),
                            has_default: !field.required,
                            positional_only: false,
                            keyword_only: field.kw_only,
                            variadic: false,
                            keyword_variadic: false,
                        }
                    }));
                    Some(SyntheticMethodStub {
                        module_key: node.module_key.clone(),
                        owner_type_name: declaration.name.clone(),
                        class_line,
                        name: String::from("__init__"),
                        method_kind: typepython_syntax::MethodKind::Instance,
                        params,
                        returns: Some(String::from("None")),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    methods.sort_by(|left, right| {
        left.module_key
            .cmp(&right.module_key)
            .then(left.owner_type_name.cmp(&right.owner_type_name))
            .then(left.class_line.cmp(&right.class_line))
            .then(left.name.cmp(&right.name))
    });
    methods
}

fn function_params_from_direct_sites(
    params: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<typepython_syntax::FunctionParam> {
    params
        .iter()
        .map(|param| typepython_syntax::FunctionParam {
            name: param.name.clone(),
            annotation: param.annotation.clone(),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect()
}
