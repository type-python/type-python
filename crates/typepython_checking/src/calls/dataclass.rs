pub(super) fn resolve_synthesized_dataclass_class_shape(
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<DataclassTransformClassShape> {
    let context = CheckerContext::new(nodes, ImportFallback::Unknown, None);
    resolve_synthesized_dataclass_class_shape_with_context(&context, node, nodes, callee)
}

pub(super) fn resolve_synthesized_dataclass_class_shape_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<DataclassTransformClassShape> {
    resolve_dataclass_transform_class_shape_with_context(context, node, nodes, callee)
        .or_else(|| resolve_plain_dataclass_class_shape_with_context(context, node, nodes, callee))
}

pub(super) fn resolve_plain_dataclass_class_shape_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<DataclassTransformClassShape> {
    let (class_node, class_decl) = resolve_direct_base(nodes, node, callee)?;
    resolve_plain_dataclass_class_shape_from_decl_with_context(
        context,
        nodes,
        class_node,
        class_decl,
        &mut BTreeSet::new(),
    )
}

pub(super) fn resolve_plain_dataclass_class_shape_from_decl_with_context(
    context: &CheckerContext<'_>,
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visiting: &mut BTreeSet<(String, String)>,
) -> Option<DataclassTransformClassShape> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visiting.insert(key) {
        return None;
    }

    let info = load_dataclass_transform_module_info_with_context(context, class_node)?;
    let class_site = info.classes.iter().find(|class_site| class_site.name == class_decl.name)?;
    let is_plain_dataclass = class_decl.class_kind == Some(DeclarationOwnerKind::DataClass)
        || class_site
            .decorators
            .iter()
            .any(|decorator| matches!(decorator.as_str(), "dataclass" | "dataclasses.dataclass"));
    if !is_plain_dataclass {
        return None;
    }

    let has_explicit_init = !class_site.plain_dataclass_init
        || class_site.methods.iter().any(|method| method == "__init__");

    let mut fields = Vec::new();
    for base in &class_site.bases {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) else {
            continue;
        };
        let mut branch_visiting = visiting.clone();
        let inherited = resolve_plain_dataclass_class_shape_from_decl_with_context(
            context,
            nodes,
            base_node,
            base_decl,
            &mut branch_visiting,
        )
        .or_else(|| {
            resolve_dataclass_transform_class_shape_from_decl_with_context(
                context,
                nodes,
                base_node,
                base_decl,
                &mut branch_visiting,
            )
        });
        let Some(inherited) = inherited else {
            continue;
        };
        for field in inherited.fields {
            if let Some(index) = fields
                .iter()
                .position(|existing: &DataclassTransformFieldShape| existing.name == field.name)
            {
                fields.remove(index);
            }
            fields.push(field);
        }
    }

    let local_fields = class_site
        .fields
        .iter()
        .filter(|field| !field.is_class_var)
        .filter_map(|field| {
            let recognized_field_specifier = field
                .field_specifier_name
                .as_ref()
                .is_some_and(|name| matches!(name.as_str(), "field" | "dataclasses.field"));
            if recognized_field_specifier && field.field_specifier_init == Some(false) {
                return None;
            }
            Some(DataclassTransformFieldShape {
                name: field.name.clone(),
                keyword_name: field.name.clone(),
                annotation: rewrite_imported_typing_aliases(class_node, &field.annotation),
                required: if recognized_field_specifier {
                    !(field.field_specifier_has_default
                        || field.field_specifier_has_default_factory)
                } else {
                    !field.has_default
                },
                kw_only: if recognized_field_specifier {
                    field.field_specifier_kw_only.unwrap_or(class_site.plain_dataclass_kw_only)
                } else {
                    class_site.plain_dataclass_kw_only
                },
            })
        })
        .collect::<Vec<_>>();
    for field in local_fields {
        if let Some(index) = fields
            .iter()
            .position(|existing: &DataclassTransformFieldShape| existing.name == field.name)
        {
            fields.remove(index);
        }
        fields.push(field);
    }

    Some(DataclassTransformClassShape {
        fields,
        frozen: class_site.plain_dataclass_frozen,
        has_explicit_init,
    })
}

pub(super) fn resolve_known_plain_dataclass_shape_from_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<DataclassTransformClassShape> {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    resolve_plain_dataclass_class_shape_with_context(context, node, nodes, &type_name)
}

pub(super) fn resolve_dataclass_transform_class_shape_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    callee: &str,
) -> Option<DataclassTransformClassShape> {
    let (class_node, class_decl) = resolve_direct_base(nodes, node, callee)?;
    resolve_dataclass_transform_class_shape_from_decl_with_context(
        context,
        nodes,
        class_node,
        class_decl,
        &mut BTreeSet::new(),
    )
}

