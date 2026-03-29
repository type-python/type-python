//! Module graph and summary construction boundary for TypePython.

use std::{
    collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use typepython_binding::{
    AssertGuardSite, AssignmentSite, BindingTable, CallSite, Declaration, DeclarationKind,
    DeclarationOwnerKind, ExceptHandlerSite, ForSite, IfGuardSite, InvalidationSite, MatchSite,
    MemberAccessSite, MethodCallSite, ReturnSite, WithSite, YieldSite,
};
use typepython_syntax::{MethodKind, SourceKind};

/// Summary node for one module.
#[derive(Debug, Clone)]
pub struct ModuleNode {
    /// Module path on disk.
    pub module_path: PathBuf,
    pub module_key: String,
    pub module_kind: SourceKind,
    pub declarations: Vec<Declaration>,
    pub calls: Vec<CallSite>,
    pub method_calls: Vec<MethodCallSite>,
    pub member_accesses: Vec<MemberAccessSite>,
    pub returns: Vec<ReturnSite>,
    pub yields: Vec<YieldSite>,
    pub if_guards: Vec<IfGuardSite>,
    pub asserts: Vec<AssertGuardSite>,
    pub invalidations: Vec<InvalidationSite>,
    pub matches: Vec<MatchSite>,
    pub for_loops: Vec<ForSite>,
    pub with_statements: Vec<WithSite>,
    pub except_handlers: Vec<ExceptHandlerSite>,
    pub assignments: Vec<AssignmentSite>,
    pub summary_fingerprint: u64,
}

/// Module graph assembled from bound modules.
#[derive(Debug, Clone, Default)]
pub struct ModuleGraph {
    /// Collected module nodes.
    pub nodes: Vec<ModuleNode>,
}

/// Builds a module graph from bound modules.
#[must_use]
pub fn build(bindings: &[BindingTable]) -> ModuleGraph {
    let mut nodes = bindings
        .iter()
        .map(|binding| ModuleNode {
            module_path: binding.module_path.clone(),
            module_key: binding.module_key.clone(),
            module_kind: binding.module_kind,
            declarations: binding.declarations.clone(),
            calls: binding.calls.clone(),
            method_calls: binding.method_calls.clone(),
            member_accesses: binding.member_accesses.clone(),
            returns: binding.returns.clone(),
            yields: binding.yields.clone(),
            if_guards: binding.if_guards.clone(),
            asserts: binding.asserts.clone(),
            invalidations: binding.invalidations.clone(),
            matches: binding.matches.clone(),
            for_loops: binding.for_loops.clone(),
            with_statements: binding.with_statements.clone(),
            except_handlers: binding.except_handlers.clone(),
            assignments: binding.assignments.clone(),
            summary_fingerprint: hash_summary(binding),
        })
        .collect::<Vec<_>>();

    inject_package_module_nodes(&mut nodes);

    if !nodes.iter().any(|node| node.module_key == "typing") {
        nodes.push(typing_prelude_node());
    }
    if !nodes.iter().any(|node| node.module_key == "typing_extensions") {
        nodes.push(typing_extensions_prelude_node());
    }
    if !nodes.iter().any(|node| node.module_key == "collections.abc") {
        nodes.push(collections_abc_prelude_node());
    }

    ModuleGraph { nodes }
}

fn hash_summary(binding: &BindingTable) -> u64 {
    let mut hasher = DefaultHasher::new();
    binding.module_path.hash(&mut hasher);
    binding.module_key.hash(&mut hasher);
    binding.declarations.hash(&mut hasher);
    binding.assignments.hash(&mut hasher);
    binding.if_guards.hash(&mut hasher);
    binding.asserts.hash(&mut hasher);
    binding.invalidations.hash(&mut hasher);
    binding.matches.hash(&mut hasher);
    hasher.finish()
}

fn hash_node_summary(node: &ModuleNode, child_summaries: &[(String, u64)]) -> u64 {
    let mut hasher = DefaultHasher::new();
    node.module_path.hash(&mut hasher);
    node.module_key.hash(&mut hasher);
    node.declarations.hash(&mut hasher);
    node.assignments.hash(&mut hasher);
    node.if_guards.hash(&mut hasher);
    node.asserts.hash(&mut hasher);
    node.invalidations.hash(&mut hasher);
    node.matches.hash(&mut hasher);
    child_summaries.hash(&mut hasher);
    hasher.finish()
}

fn hash_module_summary(
    module_path: &std::path::Path,
    module_key: &str,
    declarations: &[Declaration],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    module_path.hash(&mut hasher);
    module_key.hash(&mut hasher);
    declarations.hash(&mut hasher);
    hasher.finish()
}

fn inject_package_module_nodes(nodes: &mut Vec<ModuleNode>) {
    let actual_module_keys = nodes
        .iter()
        .filter(|node| !is_synthetic_module_path(&node.module_path))
        .map(|node| node.module_key.clone())
        .collect::<BTreeSet<_>>();
    let all_module_keys = all_package_module_keys(&actual_module_keys);
    let direct_children = direct_child_module_index(&all_module_keys);
    let existing = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.module_key.clone(), index))
        .collect::<BTreeMap<_, _>>();

    for (package_key, child_keys) in &direct_children {
        if let Some(index) = existing.get(package_key).copied() {
            add_missing_child_module_imports(&mut nodes[index].declarations, child_keys);
            continue;
        }
        nodes.push(namespace_module_node(package_key, child_keys));
    }

    recompute_package_summary_fingerprints(nodes, &direct_children);
}

