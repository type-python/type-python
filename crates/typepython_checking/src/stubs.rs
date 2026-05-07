use std::collections::BTreeSet;

use typepython_graph::ModuleGraph;
use typepython_syntax::{FunctionParam, MethodKind, SourceKind};

use crate::{
    CheckerContext, CheckerOptions, EffectiveCallableStubOverride, EffectiveValueStubOverride,
    SyntheticMethodStub, SyntheticValueStub,
    decorated_function_return_type_from_callable_annotation,
    direct_function_signature_sites_from_callable_annotation,
    framework_transform_class_supports_generated_members_with_context,
    resolve_dataclass_transform_class_shape_from_decl_with_context,
    resolve_decorated_callable_annotation_for_declaration_with_context,
    resolve_decorated_callable_semantic_type_for_declaration_with_context,
    resolve_decorated_callable_site_with_context,
    resolve_plain_dataclass_class_shape_from_decl_with_context,
};

#[must_use]
pub fn collect_effective_callable_stub_overrides(
    graph: &ModuleGraph,
) -> Vec<EffectiveCallableStubOverride> {
    collect_effective_callable_stub_overrides_with_options(
        graph,
        CheckerOptions::core_project_default(),
    )
}

#[must_use]
pub fn collect_effective_callable_stub_overrides_with_options(
    graph: &ModuleGraph,
    options: CheckerOptions,
) -> Vec<EffectiveCallableStubOverride> {
    let context =
        CheckerContext::new_with_bound_surface_facts_and_options(&graph.nodes, None, None, options);
    let mut overrides = graph
        .nodes
        .iter()
        .filter(|node| node.module_kind == SourceKind::TypePython)
        .flat_map(|node| {
            node.declarations
                .iter()
                .filter(|declaration| {
                    declaration.kind == typepython_binding::DeclarationKind::Function
                })
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
pub fn collect_effective_value_stub_overrides(
    graph: &ModuleGraph,
) -> Vec<EffectiveValueStubOverride> {
    collect_effective_value_stub_overrides_with_options(
        graph,
        CheckerOptions::core_project_default(),
    )
}

#[must_use]
pub fn collect_effective_value_stub_overrides_with_options(
    graph: &ModuleGraph,
    options: CheckerOptions,
) -> Vec<EffectiveValueStubOverride> {
    let context = CheckerContext::new_with_bound_surface_facts_and_options(
        &graph.nodes,
        None,
        None,
        CheckerOptions { strict: true, ..options },
    );
    let mut overrides = graph
        .nodes
        .iter()
        .filter(|node| node.module_kind == SourceKind::TypePython)
        .flat_map(|node| {
            node.declarations
                .iter()
                .filter(|declaration| {
                    declaration.kind == typepython_binding::DeclarationKind::Function
                })
                .filter_map(|declaration| {
                    let site =
                        resolve_decorated_callable_site_with_context(&context, node, declaration)?;
                    if let Some(annotation) =
                        computed_field_value_stub_annotation(&context, node, declaration, &site)
                    {
                        return Some(EffectiveValueStubOverride {
                            module_key: node.module_key.clone(),
                            line: site.line,
                            annotation,
                        });
                    }
                    let transformed =
                        resolve_decorated_callable_semantic_type_for_declaration_with_context(
                            &context,
                            node,
                            context.nodes,
                            declaration,
                        )?;
                    if transformed.callable_parts().is_some() {
                        return None;
                    }
                    Some(EffectiveValueStubOverride {
                        module_key: node.module_key.clone(),
                        line: site.line,
                        annotation: crate::render_semantic_type(&transformed),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    overrides.sort_by(|left, right| {
        left.module_key.cmp(&right.module_key).then(left.line.cmp(&right.line))
    });
    overrides
}

fn computed_field_value_stub_annotation(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    declaration: &typepython_binding::Declaration,
    site: &typepython_syntax::DecoratedCallableSite,
) -> Option<String> {
    let owner = declaration.owner.as_ref()?;
    if !site.decorators.iter().any(|decorator| is_computed_field_decorator_name(decorator)) {
        return None;
    }
    if !framework_transform_class_supports_generated_members_with_context(
        context,
        node,
        &owner.name,
    ) {
        return None;
    }
    declaration
        .callable_signature()
        .and_then(|signature| signature.returns.as_ref())
        .map(typepython_binding::BoundTypeExpr::render)
}

fn is_computed_field_decorator_name(name: &str) -> bool {
    name == "computed_field" || name.ends_with(".computed_field")
}

#[must_use]
pub fn collect_synthetic_value_stubs(graph: &ModuleGraph) -> Vec<SyntheticValueStub> {
    collect_synthetic_value_stubs_with_options(graph, CheckerOptions::core_project_default())
}

#[must_use]
pub fn collect_synthetic_value_stubs_with_options(
    graph: &ModuleGraph,
    options: CheckerOptions,
) -> Vec<SyntheticValueStub> {
    let context =
        CheckerContext::new_with_bound_surface_facts_and_options(&graph.nodes, None, None, options);
    let mut values = graph
        .nodes
        .iter()
        .filter(|node| node.module_kind == SourceKind::TypePython)
        .flat_map(|node| {
            let module_info =
                context.load_dataclass_transform_module_info(node).unwrap_or_default();
            node.declarations
                .iter()
                .filter(|declaration| {
                    declaration.owner.is_none()
                        && declaration.kind == typepython_binding::DeclarationKind::Class
                        && framework_transform_class_supports_generated_members_with_context(
                            &context,
                            node,
                            &declaration.name,
                        )
                })
                .flat_map(|declaration| {
                    let class_line = module_info
                        .classes
                        .iter()
                        .find(|class_site| class_site.name == declaration.name)
                        .map(|class_site| class_site.line);
                    generated_class_value_stubs(node, declaration, class_line)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| {
        left.module_key
            .cmp(&right.module_key)
            .then(left.owner_type_name.cmp(&right.owner_type_name))
            .then(left.class_line.cmp(&right.class_line))
            .then(left.name.cmp(&right.name))
    });
    values
}

fn generated_class_value_stubs(
    node: &typepython_graph::ModuleNode,
    declaration: &typepython_binding::Declaration,
    class_line: Option<usize>,
) -> Vec<SyntheticValueStub> {
    let Some(class_line) = class_line else {
        return Vec::new();
    };
    [("objects", "object"), ("metadata", "dict[str, object]"), ("validators", "dict[str, object]")]
        .into_iter()
        .map(|(name, annotation)| SyntheticValueStub {
            module_key: node.module_key.clone(),
            owner_type_name: declaration.name.clone(),
            class_line,
            name: name.to_owned(),
            annotation: annotation.to_owned(),
        })
        .collect()
}

#[must_use]
pub fn collect_synthetic_method_stubs(graph: &ModuleGraph) -> Vec<SyntheticMethodStub> {
    collect_synthetic_method_stubs_with_options(graph, CheckerOptions::core_project_default())
}

#[must_use]
pub fn collect_synthetic_method_stubs_with_options(
    graph: &ModuleGraph,
    options: CheckerOptions,
) -> Vec<SyntheticMethodStub> {
    let context =
        CheckerContext::new_with_bound_surface_facts_and_options(&graph.nodes, None, None, options);
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
                    declaration.owner.is_none()
                        && declaration.kind == typepython_binding::DeclarationKind::Class
                })
                .flat_map(|declaration| {
                    let class_line = module_info
                        .classes
                        .iter()
                        .find(|class_site| class_site.name == declaration.name)
                        .map(|class_site| class_site.line);
                    let Some(class_line) = class_line else {
                        return Vec::new();
                    };
                    let framework_shape =
                        crate::resolve_framework_transform_class_shape_from_decl_with_context(
                            &context,
                            &graph.nodes,
                            node,
                            declaration,
                            &mut BTreeSet::new(),
                        );
                    let shape = resolve_dataclass_transform_class_shape_from_decl_with_context(
                        &context,
                        &graph.nodes,
                        node,
                        declaration,
                        &mut BTreeSet::new(),
                    )
                    .or_else(|| {
                        resolve_plain_dataclass_class_shape_from_decl_with_context(
                            &context,
                            &graph.nodes,
                            node,
                            declaration,
                            &mut BTreeSet::new(),
                        )
                    })
                    .or_else(|| framework_shape.clone());
                    let Some(shape) = shape else {
                        return Vec::new();
                    };
                    if shape.has_explicit_init {
                        return Vec::new();
                    }
                    let mut params = vec![FunctionParam {
                        name: String::from("self"),
                        annotation: None,
                        annotation_expr: None,
                        has_default: false,
                        positional_only: false,
                        keyword_only: false,
                        variadic: false,
                        keyword_variadic: false,
                    }];
                    params.extend(shape.fields.iter().map(|field| FunctionParam {
                        name: field.keyword_name.clone(),
                        annotation: Some(field.annotation.clone()),
                        annotation_expr: typepython_syntax::TypeExpr::parse(&field.annotation),
                        has_default: !field.required,
                        positional_only: false,
                        keyword_only: field.kw_only,
                        variadic: false,
                        keyword_variadic: false,
                    }));
                    let mut methods = vec![SyntheticMethodStub {
                        module_key: node.module_key.clone(),
                        owner_type_name: declaration.name.clone(),
                        class_line,
                        name: String::from("__init__"),
                        method_kind: MethodKind::Instance,
                        params,
                        returns: Some(String::from("None")),
                    }];
                    if framework_shape.is_some()
                        && framework_transform_class_supports_generated_members_with_context(
                            &context,
                            node,
                            &declaration.name,
                        )
                    {
                        methods.push(generated_model_construct_method_stub(
                            node,
                            declaration,
                            class_line,
                        ));
                    }
                    methods
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

fn generated_model_construct_method_stub(
    node: &typepython_graph::ModuleNode,
    declaration: &typepython_binding::Declaration,
    class_line: usize,
) -> SyntheticMethodStub {
    SyntheticMethodStub {
        module_key: node.module_key.clone(),
        owner_type_name: declaration.name.clone(),
        class_line,
        name: String::from("model_construct"),
        method_kind: MethodKind::Class,
        params: vec![
            FunctionParam {
                name: String::from("cls"),
                annotation: None,
                annotation_expr: None,
                has_default: false,
                positional_only: false,
                keyword_only: false,
                variadic: false,
                keyword_variadic: false,
            },
            FunctionParam {
                name: String::from("_fields_set"),
                annotation: Some(String::from("set[str] | None")),
                annotation_expr: typepython_syntax::TypeExpr::parse("set[str] | None"),
                has_default: true,
                positional_only: false,
                keyword_only: false,
                variadic: false,
                keyword_variadic: false,
            },
            FunctionParam {
                name: String::from("values"),
                annotation: Some(String::from("object")),
                annotation_expr: typepython_syntax::TypeExpr::parse("object"),
                has_default: false,
                positional_only: false,
                keyword_only: false,
                variadic: false,
                keyword_variadic: true,
            },
        ],
        returns: Some(declaration.name.clone()),
    }
}

fn function_params_from_direct_sites(
    params: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<typepython_syntax::FunctionParam> {
    params
        .iter()
        .map(|param| typepython_syntax::FunctionParam {
            name: param.name.clone(),
            annotation: param.annotation.clone(),
            annotation_expr: param
                .annotation
                .as_deref()
                .and_then(typepython_syntax::TypeExpr::parse),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect()
}