pub(super) fn resolve_known_dataclass_transform_shape_from_type_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
    nodes: &[typepython_graph::ModuleNode],
    type_name: &str,
) -> Option<DataclassTransformClassShape> {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    resolve_dataclass_transform_class_shape_with_context(context, node, nodes, &type_name)
}

pub(super) fn resolve_dataclass_transform_metadata_from_decl_with_context(
    context: &CheckerContext<'_>,
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visiting: &mut BTreeSet<(String, String)>,
) -> Option<typepython_syntax::DataclassTransformMetadata> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visiting.insert(key) {
        return None;
    }

    let info = load_dataclass_transform_module_info_with_context(context, class_node)?;
    let class_site = info.classes.iter().find(|class_site| class_site.name == class_decl.name)?;

    for decorator in &class_site.decorators {
        if let Some(provider) =
            resolve_dataclass_transform_provider_with_context(context, nodes, class_node, decorator)
        {
            return Some(provider.metadata.clone());
        }
    }
    if let Some(provider_name) = class_site
        .bases
        .iter()
        .find(|base| {
            resolve_dataclass_transform_provider_with_context(context, nodes, class_node, base)
                .is_some()
        })
    {
        return resolve_dataclass_transform_provider_with_context(
            context,
            nodes,
            class_node,
            provider_name,
        )
        .map(|provider| provider.metadata.clone());
    }
    if let Some(metaclass) = class_site.metaclass.as_deref() {
        if let Some(provider) =
            resolve_dataclass_transform_provider_with_context(context, nodes, class_node, metaclass)
        {
            return Some(provider.metadata.clone());
        }
    }

    class_site.bases.iter().find_map(|base| {
        let (base_node, base_decl) = resolve_direct_base(nodes, class_node, base)?;
        let mut branch_visiting = visiting.clone();
        resolve_dataclass_transform_metadata_from_decl_with_context(
            context,
            nodes,
            base_node,
            base_decl,
            &mut branch_visiting,
        )
    })
}

pub(super) fn resolve_dataclass_transform_class_shape_from_decl_with_context(
    context: &CheckerContext<'_>,
    nodes: &[typepython_graph::ModuleNode],
    class_node: &typepython_graph::ModuleNode,
    class_decl: &Declaration,
    visiting: &mut BTreeSet<(String, String)>,
) -> Option<DataclassTransformClassShape> {
    let key = (class_node.module_key.clone(), class_decl.name.clone());
    if !visiting.insert(key) {
        return None;
    }

    let has_explicit_init = class_node.declarations.iter().any(|declaration| {
        declaration.owner.as_ref().is_some_and(|owner| owner.name == class_decl.name)
            && declaration.name == "__init__"
            && declaration.kind == DeclarationKind::Function
    });

    let info = load_dataclass_transform_module_info_with_context(context, class_node)?;
    let class_site = info.classes.iter().find(|class_site| class_site.name == class_decl.name)?;

    let mut metadata = None;
    for decorator in &class_site.decorators {
        if let Some(provider) =
            resolve_dataclass_transform_provider_with_context(context, nodes, class_node, decorator)
        {
            metadata = Some(provider.metadata.clone());
            break;
        }
    }
    if metadata.is_none() {
        if let Some(provider_name) = class_site
            .bases
            .iter()
            .find(|base| {
                resolve_dataclass_transform_provider_with_context(context, nodes, class_node, base)
                    .is_some()
            })
        {
            metadata = resolve_dataclass_transform_provider_with_context(
                context,
                nodes,
                class_node,
                provider_name,
            )
            .map(|provider| provider.metadata.clone());
        }
    }
    if metadata.is_none() {
        metadata = class_site
            .metaclass
            .as_deref()
            .and_then(|metaclass| {
                resolve_dataclass_transform_provider_with_context(
                    context,
                    nodes,
                    class_node,
                    metaclass,
                )
            })
            .map(|provider| provider.metadata.clone());
    }
    if metadata.is_none() {
        metadata = class_site.bases.iter().find_map(|base| {
            let (base_node, base_decl) = resolve_direct_base(nodes, class_node, base)?;
            let mut branch_visiting = visiting.clone();
            resolve_dataclass_transform_metadata_from_decl_with_context(
                context,
                nodes,
                base_node,
                base_decl,
                &mut branch_visiting,
            )
        });
    }
    let metadata = metadata?;

    let mut fields = Vec::new();
    for base in &class_site.bases {
        let Some((base_node, base_decl)) = resolve_direct_base(nodes, class_node, base) else {
            continue;
        };
        let mut branch_visiting = visiting.clone();
        let Some(base_shape) = resolve_dataclass_transform_class_shape_from_decl_with_context(
            context,
            nodes,
            base_node,
            base_decl,
            &mut branch_visiting,
        ) else {
            continue;
        };
        for field in base_shape.fields {
            if let Some(index) = fields
                .iter()
                .position(|existing: &DataclassTransformFieldShape| existing.name == field.name)
            {
                fields.remove(index);
            }
            fields.push(field);
        }
    }

    for field in &class_site.fields {
        if field.is_class_var {
            continue;
        }
        let recognized_specifier = field.field_specifier_name.as_ref().is_some_and(|name| {
            metadata
                .field_specifiers
                .iter()
                .any(|candidate| candidate == name || candidate.ends_with(&format!(".{name}")))
        });
        if !recognized_specifier
            && field
                .value_metadata
                .as_ref()
                .and_then(|metadata| {
                    resolve_direct_expression_type_from_metadata(
                        class_node,
                        nodes,
                        None,
                        None,
                        Some(&class_decl.name),
                        field.line,
                        metadata,
                    )
                })
                .is_some_and(|value_type| is_descriptor_type(nodes, class_node, &value_type))
        {
            continue;
        }
        let init =
            if recognized_specifier { field.field_specifier_init.unwrap_or(true) } else { true };
        if !init {
            continue;
        }
        let required = if recognized_specifier {
            !(field.field_specifier_has_default
                || field.field_specifier_has_default_factory
                || (field.has_default && field.field_specifier_name.is_none()))
        } else {
            !field.has_default
        };
        let kw_only = if recognized_specifier {
            field.field_specifier_kw_only.unwrap_or(metadata.kw_only_default)
        } else {
            metadata.kw_only_default
        };
        let synthesized = DataclassTransformFieldShape {
            name: field.name.clone(),
            keyword_name: field.field_specifier_alias.clone().unwrap_or_else(|| field.name.clone()),
            annotation: rewrite_imported_typing_aliases(class_node, &field.annotation),
            required,
            kw_only,
        };
        if let Some(index) = fields.iter().position(|existing| existing.name == synthesized.name) {
            fields.remove(index);
        }
        fields.push(synthesized);
    }

    Some(DataclassTransformClassShape {
        fields,
        frozen: metadata.frozen_default,
        has_explicit_init,
    })
}