fn is_synthetic_module_path(path: &Path) -> bool {
    path.to_string_lossy().starts_with('<')
}

fn all_package_module_keys(actual_module_keys: &BTreeSet<String>) -> BTreeSet<String> {
    let mut all = actual_module_keys.clone();
    for module_key in actual_module_keys {
        let mut current = module_key.as_str();
        while let Some((parent, _)) = current.rsplit_once('.') {
            all.insert(parent.to_owned());
            current = parent;
        }
    }
    all
}

fn direct_child_module_index(all_module_keys: &BTreeSet<String>) -> BTreeMap<String, Vec<String>> {
    let mut index: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for module_key in all_module_keys {
        let Some((parent, _)) = module_key.rsplit_once('.') else {
            continue;
        };
        index.entry(parent.to_owned()).or_default().push(module_key.clone());
    }
    index
}

fn add_missing_child_module_imports(declarations: &mut Vec<Declaration>, child_keys: &[String]) {
    for child_key in child_keys {
        let Some(name) = child_key.rsplit('.').next() else {
            continue;
        };
        if declarations
            .iter()
            .any(|declaration| declaration.owner.is_none() && declaration.name == name)
        {
            continue;
        }
        declarations.push(package_child_import_declaration(name, child_key));
    }
}

fn namespace_module_node(module_key: &str, child_keys: &[String]) -> ModuleNode {
    let mut declarations = Vec::new();
    add_missing_child_module_imports(&mut declarations, child_keys);
    ModuleNode {
        module_path: PathBuf::from(format!("<namespace-package:{module_key}>")),
        module_key: module_key.to_owned(),
        module_kind: SourceKind::Python,
        declarations,
        calls: Vec::new(),
        method_calls: Vec::new(),
        member_accesses: Vec::new(),
        returns: Vec::new(),
        yields: Vec::new(),
        if_guards: Vec::new(),
        asserts: Vec::new(),
        invalidations: Vec::new(),
        matches: Vec::new(),
        for_loops: Vec::new(),
        with_statements: Vec::new(),
        except_handlers: Vec::new(),
        assignments: Vec::new(),
        summary_fingerprint: 0,
    }
}

fn package_child_import_declaration(name: &str, module_key: &str) -> Declaration {
    Declaration {
        name: name.to_owned(),
        kind: DeclarationKind::Import,
        detail: module_key.to_owned(),
        value_type: None,
        method_kind: None,
        class_kind: None,
        owner: None,
        is_async: false,
        is_override: false,
        is_abstract_method: false,
        is_final_decorator: false,
        is_deprecated: false,
        deprecation_message: None,
        is_final: false,
        is_class_var: false,
        bases: Vec::new(),
        type_params: Vec::new(),
    }
}

