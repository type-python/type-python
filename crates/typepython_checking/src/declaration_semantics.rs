use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
};

use super::*;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct DirectSignatureParam {
    pub(crate) name: String,
    pub(crate) annotation: String,
    pub(crate) has_default: bool,
    pub(crate) positional_only: bool,
    pub(crate) keyword_only: bool,
    pub(crate) variadic: bool,
    pub(crate) keyword_variadic: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SemanticCallableDeclaration {
    pub(crate) params: Vec<typepython_syntax::DirectFunctionParamSite>,
    pub(crate) semantic_params: Vec<SemanticCallableParam>,
    pub(crate) return_annotation_text: Option<String>,
    pub(crate) return_type: Option<SemanticType>,
}

impl SemanticCallableDeclaration {
    pub(crate) fn param_names(&self) -> Vec<String> {
        self.params.iter().map(|param| param.name.clone()).collect()
    }

    pub(crate) fn param_types(&self) -> Vec<String> {
        self.params.iter().map(|param| param.annotation.clone().unwrap_or_default()).collect()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SemanticCallableParam {
    pub(crate) name: String,
    pub(crate) annotation_text: Option<String>,
    pub(crate) annotation: Option<SemanticType>,
    pub(crate) has_default: bool,
    pub(crate) positional_only: bool,
    pub(crate) keyword_only: bool,
    pub(crate) variadic: bool,
    pub(crate) keyword_variadic: bool,
}

impl SemanticCallableParam {
    pub(crate) fn annotation_or_dynamic(&self) -> SemanticType {
        self.annotation.clone().unwrap_or_else(|| SemanticType::Name(String::from("dynamic")))
    }
}

pub(crate) fn lower_param_annotation_text(
    annotation: &str,
    variadic: bool,
    keyword_variadic: bool,
) -> SemanticType {
    let annotation = annotation.trim();
    if variadic {
        if let Some(inner) = annotation.strip_prefix('*') {
            return lower_type_text_or_name(&format!("Unpack[{}]", inner.trim()));
        }
    }
    if keyword_variadic && let Some(inner) = annotation.strip_prefix("**") {
        return lower_type_text_or_name(inner.trim());
    }
    lower_type_text_or_name(annotation)
}

fn signature_site_from_semantic_param(
    param: &SemanticCallableParam,
) -> typepython_syntax::DirectFunctionParamSite {
    typepython_syntax::DirectFunctionParamSite {
        name: param.name.clone(),
        annotation: param.annotation_text.clone(),
        annotation_expr: param.annotation.clone().map(semantic_type_to_type_expr),
        has_default: param.has_default,
        positional_only: param.positional_only,
        keyword_only: param.keyword_only,
        variadic: param.variadic,
        keyword_variadic: param.keyword_variadic,
    }
}

fn semantic_type_to_type_expr(ty: SemanticType) -> typepython_syntax::TypeExpr {
    let rendered = diagnostic_type_text(&ty);
    typepython_syntax::TypeExpr::parse(&rendered)
        .unwrap_or(typepython_syntax::TypeExpr::Name(rendered))
}

pub(crate) fn callable_signature_sites_from_semantics(
    callable: &SemanticCallableDeclaration,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    callable.semantic_params.iter().map(signature_site_from_semantic_param).collect()
}

pub(crate) fn callable_signature_sites_with_self_from_semantics(
    callable: &SemanticCallableDeclaration,
    owner_type_name: &str,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    callable_semantic_params_with_self_from_semantics(callable, owner_type_name)
        .iter()
        .map(signature_site_from_semantic_param)
        .collect()
}

pub(crate) fn callable_semantic_params_from_semantics(
    callable: &SemanticCallableDeclaration,
) -> Vec<SemanticCallableParam> {
    callable.semantic_params.clone()
}

pub(crate) fn callable_semantic_params_with_self_from_semantics(
    callable: &SemanticCallableDeclaration,
    owner_type_name: &str,
) -> Vec<SemanticCallableParam> {
    callable
        .semantic_params
        .iter()
        .map(|param| SemanticCallableParam {
            name: param.name.clone(),
            annotation_text: param
                .annotation_text
                .as_deref()
                .map(|annotation| substitute_self_annotation(annotation, Some(owner_type_name))),
            annotation: param
                .annotation
                .as_ref()
                .map(|annotation| substitute_self_semantic_type(annotation, Some(owner_type_name))),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect()
}

pub(crate) fn method_semantic_params_from_semantics(
    declaration: &Declaration,
    callable: &SemanticCallableDeclaration,
    owner_type_name: &str,
) -> Vec<SemanticCallableParam> {
    let params = callable_semantic_params_with_self_from_semantics(callable, owner_type_name);
    match declaration.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
        typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => params,
        typepython_syntax::MethodKind::Instance
        | typepython_syntax::MethodKind::Class
        | typepython_syntax::MethodKind::PropertySetter => params.into_iter().skip(1).collect(),
    }
}

pub(crate) fn substitute_self_semantic_type(
    ty: &SemanticType,
    owner_type_name: Option<&str>,
) -> SemanticType {
    match ty {
        SemanticType::Name(name) if name == "Self" => owner_type_name
            .map(lower_type_text_or_name)
            .unwrap_or_else(|| SemanticType::Name(name.clone())),
        SemanticType::Name(name) => SemanticType::Name(name.clone()),
        SemanticType::Generic { head, args } => SemanticType::Generic {
            head: head.clone(),
            args: args
                .iter()
                .map(|arg| substitute_self_semantic_type(arg, owner_type_name))
                .collect(),
        },
        SemanticType::Callable { params, return_type } => SemanticType::Callable {
            params: substitute_self_semantic_callable_params(params, owner_type_name),
            return_type: Box::new(substitute_self_semantic_type(return_type, owner_type_name)),
        },
        SemanticType::Union(branches) => SemanticType::Union(
            branches
                .iter()
                .map(|branch| substitute_self_semantic_type(branch, owner_type_name))
                .collect(),
        ),
        SemanticType::Annotated { value, metadata } => SemanticType::Annotated {
            value: Box::new(substitute_self_semantic_type(value, owner_type_name)),
            metadata: metadata.clone(),
        },
        SemanticType::Unpack(inner) => {
            SemanticType::Unpack(Box::new(substitute_self_semantic_type(inner, owner_type_name)))
        }
    }
}

pub(crate) fn substitute_self_semantic_callable_params(
    params: &SemanticCallableParams,
    owner_type_name: Option<&str>,
) -> SemanticCallableParams {
    match params {
        SemanticCallableParams::Ellipsis => SemanticCallableParams::Ellipsis,
        SemanticCallableParams::ParamList(types) => SemanticCallableParams::ParamList(
            types.iter().map(|ty| substitute_self_semantic_type(ty, owner_type_name)).collect(),
        ),
        SemanticCallableParams::Concatenate(types) => SemanticCallableParams::Concatenate(
            types.iter().map(|ty| substitute_self_semantic_type(ty, owner_type_name)).collect(),
        ),
        SemanticCallableParams::Single(expr) => SemanticCallableParams::Single(Box::new(
            substitute_self_semantic_type(expr, owner_type_name),
        )),
    }
}

pub(crate) fn callable_return_semantic_type_with_self_from_semantics(
    callable: &SemanticCallableDeclaration,
    owner_type_name: &str,
) -> Option<SemanticType> {
    callable.return_type.as_ref().map(|ty| substitute_self_semantic_type(ty, Some(owner_type_name)))
}

pub(crate) fn method_signature_sites_from_semantics(
    declaration: &Declaration,
    callable: &SemanticCallableDeclaration,
    owner_type_name: &str,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    let params = callable_signature_sites_with_self_from_semantics(callable, owner_type_name);
    match declaration.method_kind.unwrap_or(typepython_syntax::MethodKind::Instance) {
        typepython_syntax::MethodKind::Static | typepython_syntax::MethodKind::Property => params,
        typepython_syntax::MethodKind::Instance
        | typepython_syntax::MethodKind::Class
        | typepython_syntax::MethodKind::PropertySetter => params.into_iter().skip(1).collect(),
    }
}

pub(crate) fn owner_generic_substitutions(
    owner_type: &SemanticType,
    class_declaration: &Declaration,
) -> GenericTypeParamSubstitutions {
    if class_declaration.type_params.is_empty() {
        return GenericTypeParamSubstitutions::default();
    }
    let Some((head, args)) = owner_type.generic_parts() else {
        return GenericTypeParamSubstitutions::default();
    };
    if head != class_declaration.name {
        return GenericTypeParamSubstitutions::default();
    }

    let type_pack_names = class_declaration
        .type_params
        .iter()
        .filter(|type_param| {
            type_param.kind == typepython_binding::GenericTypeParamKind::TypeVarTuple
        })
        .map(|type_param| type_param.name.clone())
        .collect::<BTreeSet<_>>();
    let expanded_args = expand_inferred_generic_args(args, &type_pack_names);
    let mut substitutions = GenericTypeParamSubstitutions::default();
    let mut arg_index = 0usize;

    for (index, type_param) in class_declaration.type_params.iter().enumerate() {
        match type_param.kind {
            typepython_binding::GenericTypeParamKind::TypeVar => {
                let Some(actual) = expanded_args.get(arg_index) else {
                    break;
                };
                substitutions.types.insert(type_param.name.clone(), actual.clone());
                arg_index += 1;
            }
            typepython_binding::GenericTypeParamKind::TypeVarTuple => {
                let trailing_non_pack_params = class_declaration
                    .type_params
                    .iter()
                    .skip(index + 1)
                    .filter(|candidate| {
                        candidate.kind != typepython_binding::GenericTypeParamKind::TypeVarTuple
                    })
                    .count();
                let pack_end = expanded_args.len().saturating_sub(trailing_non_pack_params);
                substitutions.type_packs.insert(
                    type_param.name.clone(),
                    TypePackBinding {
                        types: expanded_args[arg_index..pack_end].to_vec(),
                        variadic_tail: None,
                    },
                );
                arg_index = pack_end;
            }
            typepython_binding::GenericTypeParamKind::ParamSpec => {}
        }
    }

    substitutions
}

pub(crate) fn without_shadowed_generic_params(
    mut substitutions: GenericTypeParamSubstitutions,
    declaration: &Declaration,
) -> GenericTypeParamSubstitutions {
    for type_param in &declaration.type_params {
        substitutions.types.remove(&type_param.name);
        substitutions.param_lists.remove(&type_param.name);
        substitutions.type_packs.remove(&type_param.name);
    }
    substitutions
}

pub(crate) fn substitute_semantic_callable_params(
    params: &[SemanticCallableParam],
    substitutions: &GenericTypeParamSubstitutions,
) -> Vec<SemanticCallableParam> {
    params
        .iter()
        .cloned()
        .map(|mut param| {
            param.annotation = param
                .annotation
                .as_ref()
                .map(|annotation| substitute_semantic_type_params(annotation, substitutions));
            param.annotation_text = param.annotation.as_ref().map(render_semantic_type);
            param
        })
        .collect()
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SemanticValueDeclaration {
    pub(crate) annotation_text: Option<String>,
    pub(crate) annotation: Option<SemanticType>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CachedSemanticCallableDeclaration {
    params: Vec<typepython_syntax::DirectFunctionParamSite>,
    semantic_params: Vec<CachedSemanticCallableParam>,
    return_annotation_text: Option<String>,
    return_type_id: Option<TypeId>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CachedSemanticCallableParam {
    name: String,
    annotation_text: Option<String>,
    annotation_id: Option<TypeId>,
    has_default: bool,
    positional_only: bool,
    keyword_only: bool,
    variadic: bool,
    keyword_variadic: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CachedSemanticValueDeclaration {
    annotation_text: Option<String>,
    annotation_id: Option<TypeId>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SemanticTypeAliasDeclaration {
    pub(crate) head: String,
    pub(crate) type_params: Vec<typepython_binding::GenericTypeParam>,
    pub(crate) body_text: String,
    pub(crate) body: SemanticType,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CachedSemanticTypeAliasDeclaration {
    head: String,
    type_params: Vec<typepython_binding::GenericTypeParam>,
    body_text: String,
    body_id: TypeId,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SemanticImportSymbolTargetRef {
    pub(crate) module_key: String,
    pub(crate) symbol_name: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SemanticImportTargetRef {
    pub(crate) raw_target: String,
    pub(crate) module_target: String,
    pub(crate) symbol_target: Option<SemanticImportSymbolTargetRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub(crate) struct SemanticDeclarationFacts {
    pub(crate) callable: Option<SemanticCallableDeclaration>,
    pub(crate) value: Option<SemanticValueDeclaration>,
    pub(crate) type_alias: Option<SemanticTypeAliasDeclaration>,
    pub(crate) import_target: Option<SemanticImportTargetRef>,
}

#[derive(Debug, Clone, Eq, PartialEq, Default)]
struct CachedSemanticDeclarationFacts {
    callable: Option<CachedSemanticCallableDeclaration>,
    value: Option<CachedSemanticValueDeclaration>,
    type_alias: Option<CachedSemanticTypeAliasDeclaration>,
    import_target: Option<SemanticImportTargetRef>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SemanticDeclarationTypeIds {
    pub(crate) callable_return: Option<TypeId>,
    pub(crate) value_annotation: Option<TypeId>,
    pub(crate) type_alias_body: Option<TypeId>,
}

#[derive(Debug, Default)]
struct DeclarationSemanticCache {
    facts_by_declaration: HashMap<Declaration, CachedSemanticDeclarationFacts>,
    type_store: TypeStore,
}

thread_local! {
    static DECLARATION_SEMANTIC_CACHE: RefCell<DeclarationSemanticCache> =
        RefCell::new(DeclarationSemanticCache::default());
}

fn intern_semantic_type(store: &mut TypeStore, ty: Option<SemanticType>) -> Option<TypeId> {
    ty.map(|ty| store.intern(ty))
}

fn load_interned_semantic_type(store: &TypeStore, type_id: Option<TypeId>) -> Option<SemanticType> {
    type_id.and_then(|type_id| store.get(type_id).cloned())
}

fn build_cached_semantic_declaration_facts(
    declaration: &Declaration,
    type_store: &mut TypeStore,
) -> CachedSemanticDeclarationFacts {
    CachedSemanticDeclarationFacts {
        callable: matches!(declaration.kind, DeclarationKind::Function | DeclarationKind::Overload)
            .then(|| {
                declaration
                    .callable_signature()
                    .map(semantic_callable_from_bound_signature)
                    .or_else(|| parse_direct_callable_declaration(&declaration.rendered_detail()))
            })
            .flatten()
            .map(|callable| CachedSemanticCallableDeclaration {
                params: callable.params,
                semantic_params: callable
                    .semantic_params
                    .into_iter()
                    .map(|param| CachedSemanticCallableParam {
                        name: param.name,
                        annotation_text: param.annotation_text,
                        annotation_id: intern_semantic_type(type_store, param.annotation),
                        has_default: param.has_default,
                        positional_only: param.positional_only,
                        keyword_only: param.keyword_only,
                        variadic: param.variadic,
                        keyword_variadic: param.keyword_variadic,
                    })
                    .collect(),
                return_annotation_text: callable.return_annotation_text,
                return_type_id: intern_semantic_type(type_store, callable.return_type),
            }),
        value: (declaration.kind == DeclarationKind::Value).then(|| {
            let annotation_text = declaration
                .value_annotation()
                .map(typepython_binding::BoundTypeExpr::render)
                .or_else(|| {
                    (!declaration.rendered_detail().trim().is_empty())
                        .then(|| declaration.rendered_detail())
                });
            CachedSemanticValueDeclaration {
                annotation_text: annotation_text.clone(),
                annotation_id: intern_semantic_type(
                    type_store,
                    annotation_text.as_deref().map(lower_type_text_or_name),
                ),
            }
        }),
        type_alias: (declaration.kind == DeclarationKind::TypeAlias).then(|| {
            let body_text = declaration
                .type_alias_value()
                .map(typepython_binding::BoundTypeExpr::render)
                .unwrap_or_else(|| declaration.rendered_detail());
            CachedSemanticTypeAliasDeclaration {
                head: declaration.name.clone(),
                type_params: declaration.type_params.clone(),
                body_text: body_text.clone(),
                body_id: type_store.intern(lower_type_text_or_name(&body_text)),
            }
        }),
        import_target: (declaration.kind == DeclarationKind::Import)
            .then(|| {
                declaration
                    .import_target()
                    .map(semantic_import_target_from_bound_target)
                    .or_else(|| parse_import_target_ref(&declaration.rendered_detail()))
            })
            .flatten(),
    }
}

fn semantic_callable_from_bound_signature(
    signature: &typepython_binding::BoundCallableSignature,
) -> SemanticCallableDeclaration {
    let params = signature.params.clone();
    let semantic_params = params
        .iter()
        .map(|param| SemanticCallableParam {
            name: param.name.clone(),
            annotation_text: param.rendered_annotation(),
            annotation: param.rendered_annotation().as_deref().map(|annotation| {
                lower_param_annotation_text(annotation, param.variadic, param.keyword_variadic)
            }),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect::<Vec<_>>();
    let return_annotation_text =
        signature.returns.as_ref().map(typepython_binding::BoundTypeExpr::render);
    let return_type = return_annotation_text.as_deref().map(lower_type_text_or_name);
    SemanticCallableDeclaration { params, semantic_params, return_annotation_text, return_type }
}

fn semantic_import_target_from_bound_target(
    target: &typepython_binding::BoundImportTarget,
) -> SemanticImportTargetRef {
    SemanticImportTargetRef {
        raw_target: target.raw_target.clone(),
        module_target: target.module_target.clone(),
        symbol_target: target.symbol_target.as_ref().map(|symbol| SemanticImportSymbolTargetRef {
            module_key: symbol.module_key.clone(),
            symbol_name: symbol.symbol_name.clone(),
        }),
    }
}

fn with_cached_semantic_declaration_facts<T>(
    declaration: &Declaration,
    action: impl FnOnce(&CachedSemanticDeclarationFacts, &TypeStore) -> T,
) -> T {
    DECLARATION_SEMANTIC_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.facts_by_declaration.contains_key(declaration) {
            let facts = build_cached_semantic_declaration_facts(declaration, &mut cache.type_store);
            cache.facts_by_declaration.insert(declaration.clone(), facts);
        }

        let facts = cache
            .facts_by_declaration
            .get(declaration)
            .expect("cached declaration semantic facts should exist");
        action(facts, &cache.type_store)
    })
}

fn materialize_callable_semantics(
    callable: &CachedSemanticCallableDeclaration,
    type_store: &TypeStore,
) -> SemanticCallableDeclaration {
    SemanticCallableDeclaration {
        params: callable.params.clone(),
        semantic_params: callable
            .semantic_params
            .iter()
            .map(|param| SemanticCallableParam {
                name: param.name.clone(),
                annotation_text: param.annotation_text.clone(),
                annotation: load_interned_semantic_type(type_store, param.annotation_id),
                has_default: param.has_default,
                positional_only: param.positional_only,
                keyword_only: param.keyword_only,
                variadic: param.variadic,
                keyword_variadic: param.keyword_variadic,
            })
            .collect(),
        return_annotation_text: callable.return_annotation_text.clone(),
        return_type: load_interned_semantic_type(type_store, callable.return_type_id),
    }
}

fn materialize_value_semantics(
    value: &CachedSemanticValueDeclaration,
    type_store: &TypeStore,
) -> SemanticValueDeclaration {
    SemanticValueDeclaration {
        annotation_text: value.annotation_text.clone(),
        annotation: load_interned_semantic_type(type_store, value.annotation_id),
    }
}

fn materialize_type_alias_semantics(
    type_alias: &CachedSemanticTypeAliasDeclaration,
    type_store: &TypeStore,
) -> SemanticTypeAliasDeclaration {
    SemanticTypeAliasDeclaration {
        head: type_alias.head.clone(),
        type_params: type_alias.type_params.clone(),
        body_text: type_alias.body_text.clone(),
        body: type_store
            .get(type_alias.body_id)
            .expect("interned type alias body should exist")
            .clone(),
    }
}

pub(crate) fn parse_direct_signature_params(signature: &str) -> Option<Vec<DirectSignatureParam>> {
    let inner = signature.strip_prefix('(')?.split_once(')')?.0;
    if inner.is_empty() {
        return Some(Vec::new());
    }

    let parts = split_top_level_type_args(inner);
    let slash_index = parts.iter().position(|part| part.trim() == "/");
    let star_index = parts.iter().position(|part| part.trim() == "*");
    let mut params = Vec::new();
    let mut keyword_only_active = false;
    for (index, part) in parts.into_iter().enumerate() {
        let part = part.trim();
        if part == "/" {
            continue;
        }
        if part == "*" {
            keyword_only_active = true;
            continue;
        }

        let has_default = part.ends_with('=');
        let part = part.trim_end_matches('=').trim();
        let (part, variadic, keyword_variadic) = if let Some(part) = part.strip_prefix("**") {
            (part.trim(), false, true)
        } else if let Some(part) = part.strip_prefix('*') {
            keyword_only_active = true;
            (part.trim(), true, false)
        } else {
            (part, false, false)
        };
        let (name, annotation) = part
            .split_once(':')
            .map(|(name, annotation)| (name.trim(), annotation.trim().to_owned()))
            .unwrap_or((part, String::new()));
        params.push(DirectSignatureParam {
            name: name.to_owned(),
            annotation,
            has_default,
            positional_only: slash_index.is_some_and(|slash_index| index < slash_index),
            keyword_only: !variadic
                && !keyword_variadic
                && (star_index.is_some_and(|star_index| index > star_index) || keyword_only_active),
            variadic,
            keyword_variadic,
        });
    }

    Some(params)
}

pub(crate) fn parse_direct_callable_declaration(
    detail: &str,
) -> Option<SemanticCallableDeclaration> {
    let parsed_params = parse_direct_signature_params(detail)?;
    let params = parsed_params
        .into_iter()
        .map(|param| typepython_syntax::DirectFunctionParamSite {
            name: param.name,
            annotation: (!param.annotation.is_empty()).then_some(param.annotation),
            annotation_expr: None,
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect::<Vec<_>>();
    let semantic_params = parse_direct_signature_params(detail)?
        .into_iter()
        .map(|param| typepython_syntax::DirectFunctionParamSite {
            name: param.name,
            annotation: (!param.annotation.is_empty()).then_some(param.annotation),
            annotation_expr: None,
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .map(|param| SemanticCallableParam {
            name: param.name,
            annotation_text: param.annotation.clone(),
            annotation: param.annotation.as_deref().map(|annotation| {
                lower_param_annotation_text(annotation, param.variadic, param.keyword_variadic)
            }),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect::<Vec<_>>();
    let return_annotation_text = detail.split_once("->").map(|(_, right)| right.trim().to_owned());
    let return_type = return_annotation_text.as_deref().map(lower_type_text_or_name);
    Some(SemanticCallableDeclaration {
        params,
        semantic_params,
        return_annotation_text,
        return_type,
    })
}

pub(crate) fn parse_import_target_ref(detail: &str) -> Option<SemanticImportTargetRef> {
    (!detail.is_empty()).then(|| SemanticImportTargetRef {
        raw_target: detail.to_owned(),
        module_target: detail.to_owned(),
        symbol_target: detail.rsplit_once('.').map(|(module_key, symbol_name)| {
            SemanticImportSymbolTargetRef {
                module_key: module_key.to_owned(),
                symbol_name: symbol_name.to_owned(),
            }
        }),
    })
}

pub(crate) fn declaration_semantic_facts(declaration: &Declaration) -> SemanticDeclarationFacts {
    with_cached_semantic_declaration_facts(declaration, |facts, type_store| {
        SemanticDeclarationFacts {
            callable: facts
                .callable
                .as_ref()
                .map(|callable| materialize_callable_semantics(callable, type_store)),
            value: facts.value.as_ref().map(|value| materialize_value_semantics(value, type_store)),
            type_alias: facts
                .type_alias
                .as_ref()
                .map(|type_alias| materialize_type_alias_semantics(type_alias, type_store)),
            import_target: facts.import_target.clone(),
        }
    })
}

#[allow(dead_code)]
pub(crate) fn declaration_semantic_type_ids(
    declaration: &Declaration,
) -> SemanticDeclarationTypeIds {
    with_cached_semantic_declaration_facts(declaration, |facts, _| SemanticDeclarationTypeIds {
        callable_return: facts.callable.as_ref().and_then(|callable| callable.return_type_id),
        value_annotation: facts.value.as_ref().and_then(|value| value.annotation_id),
        type_alias_body: facts.type_alias.as_ref().map(|type_alias| type_alias.body_id),
    })
}

pub(crate) fn declaration_callable_semantics(
    declaration: &Declaration,
) -> Option<SemanticCallableDeclaration> {
    with_cached_semantic_declaration_facts(declaration, |facts, type_store| {
        facts.callable.as_ref().map(|callable| materialize_callable_semantics(callable, type_store))
    })
}

pub(crate) fn declaration_type_alias_semantics(
    declaration: &Declaration,
) -> Option<SemanticTypeAliasDeclaration> {
    with_cached_semantic_declaration_facts(declaration, |facts, type_store| {
        facts
            .type_alias
            .as_ref()
            .map(|type_alias| materialize_type_alias_semantics(type_alias, type_store))
    })
}

pub(crate) fn declaration_value_semantics(
    declaration: &Declaration,
) -> Option<SemanticValueDeclaration> {
    with_cached_semantic_declaration_facts(declaration, |facts, type_store| {
        facts.value.as_ref().map(|value| materialize_value_semantics(value, type_store))
    })
}

pub(crate) fn declaration_import_target_ref(
    declaration: &Declaration,
) -> Option<SemanticImportTargetRef> {
    with_cached_semantic_declaration_facts(declaration, |facts, _| facts.import_target.clone())
}

pub(crate) fn declaration_signature_sites(
    declaration: &Declaration,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    declaration_callable_semantics(declaration)
        .map(|callable| callable_signature_sites_from_semantics(&callable))
        .unwrap_or_default()
}

pub(crate) fn declaration_semantic_signature_params(
    declaration: &Declaration,
) -> Option<Vec<SemanticCallableParam>> {
    Some(callable_semantic_params_from_semantics(&declaration_callable_semantics(declaration)?))
}

pub(crate) fn declaration_semantic_signature_params_with_self(
    declaration: &Declaration,
    owner_type_name: &str,
) -> Option<Vec<SemanticCallableParam>> {
    Some(callable_semantic_params_with_self_from_semantics(
        &declaration_callable_semantics(declaration)?,
        owner_type_name,
    ))
}

pub(crate) fn declaration_signature_param_count(declaration: &Declaration) -> Option<usize> {
    Some(declaration_callable_semantics(declaration)?.params.len())
}

pub(crate) fn declaration_signature_param_names(declaration: &Declaration) -> Option<Vec<String>> {
    Some(declaration_callable_semantics(declaration)?.param_names())
}

pub(crate) fn declaration_signature_param_types(declaration: &Declaration) -> Option<Vec<String>> {
    Some(declaration_callable_semantics(declaration)?.param_types())
}

pub(crate) fn declaration_signature_return_semantic_type(
    declaration: &Declaration,
) -> Option<SemanticType> {
    declaration_callable_semantics(declaration)?.return_type
}

pub(crate) fn declaration_signature_return_semantic_type_with_self(
    declaration: &Declaration,
    owner_type_name: &str,
) -> Option<SemanticType> {
    callable_return_semantic_type_with_self_from_semantics(
        &declaration_callable_semantics(declaration)?,
        owner_type_name,
    )
}

pub(crate) fn declaration_value_annotation_text(declaration: &Declaration) -> Option<String> {
    declaration_value_semantics(declaration)?.annotation_text
}

#[allow(dead_code)]
pub(crate) fn declaration_value_annotation_semantic_type(
    declaration: &Declaration,
) -> Option<SemanticType> {
    declaration_value_semantics(declaration)?.annotation
}
