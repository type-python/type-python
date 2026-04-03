use std::{cell::RefCell, collections::HashMap};

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

pub(crate) fn callable_signature_sites_from_semantics(
    callable: &SemanticCallableDeclaration,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    callable.params.clone()
}

pub(crate) fn callable_signature_sites_with_self_from_semantics(
    callable: &SemanticCallableDeclaration,
    owner_type_name: &str,
) -> Vec<typepython_syntax::DirectFunctionParamSite> {
    callable_signature_sites_from_semantics(callable)
        .into_iter()
        .map(|mut param| {
            param.annotation = param
                .annotation
                .map(|annotation| substitute_self_annotation(&annotation, Some(owner_type_name)));
            param
        })
        .collect()
}

pub(crate) fn callable_return_annotation_text_from_semantics(
    callable: &SemanticCallableDeclaration,
) -> Option<String> {
    callable.return_annotation_text.clone()
}

pub(crate) fn callable_return_annotation_text_with_self_from_semantics(
    callable: &SemanticCallableDeclaration,
    owner_type_name: &str,
) -> Option<String> {
    callable
        .return_annotation_text
        .as_deref()
        .map(|annotation| substitute_self_annotation(annotation, Some(owner_type_name)))
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SemanticValueDeclaration {
    pub(crate) annotation_text: Option<String>,
    pub(crate) annotation: Option<SemanticType>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CachedSemanticCallableDeclaration {
    params: Vec<typepython_syntax::DirectFunctionParamSite>,
    return_annotation_text: Option<String>,
    return_type_id: Option<TypeId>,
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
            .then(|| parse_direct_callable_declaration(&declaration.detail))
            .flatten()
            .map(|callable| CachedSemanticCallableDeclaration {
                params: callable.params,
                return_annotation_text: callable.return_annotation_text,
                return_type_id: intern_semantic_type(type_store, callable.return_type),
            }),
        value: (declaration.kind == DeclarationKind::Value).then(|| {
            CachedSemanticValueDeclaration {
                annotation_text: (!declaration.detail.trim().is_empty())
                    .then(|| declaration.detail.clone()),
                annotation_id: intern_semantic_type(
                    type_store,
                    (!declaration.detail.trim().is_empty())
                        .then(|| lower_type_text_or_name(&declaration.detail)),
                ),
            }
        }),
        type_alias: (declaration.kind == DeclarationKind::TypeAlias).then(|| {
            CachedSemanticTypeAliasDeclaration {
                head: declaration.name.clone(),
                type_params: declaration.type_params.clone(),
                body_text: declaration.detail.clone(),
                body_id: type_store.intern(lower_type_text_or_name(&declaration.detail)),
            }
        }),
        import_target: (declaration.kind == DeclarationKind::Import)
            .then(|| parse_import_target_ref(&declaration.detail))
            .flatten(),
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

fn render_direct_signature_site(param: &typepython_syntax::DirectFunctionParamSite) -> String {
    let mut rendered = String::new();
    if param.keyword_variadic {
        rendered.push_str("**");
    } else if param.variadic {
        rendered.push('*');
    }
    rendered.push_str(&param.name);
    if let Some(annotation) = &param.annotation {
        rendered.push(':');
        rendered.push_str(annotation);
    }
    if param.has_default {
        rendered.push('=');
    }
    rendered
}

fn render_direct_signature_sites(
    signature: &[typepython_syntax::DirectFunctionParamSite],
    return_annotation: Option<&str>,
) -> String {
    let last_positional_only = signature.iter().rposition(|param| param.positional_only);
    let first_keyword_only = signature.iter().position(|param| param.keyword_only);
    let keyword_marker_index =
        first_keyword_only.filter(|index| !signature[..*index].iter().any(|param| param.variadic));
    let mut parts = Vec::new();
    for (index, param) in signature.iter().enumerate() {
        if keyword_marker_index == Some(index) {
            parts.push(String::from("*"));
        }
        parts.push(render_direct_signature_site(param));
        if last_positional_only == Some(index) {
            parts.push(String::from("/"));
        }
    }
    format!("({})->{}", parts.join(","), return_annotation.unwrap_or_default())
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
    let params = parse_direct_signature_params(detail)?
        .into_iter()
        .map(|param| typepython_syntax::DirectFunctionParamSite {
            name: param.name,
            annotation: (!param.annotation.is_empty()).then_some(param.annotation),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect::<Vec<_>>();
    let return_annotation_text = detail.split_once("->").map(|(_, right)| right.trim().to_owned());
    let return_type = return_annotation_text.as_deref().map(lower_type_text_or_name);
    Some(SemanticCallableDeclaration { params, return_annotation_text, return_type })
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

pub(crate) fn declaration_signature_sites_with_self(
    declaration: &Declaration,
    owner_type_name: &str,
) -> Option<Vec<typepython_syntax::DirectFunctionParamSite>> {
    Some(callable_signature_sites_with_self_from_semantics(
        &declaration_callable_semantics(declaration)?,
        owner_type_name,
    ))
}

pub(crate) fn direct_signature_params_from_sites(
    signature: &[typepython_syntax::DirectFunctionParamSite],
) -> Vec<DirectSignatureParam> {
    signature
        .iter()
        .map(|param| DirectSignatureParam {
            name: param.name.clone(),
            annotation: param.annotation.clone().unwrap_or_default(),
            has_default: param.has_default,
            positional_only: param.positional_only,
            keyword_only: param.keyword_only,
            variadic: param.variadic,
            keyword_variadic: param.keyword_variadic,
        })
        .collect()
}

pub(crate) fn declaration_signature_param_count(declaration: &Declaration) -> Option<usize> {
    Some(declaration_callable_semantics(declaration)?.params.len())
}

pub(crate) fn declaration_signature_text(declaration: &Declaration) -> Option<String> {
    let callable = declaration_callable_semantics(declaration)?;
    Some(render_direct_signature_sites(
        &callable_signature_sites_from_semantics(&callable),
        callable_return_annotation_text_from_semantics(&callable).as_deref(),
    ))
}

pub(crate) fn declaration_signature_param_names(declaration: &Declaration) -> Option<Vec<String>> {
    Some(declaration_callable_semantics(declaration)?.param_names())
}

pub(crate) fn declaration_signature_param_types(declaration: &Declaration) -> Option<Vec<String>> {
    Some(declaration_callable_semantics(declaration)?.param_types())
}

pub(crate) fn declaration_signature_return_annotation_text(
    declaration: &Declaration,
) -> Option<String> {
    callable_return_annotation_text_from_semantics(&declaration_callable_semantics(declaration)?)
}

pub(crate) fn declaration_signature_return_semantic_type(
    declaration: &Declaration,
) -> Option<SemanticType> {
    declaration_callable_semantics(declaration)?.return_type
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