fn recompute_package_summary_fingerprints(
    nodes: &mut [ModuleNode],
    direct_children: &BTreeMap<String, Vec<String>>,
) {
    let mut order = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.module_key.split('.').count(), node.module_key.clone(), index))
        .collect::<Vec<_>>();
    order.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));

    let mut summary_by_key = BTreeMap::new();
    for (_, module_key, index) in order {
        let child_summaries = direct_children
            .get(&module_key)
            .map(|children| {
                children
                    .iter()
                    .filter_map(|child| {
                        summary_by_key.get(child).map(|summary| (child.clone(), *summary))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        nodes[index].summary_fingerprint = hash_node_summary(&nodes[index], &child_summaries);
        summary_by_key.insert(module_key, nodes[index].summary_fingerprint);
    }
}

fn typing_prelude_node() -> ModuleNode {
    prelude_node("<typing-prelude>", "typing", typing_prelude_declarations())
}

fn typing_extensions_prelude_node() -> ModuleNode {
    prelude_node("<typing-extensions-prelude>", "typing_extensions", typing_prelude_declarations())
}

fn typing_prelude_declarations() -> Vec<Declaration> {
    [
        vec![prelude_type_alias("Any", "Any")],
        vec![prelude_type_alias("List", "list[Any]")],
        vec![prelude_type_alias("Dict", "dict[Any, Any]")],
        vec![prelude_type_alias("Tuple", "tuple[Any, ...]")],
        vec![prelude_type_alias("Set", "set[Any]")],
        vec![prelude_type_alias("FrozenSet", "frozenset[Any]")],
        vec![prelude_type_alias("Optional", "Optional[Any]")],
        vec![prelude_type_alias("Union", "Union[Any, Any]")],
        vec![prelude_type_alias("Callable", "Callable")],
        vec![prelude_type_alias("Literal", "Literal")],
        vec![prelude_class("TypedDict")],
        vec![prelude_protocol_class("Protocol")],
        prelude_protocol_class_with_methods(
            "Awaitable",
            &[],
            &[("__await__", "(self)->Iterator[Any]")],
        ),
        prelude_protocol_class_with_methods(
            "AsyncIterable",
            &[],
            &[("__aiter__", "(self)->AsyncIterator[Any]")],
        ),
        prelude_protocol_class_with_methods(
            "AsyncIterator",
            &["AsyncIterable"],
            &[("__anext__", "(self)->Awaitable[Any]")],
        ),
        prelude_protocol_class_with_methods(
            "AsyncGenerator",
            &["AsyncIterator"],
            &[
                ("asend", "(self, value:Any)->Awaitable[Any]"),
                ("athrow", "(self, typ:Any, val:Any, tb:Any)->Awaitable[Any]"),
                ("aclose", "(self)->Awaitable[None]"),
            ],
        ),
        prelude_protocol_class_with_methods(
            "Coroutine",
            &["Awaitable"],
            &[
                ("send", "(self, value:Any)->Any"),
                ("throw", "(self, typ:Any, val:Any, tb:Any)->Any"),
                ("close", "(self)->None"),
            ],
        ),
        prelude_protocol_class_with_methods(
            "Generator",
            &["Iterator"],
            &[
                ("send", "(self, value:Any)->Any"),
                ("throw", "(self, typ:Any, val:Any, tb:Any)->Any"),
                ("close", "(self)->None"),
            ],
        ),
        vec![prelude_function("cast", "(t:,value:)->Any")],
        vec![prelude_function("NewType", "(name:str,typ:)->NewType")],
        vec![prelude_function("TypeVar", "(name:str)->TypeVar")],
        vec![prelude_function("ParamSpec", "(name:str)->ParamSpec")],
        vec![prelude_function("TypeVarTuple", "(name:str)->TypeVarTuple")],
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
}

fn prelude_node(module_path: &str, module_key: &str, declarations: Vec<Declaration>) -> ModuleNode {
    let module_path = PathBuf::from(module_path);
    let module_key = String::from(module_key);
    let summary_fingerprint = hash_module_summary(&module_path, &module_key, &declarations);

    ModuleNode {
        module_path,
        module_key,
        module_kind: SourceKind::Stub,
        declarations,
        calls: Vec::new(),
        method_calls: Vec::new(),
        member_accesses: Vec::new(),
        returns: Vec::new(),
        yields: Vec::new(),
        if_guards: Vec::new(),
        asserts: Vec::new(),
        invalidations: Vec::new(),
        matches: Vec::new(),
        for_loops: Vec::new(),
        with_statements: Vec::new(),
        except_handlers: Vec::new(),
        assignments: Vec::new(),
        summary_fingerprint,
    }
}

fn collections_abc_prelude_node() -> ModuleNode {
    let module_path = PathBuf::from("<collections.abc-prelude>");
    let module_key = String::from("collections.abc");
    let declarations = [
        prelude_protocol_class_with_methods("Sized", &[], &[("__len__", "(self)->int")]),
        prelude_protocol_class_with_methods(
            "Iterable",
            &["Sized"],
            &[("__iter__", "(self)->Iterator[Any]")],
        ),
        prelude_protocol_class_with_methods(
            "Sequence",
            &["Sized", "Iterable"],
            &[
                ("__getitem__", "(self, index:int)->Any"),
                ("__iter__", "(self)->Iterator[Any]"),
                ("count", "(self, item:object)->int"),
                ("index", "(self, item:object)->int"),
            ],
        ),
        prelude_protocol_class_with_methods(
            "Mapping",
            &["Sized", "Iterable"],
            &[
                ("__getitem__", "(self, key:Any)->Any"),
                ("__iter__", "(self)->Iterator[Any]"),
                ("keys", "(self)->Any"),
                ("values", "(self)->Any"),
                ("items", "(self)->Any"),
                ("get", "(self, key:Any, default:)->Any"),
            ],
        ),
        prelude_protocol_class_with_methods("Callable", &[], &[("__call__", "(self)->Any")]),
        prelude_protocol_class_with_methods(
            "AsyncIterable",
            &[],
            &[("__aiter__", "(self)->AsyncIterator[Any]")],
        ),
        prelude_protocol_class_with_methods(
            "AsyncIterator",
            &["AsyncIterable"],
            &[("__anext__", "(self)->Awaitable[Any]")],
        ),
        prelude_protocol_class_with_methods(
            "AsyncGenerator",
            &["AsyncIterator"],
            &[
                ("asend", "(self, value:Any)->Awaitable[Any]"),
                ("athrow", "(self, typ:Any, val:Any, tb:Any)->Awaitable[Any]"),
                ("aclose", "(self)->Awaitable[None]"),
            ],
        ),
        prelude_protocol_class_with_methods(
            "Iterator",
            &["Iterable"],
            &[("__next__", "(self)->Any")],
        ),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let summary_fingerprint = hash_module_summary(&module_path, &module_key, &declarations);

    ModuleNode {
        module_path,
        module_key,
        module_kind: SourceKind::Stub,
        declarations,
        calls: Vec::new(),
        method_calls: Vec::new(),
        member_accesses: Vec::new(),
        returns: Vec::new(),
        yields: Vec::new(),
        if_guards: Vec::new(),
        asserts: Vec::new(),
        invalidations: Vec::new(),
        matches: Vec::new(),
        for_loops: Vec::new(),
        with_statements: Vec::new(),
        except_handlers: Vec::new(),
        assignments: Vec::new(),
        summary_fingerprint,
    }
}

fn prelude_type_alias(name: &str, detail: &str) -> Declaration {
    Declaration {
        name: String::from(name),
        kind: DeclarationKind::TypeAlias,
        detail: String::from(detail),
        value_type: None,
        method_kind: None,
        class_kind: None,
        owner: None,
        is_async: false,
        is_override: false,
        is_abstract_method: false,
        is_final_decorator: false,
        is_deprecated: false,
        deprecation_message: None,
        is_final: false,
        is_class_var: false,
        bases: Vec::new(),
        type_params: Vec::new(),
    }
}

fn prelude_function(name: &str, detail: &str) -> Declaration {
    Declaration {
        name: String::from(name),
        kind: DeclarationKind::Function,
        detail: String::from(detail),
        value_type: None,
        method_kind: None,
        class_kind: None,
        owner: None,
        is_async: false,
        is_override: false,
        is_abstract_method: false,
        is_final_decorator: false,
        is_deprecated: false,
        deprecation_message: None,
        is_final: false,
        is_class_var: false,
        bases: Vec::new(),
        type_params: Vec::new(),
    }
}

fn prelude_protocol_class(name: &str) -> Declaration {
    Declaration {
        name: String::from(name),
        kind: DeclarationKind::Class,
        detail: String::new(),
        value_type: None,
        method_kind: None,
        class_kind: Some(DeclarationOwnerKind::Interface),
        owner: None,
        is_async: false,
        is_override: false,
        is_abstract_method: false,
        is_final_decorator: false,
        is_deprecated: false,
        deprecation_message: None,
        is_final: false,
        is_class_var: false,
        bases: Vec::new(),
        type_params: Vec::new(),
    }
}

fn prelude_class(name: &str) -> Declaration {
    Declaration {
        name: String::from(name),
        kind: DeclarationKind::Class,
        detail: String::new(),
        value_type: None,
        method_kind: None,
        class_kind: Some(DeclarationOwnerKind::Class),
        owner: None,
        is_async: false,
        is_override: false,
        is_abstract_method: false,
        is_final_decorator: false,
        is_deprecated: false,
        deprecation_message: None,
        is_final: false,
        is_class_var: false,
        bases: Vec::new(),
        type_params: Vec::new(),
    }
}

fn prelude_protocol_class_with_methods(
    name: &str,
    bases: &[&str],
    methods: &[(&str, &str)],
) -> Vec<Declaration> {
    let mut declarations = vec![Declaration {
        name: String::from(name),
        kind: DeclarationKind::Class,
        detail: bases.join(","),
        value_type: None,
        method_kind: None,
        class_kind: Some(DeclarationOwnerKind::Interface),
        owner: None,
        is_async: false,
        is_override: false,
        is_abstract_method: false,
        is_final_decorator: false,
        is_deprecated: false,
        deprecation_message: None,
        is_final: false,
        is_class_var: false,
        bases: bases.iter().map(|base| String::from(*base)).collect(),
        type_params: Vec::new(),
    }];

    declarations.extend(methods.iter().map(|(method_name, detail)| Declaration {
        name: String::from(*method_name),
        kind: DeclarationKind::Function,
        detail: String::from(*detail),
        value_type: None,
        method_kind: Some(MethodKind::Instance),
        class_kind: None,
        owner: Some(typepython_binding::DeclarationOwner {
            name: String::from(name),
            kind: DeclarationOwnerKind::Interface,
        }),
        is_async: false,
        is_override: false,
        is_abstract_method: false,
        is_final_decorator: false,
        is_deprecated: false,
        deprecation_message: None,
        is_final: false,
        is_class_var: false,
        bases: Vec::new(),
        type_params: Vec::new(),
    }));

    declarations
}

#[cfg(test)]
mod tests {
    use super::build;
    use std::path::PathBuf;
    use typepython_binding::{BindingTable, Declaration, DeclarationKind, DeclarationOwnerKind};
    use typepython_syntax::SourceKind;

    #[test]
    fn build_carries_bound_symbols_into_module_nodes() {
        let graph = build(&[BindingTable {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::from("app"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("UserId"),
                    kind: DeclarationKind::TypeAlias,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
            ],
            calls: Vec::new(),
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
        }]);

        assert_eq!(
            graph.nodes[0].declarations,
            vec![
                Declaration {
                    name: String::from("UserId"),
                    kind: DeclarationKind::TypeAlias,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn build_changes_fingerprint_when_symbols_change() {
        let first = build(&[BindingTable {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::from("app"),
            module_kind: SourceKind::TypePython,
            declarations: vec![Declaration {
                name: String::from("UserId"),
                kind: DeclarationKind::TypeAlias,
                detail: String::new(),
                value_type: None,
                method_kind: None,
                class_kind: None,
                owner: None,
                is_async: false,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            }],
            calls: Vec::new(),
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
        }]);
        let second = build(&[BindingTable {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::from("app"),
            module_kind: SourceKind::TypePython,
            declarations: vec![
                Declaration {
                    name: String::from("UserId"),
                    kind: DeclarationKind::TypeAlias,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("User"),
                    kind: DeclarationKind::Class,
                    detail: String::new(),
                    value_type: None,
                    method_kind: None,
                    class_kind: Some(DeclarationOwnerKind::Class),
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
            ],
            calls: Vec::new(),
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
        }]);

        println!(
            "{} -> {}",
            first.nodes[0].summary_fingerprint, second.nodes[0].summary_fingerprint
        );
        assert_ne!(first.nodes[0].summary_fingerprint, second.nodes[0].summary_fingerprint);
    }

    #[test]
    fn build_appends_typing_prelude_node() {
        let graph = build(&[BindingTable {
            module_path: PathBuf::from("src/app/__init__.tpy"),
            module_key: String::from("app"),
            module_kind: SourceKind::TypePython,
            declarations: Vec::new(),
            calls: Vec::new(),
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
        }]);

        let typing = graph
            .nodes
            .iter()
            .find(|node| node.module_key == "typing")
            .expect("expected typing prelude node");
        let typing_extensions = graph
            .nodes
            .iter()
            .find(|node| node.module_key == "typing_extensions")
            .expect("expected typing_extensions prelude node");
        let collections_abc = graph
            .nodes
            .iter()
            .find(|node| node.module_key == "collections.abc")
            .expect("expected collections.abc prelude node");

        assert_eq!(typing.module_kind, SourceKind::Stub);
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "List"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "Callable"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "Literal"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "TypedDict"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "Awaitable"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "AsyncIterator"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "AsyncGenerator"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "Coroutine"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "Generator"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "cast"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "NewType"));
        assert!(typing.declarations.iter().any(|declaration| declaration.name == "TypeVar"));
        assert_eq!(typing_extensions.module_kind, SourceKind::Stub);
        assert!(
            typing_extensions.declarations.iter().any(|declaration| declaration.name == "Protocol")
        );
        assert!(
            typing_extensions
                .declarations
                .iter()
                .any(|declaration| declaration.name == "TypedDict")
        );
        assert!(
            typing_extensions.declarations.iter().any(|declaration| declaration.name == "TypeVar")
        );
        assert!(
            typing_extensions
                .declarations
                .iter()
                .any(|declaration| declaration.name == "Awaitable")
        );
        assert_eq!(collections_abc.module_kind, SourceKind::Stub);
        assert!(collections_abc.declarations.iter().any(|declaration| declaration.name == "Sized"));
        assert!(
            collections_abc.declarations.iter().any(|declaration| declaration.name == "Iterable")
        );
        assert!(
            collections_abc.declarations.iter().any(|declaration| declaration.name == "Callable")
        );
        assert!(
            collections_abc.declarations.iter().any(|declaration| declaration.name == "Iterator")
        );
        assert!(
            collections_abc
                .declarations
                .iter()
                .any(|declaration| declaration.name == "AsyncIterator")
        );
        assert!(
            collections_abc
                .declarations
                .iter()
                .any(|declaration| declaration.name == "AsyncGenerator")
        );
        assert!(
            collections_abc.declarations.iter().any(|declaration| declaration.name == "Sequence")
        );
        assert!(
            collections_abc.declarations.iter().any(|declaration| declaration.name == "Mapping")
        );
    }

    #[test]
    fn build_synthesizes_namespace_package_nodes_for_parent_modules() {
        let graph = build(&[BindingTable {
            module_path: PathBuf::from("src/pkg/sub/module.py"),
            module_key: String::from("pkg.sub.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("greet"),
                kind: DeclarationKind::Function,
                detail: String::from("(name:str)->str"),
                value_type: None,
                method_kind: None,
                class_kind: None,
                owner: None,
                is_async: false,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            }],
            calls: Vec::new(),
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
        }]);

        let pkg = graph
            .nodes
            .iter()
            .find(|node| node.module_key == "pkg")
            .expect("expected synthetic pkg namespace node");
        let sub = graph
            .nodes
            .iter()
            .find(|node| node.module_key == "pkg.sub")
            .expect("expected synthetic pkg.sub namespace node");

        assert!(pkg.module_path.to_string_lossy().contains("<namespace-package:pkg>"));
        assert!(
            pkg.declarations.iter().any(|declaration| declaration.kind == DeclarationKind::Import
                && declaration.name == "sub"
                && declaration.detail == "pkg.sub")
        );
        assert!(
            sub.declarations.iter().any(|declaration| declaration.kind == DeclarationKind::Import
                && declaration.name == "module"
                && declaration.detail == "pkg.sub.module")
        );
    }

    #[test]
    fn build_namespace_package_fingerprint_tracks_child_summary_changes() {
        let first = build(&[BindingTable {
            module_path: PathBuf::from("src/pkg/sub/module.py"),
            module_key: String::from("pkg.sub.module"),
            module_kind: SourceKind::Python,
            declarations: vec![Declaration {
                name: String::from("greet"),
                kind: DeclarationKind::Function,
                detail: String::from("(name:str)->str"),
                value_type: None,
                method_kind: None,
                class_kind: None,
                owner: None,
                is_async: false,
                is_override: false,
                is_abstract_method: false,
                is_final_decorator: false,
                is_deprecated: false,
                deprecation_message: None,
                is_final: false,
                is_class_var: false,
                bases: Vec::new(),
                type_params: Vec::new(),
            }],
            calls: Vec::new(),
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
        }]);
        let second = build(&[BindingTable {
            module_path: PathBuf::from("src/pkg/sub/module.py"),
            module_key: String::from("pkg.sub.module"),
            module_kind: SourceKind::Python,
            declarations: vec![
                Declaration {
                    name: String::from("greet"),
                    kind: DeclarationKind::Function,
                    detail: String::from("(name:str)->str"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
                Declaration {
                    name: String::from("version"),
                    kind: DeclarationKind::Value,
                    detail: String::from("str"),
                    value_type: None,
                    method_kind: None,
                    class_kind: None,
                    owner: None,
                    is_async: false,
                    is_override: false,
                    is_abstract_method: false,
                    is_final_decorator: false,
                    is_deprecated: false,
                    deprecation_message: None,
                    is_final: false,
                    is_class_var: false,
                    bases: Vec::new(),
                    type_params: Vec::new(),
                },
            ],
            calls: Vec::new(),
            method_calls: Vec::new(),
            member_accesses: Vec::new(),
            returns: Vec::new(),
            yields: Vec::new(),
            if_guards: Vec::new(),
            asserts: Vec::new(),
            invalidations: Vec::new(),
            matches: Vec::new(),
            for_loops: Vec::new(),
            with_statements: Vec::new(),
            except_handlers: Vec::new(),
            assignments: Vec::new(),
        }]);

        let first_pkg = first
            .nodes
            .iter()
            .find(|node| node.module_key == "pkg")
            .expect("expected synthetic pkg namespace node");
        let second_pkg = second
            .nodes
            .iter()
            .find(|node| node.module_key == "pkg")
            .expect("expected synthetic pkg namespace node");

        assert_ne!(first_pkg.summary_fingerprint, second_pkg.summary_fingerprint);
    }
}