pub(super) fn is_descriptor_type(
    nodes: &[typepython_graph::ModuleNode],
    node: &typepython_graph::ModuleNode,
    type_name: &str,
) -> bool {
    let type_name = annotated_inner(type_name).unwrap_or_else(|| normalize_type_text(type_name));
    let Some((class_node, class_decl)) = resolve_direct_base(nodes, node, &type_name) else {
        return false;
    };

    ["__get__", "__set__", "__delete__"].iter().any(|member_name| {
        find_member_declaration(nodes, class_node, class_decl, member_name, |declaration| {
            matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
        })
        .is_some()
    })
}

pub(super) fn load_dataclass_transform_module_info_with_context(
    context: &CheckerContext<'_>,
    node: &typepython_graph::ModuleNode,
) -> Option<typepython_syntax::DataclassTransformModuleInfo> {
    context.load_dataclass_transform_module_info(node)
}

pub(super) fn resolve_dataclass_transform_provider_with_context<'a>(
    context: &CheckerContext<'_>,
    nodes: &'a [typepython_graph::ModuleNode],
    node: &'a typepython_graph::ModuleNode,
    name: &str,
) -> Option<typepython_syntax::DataclassTransformProviderSite> {
    if let Some(local) = load_dataclass_transform_module_info_with_context(context, node)?
        .providers
        .into_iter()
        .find(|provider| provider.name == name)
    {
        return Some(local);
    }

    if let Some((module_alias, symbol_name)) = name.rsplit_once('.') {
        if let Some(import) = node.declarations.iter().find(|declaration| {
            declaration.kind == DeclarationKind::Import && declaration.name == module_alias
        }) {
            if let Some(target_node) =
                nodes.iter().find(|candidate| candidate.module_key == import.detail)
            {
                return load_dataclass_transform_module_info_with_context(context, target_node)?
                    .providers
                    .into_iter()
                    .find(|provider| provider.name == symbol_name);
            }
        }
    }

    let import = node.declarations.iter().find(|declaration| {
        declaration.kind == DeclarationKind::Import && declaration.name == name
    })?;
    let (module_key, symbol_name) = import.detail.rsplit_once('.')?;
    let target_node = nodes.iter().find(|candidate| candidate.module_key == module_key)?;
    load_dataclass_transform_module_info_with_context(context, target_node)?
        .providers
        .into_iter()
        .find(|provider| provider.name == symbol_name)
}
